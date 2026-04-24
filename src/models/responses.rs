use serde::{Deserialize, Serialize};
use serde_json::Value;

fn deserialize_input_items<'de, D>(deserializer: D) -> Result<Vec<Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    match value {
        Value::Null => Ok(Vec::new()),
        Value::Array(items) => Ok(items),
        Value::String(text) => Ok(vec![Value::String(text)]),
        other => Ok(vec![other]),
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResponsesRequest {
    pub model: String,
    #[serde(default)]
    pub instructions: Option<String>,
    #[serde(default, deserialize_with = "deserialize_input_items")]
    pub input: Vec<Value>,
    #[serde(default)]
    pub tools: Option<Vec<Value>>,
    #[serde(default)]
    pub tool_choice: Option<Value>,
    #[serde(default)]
    pub stream: Option<bool>,
    #[serde(default)]
    pub reasoning: Option<Value>,
    #[allow(dead_code)]
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResponsesEnvelope {
    pub id: String,
    pub object: String,
    pub model: String,
    pub output: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Value>,
}

#[cfg(test)]
mod tests {
    use super::ResponsesRequest;

    #[test]
    fn deserializes_string_input_as_single_item() {
        let payload = r#"{"model":"deepseek-v4-pro","input":"Reply exactly OK."}"#;
        let request: ResponsesRequest = serde_json::from_str(payload).unwrap();
        assert_eq!(request.input.len(), 1);
        assert_eq!(request.input[0].as_str(), Some("Reply exactly OK."));
    }
}
