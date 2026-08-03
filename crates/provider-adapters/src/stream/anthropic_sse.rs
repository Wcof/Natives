//! Anthropic Messages API SSE parser → ProviderEvent.

use crate::capabilities::{ProviderError, ProviderErrorCategory, ProviderUsage};
use crate::stream::{ProviderEvent, ProviderStopReason};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Deserialize)]
struct AnthropicEvent {
    #[serde(rename = "type")]
    event_type: String,
    index: Option<usize>,
    delta: Option<AnthropicDelta>,
    content_block: Option<ContentBlockStart>,
    message: Option<MessageStart>,
    usage: Option<AnthropicUsage>,
}

#[derive(Debug, Deserialize)]
struct AnthropicDelta {
    #[serde(rename = "type")]
    delta_type: Option<String>,
    text: Option<String>,
    thinking: Option<String>,
    partial_json: Option<String>,
    stop_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ContentBlockStart {
    #[serde(rename = "type")]
    block_type: String,
    id: Option<String>,
    name: Option<String>,
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MessageStart {
    usage: Option<AnthropicUsage>,
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    /// Prompt tokens written to the cache. Present only when the request
    /// carried `cache_control` breakpoints.
    cache_creation_input_tokens: Option<u64>,
    /// Prompt tokens served from the cache.
    cache_read_input_tokens: Option<u64>,
}

impl AnthropicUsage {
    fn to_provider(&self) -> ProviderUsage {
        ProviderUsage {
            // Anthropic already excludes cached tokens from `input_tokens`,
            // which is the convention `ProviderUsage` normalises on.
            input_tokens: self.input_tokens.unwrap_or(0),
            output_tokens: self.output_tokens.unwrap_or(0),
            reasoning_tokens: None,
            cache_creation_tokens: self.cache_creation_input_tokens,
            cache_read_tokens: self.cache_read_input_tokens,
            cost_usd: None,
        }
    }
}

#[derive(Debug, Default)]
pub struct AnthropicSseParser {
    /// content_block index → (id, name, json_acc)
    tools: BTreeMap<usize, (String, String, String)>,
    /// Running usage for this message.
    ///
    /// Anthropic reports the full prompt breakdown (input + both cache
    /// counters) once on `message_start` and then emits a terminal
    /// `message_delta` whose `usage` may carry only `output_tokens`. Replacing
    /// the accumulator on that second event — which is what the consumers do,
    /// since they keep the last `Usage` they see — silently zeroed the input and
    /// cache counts. Folding into a running total keeps the terminal event
    /// complete.
    usage: ProviderUsage,
    finished: bool,
    stop_reason: Option<ProviderStopReason>,
}

impl AnthropicSseParser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_data_line(&mut self, data: &str) -> Vec<ProviderEvent> {
        let data = data.trim();
        if data.is_empty() {
            return Vec::new();
        }
        let event: AnthropicEvent = match serde_json::from_str(data) {
            Ok(e) => e,
            Err(err) => {
                return vec![ProviderEvent::Error(ProviderError {
                    code: "parse_error".into(),
                    message: format!("Invalid Anthropic SSE: {err}"),
                    category: ProviderErrorCategory::ServerError,
                    retryable: false,
                    retry_after_ms: None,
                })];
            }
        };

        let mut out = Vec::new();
        match event.event_type.as_str() {
            "content_block_start" => {
                if let Some(block) = event.content_block {
                    let index = event.index.unwrap_or(0);
                    if block.block_type == "tool_use" {
                        let id = block.id.unwrap_or_default();
                        let name = block.name.unwrap_or_default();
                        self.tools
                            .insert(index, (id.clone(), name.clone(), String::new()));
                        out.push(ProviderEvent::ToolCallDelta {
                            index,
                            id: Some(id),
                            name: Some(name),
                            arguments_delta: String::new(),
                        });
                    } else if block.block_type == "text" {
                        if let Some(text) = block.text {
                            if !text.is_empty() {
                                out.push(ProviderEvent::TextDelta(text));
                            }
                        }
                    }
                }
            }
            "content_block_delta" => {
                if let Some(delta) = event.delta {
                    let index = event.index.unwrap_or(0);
                    match delta.delta_type.as_deref() {
                        Some("text_delta") => {
                            if let Some(text) = delta.text {
                                if !text.is_empty() {
                                    out.push(ProviderEvent::TextDelta(text));
                                }
                            }
                        }
                        Some("thinking_delta") => {
                            if let Some(text) = delta.thinking.or(delta.text) {
                                if !text.is_empty() {
                                    out.push(ProviderEvent::ReasoningDelta(text));
                                }
                            }
                        }
                        Some("input_json_delta") => {
                            if let Some(partial) = delta.partial_json {
                                if let Some(entry) = self.tools.get_mut(&index) {
                                    entry.2.push_str(&partial);
                                }
                                let (id, name) = self
                                    .tools
                                    .get(&index)
                                    .map(|(i, n, _)| (Some(i.clone()), Some(n.clone())))
                                    .unwrap_or((None, None));
                                out.push(ProviderEvent::ToolCallDelta {
                                    index,
                                    id,
                                    name,
                                    arguments_delta: partial,
                                });
                            }
                        }
                        _ => {
                            if let Some(text) = delta.text {
                                if !text.is_empty() {
                                    out.push(ProviderEvent::TextDelta(text));
                                }
                            }
                        }
                    }
                }
            }
            "message_delta" => {
                if let Some(usage) = event.usage {
                    self.usage.merge_from(&usage.to_provider());
                    out.push(ProviderEvent::Usage(self.usage.clone()));
                }
                if let Some(reason) = event.delta.as_ref().and_then(|d| d.stop_reason.as_deref()) {
                    self.stop_reason = Some(ProviderStopReason::from_raw(reason));
                }
            }
            "message_start" => {
                if let Some(msg) = event.message {
                    if let Some(usage) = msg.usage {
                        self.usage.merge_from(&usage.to_provider());
                        out.push(ProviderEvent::Usage(self.usage.clone()));
                    }
                }
            }
            "message_stop" => {
                self.finished = true;
                out.push(ProviderEvent::Completed {
                    reason: self
                        .stop_reason
                        .clone()
                        .unwrap_or_else(|| ProviderStopReason::Unknown("missing_reason".into())),
                });
            }
            "error" => {
                let message = data.chars().take(300).collect::<String>();
                let rate_limited =
                    message.to_ascii_lowercase().contains("rate") || message.contains("429");
                out.push(ProviderEvent::Error(ProviderError {
                    code: "anthropic_error".into(),
                    message,
                    category: if rate_limited {
                        ProviderErrorCategory::RateLimit
                    } else {
                        ProviderErrorCategory::ServerError
                    },
                    retryable: true,
                    retry_after_ms: None,
                }));
            }
            _ => {}
        }
        out
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_text_and_tool_json_deltas() {
        let mut p = AnthropicSseParser::new();
        let e1 = p.push_data_line(
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hi"}}"#,
        );
        assert!(matches!(e1.as_slice(), [ProviderEvent::TextDelta(t)] if t == "Hi"));

        let _ = p.push_data_line(
            r#"{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_1","name":"read_file"}}"#,
        );
        let e3 = p.push_data_line(
            r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"p\":"}}"#,
        );
        assert!(matches!(
            e3.as_slice(),
            [ProviderEvent::ToolCallDelta { name: Some(n), arguments_delta, .. }]
                if n == "read_file" && arguments_delta == "{\"p\":"
        ));
        let done = p.push_data_line(r#"{"type":"message_stop"}"#);
        assert!(matches!(done.as_slice(), [ProviderEvent::Completed { .. }]));
    }

    fn usage_of(events: &[ProviderEvent]) -> ProviderUsage {
        events
            .iter()
            .find_map(|e| match e {
                ProviderEvent::Usage(u) => Some(u.clone()),
                _ => None,
            })
            .expect("a Usage event")
    }

    #[test]
    fn reads_cache_creation_and_read_tokens_from_message_start() {
        let mut p = AnthropicSseParser::new();
        let events = p.push_data_line(
            r#"{"type":"message_start","message":{"usage":{"input_tokens":12,"output_tokens":0,"cache_creation_input_tokens":2048,"cache_read_input_tokens":16384}}}"#,
        );
        let usage = usage_of(&events);
        assert_eq!(usage.input_tokens, 12);
        assert_eq!(usage.cache_creation_tokens, Some(2048));
        assert_eq!(usage.cache_read_tokens, Some(16384));
        // Anthropic excludes cached tokens from input_tokens, so the total is
        // the sum of all three.
        assert_eq!(usage.total_prompt_tokens(), 12 + 2048 + 16384);
        assert!(usage.reported_cache());
    }

    #[test]
    fn terminal_message_delta_keeps_the_prompt_breakdown() {
        let mut p = AnthropicSseParser::new();
        p.push_data_line(
            r#"{"type":"message_start","message":{"usage":{"input_tokens":12,"output_tokens":1,"cache_creation_input_tokens":2048,"cache_read_input_tokens":16384}}}"#,
        );
        // Anthropic's terminal delta commonly carries output_tokens only.
        // Before merging, this event zeroed the input and cache counts for
        // every consumer that keeps the last Usage it sees.
        let events = p.push_data_line(
            r#"{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":312}}"#,
        );
        let usage = usage_of(&events);
        assert_eq!(usage.output_tokens, 312);
        assert_eq!(usage.input_tokens, 12);
        assert_eq!(usage.cache_creation_tokens, Some(2048));
        assert_eq!(usage.cache_read_tokens, Some(16384));
    }

    #[test]
    fn absent_cache_fields_stay_none_rather_than_zero() {
        let mut p = AnthropicSseParser::new();
        let events = p.push_data_line(
            r#"{"type":"message_start","message":{"usage":{"input_tokens":5,"output_tokens":0}}}"#,
        );
        let usage = usage_of(&events);
        assert_eq!(usage.cache_creation_tokens, None);
        assert_eq!(usage.cache_read_tokens, None);
        assert!(
            !usage.reported_cache(),
            "'not reported' must stay distinguishable from 'reported zero'"
        );
    }
}
