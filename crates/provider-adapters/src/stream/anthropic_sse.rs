//! Anthropic Messages API SSE parser → ProviderEvent.

use crate::capabilities::{ProviderError, ProviderErrorCategory, ProviderUsage};
use crate::stream::ProviderEvent;
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
}

#[derive(Debug, Default)]
pub struct AnthropicSseParser {
    /// content_block index → (id, name, json_acc)
    tools: BTreeMap<usize, (String, String, String)>,
    finished: bool,
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
                        self.tools.insert(index, (id.clone(), name.clone(), String::new()));
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
                    out.push(ProviderEvent::Usage(ProviderUsage {
                        input_tokens: usage.input_tokens.unwrap_or(0),
                        output_tokens: usage.output_tokens.unwrap_or(0),
                        reasoning_tokens: None,
                        cost_usd: None,
                    }));
                }
                if event
                    .delta
                    .as_ref()
                    .and_then(|d| d.stop_reason.as_ref())
                    .is_some()
                {
                    // terminal handled on message_stop
                }
            }
            "message_start" => {
                if let Some(msg) = event.message {
                    if let Some(usage) = msg.usage {
                        out.push(ProviderEvent::Usage(ProviderUsage {
                            input_tokens: usage.input_tokens.unwrap_or(0),
                            output_tokens: usage.output_tokens.unwrap_or(0),
                            reasoning_tokens: None,
                            cost_usd: None,
                        }));
                    }
                }
            }
            "message_stop" => {
                self.finished = true;
                out.push(ProviderEvent::Completed);
            }
            "error" => {
                let message = data.chars().take(300).collect::<String>();
                let rate_limited = message.to_ascii_lowercase().contains("rate")
                    || message.contains("429");
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
        assert!(matches!(done.as_slice(), [ProviderEvent::Completed]));
    }
}
