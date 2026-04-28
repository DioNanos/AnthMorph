//! Qwen tool call parser.
//!
//! Handles Qwen's multiple tool calling formats:
//! - XML: `<tool_call>{"name": "func", "arguments": {...}}</tool_call>`
//! - Bracket: `[Calling tool: func_name({"arg": "value"})]`
//! - Function: `<function=name><parameter=key>value</parameter></function>`
//!
//! Reference: vllm-mlx `qwen_tool_parser.py`

use regex::Regex;

use super::{ExtractedToolCall, ExtractedToolCalls, ToolParser};

static TOOL_CALL_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn next_tool_id() -> String {
    let n = TOOL_CALL_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    format!("call_{n:x}")
}

pub struct QwenToolParser {
    xml_pattern: Regex,
    bracket_pattern: Regex,
    function_pattern: Regex,
    param_pattern: Regex,
}

impl Default for QwenToolParser {
    fn default() -> Self {
        Self {
            xml_pattern: Regex::new(r"<tool_call>\s*(\{.*?\})\s*</tool_call>").unwrap(),
            bracket_pattern: Regex::new(r"\[Calling tool:\s*(\w+)\((\{.*?\})\)\]").unwrap(),
            function_pattern: Regex::new(r"<function=([^>]+)>(.*?)</function>").unwrap(),
            param_pattern: Regex::new(r"<parameter=([^>]+)>\s*(.*?)\s*</parameter>").unwrap(),
        }
    }
}

impl ToolParser for QwenToolParser {
    fn extract_tool_calls(&self, model_output: &str) -> ExtractedToolCalls {
        let mut tool_calls = Vec::new();
        let mut cleaned = model_output.to_string();

        // 1. Try bracket pattern (Qwen3 style)
        let bracket_matches: Vec<_> = self.bracket_pattern.find_iter(model_output).collect();
        for m in &bracket_matches {
            let caps = self.bracket_pattern.captures(m.as_str()).unwrap();
            let name = caps.get(1).map(|m| m.as_str().trim()).unwrap_or("");
            let args = caps.get(2).map(|m| m.as_str()).unwrap_or("");
            if !name.is_empty() {
                tool_calls.push(ExtractedToolCall {
                    id: next_tool_id(),
                    name: name.to_string(),
                    arguments: args.to_string(),
                });
            }
        }
        if !bracket_matches.is_empty() {
            cleaned = self.bracket_pattern.replace_all(&cleaned, "").to_string();
        }

        // 2. Try XML pattern (traditional Qwen)
        let xml_matches: Vec<_> = self.xml_pattern.find_iter(model_output).collect();
        for m in &xml_matches {
            let caps = self.xml_pattern.captures(m.as_str()).unwrap();
            let json_str = caps.get(1).map(|m| m.as_str()).unwrap_or("{}");
            if let Ok(data) = serde_json::from_str::<serde_json::Value>(json_str) {
                let name = data.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let args = data.get("arguments").or_else(|| data.get("parameters"));
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
        if !xml_matches.is_empty() {
            cleaned = self.xml_pattern.replace_all(&cleaned, "").to_string();
        }

        // 3. Try function-style: <function=name>...</function>
        if tool_calls.is_empty() {
            for caps in self.function_pattern.captures_iter(model_output) {
                let name = caps.get(1).map(|m| m.as_str().trim()).unwrap_or("");
                let params_block = caps.get(2).map(|m| m.as_str()).unwrap_or("");
                if name.is_empty() {
                    continue;
                }

                // Try JSON arguments first
                let trimmed = params_block.trim();
                if trimmed.starts_with('{') {
                    if serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
                        tool_calls.push(ExtractedToolCall {
                            id: next_tool_id(),
                            name: name.to_string(),
                            arguments: trimmed.to_string(),
                        });
                        continue;
                    }
                }

                // Parse <parameter=key>value</parameter> tags
                let mut arguments = serde_json::Map::new();
                for p_caps in self.param_pattern.captures_iter(params_block) {
                    let key = p_caps.get(1).map(|m| m.as_str().trim()).unwrap_or("");
                    let val = p_caps.get(2).map(|m| m.as_str().trim()).unwrap_or("");
                    if !key.is_empty() {
                        arguments
                            .insert(key.to_string(), serde_json::Value::String(val.to_string()));
                    }
                }
                if !arguments.is_empty() {
                    tool_calls.push(ExtractedToolCall {
                        id: next_tool_id(),
                        name: name.to_string(),
                        arguments: serde_json::to_string(&arguments)
                            .unwrap_or_else(|_| "{}".to_string()),
                    });
                } else if !trimmed.is_empty() {
                    tool_calls.push(ExtractedToolCall {
                        id: next_tool_id(),
                        name: name.to_string(),
                        arguments: trimmed.to_string(),
                    });
                }
            }
        }

        let content = if !tool_calls.is_empty() {
            let c = cleaned.trim();
            if c.is_empty() {
                None
            } else {
                Some(c.to_string())
            }
        } else {
            Some(model_output.to_string())
        };

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
    fn extracts_bracket_format() {
        let parser = QwenToolParser::default();
        let output = r#"I'll check the weather. [Calling tool: get_weather({"city": "Paris"})]"#;
        let result = parser.extract_tool_calls(output);
        assert!(result.tools_called);
        assert_eq!(result.tool_calls[0].name, "get_weather");
        assert!(result.tool_calls[0].arguments.contains("Paris"));
        assert!(result.content.unwrap().contains("check the weather"));
    }

    #[test]
    fn extracts_xml_format() {
        let parser = QwenToolParser::default();
        let output =
            r#"<tool_call>{"name": "get_weather", "arguments": {"city": "London"}}</tool_call>"#;
        let result = parser.extract_tool_calls(output);
        assert!(result.tools_called);
        assert_eq!(result.tool_calls[0].name, "get_weather");
    }

    #[test]
    fn no_tool_call_returns_content() {
        let parser = QwenToolParser::default();
        let result = parser.extract_tool_calls("The weather is sunny.");
        assert!(!result.tools_called);
    }
}
