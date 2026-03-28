use crate::config::BackendProfile;
use crate::error::{ProxyError, ProxyResult};
use crate::models::{anthropic, openai};
use serde_json::{json, Value};

pub fn generate_message_id() -> String {
    format!(
        "msg_{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    )
}

fn extract_tool_choice(
    extra: &serde_json::Map<String, serde_json::Value>,
) -> Option<openai::ToolChoice> {
    let tool_choice = extra.get("tool_choice")?;

    match tool_choice.get("type").and_then(|t| t.as_str()) {
        Some("auto") => Some(openai::ToolChoice::String("auto".to_string())),
        Some("any") => Some(openai::ToolChoice::String("required".to_string())),
        Some("tool") => tool_choice
            .get("name")
            .and_then(|n| n.as_str())
            .map(|name| openai::ToolChoice::Object {
                tool_type: "function".to_string(),
                function: openai::ToolChoiceFunction {
                    name: name.to_string(),
                },
            }),
        _ => None,
    }
}

pub fn anthropic_to_openai(
    req: anthropic::AnthropicRequest,
    model: &str,
    profile: BackendProfile,
) -> ProxyResult<openai::OpenAIRequest> {
    if req.max_tokens == 0 {
        return Err(ProxyError::Transform(
            "max_tokens must be greater than zero".to_string(),
        ));
    }

    if req
        .extra
        .get("thinking")
        .and_then(|v| v.get("type"))
        .is_some()
        && !profile.supports_reasoning()
    {
        return Err(ProxyError::Transform(format!(
            "thinking is not supported by backend profile {}",
            profile.as_str()
        )));
    }

    let mut openai_messages = Vec::new();

    if let Some(system) = req.system {
        let system_text = match system {
            anthropic::SystemPrompt::Single(text) => text,
            anthropic::SystemPrompt::Multiple(messages) => messages
                .into_iter()
                .map(|msg| msg.text)
                .collect::<Vec<_>>()
                .join("\n\n"),
        };

        if !system_text.is_empty() {
            openai_messages.push(openai::Message {
                role: "system".to_string(),
                content: Some(openai::MessageContent::Text(system_text)),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            });
        }
    }

    for msg in req.messages {
        openai_messages.extend(convert_message(msg, profile)?);
    }

    let tools = req.tools.and_then(|tools| {
        let filtered: Vec<_> = tools
            .into_iter()
            .filter(|t| t.tool_type.as_deref() != Some("BatchTool"))
            .collect();

        if filtered.is_empty() {
            None
        } else {
            Some(
                filtered
                    .into_iter()
                    .map(|t| openai::Tool {
                        tool_type: "function".to_string(),
                        function: openai::Function {
                            name: t.name,
                            description: t.description,
                            parameters: clean_schema(t.input_schema),
                        },
                    })
                    .collect(),
            )
        }
    });

    Ok(openai::OpenAIRequest {
        model: model.to_string(),
        messages: openai_messages,
        max_tokens: Some(req.max_tokens),
        temperature: req.temperature,
        top_p: req.top_p,
        top_k: if profile.supports_top_k() {
            req.top_k
        } else {
            None
        },
        stop: req.stop_sequences,
        stream: req.stream,
        tools,
        tool_choice: extract_tool_choice(&req.extra),
    })
}

fn convert_message(
    msg: anthropic::Message,
    profile: BackendProfile,
) -> ProxyResult<Vec<openai::Message>> {
    let mut result = Vec::new();

    match msg.content {
        anthropic::MessageContent::Text(text) => {
            result.push(openai::Message {
                role: msg.role,
                content: Some(openai::MessageContent::Text(text)),
                tool_calls: None,
                tool_call_id: None,
                name: None,
            });
        }
        anthropic::MessageContent::Blocks(blocks) => {
            let mut current_content_parts = Vec::new();
            let mut tool_calls = Vec::new();

            for block in blocks {
                match block {
                    anthropic::ContentBlock::Text { text } => {
                        current_content_parts.push(openai::ContentPart::Text { data: text });
                    }
                    anthropic::ContentBlock::Image { source } => {
                        let data_url = format!("data:{};base64,{}", source.media_type, source.data);
                        current_content_parts.push(openai::ContentPart::ImageUrl {
                            image_url: openai::ImageUrl { url: data_url },
                        });
                    }
                    anthropic::ContentBlock::ToolUse { id, name, input } => {
                        tool_calls.push(openai::ToolCall {
                            id,
                            call_type: "function".to_string(),
                            function: openai::FunctionCall {
                                name,
                                arguments: serde_json::to_string(&input)?,
                            },
                        });
                    }
                    anthropic::ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        is_error,
                    } => {
                        let mut text = match content {
                            anthropic::ToolResultContent::Text(s) => s,
                            anthropic::ToolResultContent::Blocks(blocks) => blocks
                                .iter()
                                .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                                .collect::<Vec<_>>()
                                .join("\n"),
                        };

                        if is_error.unwrap_or(false) {
                            text = format!("ERROR: {text}");
                        }

                        result.push(openai::Message {
                            role: "tool".to_string(),
                            content: Some(openai::MessageContent::Text(text)),
                            tool_calls: None,
                            tool_call_id: Some(tool_use_id),
                            name: None,
                        });
                    }
                    anthropic::ContentBlock::Thinking { thinking } => {
                        return Err(ProxyError::Transform(format!(
                            "assistant thinking blocks are not supported by backend profile {} (received {} chars)",
                            profile.as_str(),
                            thinking.len()
                        )));
                    }
                    anthropic::ContentBlock::Other => {}
                }
            }

            if !current_content_parts.is_empty() || !tool_calls.is_empty() {
                let content = if current_content_parts.is_empty() {
                    None
                } else if current_content_parts.len() == 1 {
                    match &current_content_parts[0] {
                        openai::ContentPart::Text { data } => {
                            Some(openai::MessageContent::Text(data.clone()))
                        }
                        _ => Some(openai::MessageContent::Parts(current_content_parts)),
                    }
                } else {
                    Some(openai::MessageContent::Parts(current_content_parts))
                };

                result.push(openai::Message {
                    role: msg.role,
                    content,
                    tool_calls: if tool_calls.is_empty() {
                        None
                    } else {
                        Some(tool_calls)
                    },
                    tool_call_id: None,
                    name: None,
                });
            }
        }
    }

    Ok(result)
}

fn clean_schema(mut schema: Value) -> Value {
    if let Some(obj) = schema.as_object_mut() {
        obj.remove("format");

        if let Some(properties) = obj.get_mut("properties").and_then(|v| v.as_object_mut()) {
            for (_, value) in properties.iter_mut() {
                *value = clean_schema(value.clone());
            }
        }

        if let Some(items) = obj.get_mut("items") {
            *items = clean_schema(items.clone());
        }

        for key in ["anyOf", "oneOf", "allOf"] {
            if let Some(arr) = obj.get_mut(key).and_then(|v| v.as_array_mut()) {
                for item in arr.iter_mut() {
                    *item = clean_schema(item.clone());
                }
            }
        }
    }

    schema
}

pub fn openai_to_anthropic(
    resp: openai::OpenAIResponse,
    fallback_model: &str,
    profile: BackendProfile,
) -> ProxyResult<anthropic::AnthropicResponse> {
    let choice = resp
        .choices
        .first()
        .ok_or_else(|| ProxyError::Transform("No choices in response".to_string()))?;

    let mut content = Vec::new();

    if let Some(reasoning) = choice
        .message
        .reasoning_content
        .as_ref()
        .filter(|s| !s.is_empty())
    {
        if !profile.supports_reasoning() {
            return Err(ProxyError::Transform(format!(
                "backend profile {} returned reasoning content that cannot be represented safely",
                profile.as_str()
            )));
        }
        content.push(anthropic::ResponseContent::Thinking {
            thinking: reasoning.clone(),
        });
    }
    if let Some(text) = choice.message.content.as_ref().filter(|s| !s.is_empty()) {
        content.push(anthropic::ResponseContent::Text { text: text.clone() });
    }

    if let Some(tool_calls) = &choice.message.tool_calls {
        for tool_call in tool_calls {
            let input: Value =
                serde_json::from_str(&tool_call.function.arguments).unwrap_or_else(|_| json!({}));

            content.push(anthropic::ResponseContent::ToolUse {
                id: tool_call.id.clone(),
                name: tool_call.function.name.clone(),
                input,
            });
        }
    }

    let stop_reason = choice
        .finish_reason
        .as_ref()
        .map(|r| match r.as_str() {
            "tool_calls" => "tool_use",
            "stop" => "end_turn",
            "length" => "max_tokens",
            _ => "end_turn",
        })
        .map(String::from);

    Ok(anthropic::AnthropicResponse {
        id: resp.id.unwrap_or_else(generate_message_id),
        response_type: "message".to_string(),
        role: "assistant".to_string(),
        content,
        model: resp.model.unwrap_or_else(|| fallback_model.to_string()),
        stop_reason,
        stop_sequence: None,
        usage: anthropic::Usage {
            input_tokens: resp.usage.prompt_tokens,
            output_tokens: resp.usage.completion_tokens,
        },
    })
}

pub fn map_stop_reason(finish_reason: Option<&str>) -> Option<String> {
    finish_reason
        .map(|r| match r {
            "tool_calls" => "tool_use",
            "stop" => "end_turn",
            "length" => "max_tokens",
            _ => "end_turn",
        })
        .map(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::anthropic::{
        AnthropicRequest, ContentBlock, Message, MessageContent, SystemMessage, SystemPrompt, Tool,
    };

    fn sample_request() -> AnthropicRequest {
        AnthropicRequest {
            model: "claude-sonnet-4".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::Text("ping".to_string()),
            }],
            system: Some(SystemPrompt::Single("sys".to_string())),
            stream: Some(true),
            max_tokens: 128,
            temperature: Some(0.1),
            top_p: Some(0.9),
            top_k: Some(40),
            tools: Some(vec![Tool {
                name: "weather".to_string(),
                description: Some("desc".to_string()),
                input_schema: json!({"type":"object","properties":{"city":{"type":"string","format":"city"}}}),
                tool_type: None,
            }]),
            stop_sequences: Some(vec!["STOP".to_string()]),
            extra: serde_json::Map::new(),
        }
    }

    #[test]
    fn strips_top_k_for_generic_profile() {
        let req = sample_request();
        let transformed = anthropic_to_openai(req, "model", BackendProfile::OpenaiGeneric).unwrap();
        assert_eq!(transformed.top_k, None);
    }

    #[test]
    fn keeps_top_k_for_chutes_profile() {
        let req = sample_request();
        let transformed = anthropic_to_openai(req, "model", BackendProfile::Chutes).unwrap();
        assert_eq!(transformed.top_k, Some(40));
    }

    #[test]
    fn rejects_assistant_thinking_history() {
        let mut req = sample_request();
        req.messages = vec![Message {
            role: "assistant".to_string(),
            content: MessageContent::Blocks(vec![ContentBlock::Thinking {
                thinking: "hidden".to_string(),
            }]),
        }];

        let err = anthropic_to_openai(req, "model", BackendProfile::Chutes).unwrap_err();
        assert!(err.to_string().contains("thinking blocks"));
    }

    #[test]
    fn collapses_multiple_system_prompts_into_single_openai_message() {
        let req = AnthropicRequest {
            model: "claude".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::Text("hi".to_string()),
            }],
            system: Some(SystemPrompt::Multiple(vec![
                SystemMessage {
                    text: "one".to_string(),
                },
                SystemMessage {
                    text: "two".to_string(),
                },
            ])),
            max_tokens: 64,
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            stream: None,
            tools: None,
            extra: Default::default(),
        };

        let out = anthropic_to_openai(req, "model", BackendProfile::Chutes).unwrap();
        assert_eq!(out.messages[0].role, "system");
        match out.messages[0].content.as_ref().unwrap() {
            openai::MessageContent::Text(text) => assert_eq!(text, "one\n\ntwo"),
            other => panic!("expected text system prompt, got {other:?}"),
        }
        assert_eq!(out.messages[1].role, "user");
    }

    #[test]
    fn maps_reasoning_to_thinking_block_for_chutes() {
        let resp = openai::OpenAIResponse {
            id: Some("id1".to_string()),
            model: Some("backend".to_string()),
            choices: vec![openai::Choice {
                message: openai::ChoiceMessage {
                    content: Some("answer".to_string()),
                    tool_calls: None,
                    reasoning_content: Some("chain".to_string()),
                },
                finish_reason: Some("stop".to_string()),
            }],
            usage: openai::Usage {
                prompt_tokens: 10,
                completion_tokens: 5,
            },
        };

        let out = openai_to_anthropic(resp, "fallback", BackendProfile::Chutes).unwrap();
        match &out.content[0] {
            anthropic::ResponseContent::Thinking { thinking } => assert_eq!(thinking, "chain"),
            other => panic!("expected thinking block, got {other:?}"),
        }
    }

    #[test]
    fn rejects_reasoning_for_generic_profile() {
        let resp = openai::OpenAIResponse {
            id: Some("id1".to_string()),
            model: Some("backend".to_string()),
            choices: vec![openai::Choice {
                message: openai::ChoiceMessage {
                    content: None,
                    tool_calls: None,
                    reasoning_content: Some("chain".to_string()),
                },
                finish_reason: Some("stop".to_string()),
            }],
            usage: openai::Usage {
                prompt_tokens: 10,
                completion_tokens: 5,
            },
        };

        let err = openai_to_anthropic(resp, "fallback", BackendProfile::OpenaiGeneric).unwrap_err();
        assert!(err.to_string().contains("reasoning content"));
    }
}
