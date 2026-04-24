use thiserror::Error;

#[derive(Error, Debug)]
pub enum ProxyError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Upstream error: {0}")]
    Upstream(String),

    #[error("{0}")]
    InvalidRequest(String),

    #[error("Transform error: {0}")]
    Transform(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

impl axum::response::IntoResponse for ProxyError {
    fn into_response(self) -> axum::response::Response {
        use axum::http::StatusCode;

        let (status, error_type) = match &self {
            ProxyError::Http(_) => (StatusCode::BAD_GATEWAY, "api_error"),
            ProxyError::Upstream(msg) => {
                if msg.contains("unauthorized ingress") || msg.contains("401") || msg.contains("403") {
                    (StatusCode::UNAUTHORIZED, "authentication_error")
                } else if msg.contains("429") {
                    (StatusCode::TOO_MANY_REQUESTS, "rate_limit_error")
                } else if msg.contains("404") {
                    (StatusCode::NOT_FOUND, "not_found_error")
                } else if msg.contains("503") || msg.contains("overloaded") {
                    (StatusCode::SERVICE_UNAVAILABLE, "overloaded_error")
                } else if msg.contains("413") || msg.contains("too large") {
                    (StatusCode::PAYLOAD_TOO_LARGE, "request_too_large")
                } else {
                    (StatusCode::BAD_GATEWAY, "api_error")
                }
            }
            ProxyError::InvalidRequest(_) => (StatusCode::BAD_REQUEST, "invalid_request_error"),
            ProxyError::Transform(_) => (StatusCode::BAD_REQUEST, "invalid_request_error"),
            ProxyError::Serialization(_) => (StatusCode::BAD_REQUEST, "invalid_request_error"),
            ProxyError::Io(_) => (StatusCode::INTERNAL_SERVER_ERROR, "api_error"),
        };

        let request_id = format!(
            "req_{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );

        let payload = serde_json::json!({
            "type": "error",
            "error": {
                "type": error_type,
                "message": self.to_string()
            },
            "request_id": request_id
        });
        (status, axum::Json(payload)).into_response()
    }
}

pub type ProxyResult<T> = Result<T, ProxyError>;
