use crate::config::{BackendProfile, CompatMode};
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

fn has_thinking(req: &anthropic::AnthropicRequest) -> bool {
    if let Some(thinking) = &req.thinking {
        return !thinking.thinking_type.eq_ignore_ascii_case("disabled");
    }

    req.extra
        .get("thinking")
        .and_then(|v| v.get("type"))
        .and_then(Value::as_str)
        .map(|value| !value.eq_ignore_ascii_case("disabled"))
        .is_some()
}

fn flatten_json_text(value: &Value) -> Vec<String> {
    match value {
        Value::String(text) => vec![text.clone()],
        Value::Array(items) => items.iter().flat_map(flatten_json_text).collect(),
        Value::Object(obj) => {
            let mut parts = Vec::new();
            if let Some(text) = obj.get("text").and_then(Value::as_str) {
                parts.push(text.to_string());
            }
            if let Some(query) = obj.get("query").and_then(Value::as_str) {
                parts.push(format!("query: {query}"));
            }
            if let Some(url) = obj.get("url").and_then(Value::as_str) {
                parts.push(format!("url: {url}"));
            }
            if let Some(file_id) = obj.get("file_id").and_then(Value::as_str) {
                parts.push(format!("file_id: {file_id}"));
            }
            if let Some(content) = obj.get("content") {
                parts.extend(flatten_json_text(content));
            }
            parts
        }
        _ => Vec::new(),
    }
}

fn compat_document_marker(source: &Value) -> String {
    let source_type = source
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("unknown");

    match source_type {
        "base64" => {
            let media_type = source
                .get("media_type")
                .and_then(Value::as_str)
                .unwrap_or("application/octet-stream");
            format!("[document attachment omitted: {media_type}]")
        }
        "file" => {
            let file_id = source
                .get("file_id")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            format!("[document file reference: {file_id}]")
        }
        "url" => {
            let url = source
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            format!("[document url: {url}]")
        }
        _ => "[document omitted]".to_string(),
    }
}

fn strip_think_tags(text: &str) -> (Vec<String>, String) {
    let mut reasoning = Vec::new();
    let mut visible = String::new();
    let mut rest = text;

    while let Some(start) = rest.find("<think>") {
        visible.push_str(&rest[..start]);
        let after_open = &rest[start + "<think>".len()..];
        if let Some(end) = after_open.find("</think>") {
            let think_text = after_open[..end].trim();
            if !think_text.is_empty() {
                reasoning.push(think_text.to_string());
            }
            rest = &after_open[end + "</think>".len()..];
        } else {
            let think_text = after_open.trim();
            if !think_text.is_empty() {
                reasoning.push(think_text.to_string());
            }
            rest = "";
            break;
        }
    }

    visible.push_str(rest);
    (reasoning, visible)
}

pub fn anthropic_to_openai(
    req: anthropic::AnthropicRequest,
    model: &str,
    profile: BackendProfile,
    compat_mode: CompatMode,
) -> ProxyResult<openai::OpenAIRequest> {
    let thinking_requested = has_thinking(&req);
    let _thinking_budget = req.thinking.as_ref().and_then(|cfg| cfg.budget_tokens);
    let _requested_effort = req
        .output_config
        .as_ref()
        .and_then(|cfg| cfg.effort.as_deref());

    if req.max_tokens == 0 {
        return Err(ProxyError::Transform(
            "max_tokens must be greater than zero".to_string(),
        ));
    }

    if thinking_requested && !profile.supports_reasoning() && compat_mode.is_strict() {
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
        openai_messages.extend(convert_message(msg, profile, compat_mode)?);
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
    compat_mode: CompatMode,
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
                    anthropic::ContentBlock::Document { source } => {
                        current_content_parts.push(openai::ContentPart::Text {
                            data: compat_document_marker(&source),
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
                        if !compat_mode.is_strict() {
                            current_content_parts.push(openai::ContentPart::Text {
                                data: format!("[assistant thinking omitted]\n{thinking}"),
                            });
                            continue;
                        }
                        return Err(ProxyError::Transform(format!(
                            "assistant thinking blocks are not supported by backend profile {} (received {} chars)",
                            profile.as_str(),
                            thinking.len()
                        )));
                    }
                    anthropic::ContentBlock::ServerToolUse { name, input } => {
                        let tool_name = name.unwrap_or_else(|| "server_tool".to_string());
                        let rendered_input = input
                            .map(|value| serde_json::to_string(&value).unwrap_or_default())
                            .filter(|value| !value.is_empty())
                            .unwrap_or_else(|| "{}".to_string());
                        current_content_parts.push(openai::ContentPart::Text {
                            data: format!(
                                "[server tool use omitted: {} {}]",
                                tool_name, rendered_input
                            ),
                        });
                    }
                    anthropic::ContentBlock::SearchResult { query, content } => {
                        let mut parts = Vec::new();
                        if let Some(query) = query {
                            parts.push(format!("query: {query}"));
                        }
                        for value in content {
                            parts.extend(flatten_json_text(&value));
                        }
                        let rendered = parts
                            .into_iter()
                            .filter(|part| !part.trim().is_empty())
                            .collect::<Vec<_>>()
                            .join("\n");
                        if !rendered.is_empty() {
                            current_content_parts.push(openai::ContentPart::Text {
                                data: format!("[search result]\n{rendered}"),
                            });
                        }
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
    compat_mode: CompatMode,
) -> ProxyResult<anthropic::AnthropicResponse> {
    let choice = resp
        .choices
        .first()
        .ok_or_else(|| ProxyError::Transform("No choices in response".to_string()))?;

    let mut content = Vec::new();

    let raw_content = choice.message.content.clone().unwrap_or_default();
    let (embedded_reasoning, visible_text) = strip_think_tags(&raw_content);

    if let Some(reasoning) = choice
        .message
        .reasoning_content
        .as_ref()
        .filter(|s| !s.is_empty())
    {
        if !profile.supports_reasoning() && compat_mode.is_strict() {
            return Err(ProxyError::Transform(format!(
                "backend profile {} returned reasoning content that cannot be represented safely",
                profile.as_str()
            )));
        }
        if profile.supports_reasoning() {
            content.push(anthropic::ResponseContent::Thinking {
                thinking: reasoning.clone(),
            });
        }
    }

    if choice.message.reasoning_content.is_none() && !embedded_reasoning.is_empty() && profile.supports_reasoning() {
        for reasoning in embedded_reasoning {
            content.push(anthropic::ResponseContent::Thinking {
                thinking: reasoning,
            });
        }
    }

    if !visible_text.trim().is_empty() {
        content.push(anthropic::ResponseContent::Text { text: visible_text });
    }

    if let Some(tool_calls) = &choice.message.tool_calls {
        for tool_call in tool_calls {
            let input: Value =
                match serde_json::from_str(&tool_call.function.arguments) {
                    Ok(v) => v,
                    Err(err) => {
                        tracing::warn!(
                            tool_id = %tool_call.id,
                            tool_name = %tool_call.function.name,
                            error = %err,
                            "tool_call.arguments is not valid JSON, forwarding as empty object"
                        );
                        json!({})
                    }
                };

            content.push(anthropic::ResponseContent::ToolUse {
                id: tool_call.id.clone(),
                name: tool_call.function.name.clone(),
                input,
            });
        }
    }

    let stop_reason = map_stop_reason(choice.finish_reason.as_deref());

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
            thinking: None,
            output_config: None,
            stop_sequences: Some(vec!["STOP".to_string()]),
            extra: serde_json::Map::new(),
        }
    }

    #[test]
    fn strips_top_k_for_generic_profile() {
        let req = sample_request();
        let transformed = anthropic_to_openai(
            req,
            "model",
            BackendProfile::OpenaiGeneric,
            CompatMode::Strict,
        )
        .unwrap();
        assert_eq!(transformed.top_k, None);
    }

    #[test]
    fn keeps_top_k_for_chutes_profile() {
        let req = sample_request();
        let transformed =
            anthropic_to_openai(req, "model", BackendProfile::Chutes, CompatMode::Strict).unwrap();
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

        let err = anthropic_to_openai(req, "model", BackendProfile::Chutes, CompatMode::Strict)
            .unwrap_err();
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
            thinking: None,
            output_config: None,
            extra: Default::default(),
        };

        let out =
            anthropic_to_openai(req, "model", BackendProfile::Chutes, CompatMode::Strict).unwrap();
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

        let out = openai_to_anthropic(resp, "fallback", BackendProfile::Chutes, CompatMode::Strict)
            .unwrap();
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

        let err = openai_to_anthropic(
            resp,
            "fallback",
            BackendProfile::OpenaiGeneric,
            CompatMode::Strict,
        )
        .unwrap_err();
        assert!(err.to_string().contains("reasoning content"));
    }

    #[test]
    fn compat_mode_downgrades_assistant_thinking_history() {
        let mut req = sample_request();
        req.messages = vec![Message {
            role: "assistant".to_string(),
            content: MessageContent::Blocks(vec![ContentBlock::Thinking {
                thinking: "hidden".to_string(),
            }]),
        }];

        let out = anthropic_to_openai(
            req,
            "model",
            BackendProfile::OpenaiGeneric,
            CompatMode::Compat,
        )
        .unwrap();

        let assistant = out
            .messages
            .iter()
            .find(|message| message.role == "assistant")
            .expect("assistant message");

        match assistant.content.as_ref() {
            Some(openai::MessageContent::Text(_)) | Some(openai::MessageContent::Parts(_)) => {}
            other => panic!("expected downgraded assistant content, got {other:?}"),
        }
    }

    #[test]
    fn compat_mode_degrades_documents_and_search_results_into_text() {
        let req = AnthropicRequest {
            model: "claude".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: MessageContent::Blocks(vec![
                    ContentBlock::Document {
                        source: json!({
                            "type": "url",
                            "url": "https://example.com/file.pdf"
                        }),
                    },
                    ContentBlock::SearchResult {
                        query: Some("weather".to_string()),
                        content: vec![json!({"type": "text", "text": "Sunny and 68F"})],
                    },
                ]),
            }],
            system: None,
            stream: Some(true),
            max_tokens: 64,
            temperature: None,
            top_p: None,
            top_k: None,
            tools: None,
            thinking: None,
            output_config: None,
            stop_sequences: None,
            extra: Default::default(),
        };

        let out = anthropic_to_openai(
            req,
            "model",
            BackendProfile::OpenaiGeneric,
            CompatMode::Compat,
        )
        .unwrap();
        let rendered = serde_json::to_value(&out.messages[0]).unwrap().to_string();
        assert!(rendered.contains("document url"));
        assert!(rendered.contains("Sunny and 68F"));
    }

    #[test]
    fn generic_compat_strips_embedded_think_tags() {
        let resp = openai::OpenAIResponse {
            id: Some("id1".to_string()),
            model: Some("backend".to_string()),
            choices: vec![openai::Choice {
                message: openai::ChoiceMessage {
                    content: Some("<think>hidden chain</think>visible answer".to_string()),
                    tool_calls: None,
                    reasoning_content: None,
                },
                finish_reason: Some("stop".to_string()),
            }],
            usage: openai::Usage {
                prompt_tokens: 10,
                completion_tokens: 5,
            },
        };

        let out = openai_to_anthropic(
            resp,
            "fallback",
            BackendProfile::OpenaiGeneric,
            CompatMode::Compat,
        )
        .unwrap();

        assert_eq!(out.content.len(), 1);
        match &out.content[0] {
            anthropic::ResponseContent::Text { text } => assert_eq!(text, "visible answer"),
            other => panic!("expected visible text only, got {other:?}"),
        }
    }

    #[test]
    fn malformed_tool_call_arguments_falls_back_to_empty_object() {
        let resp = openai::OpenAIResponse {
            id: Some("id1".to_string()),
            model: Some("backend".to_string()),
            choices: vec![openai::Choice {
                message: openai::ChoiceMessage {
                    content: None,
                    tool_calls: Some(vec![openai::ToolCall {
                        id: "call_1".to_string(),
                        call_type: "function".to_string(),
                        function: openai::FunctionCall {
                            name: "weather".to_string(),
                            arguments: "{ invalid json".to_string(),
                        },
                    }]),
                    reasoning_content: None,
                },
                finish_reason: Some("tool_calls".to_string()),
            }],
            usage: openai::Usage {
                prompt_tokens: 5,
                completion_tokens: 3,
            },
        };

        let out = openai_to_anthropic(
            resp,
            "fallback",
            BackendProfile::Chutes,
            CompatMode::Strict,
        )
        .unwrap();

        match &out.content[0] {
            anthropic::ResponseContent::ToolUse { input, .. } => {
                assert_eq!(input, &json!({}));
            }
            other => panic!("expected tool_use block, got {other:?}"),
        }
        assert_eq!(out.stop_reason.as_deref(), Some("tool_use"));
    }

    #[test]
    fn map_stop_reason_covers_all_cases() {
        assert_eq!(map_stop_reason(Some("tool_calls")), Some("tool_use".to_string()));
        assert_eq!(map_stop_reason(Some("stop")), Some("end_turn".to_string()));
        assert_eq!(map_stop_reason(Some("length")), Some("max_tokens".to_string()));
        assert_eq!(map_stop_reason(Some("content_filter")), Some("end_turn".to_string()));
        assert_eq!(map_stop_reason(None), None);
    }
}
