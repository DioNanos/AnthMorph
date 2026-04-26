//! Kimi/Moonshot tool call parser.
//!
//! Handles Kimi K2 tool calling format:
//! <|tool_calls_section_begin|>
//! <|tool_call_begin|>func:0<|tool_call_argument_begin|>{...}<|tool_call_end|>
//! <|tool_calls_section_end|>
//!
//! Reference: vllm-mlx `kimi_tool_parser.py`

use regex::Regex;

use super::{ExtractedToolCall, ExtractedToolCalls, ToolParser};

static TOOL_CALL_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn next_tool_id() -> String {
    let n = TOOL_CALL_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    format!("call_{n:x}")
}

pub struct KimiToolParser {
    pattern: Regex,
}

impl Default for KimiToolParser {
    fn default() -> Self {
        Self {
            pattern: Regex::new(
                r"<\|tool_call_begin\|>\s*(?P<func_id>[^<]+?)(?::\d+)?\s*<\|tool_call_argument_begin\|>\s*(?P<args>.*?)\s*<\|tool_call_end\|>"
            ).unwrap(),
        }
    }
}

impl ToolParser for KimiToolParser {
    fn extract_tool_calls(&self, model_output: &str) -> ExtractedToolCalls {
        let has_calls = model_output.contains("<|tool_calls_section_begin|>")
            || model_output.contains("<|tool_call_section_begin|>")
            || model_output.contains("<|tool_call_begin|>");

        if !has_calls {
            return ExtractedToolCalls {
                tools_called: false,
                tool_calls: Vec::new(),
                content: Some(model_output.to_string()),
            };
        }

        // Extract content before first tool call marker
        let content = model_output.find("<|tool_call_begin|>")
            .or_else(|| model_output.find("<|tool_calls_section_begin|>"))
            .and_then(|pos| {
                if pos > 0 {
                    Some(model_output[..pos].trim().to_string())
                } else {
                    None
                }
            });

        let mut tool_calls = Vec::new();
        for caps in self.pattern.captures_iter(model_output) {
            let func_id = caps.name("func_id").map(|m| m.as_str()).unwrap_or("");
            let args = caps.name("args").map(|m| m.as_str()).unwrap_or("{}");

            // func_id format: functions.get_weather:0 or get_weather:0
            let name = if let Some(colon_pos) = func_id.rfind(':') {
                let base = &func_id[..colon_pos];
                if let Some(dot_pos) = base.rfind('.') {
                    base[dot_pos + 1..].to_string()
                } else {
                    base.to_string()
                }
            } else if let Some(dot_pos) = func_id.rfind('.') {
                func_id[dot_pos + 1..].to_string()
            } else {
                func_id.to_string()
            };

            if !name.is_empty() {
                tool_calls.push(ExtractedToolCall {
                    id: next_tool_id(),
                    name,
                    arguments: args.to_string(),
                });
            }
        }

        let tools_called = !tool_calls.is_empty();
        ExtractedToolCalls {
            tools_called,
            tool_calls,
            content: if tools_called { content } else { Some(model_output.to_string()) },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_kimi_tool_call() {
        let parser = KimiToolParser::default();
        let output = "Let me check.\n<|tool_calls_section_begin|>\n<|tool_call_begin|>functions.get_weather:0<|tool_call_argument_begin|>{\"city\": \"Paris\"}<|tool_call_end|>\n<|tool_calls_section_end|>";
        let result = parser.extract_tool_calls(output);
        assert!(result.tools_called);
        assert_eq!(result.tool_calls[0].name, "get_weather");
        assert!(result.tool_calls[0].arguments.contains("Paris"));
    }

    #[test]
    fn no_tool_call() {
        let parser = KimiToolParser::default();
        let result = parser.extract_tool_calls("Some text.");
        assert!(!result.tools_called);
    }
}
