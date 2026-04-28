//! Mistral tool call parser.
//!
//! Handles Mistral tool calling format:
//! - Old: `[TOOL_CALLS] [{"name": "func", "arguments": {...}}]`
//! - New: `[TOOL_CALLS]func_name{"arg": "value"}`
//!
//! Used with Mistral-7B, Devstral, and similar models.
//! Reference: vllm-mlx `mistral_tool_parser.py`

use super::{ExtractedToolCall, ExtractedToolCalls, ToolParser};

static TOOL_CALL_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn next_tool_id() -> String {
    let n = TOOL_CALL_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    format!("call_{n:x}")
}

const BOT_TOKEN: &str = "[TOOL_CALLS]";

pub struct MistralToolParser;

impl Default for MistralToolParser {
    fn default() -> Self {
        Self
    }
}

impl ToolParser for MistralToolParser {
    fn extract_tool_calls(&self, model_output: &str) -> ExtractedToolCalls {
        if !model_output.contains(BOT_TOKEN) {
            return ExtractedToolCalls {
                tools_called: false,
                tool_calls: Vec::new(),
                content: Some(model_output.to_string()),
            };
        }

        let parts: Vec<&str> = model_output.splitn(2, BOT_TOKEN).collect();
        let content = parts
            .first()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let raw = parts.get(1).map(|s| s.trim()).unwrap_or("");

        let mut tool_calls = Vec::new();

        if raw.is_empty() {
            return ExtractedToolCalls {
                tools_called: false,
                tool_calls: Vec::new(),
                content: content.or_else(|| Some(model_output.to_string())),
            };
        }

        // Try new format: func_name{"arg": "value"}
        if !raw.starts_with('[') && raw.contains('{') {
            if let Some(brace_pos) = raw.find('{') {
                let name = raw[..brace_pos].trim();
                let args = raw[brace_pos..].to_string();
                if !name.is_empty() {
                    tool_calls.push(ExtractedToolCall {
                        id: next_tool_id(),
                        name: name.to_string(),
                        arguments: args,
                    });
                }
            }
        }

        // Try old format: [{"name": "func", "arguments": {...}}]
        if tool_calls.is_empty() {
            if let Ok(parsed) = serde_json::from_str::<Vec<serde_json::Value>>(raw) {
                for item in parsed {
                    if let Some(obj) = item.as_object() {
                        let name = obj.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        let args = obj.get("arguments").or_else(|| obj.get("parameters"));
                        if !name.is_empty() {
                            let args_str = args
                                .map(|a| a.to_string())
                                .unwrap_or_else(|| "{}".to_string());
                            tool_calls.push(ExtractedToolCall {
                                id: next_tool_id(),
                                name: name.to_string(),
                                arguments: args_str,
                            });
                        }
                    }
                }
            }
        }

        ExtractedToolCalls {
            tools_called: !tool_calls.is_empty(),
            tool_calls,
            content,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_new_format() {
        let parser = MistralToolParser::default();
        let output = r#"I'll help. [TOOL_CALLS]get_weather{"city": "Paris"}"#;
        let result = parser.extract_tool_calls(output);
        assert!(result.tools_called);
        assert_eq!(result.tool_calls[0].name, "get_weather");
        assert!(result.content.unwrap().contains("help"));
    }

    #[test]
    fn extracts_old_format() {
        let parser = MistralToolParser::default();
        let output = r#"[TOOL_CALLS] [{"name": "get_weather", "arguments": {"city": "Paris"}}]"#;
        let result = parser.extract_tool_calls(output);
        assert!(result.tools_called);
        assert_eq!(result.tool_calls[0].name, "get_weather");
    }

    #[test]
    fn no_tool_call() {
        let parser = MistralToolParser::default();
        let result = parser.extract_tool_calls("Just text.");
        assert!(!result.tools_called);
    }
}
