//! OpenAI Chat Completions SSE parser.
//!
//! Shared by OpenAI, DeepSeek, Ollama (OpenAI-compatible), and custom
//! OpenAI-compatible endpoints. Pure transform: raw SSE lines → ProviderEvent.

use crate::capabilities::{ProviderError, ProviderErrorCategory, ProviderUsage};
use serde::Deserialize;
use std::collections::BTreeMap;

/// Unified provider stream event (Protocol / Adapter boundary).
#[derive(Debug, Clone)]
pub enum ProviderEvent {
    TextDelta(String),
    ReasoningDelta(String),
    ToolCallDelta {
        index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments_delta: String,
    },
    Usage(ProviderUsage),
    Completed,
    Error(ProviderError),
}

#[derive(Debug, Deserialize)]
struct ChatChunk {
    choices: Option<Vec<ChatChoice>>,
    usage: Option<UsageWire>,
    error: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    delta: Option<ChatDelta>,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatDelta {
    content: Option<String>,
    reasoning_content: Option<String>,
    /// Some providers put reasoning under `reasoning`.
    reasoning: Option<String>,
    tool_calls: Option<Vec<ToolCallDeltaWire>>,
}

#[derive(Debug, Deserialize)]
struct ToolCallDeltaWire {
    index: Option<usize>,
    id: Option<String>,
    #[serde(rename = "type")]
    _type: Option<String>,
    function: Option<ToolFunctionDelta>,
}

#[derive(Debug, Deserialize)]
struct ToolFunctionDelta {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UsageWire {
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
    total_tokens: Option<u64>,
    completion_tokens_details: Option<CompletionDetails>,
}

#[derive(Debug, Deserialize)]
struct CompletionDetails {
    reasoning_tokens: Option<u64>,
}

/// Stateful accumulator for one SSE stream.
#[derive(Debug, Default)]
pub struct OpenAiSseParser {
    /// tool_call index → (id, name, arguments)
    tool_acc: BTreeMap<usize, (String, String, String)>,
    finished: bool,
}

impl OpenAiSseParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse a single SSE `data:` payload (without the `data:` prefix).
    /// Returns zero or more provider events.
    pub fn push_data_line(&mut self, data: &str) -> Vec<ProviderEvent> {
        let data = data.trim();
        if data.is_empty() {
            return Vec::new();
        }
        if data == "[DONE]" {
            self.finished = true;
            return vec![ProviderEvent::Completed];
        }

        let chunk: ChatChunk = match serde_json::from_str(data) {
            Ok(c) => c,
            Err(err) => {
                return vec![ProviderEvent::Error(ProviderError {
                    code: "parse_error".into(),
                    message: format!("Invalid SSE JSON: {err}"),
                    category: ProviderErrorCategory::ServerError,
                    retryable: false,
                    retry_after_ms: None,
                })];
            }
        };

        let mut events = Vec::new();

        if let Some(error) = chunk.error {
            let message = error
                .get("message")
                .and_then(|value| value.as_str())
                .unwrap_or("provider stream error")
                .chars()
                .take(400)
                .collect::<String>();
            let rate_limited = error
                .get("type")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_ascii_lowercase()
                .contains("rate")
                || message.to_ascii_lowercase().contains("rate limit");
            events.push(ProviderEvent::Error(ProviderError {
                code: "chat_stream_error".into(),
                message,
                category: if rate_limited {
                    ProviderErrorCategory::RateLimit
                } else {
                    ProviderErrorCategory::ServerError
                },
                retryable: true,
                retry_after_ms: None,
            }));
            return events;
        }

        if let Some(usage) = chunk.usage {
            events.push(ProviderEvent::Usage(ProviderUsage {
                input_tokens: usage.prompt_tokens.unwrap_or(0),
                output_tokens: usage.completion_tokens.unwrap_or(usage.total_tokens.unwrap_or(0)),
                reasoning_tokens: usage
                    .completion_tokens_details
                    .and_then(|d| d.reasoning_tokens),
                cost_usd: None,
            }));
        }

        for choice in chunk.choices.unwrap_or_default() {
            if let Some(delta) = choice.delta {
                if let Some(text) = delta.content {
                    if !text.is_empty() {
                        events.push(ProviderEvent::TextDelta(text));
                    }
                }
                let reasoning = delta.reasoning_content.or(delta.reasoning);
                if let Some(text) = reasoning {
                    if !text.is_empty() {
                        events.push(ProviderEvent::ReasoningDelta(text));
                    }
                }
                for tc in delta.tool_calls.unwrap_or_default() {
                    let index = tc.index.unwrap_or(0);
                    let entry = self.tool_acc.entry(index).or_insert_with(|| {
                        (String::new(), String::new(), String::new())
                    });
                    if let Some(id) = tc.id {
                        if !id.is_empty() {
                            entry.0 = id.clone();
                        }
                    }
                    if let Some(function) = tc.function {
                        if let Some(name) = function.name {
                            if !name.is_empty() {
                                entry.1 = name.clone();
                            }
                        }
                        let args_delta = function.arguments.unwrap_or_default();
                        if !args_delta.is_empty() {
                            entry.2.push_str(&args_delta);
                        }
                        events.push(ProviderEvent::ToolCallDelta {
                            index,
                            id: if entry.0.is_empty() {
                                None
                            } else {
                                Some(entry.0.clone())
                            },
                            name: if entry.1.is_empty() {
                                None
                            } else {
                                Some(entry.1.clone())
                            },
                            arguments_delta: args_delta,
                        });
                    }
                }
            }
            if choice.finish_reason.is_some() {
                // Stream may still send usage after finish_reason; Completed
                // is emitted on [DONE] or by the caller after the body ends.
            }
        }

        events
    }

    /// Drain completed tool calls (id, name, arguments JSON string).
    pub fn finished_tool_calls(&self) -> Vec<(String, String, String)> {
        self.tool_acc
            .values()
            .cloned()
            .filter(|(id, name, _)| !id.is_empty() || !name.is_empty())
            .collect()
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }
}

/// Split an SSE buffer into complete lines, returning remainder.
pub fn split_sse_lines(buffer: &mut String) -> Vec<String> {
    let mut lines = Vec::new();
    while let Some(idx) = buffer.find('\n') {
        let mut line = buffer[..idx].to_string();
        buffer.drain(..=idx);
        if line.ends_with('\r') {
            line.pop();
        }
        lines.push(line);
    }
    lines
}

/// Extract `data:` payload from an SSE line, if any.
pub fn sse_data_payload(line: &str) -> Option<&str> {
    let line = line.trim_end();
    if let Some(rest) = line.strip_prefix("data:") {
        Some(rest.trim_start())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_text_and_tool_call_deltas() {
        let mut parser = OpenAiSseParser::new();
        let e1 = parser.push_data_line(
            r#"{"choices":[{"delta":{"content":"Hello"}}]}"#,
        );
        assert!(matches!(e1.as_slice(), [ProviderEvent::TextDelta(t)] if t == "Hello"));

        let e2 = parser.push_data_line(
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"read_file","arguments":"{\"p"}}]}}]}"#,
        );
        assert!(matches!(
            e2.as_slice(),
            [ProviderEvent::ToolCallDelta {
                index: 0,
                id: Some(id),
                name: Some(name),
                arguments_delta
            }] if id == "call_1" && name == "read_file" && arguments_delta == "{\"p"
        ));

        let e3 = parser.push_data_line(
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"ath\":\"a\"}"}}]}}]}"#,
        );
        assert!(matches!(
            e3.as_slice(),
            [ProviderEvent::ToolCallDelta { arguments_delta, .. }] if arguments_delta == "ath\":\"a\"}"
        ));

        let done = parser.push_data_line("[DONE]");
        assert!(matches!(done.as_slice(), [ProviderEvent::Completed]));
    }

    #[test]
    fn parses_reasoning_content() {
        let mut parser = OpenAiSseParser::new();
        let events = parser.push_data_line(
            r#"{"choices":[{"delta":{"reasoning_content":"plan step"}}]}"#,
        );
        assert!(matches!(
            events.as_slice(),
            [ProviderEvent::ReasoningDelta(t)] if t == "plan step"
        ));
    }

    #[test]
    fn split_sse_lines_keeps_remainder() {
        let mut buf = "data: {\"a\":1}\n partial".to_string();
        let lines = split_sse_lines(&mut buf);
        assert_eq!(lines, vec!["data: {\"a\":1}"]);
        assert_eq!(buf, " partial");
    }
}
