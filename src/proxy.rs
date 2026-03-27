use crate::config::BackendProfile;
use crate::error::{ProxyError, ProxyResult};
use crate::models::{anthropic, openai};
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

fn map_model(client_model: &str, config: &Config) -> String {
    match client_model {
        m if m.is_empty() || m == "default" => config.model.clone(),
        m if m.starts_with("claude-") => config.model.clone(),
        other => other.to_string(),
    }
}

pub async fn proxy_handler(
    headers: HeaderMap,
    Extension(config): Extension<Arc<Config>>,
    Extension(client): Extension<Client>,
    Json(req): Json<anthropic::AnthropicRequest>,
) -> ProxyResult<Response> {
    authorize_request(&headers, &config)?;

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
                        anthropic::ContentBlock::ToolUse { .. } => "tool_use_block",
                        anthropic::ContentBlock::ToolResult { .. } => "tool_result_block",
                        anthropic::ContentBlock::Thinking { .. } => "thinking_block",
                        anthropic::ContentBlock::Other => "unknown_block",
                    }
                }
            }
        };
        tracing::debug!("Message {}: role={}, content={}", i, msg.role, content_type);
    }
    tracing::debug!("Streaming: {}", is_streaming);

    let model = if req
        .extra
        .get("thinking")
        .and_then(|v| v.get("type"))
        .is_some()
    {
        config
            .reasoning_model
            .clone()
            .unwrap_or_else(|| config.model.clone())
    } else {
        map_model(&req.model, &config)
    };

    let openai_req = transform::anthropic_to_openai(req, &model, config.backend_profile)?;

    if is_streaming {
        handle_streaming(config, client, openai_req).await
    } else {
        handle_non_streaming(config, client, openai_req).await
    }
}

async fn handle_non_streaming(
    config: Arc<Config>,
    client: Client,
    openai_req: openai::OpenAIRequest,
) -> ProxyResult<Response> {
    let url = config.chat_completions_url();
    tracing::debug!("Sending non-streaming request to {}", url);

    let mut req_builder = client
        .post(&url)
        .json(&openai_req)
        .timeout(Duration::from_secs(300));

    if let Some(api_key) = &config.api_key {
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
        tracing::error!("Upstream error ({}): {}", status, error_text);
        return Err(ProxyError::Upstream(format!(
            "Upstream returned {}: {}",
            status, error_text
        )));
    }

    let openai_resp: openai::OpenAIResponse = response.json().await?;
    let anthropic_resp =
        transform::openai_to_anthropic(openai_resp, &openai_req.model, config.backend_profile)?;

    Ok(Json(anthropic_resp).into_response())
}

async fn handle_streaming(
    config: Arc<Config>,
    client: Client,
    openai_req: openai::OpenAIRequest,
) -> ProxyResult<Response> {
    let url = config.chat_completions_url();
    tracing::debug!("Sending streaming request to {}", url);

    let mut req_builder = client
        .post(&url)
        .json(&openai_req)
        .timeout(Duration::from_secs(300));

    if let Some(api_key) = &config.api_key {
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
        tracing::error!("Upstream streaming error ({}): {}", status, error_text);
        return Err(ProxyError::Upstream(format!(
            "Upstream returned {}: {}",
            status, error_text
        )));
    }

    let stream = response.bytes_stream();
    let sse_stream = create_sse_stream(stream, openai_req.model.clone(), config.backend_profile);

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
) -> impl Stream<Item = Result<Bytes, std::io::Error>> + Send {
    async_stream::stream! {
        let mut buffer = String::new();
        let mut message_id = None;
        let mut current_model = None;
        let mut next_content_index = 0usize;
        let mut has_sent_message_start = false;
        let mut active_block: Option<ActiveBlock> = None;
        let mut tool_states: BTreeMap<usize, ToolCallState> = BTreeMap::new();

        pin!(stream);

        let mut raw_buffer: Vec<u8> = Vec::new();

        while let Some(chunk) = stream.next().await {
            match chunk {
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
                            yield Ok(Bytes::from(message_stop_sse()));
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
                                            model: current_model
                                                .clone()
                                                .unwrap_or_else(|| fallback_model.clone()),
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
                                        if !profile.supports_reasoning() {
                                            yield Ok(Bytes::from(stream_error_sse(
                                                "reasoning deltas are not supported by the active backend profile",
                                            )));
                                            break;
                                        }

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

                                if let Some(content) = &choice.delta.content {
                                    if !content.is_empty() {
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
                                                text: content.clone(),
                                            },
                                        )));
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
                                                    name,
                                                );
                                                state.content_index = Some(idx);
                                                for event in transitions {
                                                    yield Ok(Bytes::from(event));
                                                }
                                            }
                                        } else if active_block != Some(ActiveBlock::ToolUse(tool_index, state.content_index.unwrap())) {
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
            }
        }
    }
}

pub struct Config {
    pub backend_url: String,
    pub backend_profile: BackendProfile,
    pub model: String,
    pub reasoning_model: Option<String>,
    pub api_key: Option<String>,
    pub ingress_api_key: Option<String>,
    pub allow_origins: Vec<String>,
    pub port: u16,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            backend_url: std::env::var("ANTHMORPH_BACKEND_URL")
                .unwrap_or_else(|_| "https://llm.chutes.ai/v1".to_string()),
            backend_profile: std::env::var("ANTHMORPH_BACKEND_PROFILE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(BackendProfile::Chutes),
            model: std::env::var("ANTHMORPH_MODEL")
                .unwrap_or_else(|_| "Qwen/Qwen3-Coder-Next-TEE".to_string()),
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
        }
    }

    pub fn chat_completions_url(&self) -> String {
        format!(
            "{}/chat/completions",
            self.backend_url.trim_end_matches('/')
        )
    }
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("backend_url", &self.backend_url)
            .field("backend_profile", &self.backend_profile.as_str())
            .field("model", &self.model)
            .field("reasoning_model", &self.reasoning_model)
            .field("api_key", &"<hidden>")
            .field("ingress_api_key", &"<hidden>")
            .field("allow_origins", &self.allow_origins)
            .field("port", &self.port)
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
        anthropic::ContentBlockStartData::ToolUse { id, name },
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
        );
        tokio::pin!(sse);

        while let Some(item) = sse.next().await {
            output.push(String::from_utf8(item.unwrap().to_vec()).unwrap());
        }

        let joined = output.join("");
        assert!(joined.contains("\"type\":\"tool_use\""));
        assert!(joined.contains("\"partial_json\":\"{\\\"loc\""));
        assert!(joined.contains("\"partial_json\":\"ation"));
        assert_eq!(joined.matches("event: content_block_start").count(), 1);
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
            model: "model".to_string(),
            reasoning_model: None,
            api_key: None,
            ingress_api_key: Some("secret".to_string()),
            allow_origins: Vec::new(),
            port: 3000,
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
            model: "model".to_string(),
            reasoning_model: None,
            api_key: None,
            ingress_api_key: Some("secret".to_string()),
            allow_origins: Vec::new(),
            port: 3000,
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
            model: "model".to_string(),
            reasoning_model: None,
            api_key: None,
            ingress_api_key: None,
            allow_origins: vec!["https://allowed.example".to_string()],
            port: 3000,
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
}
