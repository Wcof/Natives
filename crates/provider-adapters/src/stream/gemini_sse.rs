//! Gemini GenerateContent stream parser → ProviderEvent.

use crate::capabilities::{ProviderError, ProviderErrorCategory, ProviderUsage};
use crate::stream::ProviderEvent;
use serde_json::Value;

/// Parse a single Gemini streaming JSON object (array element or SSE data).
pub fn parse_gemini_chunk(data: &str) -> Vec<ProviderEvent> {
    let data = data.trim().trim_start_matches(',').trim();
    if data.is_empty() || data == "[" || data == "]" {
        return Vec::new();
    }
    let value: Value = match serde_json::from_str(data) {
        Ok(v) => v,
        Err(err) => {
            return vec![ProviderEvent::Error(ProviderError {
                code: "parse_error".into(),
                message: format!("Invalid Gemini JSON: {err}"),
                category: ProviderErrorCategory::ServerError,
                retryable: false,
                retry_after_ms: None,
            })];
        }
    };
    parse_gemini_value(&value)
}

fn parse_gemini_value(value: &Value) -> Vec<ProviderEvent> {
    let mut events = Vec::new();

    if let Some(err) = value.get("error") {
        let message = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("gemini error")
            .chars()
            .take(400)
            .collect::<String>();
        let rate_limited = err.get("code").and_then(|v| v.as_u64()) == Some(429)
            || message.to_ascii_lowercase().contains("rate");
        events.push(ProviderEvent::Error(ProviderError {
            code: "gemini_error".into(),
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

    if let Some(usage) = value.get("usageMetadata") {
        let prompt_tokens = usage
            .get("promptTokenCount")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        // Gemini reports both implicit and explicit (CachedContent) cache hits
        // here, and `promptTokenCount` includes them — subtract to match the
        // `ProviderUsage` contract. Gemini has no request-side parameter for
        // implicit caching, and explicit CachedContent is a separate stateful
        // resource this adapter does not create, so nothing is sent on the way
        // out; this is read-back only.
        let cache_read = usage.get("cachedContentTokenCount").and_then(|v| v.as_u64());
        events.push(ProviderEvent::Usage(ProviderUsage {
            input_tokens: match cache_read {
                Some(cached) => prompt_tokens.saturating_sub(cached),
                None => prompt_tokens,
            },
            output_tokens: usage
                .get("candidatesTokenCount")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            reasoning_tokens: usage.get("thoughtsTokenCount").and_then(|v| v.as_u64()),
            // Gemini does not bill or report a distinct cache-write count.
            cache_creation_tokens: None,
            cache_read_tokens: cache_read,
            cost_usd: None,
        }));
    }

    let candidates = value
        .get("candidates")
        .and_then(|c| c.as_array())
        .cloned()
        .unwrap_or_default();

    for candidate in candidates {
        let parts = candidate
            .get("content")
            .and_then(|c| c.get("parts"))
            .and_then(|p| p.as_array())
            .cloned()
            .unwrap_or_default();
        for (index, part) in parts.into_iter().enumerate() {
            if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                if part.get("thought").and_then(|t| t.as_bool()).unwrap_or(false) {
                    events.push(ProviderEvent::ReasoningDelta(text.to_string()));
                } else if !text.is_empty() {
                    events.push(ProviderEvent::TextDelta(text.to_string()));
                }
            }
            if let Some(fc) = part.get("functionCall") {
                let name = fc
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("tool")
                    .to_string();
                let args = fc.get("args").cloned().unwrap_or(serde_json::json!({}));
                let args_str = args.to_string();
                events.push(ProviderEvent::ToolCallDelta {
                    index,
                    id: Some(format!("gemini_tool_{index}")),
                    name: Some(name),
                    arguments_delta: args_str,
                });
            }
        }
    }

    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_text_and_function_call() {
        let chunk = r#"{
          "candidates": [{
            "content": {
              "parts": [
                {"text": "Hello"},
                {"functionCall": {"name": "read_file", "args": {"path": "a.rs"}}}
              ]
            }
          }],
          "usageMetadata": {"promptTokenCount": 3, "candidatesTokenCount": 5}
        }"#;
        let events = parse_gemini_chunk(chunk);
        assert!(events.iter().any(|e| matches!(e, ProviderEvent::TextDelta(t) if t == "Hello")));
        assert!(events.iter().any(|e| matches!(
            e,
            ProviderEvent::ToolCallDelta { name: Some(n), arguments_delta, .. }
                if n == "read_file" && arguments_delta.contains("a.rs")
        )));
        assert!(events.iter().any(|e| matches!(
            e,
            ProviderEvent::Usage(u) if u.input_tokens == 3 && u.output_tokens == 5
        )));
    }

    #[test]
    fn cached_content_tokens_are_split_out_of_prompt_count() {
        let chunk = r#"{"usageMetadata":{"promptTokenCount":32000,"candidatesTokenCount":40,"cachedContentTokenCount":30720}}"#;
        let events = parse_gemini_chunk(chunk);
        let usage = events
            .iter()
            .find_map(|e| match e {
                ProviderEvent::Usage(u) => Some(u.clone()),
                _ => None,
            })
            .expect("a Usage event");
        assert_eq!(usage.cache_read_tokens, Some(30720));
        assert_eq!(usage.input_tokens, 32000 - 30720);
        assert_eq!(usage.total_prompt_tokens(), 32000);
        // Gemini never reports a distinct cache-write count.
        assert_eq!(usage.cache_creation_tokens, None);
    }

    #[test]
    fn parses_thought_as_reasoning() {
        let chunk = r#"{"candidates":[{"content":{"parts":[{"text":"plan","thought":true}]}}]}"#;
        let events = parse_gemini_chunk(chunk);
        assert!(matches!(
            events.as_slice(),
            [ProviderEvent::ReasoningDelta(t)] if t == "plan"
        ));
    }
}
