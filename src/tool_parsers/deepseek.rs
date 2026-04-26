//! DeepSeek tool call parser.
//!
//! DeepSeek V3/R1 models emit tool calls using special unicode tokens:
//!
//! ```text
//! <｜▁tool▁calls▁begin｜>
//! <｜▁tool▁call▁begin｜>function<｜▁tool▁sep｜>get_weather
//! ```json
//! {"city": "Paris"}
//! ```<｜▁tool▁call▁end｜>
//! <｜▁tool▁calls▁end｜>
//! ```
//!
//! Reference: vllm-mlx `deepseek_tool_parser.py`

use std::sync::atomic::{AtomicUsize, Ordering};

use regex::Regex;

use super::{ExtractedToolCall, ExtractedToolCalls, ToolParser};

static TOOL_CALL_COUNTER: AtomicUsize = AtomicUsize::new(0);

fn next_tool_id() -> String {
    let n = TOOL_CALL_COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("call_{n:x}")
}

// DeepSeek special unicode tokens
// U+FF5C = FULLWIDTH VERTICAL LINE (｜)
// U+2581 = LOWER ONE EIGHTH BLOCK (▁)
const TOOL_CALL_START: &str = "<\u{ff5c}tool\u{2581}call\u{2581}begin\u{ff5c}>";
const TOOL_SEP: &str = "<\u{ff5c}tool\u{2581}sep\u{ff5c}>";
const TOOL_CALL_END: &str = "<\u{ff5c}tool\u{2581}call\u{2581}end\u{ff5c}>";

/// DeepSeek tool call parser for V3 and R1 models.
pub struct DeepSeekToolParser {
    /// Pattern: <call_begin>(?P<type>.*?)<sep>(?P<name>.*?)\n```json\n(?P<args>.*?)\n```<call_end>
    pattern_with_type: Regex,
    /// Pattern: <call_begin>(?P<name>.*?)\n```json\n(?P<args>.*?)\n```<call_end>
    pattern_simple: Regex,
}

impl Default for DeepSeekToolParser {
    fn default() -> Self {
        // Build pattern: <call_begin>type<sep>name\n```json\nargs\n```<call_end>
        let pattern_with_type = Regex::new(&format!(
            r"{type_re}(?P<type>.*?){sep}(?P<name>.*?)\n```json\n(?P<args>.*?)\n```{end}",
            type_re = regex::escape(TOOL_CALL_START),
            sep = regex::escape(TOOL_SEP),
            end = regex::escape(TOOL_CALL_END),
        ))
        .expect("valid deepseek regex");

        let pattern_simple = Regex::new(&format!(
            r"{start}(?P<name>.*?)\n```json\n(?P<args>.*?)\n```{end}",
            start = regex::escape(TOOL_CALL_START),
            end = regex::escape(TOOL_CALL_END),
        ))
        .expect("valid deepseek simple regex");

        Self {
            pattern_with_type,
            pattern_simple,
        }
    }
}

impl ToolParser for DeepSeekToolParser {
    fn extract_tool_calls(&self, model_output: &str) -> ExtractedToolCalls {
        // Check for the end marker (tool calls completed)
        let calls_end = "<\u{ff5c}tool\u{2581}calls\u{2581}end\u{ff5c}>";
        let calls_start = "<\u{ff5c}tool\u{2581}calls\u{2581}begin\u{ff5c}>";

        let has_calls = model_output.contains(calls_start) || model_output.contains(TOOL_CALL_START);

        if !has_calls {
            return ExtractedToolCalls {
                tools_called: false,
                tool_calls: Vec::new(),
                content: Some(model_output.to_string()),
            };
        }

        // Extract content before the first tool call marker
        let content = model_output
            .find(TOOL_CALL_START)
            .or_else(|| model_output.find(calls_start))
            .filter(|&pos| pos > 0)
            .map(|pos| model_output[..pos].trim().to_string());

        let mut tool_calls = Vec::new();

        // Try full pattern with type first
        for cap in self.pattern_with_type.captures_iter(model_output) {
            let func_name = cap.name("name").map(|m| m.as_str().trim()).unwrap_or("");
            let func_args = cap.name("args").map(|m| m.as_str()).unwrap_or("");

            if !func_name.is_empty() {
                tool_calls.push(ExtractedToolCall {
                    id: next_tool_id(),
                    name: func_name.to_string(),
                    arguments: func_args.to_string(),
                });
            }
        }

        // Try simple pattern if no matches
        if tool_calls.is_empty() {
            for cap in self.pattern_simple.captures_iter(model_output) {
                let func_name = cap.name("name").map(|m| m.as_str().trim()).unwrap_or("");
                let func_args = cap.name("args").map(|m| m.as_str()).unwrap_or("");

                if !func_name.is_empty() {
                    tool_calls.push(ExtractedToolCall {
                        id: next_tool_id(),
                        name: func_name.to_string(),
                        arguments: func_args.to_string(),
                    });
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

    // Helper to build a DeepSeek tool call string
    fn deepseek_call(name: &str, args_json: &str) -> String {
        let start = "<\u{ff5c}tool\u{2581}call\u{2581}begin\u{ff5c}>";
        let sep = "<\u{ff5c}tool\u{2581}sep\u{ff5c}>";
        let end = "<\u{ff5c}tool\u{2581}call\u{2581}end\u{ff5c}>";
        let calls_end = "<\u{ff5c}tool\u{2581}calls\u{2581}end\u{ff5c}>";
        format!(
            "{start}function{sep}{name}\n```json\n{args_json}\n```{end}{calls_end}"
        )
    }

    #[test]
    fn extracts_simple_tool_call() {
        let parser = DeepSeekToolParser::default();
        let call = deepseek_call("get_weather", r#"{"city": "Paris"}"#);
        let output = format!("Let me check the weather.\n{}", call);

        let result = parser.extract_tool_calls(&output);

        assert!(result.tools_called, "should detect tool calls");
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].name, "get_weather");
        assert!(result.tool_calls[0].arguments.contains("Paris"));
        assert!(result.content.unwrap().contains("check the weather"));
    }

    #[test]
    fn no_tool_call_returns_content() {
        let parser = DeepSeekToolParser::default();
        let output = "The weather in Paris is sunny and 22°C.";

        let result = parser.extract_tool_calls(output);

        assert!(!result.tools_called);
        assert!(result.tool_calls.is_empty());
        assert_eq!(result.content.unwrap(), output);
    }

    #[test]
    fn handles_empty_output() {
        let parser = DeepSeekToolParser::default();
        let result = parser.extract_tool_calls("");

        assert!(!result.tools_called);
    }

    #[test]
    fn extracts_without_type_field() {
        let parser = DeepSeekToolParser::default();
        // Simplified pattern without the "function" type
        let output = format!(
            "Tool time.\n{start}get_weather\n```json\n{{\"city\": \"Paris\"}}\n```{end}",
            start = "<\u{ff5c}tool\u{2581}call\u{2581}begin\u{ff5c}>",
            end = "<\u{ff5c}tool\u{2581}call\u{2581}end\u{ff5c}>",
        );

        let result = parser.extract_tool_calls(&output);

        assert!(result.tools_called);
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].name, "get_weather");
    }
}

// ============================================================================
// Streaming filter for DeepSeek tool calls
// ============================================================================

use std::collections::VecDeque;

/// Filter that detects and suppresses DeepSeek tool call markers from
/// streaming text output, accumulating tool calls until they complete.
///
/// When DeepSeek generates a tool call, the output contains:
/// `<U+FF5C>tool<U+2581>call<U+2581>begin<U+FF5C>function<SEP>name\n```json\n{...}\n```<END>`
///
/// This filter:
/// 1. Suppresses the markup tokens from visible text output
/// 2. Accumulates the full tool call text
/// 3. When the tool call completes, provides the extracted call
pub struct DeepSeekStreamFilter {
    /// Accumulated text since entering a tool call block
    buffer: String,
    /// Whether we're currently inside a tool call
    in_tool_call: bool,
    /// Completed tool calls ready to emit
    completed_calls: VecDeque<ExtractedToolCall>,
}

impl DeepSeekStreamFilter {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            in_tool_call: false,
            completed_calls: VecDeque::new(),
        }
    }

    /// Process a chunk of streaming text.
    ///
    /// Returns `(visible_text, completed_tool_calls)` where:
    /// - `visible_text` is the text safe to emit (tool markup suppressed)
    /// - `completed_tool_calls` is any tool calls that finished in this chunk
    pub fn push(&mut self, chunk: &str) -> (String, Vec<ExtractedToolCall>) {
        let tool_call_start: &str = "<\u{ff5c}tool\u{2581}call\u{2581}begin\u{ff5c}>";
        let tool_call_end: &str = "<\u{ff5c}tool\u{2581}call\u{2581}end\u{ff5c}>";
        let calls_end: &str = "<\u{ff5c}tool\u{2581}calls\u{2581}end\u{ff5c}>";
        let _sep: &str = "<\u{ff5c}tool\u{2581}sep\u{ff5c}>";

        let mut visible = String::new();
        let mut work = chunk;

        loop {
            if self.in_tool_call {
                // Check if tool call completed
                if let Some(end_pos) = work.find(tool_call_end) {
                    self.buffer.push_str(&work[..end_pos + tool_call_end.len()]);

                    // Parse the accumulated tool call
                    let parser = DeepSeekToolParser::default();
                    let result = parser.extract_tool_calls(&self.buffer);
                    if result.tools_called {
                        for tc in result.tool_calls {
                            self.completed_calls.push_back(tc);
                        }
                    }

                    self.buffer.clear();
                    self.in_tool_call = false;
                    work = &work[end_pos + tool_call_end.len()..];
                    continue;
                }

                // Check for calls_end (end of all tool calls)
                if let Some(end_pos) = work.find(calls_end) {
                    self.buffer.push_str(&work[..end_pos + calls_end.len()]);
                    work = &work[end_pos + calls_end.len()..];
                    // Don't clear yet - we're looking for the next call_start
                    continue;
                }

                // Still inside tool call - buffer everything
                self.buffer.push_str(work);
                break;
            }

            // Not in tool call - look for start marker
            if let Some(start_pos) = work.find(tool_call_start) {
                // Emit visible text before the tool call
                visible.push_str(&work[..start_pos]);
                self.buffer.push_str(&work[start_pos..]);
                self.in_tool_call = true;
                work = "";
                continue;
            }

            // No tool markers - pass through as visible text
            visible.push_str(work);
            break;
        }

        let completed: Vec<_> = self.completed_calls.drain(..).collect();
        (visible, completed)
    }

    /// Flush any remaining buffered content when the stream ends.
    pub fn finish(&mut self) -> (String, Vec<ExtractedToolCall>) {
        let result = if self.in_tool_call && !self.buffer.is_empty() {
            // Try to parse any incomplete tool call
            let parser = DeepSeekToolParser::default();
            let parse_result = parser.extract_tool_calls(&self.buffer);
            if parse_result.tools_called {
                for tc in parse_result.tool_calls {
                    self.completed_calls.push_back(tc);
                }
                (String::new(), Vec::<ExtractedToolCall>::new())
            } else {
                // Incomplete tool call - emit as visible text
                let text = std::mem::take(&mut self.buffer);
                (text, Vec::new())
            }
        } else {
            (String::new(), Vec::new())
        };
        let completed: Vec<_> = self.completed_calls.drain(..).collect();
        (result.0, completed)
    }
}
