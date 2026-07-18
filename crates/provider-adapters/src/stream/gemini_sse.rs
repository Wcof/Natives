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
            })];
        }
    };
    parse_gemini_value(&value)
}

fn parse_gemini_value(value: &Value) -> Vec<ProviderEvent> {
    let mut events = Vec::new();

    if let Some(err) = value.get("error") {
        events.push(ProviderEvent::Error(ProviderError {
            code: "gemini_error".into(),
            message: err
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("gemini error")
                .chars()
                .take(400)
                .collect(),
            category: ProviderErrorCategory::ServerError,
            retryable: true,
        }));
        return events;
    }

    if let Some(usage) = value.get("usageMetadata") {
        events.push(ProviderEvent::Usage(ProviderUsage {
            input_tokens: usage
                .get("promptTokenCount")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            output_tokens: usage
                .get("candidatesTokenCount")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            reasoning_tokens: usage.get("thoughtsTokenCount").and_then(|v| v.as_u64()),
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
    fn parses_thought_as_reasoning() {
        let chunk = r#"{"candidates":[{"content":{"parts":[{"text":"plan","thought":true}]}}]}"#;
        let events = parse_gemini_chunk(chunk);
        assert!(matches!(
            events.as_slice(),
            [ProviderEvent::ReasoningDelta(t)] if t == "plan"
        ));
    }
}
