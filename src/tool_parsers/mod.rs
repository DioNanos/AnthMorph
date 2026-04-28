//! Tool call parsers for extracting function calls from model output text.
//!
//! Different backends emit tool calls in different formats (XML tags, unicode
//! tokens, JSON arrays, etc.). These parsers extract structured tool calls from
//! the raw text output after generation.

pub mod deepseek;
pub mod glm47;
pub mod kimi;
pub mod mistral;
pub mod qwen;

/// Information extracted from model output about tool calls.
#[derive(Debug, Clone, Default)]
pub struct ExtractedToolCalls {
    /// Whether any tool calls were detected and extracted.
    pub tools_called: bool,
    /// Extracted tool calls with name and arguments (JSON string).
    pub tool_calls: Vec<ExtractedToolCall>,
    /// Text content that was not part of any tool call.
    pub content: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ExtractedToolCall {
    pub id: String,
    pub name: String,
    /// JSON string of arguments.
    pub arguments: String,
}

/// Trait for extracting tool calls from model output text.
pub trait ToolParser: Send + Sync {
    /// Parse a complete model output and extract any tool calls.
    fn extract_tool_calls(&self, model_output: &str) -> ExtractedToolCalls;

    /// Parse streaming output, returning `None` while building a tool call
    /// and `Some(...)` when a tool call completes or content is available.
    fn extract_tool_calls_streaming(
        &self,
        _previous_text: &str,
        _current_text: &str,
        _delta_text: &str,
    ) -> Option<ExtractedStreamingDelta> {
        Some(ExtractedStreamingDelta {
            content: Some(_delta_text.to_string()),
            tool_calls: Vec::new(),
        })
    }

    /// Reset any streaming state between requests.
    fn reset(&mut self) {}
}

/// A single delta from streaming tool call extraction.
#[derive(Debug, Clone)]
pub struct ExtractedStreamingDelta {
    /// Text content to emit (if any). `None` means suppress output.
    pub content: Option<String>,
    /// Completed tool calls in this delta.
    pub tool_calls: Vec<StreamingToolCall>,
}

#[derive(Debug, Clone)]
pub struct StreamingToolCall {
    pub index: usize,
    pub id: String,
    pub name: String,
    pub arguments: String,
}
