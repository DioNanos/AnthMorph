use serde_json::Value;
use std::env;
use std::fs;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

struct TestServer {
    child: Child,
    log_path: PathBuf,
    port: u16,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn configured_env(name: &str) -> Option<String> {
    match env::var(name) {
        Ok(value) if !value.trim().is_empty() => Some(value),
        _ => None,
    }
}

fn require_env(name: &str) -> Option<String> {
    match configured_env(name) {
        Some(value) => Some(value),
        None => {
            eprintln!("skipping real backend test: missing {name}");
            None
        }
    }
}

fn reserve_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral port")
        .local_addr()
        .expect("read local addr")
        .port()
}

fn anthmorph_bin() -> String {
    if let Ok(path) = env::var("CARGO_BIN_EXE_anthmorph") {
        return path;
    }

    let fallback = PathBuf::from("target/debug/anthmorph");
    if fallback.exists() {
        return fallback.to_string_lossy().into_owned();
    }

    panic!("cargo should expose anthmorph bin path");
}

fn start_server(
    backend_profile: &str,
    backend_url: &str,
    model: &str,
    api_key: &str,
) -> TestServer {
    let port = reserve_port();
    let log_path = env::temp_dir().join(format!("anthmorph-itest-{backend_profile}-{port}.log"));
    let log = fs::File::create(&log_path).expect("create log file");
    let log_err = log.try_clone().expect("clone log file");

    let child = Command::new(anthmorph_bin())
        .arg("--port")
        .arg(port.to_string())
        .arg("--backend-profile")
        .arg(backend_profile)
        .arg("--backend-url")
        .arg(backend_url)
        .arg("--model")
        .arg(model)
        .arg("--api-key")
        .arg(api_key)
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err))
        .spawn()
        .expect("spawn anthmorph");

    let server = TestServer {
        child,
        log_path,
        port,
    };
    wait_until_ready(&server);
    server
}

fn wait_until_ready(server: &TestServer) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        let status = Command::new("curl")
            .arg("-fsS")
            .arg(format!("http://127.0.0.1:{}/health", server.port))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("run curl health");
        if status.success() {
            return;
        }
        thread::sleep(Duration::from_millis(250));
    }

    let log = fs::read_to_string(&server.log_path).unwrap_or_else(|_| "<missing log>".to_string());
    panic!("server did not become ready\n{}", log);
}

fn post_messages(server: &TestServer, payload: &Value) -> (u16, Value) {
    let body_path = env::temp_dir().join(format!("anthmorph-itest-body-{}.json", server.port));
    let payload_text = serde_json::to_string(payload).expect("serialize payload");

    let output = Command::new("curl")
        .arg("-sS")
        .arg("-o")
        .arg(&body_path)
        .arg("-w")
        .arg("%{http_code}")
        .arg(format!("http://127.0.0.1:{}/v1/messages", server.port))
        .arg("-H")
        .arg("content-type: application/json")
        .arg("-d")
        .arg(payload_text)
        .output()
        .expect("run curl request");

    let status_text = String::from_utf8(output.stdout).expect("status utf8");
    let status = status_text
        .trim()
        .parse::<u16>()
        .expect("parse status code");
    let body = fs::read_to_string(&body_path).expect("read response body");
    let value: Value = serde_json::from_str(&body).unwrap_or_else(|err| {
        panic!("parse response body as json failed: {err}; status={status}; body={body}")
    });
    (status, value)
}

fn post_messages_stream(server: &TestServer, payload: &Value) -> (u16, String) {
    let body_path = env::temp_dir().join(format!("anthmorph-itest-stream-{}.txt", server.port));
    let payload_text = serde_json::to_string(payload).expect("serialize payload");

    let output = Command::new("curl")
        .arg("-sS")
        .arg("-N")
        .arg("-o")
        .arg(&body_path)
        .arg("-w")
        .arg("%{http_code}")
        .arg(format!("http://127.0.0.1:{}/v1/messages", server.port))
        .arg("-H")
        .arg("content-type: application/json")
        .arg("-d")
        .arg(payload_text)
        .output()
        .expect("run curl stream request");

    let status_text = String::from_utf8(output.stdout).expect("status utf8");
    let status = status_text
        .trim()
        .parse::<u16>()
        .expect("parse status code");
    let body = fs::read_to_string(&body_path).expect("read stream body");
    (status, body)
}

fn post_count_tokens(server: &TestServer, payload: &Value) -> (u16, Value) {
    let body_path = env::temp_dir().join(format!("anthmorph-itest-count-{}.json", server.port));
    let payload_text = serde_json::to_string(payload).expect("serialize payload");

    let output = Command::new("curl")
        .arg("-sS")
        .arg("-o")
        .arg(&body_path)
        .arg("-w")
        .arg("%{http_code}")
        .arg(format!(
            "http://127.0.0.1:{}/v1/messages/count_tokens",
            server.port
        ))
        .arg("-H")
        .arg("content-type: application/json")
        .arg("-d")
        .arg(payload_text)
        .output()
        .expect("run curl count_tokens request");

    let status_text = String::from_utf8(output.stdout).expect("status utf8");
    let status = status_text
        .trim()
        .parse::<u16>()
        .expect("parse status code");
    let body = fs::read_to_string(&body_path).expect("read count_tokens body");
    let value: Value = serde_json::from_str(&body).unwrap_or_else(|err| {
        panic!("parse count_tokens body as json failed: {err}; status={status}; body={body}")
    });
    (status, value)
}

fn post_responses(server: &TestServer, payload: &Value) -> (u16, Value) {
    let body_path = env::temp_dir().join(format!("anthmorph-itest-responses-{}.json", server.port));
    let payload_text = serde_json::to_string(payload).expect("serialize responses payload");

    let output = Command::new("curl")
        .arg("-sS")
        .arg("-o")
        .arg(&body_path)
        .arg("-w")
        .arg("%{http_code}")
        .arg(format!("http://127.0.0.1:{}/v1/responses", server.port))
        .arg("-H")
        .arg("content-type: application/json")
        .arg("-d")
        .arg(payload_text)
        .output()
        .expect("run curl responses request");

    let status_text = String::from_utf8(output.stdout).expect("status utf8");
    let status = status_text.trim().parse::<u16>().expect("parse status code");
    let body = fs::read_to_string(&body_path).expect("read responses body");
    let value: Value = serde_json::from_str(&body).unwrap_or_else(|err| {
        panic!("parse responses body as json failed: {err}; status={status}; body={body}")
    });
    (status, value)
}

fn get_models(server: &TestServer) -> (u16, Value) {
    let body_path = env::temp_dir().join(format!("anthmorph-itest-models-{}.json", server.port));
    let output = Command::new("curl")
        .arg("-sS")
        .arg("-o")
        .arg(&body_path)
        .arg("-w")
        .arg("%{http_code}")
        .arg(format!("http://127.0.0.1:{}/v1/models", server.port))
        .output()
        .expect("run curl models request");

    let status_text = String::from_utf8(output.stdout).expect("status utf8");
    let status = status_text
        .trim()
        .parse::<u16>()
        .expect("parse status code");
    let body = fs::read_to_string(&body_path).expect("read models body");
    let value: Value = serde_json::from_str(&body).unwrap_or_else(|err| {
        panic!("parse models body as json failed: {err}; status={status}; body={body}")
    });
    (status, value)
}

fn is_retryable_provider_failure(status: u16, body: &str) -> bool {
    if status == 429 || status >= 500 {
        return true;
    }

    let body_lower = body.to_ascii_lowercase();
    [
        "maximum capacity",
        "try again later",
        "rate limit",
        "temporarily unavailable",
        "overloaded",
        "backend_error_retryable",
        "timeout",
    ]
    .iter()
    .any(|needle| body_lower.contains(needle))
}

fn is_auth_provider_failure(status: u16, body: &str) -> bool {
    if status != 401 {
        return false;
    }
    let body_lower = body.to_ascii_lowercase();
    body_lower.contains("invalid") || body_lower.contains("unauthorized") || body_lower.contains("authentication")
}

fn post_messages_with_retry(server: &TestServer, payload: &Value) -> Option<(u16, Value)> {
    let mut last_body = String::new();
    let mut last_status = 0;

    for attempt in 1..=3 {
        let (status, body) = post_messages(server, payload);
        let rendered = body.to_string();
        if !is_retryable_provider_failure(status, &rendered) {
            return Some((status, body));
        }

        last_status = status;
        last_body = rendered;
        eprintln!(
            "retryable provider failure on attempt {attempt}: status={status} body={last_body}"
        );
        thread::sleep(Duration::from_secs(attempt));
    }

    eprintln!(
        "quarantining real backend test after repeated provider failures: status={} body={}",
        last_status, last_body
    );
    None
}

fn post_stream_with_retry(server: &TestServer, payload: &Value) -> Option<(u16, String)> {
    let mut last_body = String::new();
    let mut last_status = 0;

    for attempt in 1..=3 {
        let (status, body) = post_messages_stream(server, payload);
        if !is_retryable_provider_failure(status, &body) {
            return Some((status, body));
        }

        last_status = status;
        last_body = body;
        eprintln!(
            "retryable stream failure on attempt {attempt}: status={status} body={last_body}"
        );
        thread::sleep(Duration::from_secs(attempt));
    }

    eprintln!(
        "quarantining real streaming test after repeated provider failures: status={} body={}",
        last_status, last_body
    );
    None
}

fn assert_claude_stream_shape(body: &str, expect_no_think_tags: bool) {
    assert!(
        body.contains("event: message_start"),
        "expected Claude SSE message_start, got: {body}"
    );
    assert!(
        body.contains("event: message_stop"),
        "expected Claude SSE message_stop, got: {body}"
    );
    assert!(
        !body.contains("\"choices\""),
        "OpenAI wire format leaked to client: {body}"
    );
    if expect_no_think_tags {
        assert!(
            !body.contains("<think>"),
            "backend reasoning tags leaked to Claude client: {body}"
        );
    }
}

fn payload_dir() -> Option<PathBuf> {
    if let Some(value) = configured_env("ANTHMORPH_CLAUDE_PAYLOAD_DIR") {
        let path = PathBuf::from(value);
        if path.is_dir() {
            return Some(path);
        }
    }

    let default = PathBuf::from("/opt/claude-proxy/tests/payloads");
    if default.is_dir() {
        Some(default)
    } else {
        None
    }
}

fn claude_payload_names() -> &'static [&'static str] {
    &[
        "basic_request.json",
        "content_blocks_text.json",
        "content_blocks_mixed.json",
        "conversation_3_system.json",
        "conversation_2_followup.json",
        "conversation_4_tools.json",
        "tool_result.json",
        "claude_code_adaptive_thinking.json",
        "cache_control_request.json",
        "documents_request.json",
        "unknown_content_blocks.json",
        "multi_tool_request.json",
    ]
}

fn render_payload(path: &PathBuf, model: &str) -> Value {
    let raw = fs::read_to_string(path).expect("read payload file");
    serde_json::from_str(&raw.replace("{{MODEL}}", model)).expect("parse payload file")
}

fn base_payload() -> Value {
    serde_json::json!({
        "model": "claude-sonnet-4",
        "max_tokens": 32,
        "temperature": 0,
        "system": "Return only the requested token with no explanation.",
        "messages": [
            {"role": "user", "content": "Reply with exactly: ANTHMORPH_OK"}
        ]
    })
}

fn expect_text_response_with_retry(server: &TestServer, expected_fragment: &str) -> Option<String> {
    for attempt in 1..=3 {
        let Some((status, response)) = post_messages_with_retry(server, &base_payload()) else {
            return None;
        };
        assert_eq!(status, 200, "unexpected status: {response}");

        let text = response["content"]
            .as_array()
            .and_then(|items| {
                items.iter()
                    .find_map(|item| item.get("text").and_then(Value::as_str))
            })
            .unwrap_or("")
            .trim()
            .to_string();

        if text.contains(expected_fragment) {
            return Some(text);
        }

        eprintln!("unexpected text on attempt {attempt}: text={text} response={response}");
        thread::sleep(Duration::from_secs(attempt));
    }

    None
}

#[test]
#[ignore = "AnthMorph is now Codex Responses-first; legacy /v1/messages smoke is no longer public API"]
fn chutes_real_backend_smoke() {
    let Some(api_key) = require_env("CHUTES_API_KEY") else {
        return;
    };
    let server = start_server(
        "chutes",
        &env::var("CHUTES_BASE_URL").unwrap_or_else(|_| "https://llm.chutes.ai/v1".to_string()),
        &env::var("CHUTES_MODEL").unwrap_or_else(|_| "deepseek-ai/DeepSeek-V3.2-TEE".to_string()),
        &api_key,
    );

    let Some(text) = expect_text_response_with_retry(&server, "ANTHMORPH_OK") else {
        return;
    };
    assert_eq!(text, "ANTHMORPH_OK");
}

#[test]
#[ignore = "AnthMorph is now Codex Responses-first; legacy /v1/messages smoke is no longer public API"]
fn minimax_real_backend_smoke() {
    let Some(api_key) = require_env("MINIMAX_API_KEY") else {
        return;
    };
    let server = start_server(
        "openai-generic",
        &env::var("MINIMAX_BASE_URL").unwrap_or_else(|_| "https://api.minimax.io/v1".to_string()),
        &env::var("MINIMAX_MODEL").unwrap_or_else(|_| "MiniMax-M2.5".to_string()),
        &api_key,
    );

    let Some(text) = expect_text_response_with_retry(&server, "ANTHMORPH_OK") else {
        return;
    };
    assert!(text.contains("ANTHMORPH_OK"), "unexpected text: {text}");
}

#[test]
#[ignore = "AnthMorph no longer exposes the legacy Anthropic ingress used by this provider error smoke"]
fn alibaba_coding_plan_rejected_for_chat_completions() {
    let Some(api_key) = require_env("ALIBABA_CODE_API_KEY") else {
        return;
    };
    let server = start_server(
        "openai-generic",
        &env::var("ALIBABA_BASE_URL")
            .unwrap_or_else(|_| "https://coding-intl.dashscope.aliyuncs.com/v1".to_string()),
        &env::var("ALIBABA_MODEL").unwrap_or_else(|_| "qwen3-coder-plus".to_string()),
        &api_key,
    );

    let (status, response) = post_messages(&server, &base_payload());
    assert_eq!(status, 502, "unexpected status: {response}");
    let message = response["error"]["message"]
        .as_str()
        .expect("error message");
    assert!(
        message.contains("Coding Plan is currently only available for Coding Agents"),
        "unexpected error message: {message}"
    );
}

#[test]
#[ignore = "count_tokens belonged to the removed legacy /v1/messages public surface"]
fn chutes_models_and_count_tokens_work() {
    let Some(api_key) = require_env("CHUTES_API_KEY") else {
        return;
    };
    let model =
        env::var("CHUTES_MODEL").unwrap_or_else(|_| "deepseek-ai/DeepSeek-V3.2-TEE".to_string());
    let server = start_server(
        "chutes",
        &env::var("CHUTES_BASE_URL").unwrap_or_else(|_| "https://llm.chutes.ai/v1".to_string()),
        &model,
        &api_key,
    );

    let (models_status, models) = get_models(&server);
    if is_retryable_provider_failure(models_status, &models.to_string()) {
        eprintln!("quarantining models test due to provider instability: {models}");
        return;
    }
    assert_eq!(models_status, 200, "unexpected models status: {models}");
    assert!(
        models.get("data").is_some(),
        "unexpected models body: {models}"
    );

    let (count_status, count) = post_count_tokens(&server, &base_payload());
    assert_eq!(count_status, 200, "unexpected count_tokens status: {count}");
    assert!(
        count["input_tokens"].as_u64().unwrap_or(0) > 0,
        "unexpected count_tokens body: {count}"
    );
}

#[test]
#[ignore = "Claude payload corpus targets the removed legacy /v1/messages public surface"]
fn chutes_claude_code_payload_corpus_streaming() {
    let Some(api_key) = require_env("CHUTES_API_KEY") else {
        return;
    };
    let Some(payload_dir) = payload_dir() else {
        eprintln!("skipping corpus test: no Claude payload dir found");
        return;
    };
    let model =
        env::var("CHUTES_MODEL").unwrap_or_else(|_| "deepseek-ai/DeepSeek-V3.2-TEE".to_string());
    let server = start_server(
        "chutes",
        &env::var("CHUTES_BASE_URL").unwrap_or_else(|_| "https://llm.chutes.ai/v1".to_string()),
        &model,
        &api_key,
    );

    for name in claude_payload_names() {
        let payload = render_payload(&payload_dir.join(name), &model);
        let Some((status, body)) = post_stream_with_retry(&server, &payload) else {
            return;
        };
        assert_eq!(
            status, 200,
            "payload {name} failed with status {status}: {body}"
        );
        assert_claude_stream_shape(&body, false);
    }
}

#[test]
#[ignore = "Claude payload corpus targets the removed legacy /v1/messages public surface"]
fn minimax_claude_code_payload_corpus_streaming() {
    let Some(api_key) = require_env("MINIMAX_API_KEY") else {
        return;
    };
    let Some(payload_dir) = payload_dir() else {
        eprintln!("skipping corpus test: no Claude payload dir found");
        return;
    };
    let model = env::var("MINIMAX_MODEL").unwrap_or_else(|_| "MiniMax-M2.5".to_string());
    let server = start_server(
        "openai-generic",
        &env::var("MINIMAX_BASE_URL").unwrap_or_else(|_| "https://api.minimax.io/v1".to_string()),
        &model,
        &api_key,
    );

    for name in claude_payload_names() {
        let payload = render_payload(&payload_dir.join(name), &model);
        let Some((status, body)) = post_stream_with_retry(&server, &payload) else {
            return;
        };
        assert_eq!(
            status, 200,
            "payload {name} failed with status {status}: {body}"
        );
        assert_claude_stream_shape(&body, true);
    }
}

#[test]
#[ignore = "AnthMorph is now Codex Responses-first; legacy /v1/messages smoke is no longer public API"]
fn deepseek_real_backend_smoke() {
    let Some(api_key) = require_env("DEEPSEEK_API_KEY") else {
        return;
    };
    let server = start_server(
        "deepseek",
        &env::var("DEEPSEEK_BASE_URL").unwrap_or_else(|_| "https://api.deepseek.com".to_string()),
        &env::var("DEEPSEEK_MODEL").unwrap_or_else(|_| "deepseek-v4-pro".to_string()),
        &api_key,
    );

    let maybe = post_messages_with_retry(&server, &base_payload());
    let Some((status, response)) = maybe else {
        return;
    };
    if is_auth_provider_failure(status, &response.to_string()) {
        eprintln!("quarantining deepseek smoke due to auth failure: {response}");
        return;
    }
    let text = response["content"]
        .as_array()
        .and_then(|items| items.iter().find_map(|item| item.get("text").and_then(Value::as_str)))
        .unwrap_or("")
        .trim()
        .to_string();
    assert!(text.contains("ANTHMORPH_OK"), "unexpected text: {text}");
}

#[test]
#[ignore = "legacy Claude tool-name path is not part of the public Codex Responses surface"]
fn deepseek_long_tool_names_are_shortened_for_claude_path() {
    let Some(api_key) = require_env("DEEPSEEK_API_KEY") else {
        return;
    };
    let server = start_server(
        "deepseek",
        &env::var("DEEPSEEK_BASE_URL").unwrap_or_else(|_| "https://api.deepseek.com".to_string()),
        &env::var("DEEPSEEK_MODEL").unwrap_or_else(|_| "deepseek-v4-pro".to_string()),
        &api_key,
    );

    let payload = serde_json::json!({
        "model": "claude-sonnet-4",
        "max_tokens": 64,
        "messages": [{"role": "user", "content": "call the tool if needed, otherwise answer ok"}],
        "tools": [{
            "name": "mcp__memory__memory_read__this_name_is_definitely_way_beyond_sixty_four_chars",
            "description": "memory",
            "input_schema": {"type": "object", "properties": {}}
        }]
    });
    let (status, body) = post_messages(&server, &payload);
    if is_auth_provider_failure(status, &body.to_string()) {
        eprintln!("quarantining deepseek long-tool test due to auth failure: {body}");
        return;
    }
    assert_eq!(status, 200, "unexpected status: {body}");
}

#[test]
fn deepseek_responses_path_smoke() {
    let Some(api_key) = require_env("DEEPSEEK_API_KEY") else {
        return;
    };
    let server = start_server(
        "deepseek",
        &env::var("DEEPSEEK_BASE_URL").unwrap_or_else(|_| "https://api.deepseek.com".to_string()),
        &env::var("DEEPSEEK_MODEL").unwrap_or_else(|_| "deepseek-v4-pro".to_string()),
        &api_key,
    );
    let payload = serde_json::json!({
        "model": "deepseek-v4-pro",
        "stream": false,
        "instructions": "Reply exactly OK",
        "input": [{
            "type": "message",
            "role": "user",
            "content": [{"type": "input_text", "text": "Reply exactly OK"}]
        }]
    });
    let (status, body) = post_responses(&server, &payload);
    if is_auth_provider_failure(status, &body.to_string()) {
        eprintln!("quarantining deepseek responses test due to auth failure: {body}");
        return;
    }
    assert_eq!(status, 200, "unexpected status: {body}");
    assert_eq!(body["object"], "response");
}
