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
    env::var("CARGO_BIN_EXE_anthmorph").expect("cargo should expose anthmorph bin path")
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

fn base_payload() -> Value {
    serde_json::json!({
        "model": "claude-sonnet-4",
        "max_tokens": 128,
        "messages": [
            {"role": "user", "content": "Reply with exactly: anthmorph-smoke-ok"}
        ]
    })
}

#[test]
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

    let (status, response) = post_messages(&server, &base_payload());
    assert_eq!(status, 200, "unexpected status: {response}");
    let text = response["content"][0]["text"]
        .as_str()
        .expect("text response");
    assert_eq!(text.trim(), "anthmorph-smoke-ok");
}

#[test]
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

    let (status, response) = post_messages(&server, &base_payload());
    assert_eq!(status, 200, "unexpected status: {response}");
    let text = response["content"][0]["text"]
        .as_str()
        .expect("text response");
    assert!(
        text.contains("anthmorph-smoke-ok"),
        "unexpected text: {text}"
    );
}

#[test]
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
