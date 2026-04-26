//! GLM-4.7 tool call parser.
//!
//! Handles GLM-4.7-Flash tool calling format:
//! <tool_call>function_name
//! <arg_key>param1</arg_key><arg_value>value1</arg_value>
//! <arg_key>param2</arg_key><arg_value>value2</arg_value>
//! </tool_call>
//!
//! Reference: vllm-mlx `glm47_tool_parser.py`

use regex::Regex;

use super::{ExtractedToolCall, ExtractedToolCalls, ToolParser};

static TOOL_CALL_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn next_tool_id() -> String {
    let n = TOOL_CALL_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    format!("call_{n:x}")
}

pub struct Glm47ToolParser {
    func_detail_pattern: Regex,
    arg_pattern: Regex,
}

impl Default for Glm47ToolParser {
    fn default() -> Self {
        Self {
            func_detail_pattern: Regex::new(
                r"<tool_call>\s*([\s\S]+?)</tool_call>"
            ).unwrap(),
            arg_pattern: Regex::new(
                r"<arg_key>\s*(.*?)\s*</arg_key>\s*<arg_value>(.*?)</arg_value>"
            ).unwrap(),
        }
    }
}

impl ToolParser for Glm47ToolParser {
    fn extract_tool_calls(&self, model_output: &str) -> ExtractedToolCalls {
        if !model_output.contains("<tool_call>") {
            return ExtractedToolCalls {
                tools_called: false,
                tool_calls: Vec::new(),
                content: Some(model_output.to_string()),
            };
        }

        let mut tool_calls = Vec::new();
        let mut cleaned = model_output.to_string();

        for caps in self.func_detail_pattern.captures_iter(model_output) {
            let inner = caps.get(1).map(|m| m.as_str().trim()).unwrap_or("");
            if inner.is_empty() { continue; }

            // First line is function name, rest is arg definitions
            let (raw_name, args_section) = if let Some(pos) = inner.find('<') {
                (inner[..pos].trim().to_string(), inner[pos..].to_string())
            } else {
                (inner.to_string(), String::new())
            };

            if raw_name.is_empty() { continue; }

            let mut arguments = serde_json::Map::new();
            for a_caps in self.arg_pattern.captures_iter(&args_section) {
                let key = a_caps.get(1).map(|m| m.as_str().trim()).unwrap_or("");
                let val = a_caps.get(2).map(|m| m.as_str().trim()).unwrap_or("");
                if !key.is_empty() {
                    let value = serde_json::from_str::<serde_json::Value>(val)
                        .unwrap_or_else(|_| serde_json::Value::String(val.to_string()));
                    arguments.insert(key.to_string(), value);
                }
            }

            tool_calls.push(ExtractedToolCall {
                id: next_tool_id(),
                name: raw_name,
                arguments: serde_json::to_string(&arguments).unwrap_or_else(|_| "{}".to_string()),
            });
        }

        if !tool_calls.is_empty() {
            cleaned = self.func_detail_pattern.replace_all(&cleaned, "").to_string();
        }

        let tools_called = !tool_calls.is_empty();
        ExtractedToolCalls {
            tools_called,
            tool_calls,
            content: if tools_called {
                let c = cleaned.trim();
                if c.is_empty() { None } else { Some(c.to_string()) }
            } else {
                Some(model_output.to_string())
            },
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_glm_tool_call() {
        let parser = Glm47ToolParser::default();
        let output = "<tool_call>get_weather\n<arg_key>city</arg_key><arg_value>Paris</arg_value>\n</tool_call>";
        let result = parser.extract_tool_calls(output);
        assert!(result.tools_called);
        assert_eq!(result.tool_calls[0].name, "get_weather");
        assert!(result.tool_calls[0].arguments.contains("Paris"));
    }

    #[test]
    fn no_tool_call() {
        let parser = Glm47ToolParser::default();
        let result = parser.extract_tool_calls("Just text.");
        assert!(!result.tools_called);
    }
}
