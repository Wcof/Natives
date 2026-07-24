//! OpenAI Responses API event parser (subset) → ProviderEvent.

use crate::capabilities::{ProviderError, ProviderErrorCategory, ProviderUsage};
use crate::stream::ProviderEvent;
use serde_json::Value;

/// Parse one Responses API SSE `data:` JSON object.
pub fn parse_responses_event(data: &str) -> Vec<ProviderEvent> {
    let data = data.trim();
    if data.is_empty() {
        return Vec::new();
    }
    if data == "[DONE]" {
        return vec![ProviderEvent::Completed];
    }
    let value: Value = match serde_json::from_str(data) {
        Ok(v) => v,
        Err(err) => {
            return vec![ProviderEvent::Error(ProviderError {
                code: "parse_error".into(),
                message: format!("Invalid Responses JSON: {err}"),
                category: ProviderErrorCategory::ServerError,
                retryable: false,
                retry_after_ms: None,
            })];
        }
    };

    let mut events = Vec::new();
    let event_type = value.get("type").and_then(|t| t.as_str()).unwrap_or("");

    match event_type {
        "response.output_text.delta" => {
            if let Some(text) = value.get("delta").and_then(|d| d.as_str()) {
                if !text.is_empty() {
                    events.push(ProviderEvent::TextDelta(text.to_string()));
                }
            }
        }
        "response.reasoning.delta" | "response.reasoning_summary_text.delta" => {
            if let Some(text) = value.get("delta").and_then(|d| d.as_str()) {
                if !text.is_empty() {
                    events.push(ProviderEvent::ReasoningDelta(text.to_string()));
                }
            }
        }
        "response.function_call_arguments.delta" => {
            let args = value
                .get("delta")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .to_string();
            let name = value
                .get("name")
                .and_then(|n| n.as_str())
                .map(str::to_string);
            let id = value
                .get("item_id")
                .or_else(|| value.get("call_id"))
                .and_then(|i| i.as_str())
                .map(str::to_string);
            events.push(ProviderEvent::ToolCallDelta {
                index: value.get("output_index").and_then(|i| i.as_u64()).unwrap_or(0) as usize,
                id,
                name,
                arguments_delta: args,
            });
        }
        "response.completed" => {
            if let Some(usage) = value.pointer("/response/usage") {
                events.push(ProviderEvent::Usage(ProviderUsage {
                    input_tokens: usage
                        .get("input_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0),
                    output_tokens: usage
                        .get("output_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0),
                    reasoning_tokens: usage
                        .pointer("/output_tokens_details/reasoning_tokens")
                        .and_then(|v| v.as_u64()),
                    cost_usd: None,
                }));
            }
            events.push(ProviderEvent::Completed);
        }
        "error" => {
            let message = value
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("responses error")
                .chars()
                .take(400)
                .collect::<String>();
            let rate_limited = message.to_ascii_lowercase().contains("rate")
                || value.get("code").and_then(|v| v.as_str()).unwrap_or_default() == "429";
            events.push(ProviderEvent::Error(ProviderError {
                code: "responses_error".into(),
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

    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_text_reasoning_and_tool_deltas() {
        let t = parse_responses_event(
            r#"{"type":"response.output_text.delta","delta":"Hi"}"#,
        );
        assert!(matches!(t.as_slice(), [ProviderEvent::TextDelta(s)] if s == "Hi"));

        let r = parse_responses_event(
            r#"{"type":"response.reasoning.delta","delta":"think"}"#,
        );
        assert!(matches!(r.as_slice(), [ProviderEvent::ReasoningDelta(s)] if s == "think"));

        let tool = parse_responses_event(
            r#"{"type":"response.function_call_arguments.delta","delta":"{\"a\":1}","name":"read_file","item_id":"fc_1","output_index":0}"#,
        );
        assert!(matches!(
            tool.as_slice(),
            [ProviderEvent::ToolCallDelta { name: Some(n), id: Some(i), arguments_delta, .. }]
                if n == "read_file" && i == "fc_1" && arguments_delta.contains("a")
        ));
    }
}
