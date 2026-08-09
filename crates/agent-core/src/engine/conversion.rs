//! Message serialisation and shaping between engine domain types, provider
//! JSON, hooks and agent messages.
//!
//! Extracted from `engine_core.rs` so the run loop keeps only orchestration.
//! All functions here are pure transformations; none hold state.

use serde_json::{json, Value};

use crate::hooks::HookDecision;

use super::error::EngineError;
use super::provider::{
    EngineImage, EngineMessage, EngineRunConfig, EngineToolCall, ProviderStopReason,
};

/// Apply SessionStart/UserPromptSubmit hook decisions.
///
/// Returns the messages a hook asked to inject. The engine places them in the
/// typed transcript (see [`AgentEngine::run_inner`]) so they reach the
/// `ProviderTurnRequest`; the legacy `config.messages` list is not a channel
/// for hook context and nothing injects into it any more.
pub(super) fn apply_prompt_hook_responses(
    config: &mut EngineRunConfig,
    responses: Vec<crate::hooks::HookResponse>,
) -> Result<Vec<String>, EngineError> {
    let mut injected = Vec::new();
    for response in responses {
        match response.decision {
            HookDecision::Deny { reason } => {
                return Err(EngineError::Message(format!("hook denied: {reason}")));
            }
            HookDecision::Modify { payload } => {
                if let Some(content) = payload.get("content").and_then(Value::as_str) {
                    config.user_content = content.to_string();
                }
            }
            HookDecision::Inject { messages } => {
                injected.extend(messages);
            }
            HookDecision::Allow => {}
        }
    }
    Ok(injected)
}

/// Leading characters of a tool's arguments kept verbatim in its doom-loop key.
///
/// Long enough to stay readable in a [`crate::doom_loop::DoomLoopReason`]
/// pattern, short enough that the key does not carry a whole file body.
const TOOL_FINGERPRINT_PREFIX_CHARS: usize = 80;

/// Identity of one tool invocation for doom-loop purposes.
///
/// A bare 80-character prefix is not an identity: two `edit` calls on the same
/// file whose argument JSON happens to agree for 80 characters and diverges at
/// the 400th would look identical, and three of them would abort a run that was
/// making progress. The prefix is kept for readability and a hash of the *full*
/// arguments is appended so distinct calls stay distinct.
pub(super) fn tool_args_fingerprint(args: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut chars = args.chars();
    let prefix: String = chars.by_ref().take(TOOL_FINGERPRINT_PREFIX_CHARS).collect();
    if chars.next().is_none() {
        return prefix;
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    args.hash(&mut hasher);
    format!("{prefix}#{:016x}", hasher.finish())
}

/// Ceiling on a provider-supplied `retry_after_ms`.
///
/// A provider (or a routing layer, see `routing::route_unavailable` with its
/// 60s hint) can name any delay it likes, and an absurd one would pin a run
/// open for as long as it wants. One minute is the longest wait that is still
/// plausibly worth doing inside a single generation attempt; past that the run
/// is better off failing so the caller can decide. The wait is cancellable
/// throughout, so the ceiling bounds patience, not responsiveness.
pub(super) const MAX_PROVIDER_BACKOFF_MS: u64 = 60_000;

/// How long to wait before retrying a failed generation attempt.
///
/// The provider's own hint wins when it asks for *more* than the local
/// schedule — that is the whole point of `Retry-After`, and ignoring it is how
/// a 429 turns into three instant retries and a longer ban. It never shortens
/// the wait, and it never exceeds [`MAX_PROVIDER_BACKOFF_MS`].
pub(super) fn provider_backoff_ms(attempt: u32, retry_after_ms: Option<u64>) -> u64 {
    let local = match attempt {
        1 => 500,
        2 => 1_000,
        _ => 2_000,
    };
    retry_after_ms
        .unwrap_or(0)
        .min(MAX_PROVIDER_BACKOFF_MS)
        .max(local)
}

pub(super) fn stop_reason_label(reason: Option<&ProviderStopReason>) -> String {
    match reason {
        Some(ProviderStopReason::Stop) => "stop".into(),
        Some(ProviderStopReason::ToolUse) => "tool_use".into(),
        Some(ProviderStopReason::Length) => "length".into(),
        Some(ProviderStopReason::Cancelled) => "cancelled".into(),
        Some(ProviderStopReason::Error) => "error".into(),
        Some(ProviderStopReason::Unknown(raw)) => format!("unknown:{raw}"),
        None => "unknown".into(),
    }
}

pub(super) fn core_stop_reason(reason: Option<&ProviderStopReason>) -> crate::StopReason {
    match reason.cloned().unwrap_or(ProviderStopReason::ToolUse) {
        ProviderStopReason::Stop => crate::StopReason::Stop,
        ProviderStopReason::ToolUse => crate::StopReason::ToolUse,
        ProviderStopReason::Length => crate::StopReason::Length,
        ProviderStopReason::Cancelled => crate::StopReason::Cancelled,
        ProviderStopReason::Error => crate::StopReason::Error,
        ProviderStopReason::Unknown(raw) => crate::StopReason::Provider(raw),
    }
}

#[allow(dead_code)]
pub(super) fn engine_messages_to_values(messages: &[EngineMessage]) -> Vec<Value> {
    messages
        .iter()
        .map(|m| {
            let mut obj = serde_json::Map::new();
            obj.insert("role".into(), json!(m.role));
            obj.insert("content".into(), json!(m.content));
            if let Some(id) = &m.tool_call_id {
                obj.insert("tool_call_id".into(), json!(id));
            }
            if let Some(name) = &m.tool_name {
                obj.insert("name".into(), json!(name));
            }
            if let Some(calls) = &m.tool_calls {
                let arr: Vec<Value> = calls
                    .iter()
                    .map(|c| {
                        json!({
                            "id": c.id,
                            "type": "function",
                            "function": {
                                "name": c.name,
                                "arguments": c.arguments,
                            }
                        })
                    })
                    .collect();
                obj.insert("tool_calls".into(), Value::Array(arr));
            }
            // Compaction round-trips history through JSON. Images have to make
            // the trip or they would vanish at the first compaction, which is
            // exactly the silent-drop failure this field was added to stop.
            if !m.images.is_empty() {
                let arr: Vec<Value> = m
                    .images
                    .iter()
                    .map(|img| {
                        let mut obj = serde_json::Map::new();
                        obj.insert("url".into(), json!(img.url));
                        if let Some(media_type) = &img.media_type {
                            obj.insert("media_type".into(), json!(media_type));
                        }
                        if let Some(detail) = &img.detail {
                            obj.insert("detail".into(), json!(detail));
                        }
                        Value::Object(obj)
                    })
                    .collect();
                obj.insert("images".into(), Value::Array(arr));
            }
            Value::Object(obj)
        })
        .collect()
}

/// Cheap estimate of a transcript's serialized length WITHOUT allocating a
/// string (PERF-001). Walks the `Value` tree summing string lengths and key
/// lengths; structural overhead is folded in per node but no bytes are
/// formatted or escaped. This is what the engine observes every
/// provider/tool round instead of `messages.iter().map(|m|
/// m.to_string().len()).sum()` — same order of growth, no per-round heap
/// churn from JSON serialization.
pub(super) fn values_chars(messages: &[Value]) -> usize {
    messages.iter().map(value_chars).sum()
}

pub(super) fn value_chars(value: &Value) -> usize {
    match value {
        Value::Null => 4,
        Value::Bool(boolean) => {
            if *boolean {
                4
            } else {
                5
            }
        }
        Value::Number(number) => number.to_string().len(),
        Value::String(text) => text.len(),
        Value::Array(items) => items.iter().map(value_chars).sum(),
        Value::Object(map) => map
            .iter()
            .map(|(key, value)| key.len() + value_chars(value))
            .sum(),
    }
}

pub(super) fn agent_messages_to_values(messages: &[crate::AgentMessage]) -> Vec<Value> {
    messages
        .iter()
        .map(|message| match message {
            crate::AgentMessage::User(message) => json!({
                "role": "user",
                "message_id": message.message_id,
                "content": message
                    .content
                    .iter()
                    .map(content_block_to_value)
                    .collect::<Vec<_>>(),
            }),
            crate::AgentMessage::Assistant(message) => json!({
                "role": "assistant",
                "message_id": message.message_id,
                "content": message
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        crate::ContentBlock::Text { text }
                        | crate::ContentBlock::Thinking { text, .. } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<String>(),
                "blocks": message
                    .content
                    .iter()
                    .map(content_block_to_value)
                    .collect::<Vec<_>>(),
                "tool_calls": message
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        crate::ContentBlock::ToolCall(call) => Some(json!({
                            "id": call.tool_call_id,
                            "type": "function",
                            "function": {
                                "name": call.name,
                                "arguments": call.arguments_json,
                            }
                        })),
                        _ => None,
                    })
                    .collect::<Vec<_>>(),
                "stop_reason": message.stop_reason.as_ref().map(ToString::to_string),
            }),
            crate::AgentMessage::ToolResult(message) => json!({
                "role": "tool",
                "message_id": message.message_id,
                "tool_call_id": message.tool_call_id,
                "name": message.tool_name,
                "content": tool_result_content(&message.content),
                "tool_result_blocks": message
                    .content
                    .iter()
                    .map(tool_result_block_to_value)
                    .collect::<Vec<_>>(),
                "is_error": message.is_error,
                "error_code": message.code,
            }),
            crate::AgentMessage::System(message) => json!({
                "role": "system",
                "message_id": message.message_id,
                "content": message.text,
            }),
            crate::AgentMessage::Custom(message) => json!({
                "role": "custom",
                "message_id": message.message_id,
                "kind": message.kind,
                "payload": message.payload,
            }),
        })
        .collect()
}

pub(super) fn agent_message_id(message: &crate::AgentMessage) -> String {
    match message {
        crate::AgentMessage::User(value) => value.message_id.to_string(),
        crate::AgentMessage::Assistant(value) => value.message_id.to_string(),
        crate::AgentMessage::ToolResult(value) => value.message_id.to_string(),
        crate::AgentMessage::System(value) => value.message_id.to_string(),
        crate::AgentMessage::Custom(value) => value.message_id.to_string(),
    }
}

pub(super) fn content_block_to_value(block: &crate::ContentBlock) -> Value {
    match block {
        crate::ContentBlock::Text { text } => json!({ "type": "text", "text": text }),
        crate::ContentBlock::Thinking { text, signature } => {
            json!({ "type": "thinking", "text": text, "signature": signature })
        }
        crate::ContentBlock::Image { source } => json!({ "type": "image", "source": source }),
        crate::ContentBlock::ToolCall(call) => json!({
            "type": "tool_call",
            "tool_call_id": call.tool_call_id,
            "name": call.name,
            "arguments": call.arguments_json,
        }),
    }
}

pub(super) fn tool_result_block_to_value(block: &crate::ToolResultBlock) -> Value {
    match block {
        crate::ToolResultBlock::Text { text } => json!({ "type": "text", "text": text }),
        crate::ToolResultBlock::Json { value } => json!({ "type": "json", "value": value }),
        crate::ToolResultBlock::Artifact {
            artifact_id,
            preview,
        } => json!({ "type": "artifact", "artifact_id": artifact_id, "preview": preview }),
    }
}

#[allow(clippy::unnecessary_filter_map)] // every arm returns Some; rewrite is large and risky
pub(super) fn values_to_agent_messages(values: &[Value]) -> Vec<crate::AgentMessage> {
    values
        .iter()
        .filter_map(|value| {
            let role = value.get("role").and_then(Value::as_str).unwrap_or("user");
            let message_id = value
                .get("message_id")
                .and_then(Value::as_str)
                .map(crate::MessageId::from)
                .unwrap_or_else(crate::MessageId::new);
            match role {
                "tool" => {
                    let blocks = value
                        .get("tool_result_blocks")
                        .and_then(Value::as_array)
                        .map(|blocks| {
                            blocks
                                .iter()
                                .filter_map(value_to_tool_result_block)
                                .collect::<Vec<_>>()
                        })
                        .filter(|blocks| !blocks.is_empty())
                        .unwrap_or_else(|| {
                            vec![crate::ToolResultBlock::Text {
                                text: value
                                    .get("content")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default()
                                    .to_string(),
                            }]
                        });
                    Some(crate::AgentMessage::ToolResult(crate::ToolResultMessage {
                        message_id,
                        tool_call_id: crate::ToolCallId::from(
                            value
                                .get("tool_call_id")
                                .and_then(Value::as_str)
                                .unwrap_or_default(),
                        ),
                        tool_name: value
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        content: blocks,
                        is_error: value
                            .get("is_error")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                        code: value
                            .get("error_code")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                    }))
                }
                "assistant" => Some(crate::AgentMessage::Assistant(crate::AssistantMessage {
                    message_id,
                    content: value
                        .get("blocks")
                        .and_then(Value::as_array)
                        .map(|blocks| blocks.iter().filter_map(value_to_content_block).collect())
                        .unwrap_or_else(|| {
                            value
                                .get("content")
                                .and_then(Value::as_str)
                                .filter(|text| !text.is_empty())
                                .map(|text| vec![crate::ContentBlock::Text { text: text.into() }])
                                .unwrap_or_default()
                        }),
                    stop_reason: value
                        .get("stop_reason")
                        .and_then(Value::as_str)
                        .map(stop_reason_from_label),
                })),
                "system" => Some(crate::AgentMessage::System(crate::SystemMessage {
                    message_id,
                    text: value
                        .get("content")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                })),
                "user" => Some(crate::AgentMessage::User(crate::UserMessage {
                    message_id,
                    content: value
                        .get("content")
                        .and_then(Value::as_array)
                        .map(|blocks| blocks.iter().filter_map(value_to_content_block).collect())
                        .unwrap_or_else(|| {
                            value
                                .get("content")
                                .and_then(Value::as_str)
                                .map(|text| vec![crate::ContentBlock::Text { text: text.into() }])
                                .unwrap_or_default()
                        }),
                })),
                "custom" => Some(crate::AgentMessage::Custom(crate::CustomMessage {
                    message_id,
                    kind: value
                        .get("kind")
                        .and_then(Value::as_str)
                        .unwrap_or("custom")
                        .to_string(),
                    payload: value.get("payload").cloned().unwrap_or(Value::Null),
                })),
                kind => Some(crate::AgentMessage::Custom(crate::CustomMessage {
                    message_id,
                    kind: kind.to_string(),
                    payload: value.get("content").cloned().unwrap_or(Value::Null),
                })),
            }
        })
        .collect()
}

/// Replay a persisted active-context snapshot without exposing the provider
/// wire representation to the daemon.
pub fn agent_messages_from_json(value: &Value) -> Vec<crate::AgentMessage> {
    value
        .as_array()
        .map_or_else(Vec::new, |items| values_to_agent_messages(items))
}

/// Strict active-context snapshot decoder.  The compatibility decoder above
/// intentionally tolerates old provider-shaped values; durable recovery must
/// not silently invent message IDs or drop malformed blocks.
pub fn try_agent_messages_from_json(value: &Value) -> Result<Vec<crate::AgentMessage>, String> {
    let items = value
        .as_array()
        .ok_or_else(|| "active context snapshot must be an array".to_string())?;
    for (index, item) in items.iter().enumerate() {
        let object = item
            .as_object()
            .ok_or_else(|| format!("snapshot message {index} is not an object"))?;
        let message_id = object
            .get("message_id")
            .and_then(Value::as_str)
            .filter(|id| !id.trim().is_empty())
            .ok_or_else(|| format!("snapshot message {index} is missing message_id"))?;
        let role = object
            .get("role")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("snapshot message {message_id} is missing role"))?;
        match role {
            "assistant" => {
                if let Some(blocks) = object.get("blocks") {
                    validate_snapshot_content_blocks(message_id, blocks)?;
                } else if object.get("content").and_then(Value::as_str).is_none() {
                    return Err(format!("snapshot assistant {message_id} has no content"));
                }
            }
            "user" => {
                if let Some(blocks) = object.get("content") {
                    if !blocks.is_string() {
                        validate_snapshot_content_blocks(message_id, blocks)?;
                    }
                } else {
                    return Err(format!("snapshot user {message_id} has no content"));
                }
            }
            "system" => {
                if object.get("content").and_then(Value::as_str).is_none() {
                    return Err(format!("snapshot system {message_id} has no content"));
                }
            }
            "tool" => {
                let call_id = object
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .filter(|id| !id.trim().is_empty())
                    .ok_or_else(|| format!("snapshot tool {message_id} is missing tool_call_id"))?;
                if object
                    .get("name")
                    .and_then(Value::as_str)
                    .is_none_or(|name| name.trim().is_empty())
                {
                    return Err(format!("snapshot tool {call_id} is missing name"));
                }
                if let Some(blocks) = object.get("tool_result_blocks") {
                    validate_snapshot_tool_result_blocks(call_id, blocks)?;
                } else if object.get("content").and_then(Value::as_str).is_none() {
                    return Err(format!("snapshot tool {call_id} has no content"));
                }
            }
            "custom" => {
                if object
                    .get("kind")
                    .and_then(Value::as_str)
                    .is_none_or(|kind| kind.trim().is_empty())
                {
                    return Err(format!("snapshot custom {message_id} is missing kind"));
                }
                if object.get("payload").is_none() {
                    return Err(format!("snapshot custom {message_id} has no payload"));
                }
            }
            other => {
                return Err(format!(
                    "snapshot message {message_id} has unknown role {other}"
                ))
            }
        }
    }
    Ok(values_to_agent_messages(items))
}

pub(super) fn validate_snapshot_content_blocks(
    message_id: &str,
    value: &Value,
) -> Result<(), String> {
    let blocks = value
        .as_array()
        .ok_or_else(|| format!("snapshot message {message_id} blocks are not an array"))?;
    for (index, block) in blocks.iter().enumerate() {
        let kind = block
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("snapshot message {message_id} block {index} has no type"))?;
        match kind {
            "text" | "thinking" => {
                if block.get("text").and_then(Value::as_str).is_none() {
                    return Err(format!(
                        "snapshot message {message_id} block {index} has no text"
                    ));
                }
            }
            "image" => {
                let source = block
                    .get("source")
                    .ok_or_else(|| format!("snapshot message {message_id} image has no source"))?;
                serde_json::from_value::<crate::ImageSource>(source.clone())
                    .map_err(|e| format!("invalid snapshot image source: {e}"))?;
            }
            "tool_call" => {
                for field in ["tool_call_id", "name", "arguments"] {
                    if block
                        .get(field)
                        .and_then(Value::as_str)
                        .is_none_or(|value| value.trim().is_empty())
                    {
                        return Err(format!(
                            "snapshot message {message_id} tool call missing {field}"
                        ));
                    }
                }
                let arguments =
                    block
                        .get("arguments")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            format!("snapshot message {message_id} tool call arguments are missing")
                        })?;
                serde_json::from_str::<Value>(arguments).map_err(|error| {
                    format!(
                        "snapshot message {message_id} tool call arguments are invalid JSON: {error}"
                    )
                })?;
            }
            other => {
                return Err(format!(
                    "snapshot message {message_id} has unknown block {other}"
                ))
            }
        }
    }
    Ok(())
}

pub(super) fn validate_snapshot_tool_result_blocks(
    call_id: &str,
    value: &Value,
) -> Result<(), String> {
    let blocks = value
        .as_array()
        .ok_or_else(|| format!("snapshot tool {call_id} result blocks are not an array"))?;
    for (index, block) in blocks.iter().enumerate() {
        let kind = block
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("snapshot tool {call_id} result {index} has no type"))?;
        match kind {
            "text" => {
                if block.get("text").and_then(Value::as_str).is_none() {
                    return Err(format!(
                        "snapshot tool {call_id} result {index} has no text"
                    ));
                }
            }
            "json" => {
                if block.get("value").is_none() {
                    return Err(format!(
                        "snapshot tool {call_id} result {index} has no value"
                    ));
                }
            }
            "artifact" => {
                if block
                    .get("artifact_id")
                    .and_then(Value::as_str)
                    .is_none_or(|id| id.trim().is_empty())
                {
                    return Err(format!(
                        "snapshot tool {call_id} result {index} has no artifact_id"
                    ));
                }
            }
            other => {
                return Err(format!(
                    "snapshot tool {call_id} has unknown result block {other}"
                ))
            }
        }
    }
    Ok(())
}

pub(super) fn value_to_content_block(value: &Value) -> Option<crate::ContentBlock> {
    match value.get("type").and_then(Value::as_str)? {
        "text" => Some(crate::ContentBlock::Text {
            text: value
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .into(),
        }),
        "thinking" => Some(crate::ContentBlock::Thinking {
            text: value
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .into(),
            signature: value
                .get("signature")
                .and_then(Value::as_str)
                .map(str::to_string),
        }),
        "image" => serde_json::from_value(value.get("source")?.clone())
            .ok()
            .map(|source| crate::ContentBlock::Image { source }),
        "tool_call" => Some(crate::ContentBlock::ToolCall(crate::ToolCall {
            tool_call_id: crate::ToolCallId::from(
                value
                    .get("tool_call_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            ),
            name: value
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .into(),
            arguments_json: value
                .get("arguments")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .into(),
        })),
        _ => None,
    }
}

pub(super) fn value_to_tool_result_block(value: &Value) -> Option<crate::ToolResultBlock> {
    match value.get("type").and_then(Value::as_str)? {
        "text" => Some(crate::ToolResultBlock::Text {
            text: value
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .into(),
        }),
        "json" => Some(crate::ToolResultBlock::Json {
            value: value.get("value").cloned().unwrap_or(Value::Null),
        }),
        "artifact" => Some(crate::ToolResultBlock::Artifact {
            artifact_id: value
                .get("artifact_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .into(),
            preview: value
                .get("preview")
                .and_then(Value::as_str)
                .map(str::to_string),
        }),
        _ => None,
    }
}

pub(super) fn stop_reason_from_label(label: &str) -> crate::StopReason {
    match label {
        "stop" => crate::StopReason::Stop,
        "tool_use" => crate::StopReason::ToolUse,
        "length" => crate::StopReason::Length,
        "cancelled" => crate::StopReason::Cancelled,
        "error" => crate::StopReason::Error,
        value => crate::StopReason::Provider(value.to_string()),
    }
}

pub fn engine_messages_to_agent_messages(messages: &[EngineMessage]) -> Vec<crate::AgentMessage> {
    messages
        .iter()
        .map(|message| {
            let message_id = crate::MessageId::new();
            let content = {
                let mut blocks = Vec::new();
                if !message.content.is_empty() {
                    blocks.push(crate::ContentBlock::Text {
                        text: message.content.clone(),
                    });
                }
                blocks.extend(message.images.iter().cloned().map(|image| {
                    crate::ContentBlock::Image {
                        source: crate::ImageSource {
                            url: image.url,
                            media_type: image.media_type,
                            detail: image.detail,
                        },
                    }
                }));
                blocks
            };
            match message.role.as_str() {
                "assistant" => {
                    let mut blocks = content;
                    if let Some(calls) = &message.tool_calls {
                        blocks.extend(calls.iter().map(|call| {
                            crate::ContentBlock::ToolCall(crate::ToolCall {
                                tool_call_id: crate::ToolCallId::from(call.id.clone()),
                                name: call.name.clone(),
                                arguments_json: call.arguments.clone(),
                            })
                        }));
                    }
                    crate::AgentMessage::Assistant(crate::AssistantMessage {
                        message_id,
                        content: blocks,
                        stop_reason: None,
                    })
                }
                "tool" => crate::AgentMessage::ToolResult(crate::ToolResultMessage {
                    message_id,
                    tool_call_id: crate::ToolCallId::from(
                        message.tool_call_id.clone().unwrap_or_default(),
                    ),
                    tool_name: message.tool_name.clone().unwrap_or_default(),
                    content: vec![crate::ToolResultBlock::Json {
                        value: serde_json::from_str(&message.content)
                            .unwrap_or_else(|_| json!(message.content)),
                    }],
                    is_error: false,
                    code: None,
                }),
                "system" => crate::AgentMessage::System(crate::SystemMessage {
                    message_id,
                    text: message.content.clone(),
                }),
                "user" => crate::AgentMessage::User(crate::UserMessage {
                    message_id,
                    content,
                }),
                kind => crate::AgentMessage::Custom(crate::CustomMessage {
                    message_id,
                    kind: kind.to_string(),
                    payload: json!({
                        "role": message.role,
                        "content": message.content,
                    }),
                }),
            }
        })
        .collect()
}

pub fn agent_messages_to_engine_messages(messages: &[crate::AgentMessage]) -> Vec<EngineMessage> {
    messages
        .iter()
        .map(|message| match message {
            crate::AgentMessage::User(message) => {
                engine_message_from_blocks("user", &message.content, None, None)
            }
            crate::AgentMessage::Assistant(message) => {
                let (content, images, tool_calls) = blocks_to_engine_parts(&message.content);
                EngineMessage {
                    role: "assistant".into(),
                    content,
                    tool_call_id: None,
                    tool_name: None,
                    tool_calls: (!tool_calls.is_empty()).then_some(tool_calls),
                    images,
                }
            }
            crate::AgentMessage::ToolResult(message) => EngineMessage {
                role: "tool".into(),
                content: tool_result_content(&message.content),
                tool_call_id: Some(message.tool_call_id.to_string()),
                tool_name: Some(message.tool_name.clone()),
                tool_calls: None,
                images: Vec::new(),
            },
            crate::AgentMessage::System(message) => {
                EngineMessage::text("system", message.text.clone())
            }
            crate::AgentMessage::Custom(message) => {
                EngineMessage::text(message.kind.clone(), message.payload.to_string())
            }
        })
        .collect()
}

pub(super) fn engine_message_from_blocks(
    role: &str,
    blocks: &[crate::ContentBlock],
    tool_call_id: Option<String>,
    tool_name: Option<String>,
) -> EngineMessage {
    let (content, images, tool_calls) = blocks_to_engine_parts(blocks);
    EngineMessage {
        role: role.into(),
        content,
        tool_call_id,
        tool_name,
        tool_calls: (!tool_calls.is_empty()).then_some(tool_calls),
        images,
    }
}

pub(super) fn blocks_to_engine_parts(
    blocks: &[crate::ContentBlock],
) -> (String, Vec<EngineImage>, Vec<EngineToolCall>) {
    let mut text = String::new();
    let mut images = Vec::new();
    let mut calls = Vec::new();
    for block in blocks {
        match block {
            crate::ContentBlock::Text { text: value }
            | crate::ContentBlock::Thinking { text: value, .. } => text.push_str(value),
            crate::ContentBlock::Image { source } => images.push(EngineImage {
                url: source.url.clone(),
                media_type: source.media_type.clone(),
                detail: source.detail.clone(),
            }),
            crate::ContentBlock::ToolCall(call) => calls.push(EngineToolCall {
                id: call.tool_call_id.to_string(),
                name: call.name.clone(),
                arguments: call.arguments_json.clone(),
            }),
        }
    }
    (text, images, calls)
}

pub(super) fn tool_result_content(blocks: &[crate::ToolResultBlock]) -> String {
    blocks
        .iter()
        .map(|block| match block {
            crate::ToolResultBlock::Text { text } => text.clone(),
            crate::ToolResultBlock::Json { value } => value.to_string(),
            crate::ToolResultBlock::Artifact {
                artifact_id,
                preview,
            } => preview.clone().unwrap_or_else(|| artifact_id.clone()),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[allow(dead_code)]
pub(super) fn values_to_engine_messages(values: &[Value]) -> Vec<EngineMessage> {
    values
        .iter()
        .map(|v| {
            let role = v
                .get("role")
                .and_then(|x| x.as_str())
                .unwrap_or("user")
                .to_string();
            let content = v
                .get("content")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let tool_call_id = v
                .get("tool_call_id")
                .and_then(|x| x.as_str())
                .map(str::to_string);
            let tool_name = v.get("name").and_then(|x| x.as_str()).map(str::to_string);
            let tool_calls = v.get("tool_calls").and_then(|x| x.as_array()).map(|arr| {
                arr.iter()
                    .filter_map(|c| {
                        let id = c.get("id")?.as_str()?.to_string();
                        let name = c
                            .get("function")
                            .and_then(|f| f.get("name"))
                            .and_then(|n| n.as_str())
                            .unwrap_or("")
                            .to_string();
                        let arguments = c
                            .get("function")
                            .and_then(|f| f.get("arguments"))
                            .and_then(|a| a.as_str())
                            .unwrap_or("{}")
                            .to_string();
                        Some(EngineToolCall {
                            id,
                            name,
                            arguments,
                        })
                    })
                    .collect()
            });
            let images = v
                .get("images")
                .and_then(|x| x.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|img| {
                            Some(EngineImage {
                                url: img.get("url")?.as_str()?.to_string(),
                                media_type: img
                                    .get("media_type")
                                    .and_then(|x| x.as_str())
                                    .map(str::to_string),
                                detail: img
                                    .get("detail")
                                    .and_then(|x| x.as_str())
                                    .map(str::to_string),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            EngineMessage {
                role,
                content,
                tool_call_id,
                tool_name,
                tool_calls,
                images,
            }
        })
        .collect()
}
