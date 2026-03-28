use serde::{Deserialize, Serialize};

// ============================================================================
// Anthropic Request
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
pub struct AnthropicRequest {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(default)]
    pub system: Option<SystemPrompt>,
    #[serde(default)]
    pub stream: Option<bool>,
    #[serde(default)]
    pub max_tokens: usize,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub top_k: Option<i32>,
    #[serde(default)]
    pub tools: Option<Vec<Tool>>,
    #[serde(default)]
    pub stop_sequences: Option<Vec<String>>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone)]
pub enum SystemPrompt {
    Single(String),
    Multiple(Vec<SystemMessage>),
}

impl<'de> Deserialize<'de> for SystemPrompt {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value {
            serde_json::Value::String(s) => Ok(SystemPrompt::Single(s)),
            serde_json::Value::Array(arr) => {
                let messages: Vec<SystemMessage> =
                    serde_json::from_value(serde_json::Value::Array(arr))
                        .map_err(|e| serde::de::Error::custom(e))?;
                Ok(SystemPrompt::Multiple(messages))
            }
            serde_json::Value::Object(obj) => {
                // Check if it's the "Single" or "Multiple" tagged format
                if let Some(text) = obj.get("Single").and_then(|v| v.as_str()) {
                    Ok(SystemPrompt::Single(text.to_string()))
                } else if let Some(arr) = obj.get("Multiple").and_then(|v| v.as_array()) {
                    let messages: Vec<SystemMessage> =
                        serde_json::from_value(serde_json::Value::Array(arr.clone()))
                            .map_err(|e| serde::de::Error::custom(e))?;
                    Ok(SystemPrompt::Multiple(messages))
                } else {
                    Err(serde::de::Error::custom("expected Single or Multiple"))
                }
            }
            _ => Err(serde::de::Error::custom("expected string or array")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemMessage {
    pub text: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: MessageContent,
}

#[derive(Debug, Clone)]
pub enum MessageContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

impl<'de> Deserialize<'de> for MessageContent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value {
            serde_json::Value::String(s) => Ok(MessageContent::Text(s)),
            serde_json::Value::Array(arr) => {
                let blocks: Vec<ContentBlock> =
                    match serde_json::from_value(serde_json::Value::Array(arr.clone())) {
                        Ok(b) => b,
                        Err(_) => {
                            // If array doesn't match ContentBlock variants, treat as text
                            let text = serde_json::to_string(&arr).unwrap_or_default();
                            return Ok(MessageContent::Text(text));
                        }
                    };
                Ok(MessageContent::Blocks(blocks))
            }
            _ => Err(serde::de::Error::custom("expected string or array")),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image { source: ImageSource },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: ToolResultContent,
        #[serde(default)]
        is_error: Option<bool>,
    },
    #[serde(rename = "thinking")]
    Thinking { thinking: String },
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ImageSource {
    pub media_type: String,
    pub data: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Tool {
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "input_schema")]
    pub input_schema: serde_json::Value,
    #[serde(rename = "type")]
    pub tool_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ToolResultContent {
    Text(String),
    Blocks(Vec<serde_json::Value>),
}

// ============================================================================
// Anthropic Response
// ============================================================================

#[derive(Debug, Clone, Serialize)]
pub struct AnthropicResponse {
    pub id: String,
    #[serde(rename = "type")]
    pub response_type: String,
    pub role: String,
    pub content: Vec<ResponseContent>,
    pub model: String,
    #[serde(rename = "stop_reason")]
    pub stop_reason: Option<String>,
    #[serde(rename = "stop_sequence")]
    pub stop_sequence: Option<()>,
    pub usage: Usage,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum ResponseContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "thinking")]
    Thinking { thinking: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    #[serde(rename = "input_tokens")]
    pub input_tokens: usize,
    #[serde(rename = "output_tokens")]
    pub output_tokens: usize,
}

// ============================================================================
// Anthropic SSE Events
// ============================================================================

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum StreamEvent {
    #[serde(rename = "message_start")]
    MessageStart { message: MessageStartData },
    #[serde(rename = "content_block_start")]
    ContentBlockStart {
        index: usize,
        content_block: ContentBlockStartData,
    },
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta {
        index: usize,
        delta: ContentBlockDeltaData,
    },
    #[serde(rename = "content_block_stop")]
    ContentBlockStop { index: usize },
    #[serde(rename = "message_delta")]
    MessageDelta {
        delta: MessageDeltaData,
        #[serde(skip_serializing_if = "Option::is_none")]
        usage: Option<MessageDeltaUsage>,
    },
    #[serde(rename = "message_stop")]
    MessageStop,
}

#[derive(Debug, Clone, Serialize)]
pub struct MessageStartData {
    pub id: String,
    #[serde(rename = "type")]
    pub message_type: String,
    pub role: String,
    pub content: Vec<serde_json::Value>,
    pub model: String,
    #[serde(rename = "stop_reason")]
    pub stop_reason: Option<serde_json::Value>,
    #[serde(rename = "stop_sequence")]
    pub stop_sequence: Option<serde_json::Value>,
    pub usage: Usage,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum ContentBlockStartData {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "thinking")]
    Thinking { thinking: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlockDeltaData {
    TextDelta { text: String },
    ThinkingDelta { thinking: String },
    InputJsonDelta { partial_json: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct MessageDeltaData {
    #[serde(rename = "stop_reason")]
    pub stop_reason: Option<String>,
    #[serde(rename = "stop_sequence")]
    pub stop_sequence: (),
}

#[derive(Debug, Clone, Serialize)]
pub struct MessageDeltaUsage {
    #[serde(rename = "output_tokens")]
    pub output_tokens: usize,
}
