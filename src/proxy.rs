use crate::config::{BackendProfile, CompatMode};
use crate::error::{ProxyError, ProxyResult};
use crate::model_cache;
use crate::models::{anthropic, openai, responses};
use crate::rate_limiter::SharedRateLimiter;
use crate::tool_names::ToolNameMap;
use crate::transform::{self, generate_message_id};
use axum::{
    body::Body,
    http::{header, HeaderMap, HeaderName, HeaderValue},
    response::{IntoResponse, Response},
    Extension, Json,
};
use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use reqwest::Client;
use serde_json::json;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;
use tokio::pin;
use tower_http::cors::{AllowOrigin, CorsLayer};

fn redact_secrets(input: &str) -> String {
    let mut result = input.to_string();
    result = redact_pattern(&result, "Bearer ", 8);
    result = redact_pattern(&result, "bearer ", 8);
    result = redact_pattern(&result, "x-api-key: ", 8);
    result = redact_pattern(&result, "x-api-key=", 8);
    for prefix in &["sk-", "sk_", "cpk_"] {
        result = redact_pattern(&result, prefix, 20);
    }
    if result.len() > 2048 {
        result.truncate(2048);
        result.push_str("… [truncated]");
    }
    result
}

fn redact_pattern(input: &str, prefix: &str, min_token_len: usize) -> String {
    let mut result = input.to_string();
    let search_from_pos = |s: &str, start: usize, needle: &str| -> Option<usize> {
        s[start..].find(needle).map(|p| start + p)
    };
    let mut offset = 0;
    while let Some(pos) = search_from_pos(&result, offset, prefix) {
        let token_start = pos + prefix.len();
        let token_end = result[token_start..]
            .find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == '}' || c == ',')
            .map(|i| token_start + i)
            .unwrap_or(result.len());
        if token_end - token_start >= min_token_len {
            result.replace_range(token_start..token_end, "***");
            offset = token_start + 3;
        } else {
            offset = token_start;
        }
        if offset >= result.len() {
            break;
        }
    }
    result
}

fn extract_client_key(headers: &HeaderMap) -> Option<String> {
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let x_api_key = headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    bearer.or(x_api_key)
}

fn resolve_backend_key(client_key: Option<&str>, config: &Config) -> Option<String> {
    if let (Some(client), Some(ingress)) = (client_key, config.ingress_api_key.as_deref()) {
        if client == ingress {
            return config.api_key.clone();
        }
    }

    client_key
        .map(|s| s.to_string())
        .or_else(|| config.api_key.clone())
}

fn map_model(client_model: &str, config: &Config) -> String {
    match client_model {
        m if m.is_empty() || m == "default" => config.primary_model.clone(),
        m if m.starts_with("claude-") => config.primary_model.clone(),
        other => other.to_string(),
    }
}

fn request_has_thinking(req: &anthropic::AnthropicRequest) -> bool {
    if let Some(thinking) = &req.thinking {
        return !thinking.thinking_type.eq_ignore_ascii_case("disabled");
    }

    req.extra
        .get("thinking")
        .and_then(|value| value.get("type").and_then(|type_value| type_value.as_str()))
        .map(|value| !value.eq_ignore_ascii_case("disabled"))
        .is_some()
}

fn responses_request_has_reasoning(req: &responses::ResponsesRequest) -> bool {
    req.reasoning
        .as_ref()
        .and_then(|value| value.get("effort").and_then(|v| v.as_str()).or_else(|| value.get("summary").and_then(|v| v.as_str())))
        .is_some()
}

fn build_tool_name_map<'a>(
    names: impl IntoIterator<Item = &'a str>,
    profile: BackendProfile,
) -> ToolNameMap {
    match profile.max_tool_name_len() {
        Some(limit) => ToolNameMap::from_names(names, limit),
        None => ToolNameMap::identity(),
    }
}

fn anthropic_tool_name_map(
    req: &anthropic::AnthropicRequest,
    profile: BackendProfile,
) -> ToolNameMap {
    build_tool_name_map(
        req.tools
            .as_ref()
            .into_iter()
            .flatten()
            .map(|tool| tool.name.as_str()),
        profile,
    )
}

fn responses_tool_name_map(
    req: &responses::ResponsesRequest,
    profile: BackendProfile,
) -> ToolNameMap {
    build_tool_name_map(
        req.tools
            .as_ref()
            .into_iter()
            .flatten()
            .filter(|tool| tool.get("type").and_then(|v| v.as_str()) == Some("function"))
            .filter_map(|tool| tool.get("name").and_then(|v| v.as_str())),
        profile,
    )
}

fn validate_deepseek_tool_name(name: &str, path: String) -> ProxyResult<()> {
    if name.trim().is_empty() {
        return Err(ProxyError::Upstream(format!(
            "invalid DeepSeek tool payload: {path} is empty"
        )));
    }
    if name.len() > 64 {
        return Err(ProxyError::Upstream(format!(
            "invalid DeepSeek tool payload: {path} exceeds 64 characters"
        )));
    }
    Ok(())
}

fn validate_deepseek_request(
    profile: BackendProfile,
    request: &openai::OpenAIRequest,
) -> ProxyResult<()> {
    if profile != BackendProfile::Deepseek {
        return Ok(());
    }

    if let Some(tools) = &request.tools {
        for (index, tool) in tools.iter().enumerate() {
            if tool.tool_type != "function" {
                return Err(ProxyError::Upstream(format!(
                    "unsupported DeepSeek tool at index {index}: expected type=function"
                )));
            }
            validate_deepseek_tool_name(
                &tool.function.name,
                format!("tools[{index}].function.name"),
            )?;
        }
    }

    if let Some(choice) = &request.tool_choice {
        if let openai::ToolChoice::Object { function, .. } = choice {
            validate_deepseek_tool_name(
                &function.name,
                "tool_choice.function.name".to_string(),
            )?;
        }
    }

    for (msg_index, message) in request.messages.iter().enumerate() {
        if let Some(tool_calls) = &message.tool_calls {
            for (tool_index, call) in tool_calls.iter().enumerate() {
                validate_deepseek_tool_name(
                    &call.function.name,
                    format!("messages[{msg_index}].tool_calls[{tool_index}].function.name"),
                )?;
            }
        }
    }

    Ok(())
}

fn models_response_json(models: &[model_cache::ModelInfo]) -> serde_json::Value {
    json!({
        "object": "list",
        "data": models
            .iter()
            .map(|model| json!({"id": model.id, "object": "model", "owned_by": "anthmorph"}))
            .collect::<Vec<_>>()
    })
}

pub async fn proxy_handler(
    headers: HeaderMap,
    Extension(config): Extension<Arc<Config>>,
    Extension(client): Extension<Client>,
    Extension(models_cache): Extension<model_cache::Cache>,
    Extension(rate_limiter): Extension<Option<SharedRateLimiter>>,
    Json(req): Json<anthropic::AnthropicRequest>,
) -> ProxyResult<Response> {
    authorize_request(&headers, &config)?;

    if let Some(limiter) = &rate_limiter {
        let client_key = extract_client_key(&headers)
            .unwrap_or_else(|| "anonymous".to_string());
        if !limiter.check(&client_key).await {
            return Err(ProxyError::Upstream(
                "429 rate limit exceeded".to_string(),
            ));
        }
    }

    let is_streaming = req.stream.unwrap_or(false);

    tracing::debug!("Received request for model: {}", req.model);
    tracing::debug!("Messages count: {}", req.messages.len());
    for (i, msg) in req.messages.iter().enumerate() {
        let content_type = match &msg.content {
            anthropic::MessageContent::Text(_) => "Text",
            anthropic::MessageContent::Blocks(blocks) => {
                if blocks.is_empty() {
                    "empty_blocks"
                } else {
                    match &blocks[0] {
                        anthropic::ContentBlock::Text { .. } => "text_block",
                        anthropic::ContentBlock::Image { .. } => "image_block",
                        anthropic::ContentBlock::Document { .. } => "document_block",
                        anthropic::ContentBlock::ToolUse { .. } => "tool_use_block",
                        anthropic::ContentBlock::ToolResult { .. } => "tool_result_block",
                        anthropic::ContentBlock::Thinking { .. } => "thinking_block",
                        anthropic::ContentBlock::ServerToolUse { .. } => "server_tool_use_block",
                        anthropic::ContentBlock::SearchResult { .. } => "search_result_block",
                        anthropic::ContentBlock::Other => "unknown_block",
                    }
                }
            }
        };
        tracing::debug!("Message {}: role={}, content={}", i, msg.role, content_type);
    }
    tracing::debug!("Streaming: {}", is_streaming);

    let tool_name_map = anthropic_tool_name_map(&req, config.backend_profile);

    let model = if request_has_thinking(&req) {
        config
            .reasoning_model
            .clone()
            .unwrap_or_else(|| config.primary_model.clone())
    } else {
        let mapped = map_model(&req.model, &config);
        model_cache::normalize_model(&mapped, &models_cache).await
    };

    let openai_req =
        transform::anthropic_to_openai(
            req,
            &model,
            config.backend_profile,
            config.compat_mode,
            &tool_name_map,
        )?;
    validate_deepseek_request(config.backend_profile, &openai_req)?;

    let client_key = extract_client_key(&headers);
    let backend_key = resolve_backend_key(client_key.as_deref(), &config);

    if is_streaming {
        handle_streaming(config, client, openai_req, backend_key, tool_name_map).await
    } else {
        handle_non_streaming(config, client, openai_req, backend_key, tool_name_map).await
    }
}

pub async fn count_tokens_handler(
    Extension(config): Extension<Arc<Config>>,
    Json(req): Json<anthropic::AnthropicRequest>,
) -> ProxyResult<Json<anthropic::CountTokensResponse>> {
    let model = if request_has_thinking(&req) {
        config
            .reasoning_model
            .clone()
            .unwrap_or_else(|| config.primary_model.clone())
    } else {
        map_model(&req.model, &config)
    };
    let tool_name_map = anthropic_tool_name_map(&req, config.backend_profile);
    let openai_req =
        transform::anthropic_to_openai(
            req,
            &model,
            config.backend_profile,
            config.compat_mode,
            &tool_name_map,
        )?;
    let serialized = serde_json::to_string(&openai_req)?;
    let estimated = std::cmp::max(1, serialized.chars().count() / 4);
    Ok(Json(anthropic::CountTokensResponse {
        input_tokens: estimated,
    }))
}

pub async fn responses_handler(
    headers: HeaderMap,
    Extension(config): Extension<Arc<Config>>,
    Extension(client): Extension<Client>,
    Extension(models_cache): Extension<model_cache::Cache>,
    Extension(rate_limiter): Extension<Option<SharedRateLimiter>>,
    Json(req): Json<responses::ResponsesRequest>,
) -> ProxyResult<Response> {
    authorize_request(&headers, &config)?;

    if let Some(limiter) = &rate_limiter {
        let client_key = extract_client_key(&headers).unwrap_or_else(|| "anonymous".to_string());
        if !limiter.check(&client_key).await {
            return Err(ProxyError::Upstream("429 rate limit exceeded".to_string()));
        }
    }

    let tool_name_map = responses_tool_name_map(&req, config.backend_profile);
    let wants_stream = req.stream.unwrap_or(false);
    let model = if responses_request_has_reasoning(&req) {
        config
            .reasoning_model
            .clone()
            .unwrap_or_else(|| config.primary_model.clone())
    } else {
        let mapped = map_model(&req.model, &config);
        model_cache::normalize_model(&mapped, &models_cache).await
    };

    let openai_req = responses_to_openai(
        &req,
        &model,
        config.backend_profile,
        &tool_name_map,
    )?;
    validate_deepseek_request(config.backend_profile, &openai_req)?;

    let client_key = extract_client_key(&headers);
    let backend_key = resolve_backend_key(client_key.as_deref(), &config);

    if wants_stream {
        handle_responses_streaming(config, client, openai_req, backend_key, tool_name_map).await
    } else {
        handle_responses_non_streaming(config, client, openai_req, backend_key, tool_name_map)
            .await
    }
}

pub async fn models_handler(
    headers: HeaderMap,
    Extension(config): Extension<Arc<Config>>,
    Extension(client): Extension<Client>,
    Extension(models_cache): Extension<model_cache::Cache>,
) -> ProxyResult<Response> {
    let url = config.models_url();
    let mut req_builder = client.get(&url).timeout(Duration::from_secs(60));

    let client_key = extract_client_key(&headers);
    let backend_key = resolve_backend_key(client_key.as_deref(), &config);
    if let Some(api_key) = &backend_key {
        req_builder = req_builder.header("Authorization", format!("Bearer {}", api_key));
    }

    let response = req_builder.send().await.map_err(ProxyError::Http)?;
    let status = response.status();
    let body = response.bytes().await.map_err(ProxyError::Http)?;

    if !status.is_success() {
        tracing::warn!(
            "upstream models lookup failed ({}), serving local registry",
            status
        );
        let fallback = model_cache::snapshot(&models_cache).await;
        return Ok(Json(models_response_json(&fallback)).into_response());
    }

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    Ok((headers, body).into_response())
}

async fn handle_non_streaming(
    config: Arc<Config>,
    client: Client,
    openai_req: openai::OpenAIRequest,
    backend_key: Option<String>,
    tool_name_map: ToolNameMap,
) -> ProxyResult<Response> {
    let url = config.chat_completions_url();
    tracing::debug!("Sending non-streaming request to {}", url);

    let mut req_builder = client
        .post(&url)
        .json(&openai_req)
        .timeout(Duration::from_secs(300));

    if let Some(api_key) = &backend_key {
        req_builder = req_builder.header("Authorization", format!("Bearer {}", api_key));
    }

    let response = req_builder.send().await.map_err(|err| {
        tracing::error!("Failed to send request to {}: {:?}", url, err);
        ProxyError::Http(err)
    })?;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        tracing::error!("Upstream error ({}): {}", status, redact_secrets(&error_text));
        return Err(ProxyError::Upstream(format!(
            "Upstream returned {}: {}",
            status, error_text
        )));
    }

    let openai_resp: openai::OpenAIResponse = response.json().await?;
    let anthropic_resp = transform::openai_to_anthropic(
        openai_resp,
        &openai_req.model,
        config.backend_profile,
        config.compat_mode,
        &tool_name_map,
    )?;

    Ok(Json(anthropic_resp).into_response())
}

async fn handle_streaming(
    config: Arc<Config>,
    client: Client,
    openai_req: openai::OpenAIRequest,
    backend_key: Option<String>,
    tool_name_map: ToolNameMap,
) -> ProxyResult<Response> {
    let url = config.chat_completions_url();
    tracing::debug!("Sending streaming request to {}", url);

    let mut req_builder = client
        .post(&url)
        .json(&openai_req)
        .timeout(Duration::from_secs(300));

    if let Some(api_key) = &backend_key {
        req_builder = req_builder.header("Authorization", format!("Bearer {}", api_key));
    }

    let response = req_builder.send().await.map_err(|err| {
        tracing::error!("Failed to send streaming request: {:?}", err);
        ProxyError::Http(err)
    })?;

    if !response.status().is_success() {
        let status = response.status();
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        tracing::error!("Upstream streaming error ({}): {}", status, redact_secrets(&error_text));
        return Err(ProxyError::Upstream(format!(
            "Upstream returned {}: {}",
            status, error_text
        )));
    }

    let stream = response.bytes_stream();
    let sse_stream = create_sse_stream(
        stream,
        openai_req.model.clone(),
        config.backend_profile,
        config.compat_mode,
        config.stream_chunk_timeout_secs,
        tool_name_map,
    );

    let mut headers = HeaderMap::new();
    headers.insert(
        "Content-Type",
        HeaderValue::from_static("text/event-stream"),
    );
    headers.insert("Cache-Control", HeaderValue::from_static("no-cache"));
    headers.insert("Connection", HeaderValue::from_static("keep-alive"));

    Ok((headers, Body::from_stream(sse_stream)).into_response())
}

fn create_sse_stream(
    stream: impl Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
    fallback_model: String,
    profile: BackendProfile,
    compat_mode: CompatMode,
    chunk_timeout_secs: u64,
    tool_name_map: ToolNameMap,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let mut buffer = String::new();
        let mut message_id = None;
        let mut current_model = None;
        let mut next_content_index = 0usize;
        let mut has_sent_message_start = false;
        let mut has_sent_message_delta = false;
        let mut has_sent_message_stop = false;
        let mut active_block: Option<ActiveBlock> = None;
        let mut tool_states: BTreeMap<usize, ToolCallState> = BTreeMap::new();
        let mut think_filter = ThinkTagStreamFilter::default();

        pin!(stream);

        let mut raw_buffer: Vec<u8> = Vec::new();
        let chunk_timeout = Duration::from_secs(chunk_timeout_secs);

        loop {
            let chunk_result = tokio::time::timeout(chunk_timeout, stream.next());
            match chunk_result.await {
                Ok(Some(chunk)) => match chunk {
                    Ok(bytes) => {
                        raw_buffer.extend_from_slice(&bytes);

                        loop {
                            match std::str::from_utf8(&raw_buffer) {
                                Ok(text) => {
                                    buffer.push_str(&text.replace("\r\n", "\n"));
                                    raw_buffer.clear();
                                    break;
                                }
                                Err(e) => {
                                    let valid_up_to = e.valid_up_to();
                                    if valid_up_to > 0 {
                                        let partial = std::str::from_utf8(&raw_buffer[..valid_up_to]).unwrap();
                                        buffer.push_str(&partial.replace("\r\n", "\n"));
                                        raw_buffer = raw_buffer[valid_up_to..].to_vec();
                                    }
                                    if raw_buffer.is_empty() || valid_up_to == 0 {
                                        break;
                                    }
                                }
                            }
                        }

                        while let Some(pos) = buffer.find("\n\n") {
                            let event_block = buffer[..pos].to_string();
                            buffer = buffer[pos + 2..].to_string();

                            if event_block.trim().is_empty() {
                                continue;
                            }

                            let Some(data) = extract_sse_data(&event_block) else {
                                continue;
                            };

                            if data.trim() == "[DONE]" {
                                if let Some(previous) = active_block.take() {
                                    yield Ok(Bytes::from(stop_block_sse(previous.index())));
                                }
                                if has_sent_message_start && !has_sent_message_delta {
                                    let event = anthropic::StreamEvent::MessageDelta {
                                        delta: anthropic::MessageDeltaData {
                                            stop_reason: Some("end_turn".to_string()),
                                            stop_sequence: (),
                                        },
                                        usage: None,
                                    };
                                    yield Ok(Bytes::from(sse_event("message_delta", &event)));
                                    has_sent_message_delta = true;
                                }
                                if has_sent_message_start && !has_sent_message_stop {
                                    yield Ok(Bytes::from(message_stop_sse()));
                                    has_sent_message_stop = true;
                                }
                                continue;
                            }

                            if let Ok(chunk) = serde_json::from_str::<openai::StreamChunk>(&data) {
                                if message_id.is_none() {
                                    if let Some(id) = &chunk.id {
                                        message_id = Some(id.clone());
                                    }
                                }
                                if current_model.is_none() {
                                    if let Some(model) = &chunk.model {
                                        current_model = Some(model.clone());
                                    }
                                }

                                if let Some(choice) = chunk.choices.first() {
                                    if !has_sent_message_start {
                                        let event = anthropic::StreamEvent::MessageStart {
                                            message: anthropic::MessageStartData {
                                                id: message_id.clone().unwrap_or_else(generate_message_id),
                                                message_type: "message".to_string(),
                                                role: "assistant".to_string(),
                                                content: vec![],
                                                model: current_model
                                                    .clone()
                                                    .unwrap_or_else(|| fallback_model.clone()),
                                                stop_reason: None,
                                                stop_sequence: None,
                                                usage: anthropic::Usage {
                                                    input_tokens: 0,
                                                    output_tokens: 0,
                                                },
                                            },
                                        };
                                        yield Ok(Bytes::from(sse_event("message_start", &event)));
                                        has_sent_message_start = true;
                                    }

                                    if let Some(reasoning) = &choice.delta.reasoning {
                                        if !reasoning.is_empty() {
                                            if !profile.supports_reasoning() && compat_mode.is_strict() {
                                                yield Ok(Bytes::from(stream_error_sse(
                                                    "reasoning deltas are not supported by the active backend profile",
                                                )));
                                                break;
                                            }

                                            if profile.supports_reasoning() {
                                                let (idx, transitions) = transition_to_thinking(
                                                    &mut active_block,
                                                    &mut next_content_index,
                                                );
                                                for event in transitions {
                                                    yield Ok(Bytes::from(event));
                                                }
                                                yield Ok(Bytes::from(delta_block_sse(
                                                    idx,
                                                    anthropic::ContentBlockDeltaData::ThinkingDelta {
                                                        thinking: reasoning.clone(),
                                                    },
                                                )));
                                            }
                                        }
                                    }

                                    if let Some(content) = &choice.delta.content {
                                        if !content.is_empty() {
                                            let (embedded_reasoning, visible_text) = think_filter.push(content);

                                            if profile.supports_reasoning() {
                                                for reasoning in embedded_reasoning {
                                                    let (idx, transitions) = transition_to_thinking(
                                                        &mut active_block,
                                                        &mut next_content_index,
                                                    );
                                                    for event in transitions {
                                                        yield Ok(Bytes::from(event));
                                                    }
                                                    yield Ok(Bytes::from(delta_block_sse(
                                                        idx,
                                                        anthropic::ContentBlockDeltaData::ThinkingDelta {
                                                            thinking: reasoning,
                                                        },
                                                    )));
                                                }
                                            }

                                            if !visible_text.is_empty() {
                                                let (idx, transitions) = transition_to_text(
                                                    &mut active_block,
                                                    &mut next_content_index,
                                                );
                                                for event in transitions {
                                                    yield Ok(Bytes::from(event));
                                                }
                                                yield Ok(Bytes::from(delta_block_sse(
                                                    idx,
                                                    anthropic::ContentBlockDeltaData::TextDelta {
                                                        text: visible_text,
                                                    },
                                                )));
                                            }
                                        }
                                    }

                                    if let Some(tool_calls) = &choice.delta.tool_calls {
                                        for tool_call in tool_calls {
                                            let tool_index = tool_call.index.unwrap_or(0);
                                            let state = tool_states.entry(tool_index).or_default();

                                            if let Some(id) = &tool_call.id {
                                                state.id = Some(id.clone());
                                            }
                                            if let Some(function) = &tool_call.function {
                                                if let Some(name) = &function.name {
                                                    state.name = Some(name.clone());
                                                }
                                            }

                                            if state.content_index.is_none() {
                                                if let (Some(id), Some(name)) = (state.id.clone(), state.name.clone()) {
                                                    let (idx, transitions) = transition_to_tool(
                                                        &mut active_block,
                                                        &mut next_content_index,
                                                        tool_index,
                                                        id,
                                                        tool_name_map.to_client(&name),
                                                    );
                                                    state.content_index = Some(idx);
                                                    for event in transitions {
                                                        yield Ok(Bytes::from(event));
                                                    }
                                                }
                                            } else if active_block != Some(ActiveBlock::ToolUse(tool_index, state.content_index.unwrap())) {
                                                if !compat_mode.is_strict() {
                                                    continue;
                                                }
                                                yield Ok(Bytes::from(stream_error_sse(
                                                    "interleaved tool call deltas are not supported safely",
                                                )));
                                                break;
                                            }

                                            if let Some(function) = &tool_call.function {
                                                if let Some(arguments) = &function.arguments {
                                                    if let Some(idx) = state.content_index {
                                                        yield Ok(Bytes::from(delta_block_sse(
                                                            idx,
                                                            anthropic::ContentBlockDeltaData::InputJsonDelta {
                                                                partial_json: arguments.clone(),
                                                            },
                                                        )));
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    if let Some(finish_reason) = &choice.finish_reason {
                                        if let Some(previous) = active_block.take() {
                                            yield Ok(Bytes::from(stop_block_sse(previous.index())));
                                        }

                                        let event = anthropic::StreamEvent::MessageDelta {
                                            delta: anthropic::MessageDeltaData {
                                                stop_reason: transform::map_stop_reason(Some(finish_reason)),
                                                stop_sequence: (),
                                            },
                                            usage: chunk.usage.as_ref().and_then(|u| {
                                                u.completion_tokens.map(|tokens| anthropic::MessageDeltaUsage {
                                                    output_tokens: tokens,
                                                })
                                            }),
                                        };
                                        yield Ok(Bytes::from(sse_event("message_delta", &event)));
                                        has_sent_message_delta = true;
                                        if !has_sent_message_stop {
                                            yield Ok(Bytes::from(message_stop_sse()));
                                            has_sent_message_stop = true;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Stream error: {}", e);
                        yield Ok(Bytes::from(stream_error_sse(&format!("Stream error: {}", e))));
                        break;
                    }
                },
                Ok(None) => break,
                Err(_) => {
                    tracing::warn!("Stream chunk timeout ({}s), closing stream", chunk_timeout_secs);
                    yield Ok(Bytes::from(stream_error_sse("stream chunk timeout")));
                    break;
                }
            }
        }

        let (embedded_reasoning, visible_tail) = think_filter.finish();
        if profile.supports_reasoning() {
            for reasoning in embedded_reasoning {
                let (idx, transitions) =
                    transition_to_thinking(&mut active_block, &mut next_content_index);
                for event in transitions {
                    yield Ok(Bytes::from(event));
                }
                yield Ok(Bytes::from(delta_block_sse(
                    idx,
                    anthropic::ContentBlockDeltaData::ThinkingDelta { thinking: reasoning },
                )));
            }
        }
        if !visible_tail.is_empty() {
            let (idx, transitions) = transition_to_text(&mut active_block, &mut next_content_index);
            for event in transitions {
                yield Ok(Bytes::from(event));
            }
            yield Ok(Bytes::from(delta_block_sse(
                idx,
                anthropic::ContentBlockDeltaData::TextDelta { text: visible_tail },
            )));
        }
        if let Some(previous) = active_block.take() {
            yield Ok(Bytes::from(stop_block_sse(previous.index())));
        }
        if has_sent_message_start && !has_sent_message_delta {
            let event = anthropic::StreamEvent::MessageDelta {
                delta: anthropic::MessageDeltaData {
                    stop_reason: Some("end_turn".to_string()),
                    stop_sequence: (),
                },
                usage: None,
            };
            yield Ok(Bytes::from(sse_event("message_delta", &event)));
        }
        if has_sent_message_start && !has_sent_message_stop {
            yield Ok(Bytes::from(message_stop_sse()));
        }
    }
}

fn responses_to_openai(
    req: &responses::ResponsesRequest,
    model: &str,
    profile: BackendProfile,
    tool_name_map: &ToolNameMap,
) -> ProxyResult<openai::OpenAIRequest> {
    let mut messages = Vec::new();

    if let Some(instructions) = req.instructions.as_ref().filter(|value| !value.trim().is_empty()) {
        messages.push(openai::Message {
            role: "system".to_string(),
            content: Some(openai::MessageContent::Text(instructions.clone())),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        });
    }

    for item in &req.input {
        let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
        match item_type {
            "message" => {
                let role = item
                    .get("role")
                    .and_then(|v| v.as_str())
                    .unwrap_or("user")
                    .to_string();
                let mut parts = Vec::new();
                if let Some(content) = item.get("content").and_then(|v| v.as_array()) {
                    for span in content {
                        match span.get("type").and_then(|v| v.as_str()).unwrap_or("") {
                            "input_text" | "output_text" | "text" => {
                                if let Some(text) = span.get("text").and_then(|v| v.as_str()) {
                                    parts.push(openai::ContentPart::Text {
                                        data: text.to_string(),
                                    });
                                }
                            }
                            "input_image" => {
                                if let Some(url) = span.get("image_url").and_then(|v| v.as_str()) {
                                    parts.push(openai::ContentPart::ImageUrl {
                                        image_url: openai::ImageUrl {
                                            url: url.to_string(),
                                        },
                                    });
                                }
                            }
                            _ => {}
                        }
                    }
                }
                let content = if parts.is_empty() {
                    None
                } else if parts.len() == 1 {
                    match &parts[0] {
                        openai::ContentPart::Text { data } => {
                            Some(openai::MessageContent::Text(data.clone()))
                        }
                        _ => Some(openai::MessageContent::Parts(parts)),
                    }
                } else {
                    Some(openai::MessageContent::Parts(parts))
                };
                messages.push(openai::Message {
                    role,
                    content,
                    name: None,
                    tool_calls: None,
                    tool_call_id: None,
                });
            }
            "function_call" => {
                let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("tool");
                let call_id = item
                    .get("call_id")
                    .or_else(|| item.get("id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("call_1");
                let arguments = item
                    .get("arguments")
                    .map(|v| {
                        if let Some(text) = v.as_str() {
                            text.to_string()
                        } else {
                            serde_json::to_string(v).unwrap_or_else(|_| "{}".to_string())
                        }
                    })
                    .unwrap_or_else(|| "{}".to_string());
                messages.push(openai::Message {
                    role: "assistant".to_string(),
                    content: None,
                    name: None,
                    tool_calls: Some(vec![openai::ToolCall {
                        id: call_id.to_string(),
                        call_type: "function".to_string(),
                        function: openai::FunctionCall {
                            name: tool_name_map.to_backend(name),
                            arguments,
                        },
                    }]),
                    tool_call_id: None,
                });
            }
            "function_call_output" | "custom_tool_call_output" => {
                let call_id = item
                    .get("call_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("call_1")
                    .to_string();
                let output = item
                    .get("output")
                    .map(|v| {
                        if let Some(text) = v.as_str() {
                            text.to_string()
                        } else {
                            serde_json::to_string(v).unwrap_or_default()
                        }
                    })
                    .unwrap_or_default();
                messages.push(openai::Message {
                    role: "tool".to_string(),
                    content: Some(openai::MessageContent::Text(output)),
                    name: None,
                    tool_calls: None,
                    tool_call_id: Some(call_id),
                });
            }
            _ => {}
        }
    }

    let tools = req.tools.as_ref().map(|tools| {
        tools.iter()
            .filter(|tool| tool.get("type").and_then(|v| v.as_str()) == Some("function"))
            .filter_map(|tool| {
                let name = tool.get("name").and_then(|v| v.as_str())?;
                let parameters = tool.get("parameters").cloned().or_else(|| tool.get("input_schema").cloned());
                Some(openai::Tool {
                    tool_type: "function".to_string(),
                    function: openai::Function {
                        name: tool_name_map.to_backend(name),
                        description: tool
                            .get("description")
                            .and_then(|v| v.as_str())
                            .map(str::to_string),
                        parameters: parameters
                            .unwrap_or_else(|| json!({"type":"object","properties":{}})),
                    },
                })
            })
            .collect::<Vec<_>>()
    });

    let tool_choice = req.tool_choice.as_ref().and_then(|choice| {
        if let Some(text) = choice.as_str() {
            return Some(openai::ToolChoice::String(text.to_string()));
        }
        let choice_type = choice.get("type").and_then(|v| v.as_str()).unwrap_or("");
        match choice_type {
            "auto" | "none" | "required" => Some(openai::ToolChoice::String(choice_type.to_string())),
            "function" => choice.get("name").and_then(|v| v.as_str()).map(|name| {
                openai::ToolChoice::Object {
                    tool_type: "function".to_string(),
                    function: openai::ToolChoiceFunction {
                        name: tool_name_map.to_backend(name),
                    },
                }
            }),
            _ => None,
        }
    });

    Ok(openai::OpenAIRequest {
        model: model.to_string(),
        messages,
        max_tokens: None,
        temperature: None,
        top_p: None,
        top_k: if profile.supports_top_k() { Some(40) } else { None },
        stop: None,
        stream: req.stream,
        tools,
        tool_choice,
    })
}

async fn handle_responses_non_streaming(
    config: Arc<Config>,
    client: Client,
    openai_req: openai::OpenAIRequest,
    backend_key: Option<String>,
    tool_name_map: ToolNameMap,
) -> ProxyResult<Response> {
    let url = config.chat_completions_url();
    let mut req_builder = client.post(&url).json(&openai_req).timeout(Duration::from_secs(300));
    if let Some(api_key) = &backend_key {
        req_builder = req_builder.header("Authorization", format!("Bearer {}", api_key));
    }
    let response = req_builder.send().await.map_err(ProxyError::Http)?;
    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
        return Err(ProxyError::Upstream(format!("Upstream returned {}: {}", status, error_text)));
    }
    let openai_resp: openai::OpenAIResponse = response.json().await?;
    let response_id = openai_resp.id.clone().unwrap_or_else(generate_message_id);
    let model = openai_resp
        .model
        .clone()
        .unwrap_or_else(|| openai_req.model.clone());
    let mut output = Vec::new();
    if let Some(choice) = openai_resp.choices.first() {
        if let Some(text) = choice.message.content.as_ref().filter(|text| !text.is_empty()) {
            output.push(json!({
                "type": "message",
                "role": "assistant",
                "content": [{"type": "output_text", "text": text}],
            }));
        }
        if let Some(tool_calls) = &choice.message.tool_calls {
            for tool_call in tool_calls {
                output.push(json!({
                    "type": "function_call",
                    "call_id": tool_call.id,
                    "name": tool_name_map.to_client(&tool_call.function.name),
                    "arguments": tool_call.function.arguments,
                }));
            }
        }
    }
    let envelope = responses::ResponsesEnvelope {
        id: response_id,
        object: "response".to_string(),
        model,
        output,
        usage: Some(json!({
            "input_tokens": openai_resp.usage.prompt_tokens,
            "output_tokens": openai_resp.usage.completion_tokens,
        })),
    };
    Ok(Json(envelope).into_response())
}

async fn handle_responses_streaming(
    config: Arc<Config>,
    client: Client,
    openai_req: openai::OpenAIRequest,
    backend_key: Option<String>,
    tool_name_map: ToolNameMap,
) -> ProxyResult<Response> {
    let url = config.chat_completions_url();
    let mut req_builder = client.post(&url).json(&openai_req).timeout(Duration::from_secs(300));
    if let Some(api_key) = &backend_key {
        req_builder = req_builder.header("Authorization", format!("Bearer {}", api_key));
    }
    let response = req_builder.send().await.map_err(ProxyError::Http)?;
    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
        return Err(ProxyError::Upstream(format!("Upstream returned {}: {}", status, error_text)));
    }

    let stream = response.bytes_stream();
    let sse_stream =
        create_responses_sse_stream(stream, openai_req.model.clone(), config.stream_chunk_timeout_secs, tool_name_map);

    let mut headers = HeaderMap::new();
    headers.insert("Content-Type", HeaderValue::from_static("text/event-stream"));
    headers.insert("Cache-Control", HeaderValue::from_static("no-cache"));
    headers.insert("Connection", HeaderValue::from_static("keep-alive"));
    Ok((headers, Body::from_stream(sse_stream)).into_response())
}

fn create_responses_sse_stream(
    stream: impl Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
    fallback_model: String,
    chunk_timeout_secs: u64,
    tool_name_map: ToolNameMap,
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    #[derive(Default)]
    struct FunctionState {
        id: Option<String>,
        name: Option<String>,
        arguments: String,
        started: bool,
    }

    async_stream::stream! {
        let mut buffer = String::new();
        let mut response_id: Option<String> = None;
        let mut model_name: Option<String> = None;
        let mut created_sent = false;
        let mut message_started = false;
        let mut message_text = String::new();
        let mut functions: BTreeMap<usize, FunctionState> = BTreeMap::new();
        pin!(stream);
        let mut raw_buffer: Vec<u8> = Vec::new();
        let chunk_timeout = Duration::from_secs(chunk_timeout_secs);

        loop {
            let chunk_result = tokio::time::timeout(chunk_timeout, stream.next());
            match chunk_result.await {
                Ok(Some(chunk)) => match chunk {
                    Ok(bytes) => {
                        raw_buffer.extend_from_slice(&bytes);
                        loop {
                            match std::str::from_utf8(&raw_buffer) {
                                Ok(text) => {
                                    buffer.push_str(&text.replace("\r\n", "\n"));
                                    raw_buffer.clear();
                                    break;
                                }
                                Err(e) => {
                                    let valid_up_to = e.valid_up_to();
                                    if valid_up_to > 0 {
                                        let partial = std::str::from_utf8(&raw_buffer[..valid_up_to]).unwrap();
                                        buffer.push_str(&partial.replace("\r\n", "\n"));
                                        raw_buffer = raw_buffer[valid_up_to..].to_vec();
                                    }
                                    if raw_buffer.is_empty() || valid_up_to == 0 {
                                        break;
                                    }
                                }
                            }
                        }

                        while let Some(pos) = buffer.find("\n\n") {
                            let event_block = buffer[..pos].to_string();
                            buffer = buffer[pos + 2..].to_string();
                            if event_block.trim().is_empty() {
                                continue;
                            }
                            let Some(data) = extract_sse_data(&event_block) else {
                                continue;
                            };
                            if data.trim() == "[DONE]" {
                                continue;
                            }
                            if let Ok(chunk) = serde_json::from_str::<openai::StreamChunk>(&data) {
                                if response_id.is_none() {
                                    response_id = chunk.id.clone().or_else(|| Some(generate_message_id()));
                                }
                                if model_name.is_none() {
                                    model_name = chunk.model.clone().or_else(|| Some(fallback_model.clone()));
                                }
                                if !created_sent {
                                    let created = json!({
                                        "type": "response.created",
                                        "response": {
                                            "id": response_id.clone().unwrap_or_else(generate_message_id),
                                            "model": model_name.clone().unwrap_or_else(|| fallback_model.clone())
                                        }
                                    });
                                    yield Ok(Bytes::from(sse_event("response.created", &created)));
                                    created_sent = true;
                                }

                                if let Some(choice) = chunk.choices.first() {
                                    if let Some(content) = &choice.delta.content {
                                        if !content.is_empty() {
                                            if !message_started {
                                                let item = json!({
                                                    "type": "message",
                                                    "role": "assistant",
                                                    "content": [{"type": "output_text", "text": ""}]
                                                });
                                                yield Ok(Bytes::from(sse_event("response.output_item.added", &json!({
                                                    "type": "response.output_item.added",
                                                    "item": item,
                                                }))));
                                                message_started = true;
                                            }
                                            message_text.push_str(content);
                                            yield Ok(Bytes::from(sse_event("response.output_text.delta", &json!({
                                                "type": "response.output_text.delta",
                                                "delta": content,
                                            }))));
                                        }
                                    }

                                    if let Some(tool_calls) = &choice.delta.tool_calls {
                                        for tool_call in tool_calls {
                                            let index = tool_call.index.unwrap_or(0);
                                            let state = functions.entry(index).or_default();
                                            if let Some(id) = &tool_call.id {
                                                state.id = Some(id.clone());
                                            }
                                            if let Some(function) = &tool_call.function {
                                                if let Some(name) = &function.name {
                                                    state.name = Some(tool_name_map.to_client(name));
                                                }
                                                if let Some(arguments) = &function.arguments {
                                                    state.arguments.push_str(arguments);
                                                }
                                            }
                                            if !state.started {
                                                if let (Some(call_id), Some(name)) = (state.id.clone(), state.name.clone()) {
                                                    let item = json!({
                                                        "type": "function_call",
                                                        "call_id": call_id,
                                                        "name": name,
                                                        "arguments": ""
                                                    });
                                                    yield Ok(Bytes::from(sse_event("response.output_item.added", &json!({
                                                        "type": "response.output_item.added",
                                                        "item": item,
                                                    }))));
                                                    state.started = true;
                                                }
                                            }
                                            if let Some(function) = &tool_call.function {
                                                if let Some(arguments) = &function.arguments {
                                                    yield Ok(Bytes::from(sse_event("response.function_call_arguments.delta", &json!({
                                                        "type": "response.function_call_arguments.delta",
                                                        "delta": arguments,
                                                    }))));
                                                }
                                            }
                                        }
                                    }

                                    if choice.finish_reason.is_some() {
                                        if message_started {
                                            yield Ok(Bytes::from(sse_event("response.output_item.done", &json!({
                                                "type": "response.output_item.done",
                                                "item": {
                                                    "type": "message",
                                                    "role": "assistant",
                                                    "content": [{"type": "output_text", "text": message_text}],
                                                }
                                            }))));
                                        }
                                        for state in functions.values() {
                                            if let (Some(call_id), Some(name)) = (state.id.clone(), state.name.clone()) {
                                                yield Ok(Bytes::from(sse_event("response.output_item.done", &json!({
                                                    "type": "response.output_item.done",
                                                    "item": {
                                                        "type": "function_call",
                                                        "call_id": call_id,
                                                        "name": name,
                                                        "arguments": state.arguments,
                                                    }
                                                }))));
                                            }
                                        }
                                        yield Ok(Bytes::from(sse_event("response.completed", &json!({
                                            "type": "response.completed",
                                            "response": {
                                                "id": response_id.clone().unwrap_or_else(generate_message_id),
                                                "model": model_name.clone().unwrap_or_else(|| fallback_model.clone()),
                                                "output": [],
                                            }
                                        }))));
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Responses stream error: {}", e);
                        yield Ok(Bytes::from(stream_error_sse(&format!("Stream error: {}", e))));
                        break;
                    }
                },
                Ok(None) => break,
                Err(_) => {
                    tracing::warn!("Responses stream chunk timeout ({}s), closing stream", chunk_timeout_secs);
                    yield Ok(Bytes::from(stream_error_sse("stream chunk timeout")));
                    break;
                }
            }
        }
    }
}

pub struct Config {
    pub backend_url: String,
    pub backend_profile: BackendProfile,
    pub compat_mode: CompatMode,
    pub primary_model: String,
    pub reasoning_model: Option<String>,
    pub api_key: Option<String>,
    pub ingress_api_key: Option<String>,
    pub allow_origins: Vec<String>,
    pub port: u16,
    pub rate_limit_per_minute: Option<u32>,
    pub stream_chunk_timeout_secs: u64,
}

impl Config {
    pub fn from_env() -> Self {
        let legacy_model = std::env::var("ANTHMORPH_MODEL").ok();
        let primary_model = std::env::var("ANTHMORPH_PRIMARY_MODEL")
            .ok()
            .or_else(|| {
                legacy_model.as_ref().and_then(|value| {
                    value
                        .split(',')
                        .next()
                        .map(str::trim)
                        .map(ToOwned::to_owned)
                })
            })
            .unwrap_or_else(|| "Qwen/Qwen3.5-397B-A17B-TEE".to_string());

        Self {
            backend_url: std::env::var("ANTHMORPH_BACKEND_URL")
                .unwrap_or_else(|_| "https://llm.chutes.ai/v1".to_string()),
            backend_profile: std::env::var("ANTHMORPH_BACKEND_PROFILE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(BackendProfile::Chutes),
            compat_mode: std::env::var("ANTHMORPH_COMPAT_MODE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(CompatMode::Compat),
            primary_model,
            reasoning_model: std::env::var("ANTHMORPH_REASONING_MODEL").ok(),
            api_key: std::env::var("ANTHMORPH_API_KEY").ok(),
            ingress_api_key: std::env::var("ANTHMORPH_INGRESS_API_KEY").ok(),
            allow_origins: std::env::var("ANTHMORPH_ALLOWED_ORIGINS")
                .ok()
                .map(|v| {
                    v.split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(ToOwned::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
            port: std::env::var("PORT")
                .unwrap_or_else(|_| "3000".to_string())
                .parse()
                .unwrap_or(3000),
            rate_limit_per_minute: std::env::var("ANTHMORPH_RATE_LIMIT_PER_MINUTE")
                .ok()
                .and_then(|v| v.parse().ok()),
            stream_chunk_timeout_secs: std::env::var("ANTHMORPH_STREAM_CHUNK_TIMEOUT_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(30),
        }
    }

    pub fn chat_completions_url(&self) -> String {
        format!(
            "{}/chat/completions",
            self.backend_url.trim_end_matches('/')
        )
    }

    pub fn models_url(&self) -> String {
        format!("{}/models", self.backend_url.trim_end_matches('/'))
    }

    pub fn known_models(&self) -> Vec<String> {
        let mut models = vec![self.primary_model.clone()];
        if let Some(reasoning_model) = &self.reasoning_model {
            if !models.iter().any(|model| model == reasoning_model) {
                models.push(reasoning_model.clone());
            }
        }
        models
    }
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("backend_url", &self.backend_url)
            .field("backend_profile", &self.backend_profile.as_str())
            .field("compat_mode", &self.compat_mode.as_str())
            .field("primary_model", &self.primary_model)
            .field("reasoning_model", &self.reasoning_model)
            .field("api_key", &"<hidden>")
            .field("ingress_api_key", &"<hidden>")
            .field("allow_origins", &self.allow_origins)
            .field("port", &self.port)
            .field("rate_limit_per_minute", &self.rate_limit_per_minute)
            .field("stream_chunk_timeout_secs", &self.stream_chunk_timeout_secs)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActiveBlock {
    Thinking(usize),
    Text(usize),
    ToolUse(usize, usize),
}

impl ActiveBlock {
    fn index(self) -> usize {
        match self {
            ActiveBlock::Thinking(index) | ActiveBlock::Text(index) => index,
            ActiveBlock::ToolUse(_, index) => index,
        }
    }
}

#[derive(Debug, Default)]
struct ToolCallState {
    id: Option<String>,
    name: Option<String>,
    content_index: Option<usize>,
}

#[derive(Debug, Default)]
struct ThinkTagStreamFilter {
    carry: String,
    in_think: bool,
}

impl ThinkTagStreamFilter {
    fn push(&mut self, chunk: &str) -> (Vec<String>, String) {
        let mut reasoning = Vec::new();
        let mut visible = String::new();
        let mut work = format!("{}{}", self.carry, chunk);
        self.carry.clear();

        loop {
            if self.in_think {
                if let Some(end) = work.find("</think>") {
                    let think_text = &work[..end];
                    if !think_text.is_empty() {
                        reasoning.push(think_text.to_string());
                    }
                    work = work[end + "</think>".len()..].to_string();
                    self.in_think = false;
                    continue;
                }

                let split_at = partial_tag_suffix_start(&work, &["</think>"]);
                if split_at > 0 {
                    reasoning.push(work[..split_at].to_string());
                }
                self.carry = work[split_at..].to_string();
                break;
            }

            if let Some(start) = work.find("<think>") {
                visible.push_str(&work[..start]);
                work = work[start + "<think>".len()..].to_string();
                self.in_think = true;
                continue;
            }

            let split_at = partial_tag_suffix_start(&work, &["<think>", "</think>"]);
            visible.push_str(&work[..split_at]);
            self.carry = work[split_at..].to_string();
            break;
        }

        (reasoning, visible)
    }

    fn finish(&mut self) -> (Vec<String>, String) {
        if self.carry.is_empty() {
            return (Vec::new(), String::new());
        }

        let leftover = std::mem::take(&mut self.carry);
        if self.in_think {
            self.in_think = false;
            (vec![leftover], String::new())
        } else {
            (Vec::new(), leftover)
        }
    }
}

fn partial_tag_suffix_start(value: &str, tags: &[&str]) -> usize {
    for (start, _) in value.char_indices().rev() {
        let suffix = &value[start..];
        if tags.iter().any(|tag| tag.starts_with(suffix)) {
            return start;
        }
    }
    value.len()
}

fn transition_to_thinking(
    active_block: &mut Option<ActiveBlock>,
    next_content_index: &mut usize,
) -> (usize, Vec<String>) {
    match active_block {
        Some(ActiveBlock::Thinking(index)) => (*index, Vec::new()),
        _ => {
            let mut events = Vec::new();
            if let Some(previous) = active_block.take() {
                events.push(stop_block_sse(previous.index()));
                *next_content_index += 1;
            }
            let index = *next_content_index;
            *active_block = Some(ActiveBlock::Thinking(index));
            events.push(start_block_sse(
                index,
                anthropic::ContentBlockStartData::Thinking {
                    thinking: String::new(),
                },
            ));
            (index, events)
        }
    }
}

fn transition_to_text(
    active_block: &mut Option<ActiveBlock>,
    next_content_index: &mut usize,
) -> (usize, Vec<String>) {
    match active_block {
        Some(ActiveBlock::Text(index)) => (*index, Vec::new()),
        _ => {
            let mut events = Vec::new();
            if let Some(previous) = active_block.take() {
                events.push(stop_block_sse(previous.index()));
                *next_content_index += 1;
            }
            let index = *next_content_index;
            *active_block = Some(ActiveBlock::Text(index));
            events.push(start_block_sse(
                index,
                anthropic::ContentBlockStartData::Text {
                    text: String::new(),
                },
            ));
            (index, events)
        }
    }
}

fn transition_to_tool(
    active_block: &mut Option<ActiveBlock>,
    next_content_index: &mut usize,
    tool_index: usize,
    id: String,
    name: String,
) -> (usize, Vec<String>) {
    if let Some(ActiveBlock::ToolUse(active_tool_index, index)) = active_block {
        if *active_tool_index == tool_index {
            return (*index, Vec::new());
        }
    }

    let mut events = Vec::new();
    if let Some(previous) = active_block.take() {
        events.push(stop_block_sse(previous.index()));
        *next_content_index += 1;
    }

    let index = *next_content_index;
    *active_block = Some(ActiveBlock::ToolUse(tool_index, index));
    events.push(start_block_sse(
        index,
        anthropic::ContentBlockStartData::ToolUse {
            id,
            name,
            input: json!({}),
        },
    ));
    (index, events)
}

fn extract_sse_data(event_block: &str) -> Option<String> {
    let data_lines: Vec<_> = event_block
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .collect();

    if data_lines.is_empty() {
        None
    } else {
        Some(data_lines.join("\n"))
    }
}

fn sse_event<T: serde::Serialize>(name: &str, payload: &T) -> String {
    format!(
        "event: {name}\ndata: {}\n\n",
        serde_json::to_string(payload).unwrap_or_default()
    )
}

fn start_block_sse(index: usize, content_block: anthropic::ContentBlockStartData) -> String {
    let event = anthropic::StreamEvent::ContentBlockStart {
        index,
        content_block,
    };
    sse_event("content_block_start", &event)
}

fn delta_block_sse(index: usize, delta: anthropic::ContentBlockDeltaData) -> String {
    let event = anthropic::StreamEvent::ContentBlockDelta { index, delta };
    sse_event("content_block_delta", &event)
}

fn stop_block_sse(index: usize) -> String {
    let event = anthropic::StreamEvent::ContentBlockStop { index };
    sse_event("content_block_stop", &event)
}

fn message_stop_sse() -> String {
    let event = anthropic::StreamEvent::MessageStop;
    sse_event("message_stop", &event)
}

fn stream_error_sse(message: &str) -> String {
    let event = json!({
        "type": "error",
        "error": {
            "type": "stream_error",
            "message": message,
        }
    });
    format!(
        "event: error\ndata: {}\n\n",
        serde_json::to_string(&event).unwrap_or_default()
    )
}

fn authorize_request(headers: &HeaderMap, config: &Config) -> ProxyResult<()> {
    let Some(expected) = &config.ingress_api_key else {
        return Ok(());
    };

    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    let x_api_key = headers.get("x-api-key").and_then(|v| v.to_str().ok());

    if bearer == Some(expected.as_str()) || x_api_key == Some(expected.as_str()) {
        Ok(())
    } else {
        Err(ProxyError::Upstream(
            "401 unauthorized ingress request".to_string(),
        ))
    }
}

pub fn build_cors_layer(config: &Config) -> anyhow::Result<Option<CorsLayer>> {
    if config.allow_origins.is_empty() {
        return Ok(None);
    }

    for origin in &config.allow_origins {
        if origin.contains('*') {
            anyhow::bail!(
                "wildcard origin '*' is not supported in ANTHMORPH_ALLOWED_ORIGINS; use a reverse proxy for open CORS"
            );
        }
    }

    let origins: Vec<HeaderValue> = config
        .allow_origins
        .iter()
        .map(|origin| HeaderValue::from_str(origin))
        .collect::<Result<_, _>>()?;

    Ok(Some(
        CorsLayer::new()
            .allow_methods([axum::http::Method::POST, axum::http::Method::GET])
            .allow_headers([
                header::AUTHORIZATION,
                HeaderName::from_static("x-api-key"),
                header::CONTENT_TYPE,
            ])
            .allow_origin(AllowOrigin::list(origins)),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{
            header::ACCESS_CONTROL_REQUEST_METHOD, header::ORIGIN, Method, Request, StatusCode,
        },
        routing::get,
        Router,
    };
    use futures::stream;
    use tower::ServiceExt;

    #[tokio::test]
    async fn create_sse_stream_accumulates_fragmented_tool_calls() {
        let first = serde_json::to_string(&json!({
            "id": "abc",
            "model": "qwen",
            "choices": [{
                "index": 0,
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_1",
                        "function": {
                            "name": "weather",
                            "arguments": "{\"loc"
                        }
                    }]
                },
                "finish_reason": null
            }],
            "usage": null
        }))
        .unwrap();
        let second = serde_json::to_string(&json!({
            "choices": [{
                "index": 0,
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "function": {
                            "arguments": "ation\":\"Rome\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {
                "completion_tokens": 7
            }
        }))
        .unwrap();

        let chunks = vec![
            Ok(Bytes::from(format!("data: {first}\n\n"))),
            Ok(Bytes::from(format!("data: {second}\n\n"))),
            Ok(Bytes::from("data: [DONE]\n\n")),
        ];

        let mut output = Vec::new();
        let sse = create_sse_stream(
            stream::iter(chunks),
            "fallback".to_string(),
            BackendProfile::Chutes,
            CompatMode::Strict,
            30,
            ToolNameMap::identity(),
        );
        tokio::pin!(sse);

        while let Some(item) = sse.next().await {
            output.push(String::from_utf8(item.unwrap().to_vec()).unwrap());
        }

        let joined = output.join("");
        assert!(joined.contains("\"type\":\"tool_use\""));
        assert!(joined.contains("\"input\":{}"));
        assert!(joined.contains("\"partial_json\":\"{\\\"loc\""));
        assert!(joined.contains("\"partial_json\":\"ation"));
        assert_eq!(joined.matches("event: content_block_start").count(), 1);
    }

    #[tokio::test]
    async fn create_sse_stream_strips_think_tags_for_generic_compat() {
        let first = serde_json::to_string(&json!({
            "id": "abc",
            "model": "minimax",
            "choices": [{
                "index": 0,
                "delta": {
                    "content": "<think>secret</think>visible"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "completion_tokens": 4
            }
        }))
        .unwrap();

        let chunks = vec![
            Ok(Bytes::from(format!("data: {first}\n\n"))),
            Ok(Bytes::from("data: [DONE]\n\n")),
        ];

        let mut output = Vec::new();
        let sse = create_sse_stream(
            stream::iter(chunks),
            "fallback".to_string(),
            BackendProfile::OpenaiGeneric,
            CompatMode::Compat,
            30,
            ToolNameMap::identity(),
        );
        tokio::pin!(sse);

        while let Some(item) = sse.next().await {
            output.push(String::from_utf8(item.unwrap().to_vec()).unwrap());
        }

        let joined = output.join("");
        assert!(joined.contains("visible"));
        assert!(!joined.contains("secret"));
    }

    #[test]
    fn message_start_sse_includes_required_anthropic_fields() {
        let event = anthropic::StreamEvent::MessageStart {
            message: anthropic::MessageStartData {
                id: "msg_test".to_string(),
                message_type: "message".to_string(),
                role: "assistant".to_string(),
                content: vec![],
                model: "glm-5.1".to_string(),
                stop_reason: None,
                stop_sequence: None,
                usage: anthropic::Usage {
                    input_tokens: 0,
                    output_tokens: 0,
                },
            },
        };

        let serialized = sse_event("message_start", &event);
        let payload = serialized
            .lines()
            .find_map(|line| line.strip_prefix("data: "))
            .expect("message_start data line");
        let parsed: serde_json::Value = serde_json::from_str(payload).expect("valid json");

        assert_eq!(parsed["message"]["type"], "message");
        assert_eq!(parsed["message"]["role"], "assistant");
        assert_eq!(parsed["message"]["content"], json!([]));
        assert!(parsed["message"]["stop_reason"].is_null());
        assert!(parsed["message"]["stop_sequence"].is_null());
    }

    #[test]
    fn content_block_start_tool_use_has_flat_anthropic_shape() {
        let payload = start_block_sse(
            0,
            anthropic::ContentBlockStartData::ToolUse {
                id: "toolu_123".to_string(),
                name: "mcp__memory__memory_read".to_string(),
                input: json!({}),
            },
        )
        .lines()
        .find_map(|line| line.strip_prefix("data: "))
        .expect("content_block_start data line")
        .to_string();

        let parsed: serde_json::Value = serde_json::from_str(&payload).expect("valid json");
        assert_eq!(parsed["content_block"]["type"], "tool_use");
        assert_eq!(parsed["content_block"]["id"], "toolu_123");
        assert_eq!(parsed["content_block"]["name"], "mcp__memory__memory_read");
        assert_eq!(parsed["content_block"]["input"], json!({}));
        assert!(parsed["content_block"].get("content_block").is_none());
    }

    #[test]
    fn extracts_multi_line_sse_data() {
        let block = "event: message\ndata: first\ndata: second\n";
        assert_eq!(extract_sse_data(block).as_deref(), Some("first\nsecond"));
    }

    #[test]
    fn authorize_request_accepts_bearer_and_x_api_key() {
        let config = Config {
            backend_url: "https://example.com".to_string(),
            backend_profile: BackendProfile::OpenaiGeneric,
            compat_mode: CompatMode::Strict,
            primary_model: "model".to_string(),
            reasoning_model: None,
            api_key: None,
            ingress_api_key: Some("secret".to_string()),
            allow_origins: Vec::new(),
            port: 3000,
            rate_limit_per_minute: None,
            stream_chunk_timeout_secs: 30,
        };

        let mut bearer_headers = HeaderMap::new();
        bearer_headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer secret"),
        );
        assert!(authorize_request(&bearer_headers, &config).is_ok());

        let mut x_api_headers = HeaderMap::new();
        x_api_headers.insert(
            HeaderName::from_static("x-api-key"),
            HeaderValue::from_static("secret"),
        );
        assert!(authorize_request(&x_api_headers, &config).is_ok());
    }

    #[test]
    fn authorize_request_rejects_invalid_ingress_key() {
        let config = Config {
            backend_url: "https://example.com".to_string(),
            backend_profile: BackendProfile::OpenaiGeneric,
            compat_mode: CompatMode::Strict,
            primary_model: "model".to_string(),
            reasoning_model: None,
            api_key: None,
            ingress_api_key: Some("secret".to_string()),
            allow_origins: Vec::new(),
            port: 3000,
            rate_limit_per_minute: None,
            stream_chunk_timeout_secs: 30,
        };

        let headers = HeaderMap::new();
        let err = authorize_request(&headers, &config).unwrap_err();
        assert!(err.to_string().contains("unauthorized ingress request"));
    }

    #[tokio::test]
    async fn build_cors_layer_allows_configured_origin() {
        let config = Config {
            backend_url: "https://example.com".to_string(),
            backend_profile: BackendProfile::OpenaiGeneric,
            compat_mode: CompatMode::Strict,
            primary_model: "model".to_string(),
            reasoning_model: None,
            api_key: None,
            ingress_api_key: None,
            allow_origins: vec!["https://allowed.example".to_string()],
            port: 3000,
            rate_limit_per_minute: None,
            stream_chunk_timeout_secs: 30,
        };

        let app = Router::new().route("/health", get(|| async { StatusCode::OK }));
        let app = app.layer(build_cors_layer(&config).unwrap().expect("cors layer"));

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    .uri("/health")
                    .header(ORIGIN, "https://allowed.example")
                    .header(ACCESS_CONTROL_REQUEST_METHOD, "GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
            Some(&HeaderValue::from_static("https://allowed.example"))
        );
    }

    #[test]
    fn redact_secrets_hides_bearer_tokens() {
        let input = r#"{"error":"Bearer sk-ant-api03-longtoken1234567890abcdef is invalid"}"#;
        let redacted = redact_secrets(input);
        assert!(!redacted.contains("sk-ant-api03-longtoken1234567890abcdef"));
        assert!(redacted.contains("Bearer ***"));
    }

    #[test]
    fn redact_secrets_hides_cpk_prefix() {
        let input = r#"error for cpk_1234567890abcdef1234"#;
        let redacted = redact_secrets(input);
        assert!(!redacted.contains("cpk_1234567890abcdef1234"));
        assert!(redacted.contains("***"));
    }

    #[test]
    fn redact_secrets_preserves_clean_text() {
        let input = r#"{"error":"model not found"}"#;
        assert_eq!(redact_secrets(input), input);
    }

    #[test]
    fn redact_secrets_truncates_long_input() {
        let input = "x".repeat(3000);
        let redacted = redact_secrets(&input);
        assert!(redacted.len() <= 2070);
        assert!(redacted.ends_with("… [truncated]"));
    }

    #[test]
    fn redact_secrets_hides_x_api_key() {
        let input = "upstream rejected: x-api-key: cpk_abcdef1234567890 is invalid";
        let redacted = redact_secrets(input);
        assert!(!redacted.contains("cpk_abcdef1234567890"));
        assert!(redacted.contains("x-api-key: ***"));
    }

    #[test]
    fn rate_limit_error_returns_429_format() {
        use crate::error::ProxyError;
        use axum::response::IntoResponse;
        let err = ProxyError::Upstream("429 rate limit exceeded".to_string());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[test]
    fn validate_deepseek_request_rejects_long_tool_names() {
        let request = openai::OpenAIRequest {
            model: "deepseek-v4-pro".to_string(),
            messages: vec![],
            max_tokens: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stop: None,
            stream: None,
            tools: Some(vec![openai::Tool {
                tool_type: "function".to_string(),
                function: openai::Function {
                    name: "mcp__memory__memory_read__aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
                    description: None,
                    parameters: json!({}),
                },
            }]),
            tool_choice: None,
        };

        let err = validate_deepseek_request(BackendProfile::Deepseek, &request).unwrap_err();
        assert!(err.to_string().contains("exceeds 64 characters"));
    }

    #[test]
    fn models_response_json_emits_openai_list_shape() {
        let payload = models_response_json(&[
            model_cache::ModelInfo {
                id: "deepseek-v4-pro".to_string(),
            },
            model_cache::ModelInfo {
                id: "deepseek-v4-flash".to_string(),
            },
        ]);

        assert_eq!(payload["object"], "list");
        assert_eq!(payload["data"][0]["id"], "deepseek-v4-pro");
        assert_eq!(payload["data"][1]["id"], "deepseek-v4-flash");
    }

    #[test]
    fn resolve_backend_key_prefers_saved_key_for_ingress_token() {
        let config = Config {
            backend_url: "https://example.com".to_string(),
            backend_profile: BackendProfile::Deepseek,
            compat_mode: CompatMode::Compat,
            primary_model: "deepseek-v4-pro".to_string(),
            reasoning_model: None,
            api_key: Some("backend-secret".to_string()),
            ingress_api_key: Some("anthmorph-local".to_string()),
            allow_origins: Vec::new(),
            port: 3108,
            rate_limit_per_minute: None,
            stream_chunk_timeout_secs: 30,
        };

        let resolved = resolve_backend_key(Some("anthmorph-local"), &config);
        assert_eq!(resolved.as_deref(), Some("backend-secret"));
    }

    #[test]
    fn build_cors_layer_rejects_wildcard() {
        let config = Config {
            backend_url: "https://example.com".to_string(),
            backend_profile: BackendProfile::OpenaiGeneric,
            compat_mode: CompatMode::Strict,
            primary_model: "model".to_string(),
            reasoning_model: None,
            api_key: None,
            ingress_api_key: None,
            allow_origins: vec!["*".to_string()],
            port: 3000,
            rate_limit_per_minute: None,
            stream_chunk_timeout_secs: 30,
        };
        let result = build_cors_layer(&config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("wildcard"));
    }
}
