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
                let input_tokens = usage
                    .get("input_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                // Automatic prefix caching. `input_tokens` includes the cached
                // span, so subtract it to match the `ProviderUsage` contract.
                let cache_read = usage
                    .pointer("/input_tokens_details/cached_tokens")
                    .and_then(|v| v.as_u64());
                events.push(ProviderEvent::Usage(ProviderUsage {
                    input_tokens: match cache_read {
                        Some(cached) => input_tokens.saturating_sub(cached),
                        None => input_tokens,
                    },
                    output_tokens: usage
                        .get("output_tokens")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0),
                    reasoning_tokens: usage
                        .pointer("/output_tokens_details/reasoning_tokens")
                        .and_then(|v| v.as_u64()),
                    // The Responses API does not report cache writes separately.
                    cache_creation_tokens: None,
                    cache_read_tokens: cache_read,
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

    #[test]
    fn completed_event_splits_cached_input_tokens() {
        let events = parse_responses_event(
            r#"{"type":"response.completed","response":{"usage":{"input_tokens":9000,"output_tokens":200,"input_tokens_details":{"cached_tokens":8192},"output_tokens_details":{"reasoning_tokens":64}}}}"#,
        );
        let usage = events
            .iter()
            .find_map(|e| match e {
                ProviderEvent::Usage(u) => Some(u.clone()),
                _ => None,
            })
            .expect("a Usage event");
        assert_eq!(usage.cache_read_tokens, Some(8192));
        assert_eq!(usage.input_tokens, 9000 - 8192);
        assert_eq!(usage.total_prompt_tokens(), 9000);
        assert_eq!(usage.reasoning_tokens, Some(64));
        assert!(events
            .iter()
            .any(|e| matches!(e, ProviderEvent::Completed)));
    }
}
