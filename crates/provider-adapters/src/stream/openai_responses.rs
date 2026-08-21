//! Stateful OpenAI Responses API event parser -> [`ProviderEvent`].

use crate::capabilities::{ProviderError, ProviderErrorCategory, ProviderUsage};
use crate::stream::{ProviderEvent, ProviderStopReason};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Default)]
struct ToolState {
    id: Option<String>,
    name: Option<String>,
    saw_arguments: bool,
}

#[derive(Debug, Default)]
pub struct OpenAiResponsesParser {
    tools: BTreeMap<usize, ToolState>,
    finished: bool,
}

impl OpenAiResponsesParser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_data_line(&mut self, data: &str) -> Vec<ProviderEvent> {
        let data = data.trim();
        if data.is_empty() {
            return Vec::new();
        }
        if data == "[DONE]" {
            self.finished = true;
            return vec![ProviderEvent::Completed {
                reason: ProviderStopReason::Unknown("missing_reason".into()),
            }];
        }

        let value: Value = match serde_json::from_str(data) {
            Ok(value) => value,
            Err(error) => {
                return vec![ProviderEvent::Error(ProviderError {
                    code: "parse_error".into(),
                    message: format!("Invalid Responses JSON: {error}"),
                    category: ProviderErrorCategory::ServerError,
                    retryable: false,
                    retry_after_ms: None,
                })];
            }
        };

        match value.get("type").and_then(Value::as_str).unwrap_or("") {
            "response.output_text.delta" => text_delta(&value),
            "response.reasoning.delta"
            | "response.reasoning_text.delta"
            | "response.reasoning_summary_text.delta" => reasoning_delta(&value),
            "response.output_item.added" => self.output_item_added(&value),
            "response.function_call_arguments.delta" => self.tool_arguments_delta(&value),
            "response.function_call_arguments.done" => self.tool_arguments_done(&value),
            "response.completed" => {
                self.finished = true;
                terminal_events(&value, ProviderStopReason::Stop)
            }
            "response.incomplete" => {
                self.finished = true;
                terminal_events(&value, ProviderStopReason::Length)
            }
            "response.cancelled" => {
                self.finished = true;
                terminal_events(&value, ProviderStopReason::Cancelled)
            }
            "response.failed" | "error" => {
                self.finished = true;
                vec![ProviderEvent::Error(response_error(&value))]
            }
            _ => Vec::new(),
        }
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    fn output_item_added(&mut self, value: &Value) -> Vec<ProviderEvent> {
        let Some(item) = value.get("item") else {
            return Vec::new();
        };
        if item.get("type").and_then(Value::as_str) != Some("function_call") {
            return Vec::new();
        }

        let index = output_index(value);
        let id = string_field(item, "call_id").or_else(|| string_field(item, "id"));
        let name = string_field(item, "name");
        self.tools.insert(
            index,
            ToolState {
                id: id.clone(),
                name: name.clone(),
                saw_arguments: false,
            },
        );
        vec![ProviderEvent::ToolCallDelta {
            index,
            id,
            name,
            arguments_delta: String::new(),
        }]
    }

    fn tool_arguments_delta(&mut self, value: &Value) -> Vec<ProviderEvent> {
        let index = output_index(value);
        let state = self.tools.entry(index).or_default();
        update_tool_identity(state, value);
        state.saw_arguments = true;
        vec![ProviderEvent::ToolCallDelta {
            index,
            id: state.id.clone(),
            name: state.name.clone(),
            arguments_delta: value
                .get("delta")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        }]
    }

    fn tool_arguments_done(&mut self, value: &Value) -> Vec<ProviderEvent> {
        let index = output_index(value);
        let state = self.tools.entry(index).or_default();
        update_tool_identity(state, value);
        if state.saw_arguments {
            return Vec::new();
        }
        let arguments = value.get("arguments").and_then(Value::as_str).unwrap_or("");
        if arguments.is_empty() {
            return Vec::new();
        }
        state.saw_arguments = true;
        vec![ProviderEvent::ToolCallDelta {
            index,
            id: state.id.clone(),
            name: state.name.clone(),
            arguments_delta: arguments.to_string(),
        }]
    }
}

/// Parse one standalone Responses event.
///
/// Callers processing a stream must keep one [`OpenAiResponsesParser`] for the
/// whole response so tool identity survives across event boundaries.
pub fn parse_responses_event(data: &str) -> Vec<ProviderEvent> {
    OpenAiResponsesParser::new().push_data_line(data)
}

fn text_delta(value: &Value) -> Vec<ProviderEvent> {
    value
        .get("delta")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(|text| vec![ProviderEvent::TextDelta(text.to_string())])
        .unwrap_or_default()
}

fn reasoning_delta(value: &Value) -> Vec<ProviderEvent> {
    value
        .get("delta")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(|text| vec![ProviderEvent::ReasoningDelta(text.to_string())])
        .unwrap_or_default()
}

fn terminal_events(value: &Value, fallback_reason: ProviderStopReason) -> Vec<ProviderEvent> {
    let response = value.get("response").unwrap_or(value);
    let mut events = response
        .get("usage")
        .map(provider_usage)
        .map(ProviderEvent::Usage)
        .into_iter()
        .collect::<Vec<_>>();
    events.push(ProviderEvent::Completed {
        reason: response_stop_reason(response, fallback_reason),
    });
    events
}

fn response_stop_reason(response: &Value, fallback: ProviderStopReason) -> ProviderStopReason {
    if let Some(reason) = response
        .pointer("/incomplete_details/reason")
        .and_then(Value::as_str)
    {
        return ProviderStopReason::from_raw(reason);
    }
    if response
        .get("output")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items
                .iter()
                .any(|item| item.get("type").and_then(Value::as_str) == Some("function_call"))
        })
    {
        return ProviderStopReason::ToolUse;
    }
    fallback
}

fn provider_usage(usage: &Value) -> ProviderUsage {
    let input_tokens = u64_field(usage, "input_tokens");
    let cache_read_tokens = usage
        .pointer("/input_tokens_details/cached_tokens")
        .and_then(Value::as_u64);
    ProviderUsage {
        input_tokens: cache_read_tokens
            .map(|cached| input_tokens.saturating_sub(cached))
            .unwrap_or(input_tokens),
        output_tokens: u64_field(usage, "output_tokens"),
        reasoning_tokens: usage
            .pointer("/output_tokens_details/reasoning_tokens")
            .and_then(Value::as_u64),
        cache_creation_tokens: None,
        cache_read_tokens,
        cost_usd: None,
    }
}

fn response_error(value: &Value) -> ProviderError {
    let error = value
        .pointer("/response/error")
        .or_else(|| value.get("error"))
        .unwrap_or(value);
    let raw_message = error
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("responses stream failed");
    let message = crate::redact::redact_secrets(raw_message)
        .chars()
        .take(400)
        .collect::<String>();
    let code = error
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or("responses_error");
    let rate_limited = code == "429" || message.to_ascii_lowercase().contains("rate limit");
    ProviderError {
        code: code.to_string(),
        message,
        category: if rate_limited {
            ProviderErrorCategory::RateLimit
        } else {
            ProviderErrorCategory::ServerError
        },
        retryable: rate_limited || code.starts_with('5'),
        retry_after_ms: None,
    }
}

fn update_tool_identity(state: &mut ToolState, value: &Value) {
    if let Some(id) = string_field(value, "call_id") {
        state.id = Some(id);
    } else if state.id.is_none() {
        state.id = string_field(value, "item_id");
    }
    if let Some(name) = string_field(value, "name") {
        state.name = Some(name);
    }
}

fn output_index(value: &Value) -> usize {
    value
        .get("output_index")
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize
}

fn string_field(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn u64_field(value: &Value, field: &str) -> u64 {
    value.get(field).and_then(Value::as_u64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_tool_identity_across_responses_events() {
        let mut parser = OpenAiResponsesParser::new();
        let added = parser.push_data_line(
            r#"{"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","id":"fc_1","call_id":"call_1","name":"read_file","arguments":""}}"#,
        );
        assert!(matches!(
            added.as_slice(),
            [ProviderEvent::ToolCallDelta { id: Some(id), name: Some(name), arguments_delta, .. }]
                if id == "call_1" && name == "read_file" && arguments_delta.is_empty()
        ));

        let delta = parser.push_data_line(
            r#"{"type":"response.function_call_arguments.delta","item_id":"fc_1","output_index":0,"delta":"{\"path\":"}"#,
        );
        assert!(matches!(
            delta.as_slice(),
            [ProviderEvent::ToolCallDelta { id: Some(id), name: Some(name), arguments_delta, .. }]
                if id == "call_1" && name == "read_file" && arguments_delta.contains("path")
        ));
    }

    #[test]
    fn terminal_event_splits_cached_input_tokens() {
        let events = parse_responses_event(
            r#"{"type":"response.completed","response":{"usage":{"input_tokens":9000,"output_tokens":200,"input_tokens_details":{"cached_tokens":8192},"output_tokens_details":{"reasoning_tokens":64}}}}"#,
        );
        let usage = events
            .iter()
            .find_map(|event| match event {
                ProviderEvent::Usage(usage) => Some(usage),
                _ => None,
            })
            .expect("a Usage event");
        assert_eq!(usage.cache_read_tokens, Some(8192));
        assert_eq!(usage.input_tokens, 9000 - 8192);
        assert_eq!(usage.total_prompt_tokens(), 9000);
        assert_eq!(usage.reasoning_tokens, Some(64));
        assert!(events
            .iter()
            .any(|event| matches!(event, ProviderEvent::Completed { .. })));
    }
}
