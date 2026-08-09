//! Event payload decode/normalize helpers (W1/W2 split from event_log.rs).
//!
//! Owns legacy-row recovery: untagged payload JSON gets the serde `type` tag
//! injected, unknown event types surface as the forward-compatibility
//! `Unknown` variant so one undecodable row never fails a whole replay.

use crate::Result;
use assistant_protocol::v2::{RunEventKind, RunEventPayload, RunEventV2};

/// Ensure stored payload JSON includes the serde tag for RunEventPayload.
pub fn normalize_stored_payload(event_type: &str, payload: &str) -> String {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) {
        if value.get("type").is_some() {
            return payload.to_string();
        }
        if let serde_json::Value::Object(mut map) = value {
            map.insert(
                "type".into(),
                serde_json::Value::String(event_type.to_string()),
            );
            return serde_json::Value::Object(map).to_string();
        }
    }
    // Unit variants / empty payloads
    serde_json::json!({ "type": event_type }).to_string()
}

pub fn decode_payload(event_type: &str, payload_str: &str) -> Result<RunEventPayload, String> {
    if let Ok(payload) = serde_json::from_str::<RunEventPayload>(payload_str) {
        return Ok(payload);
    }
    // Recover from untagged historical rows.
    let normalized = normalize_stored_payload(event_type, payload_str);
    serde_json::from_str(&normalized).or_else(|_| {
        // Unknown/unregistered event type (e.g. a row written by a newer or
        // foreign schema). Surface it as the forward-compatibility Unknown
        // variant instead of failing the whole replay for one undecodable row.
        serde_json::from_str::<RunEventPayload>(
            &serde_json::json!({"type": "unknown", "raw": {"event_type": event_type, "payload": payload_str}}).to_string(),
        )
        .map_err(|e| format!("decode payload: {e}"))
    })
}

pub fn decode_event_v2(
    run_id: &str,
    sequence: u64,
    event_type: &str,
    payload_str: &str,
    timestamp: &str,
) -> Result<RunEventV2, String> {
    if let Ok(mut event) = serde_json::from_str::<RunEventV2>(payload_str) {
        event.run_id = run_id.to_string();
        event.run_sequence = sequence;
        return Ok(event);
    }
    let payload = decode_payload_v2(event_type, payload_str)?;
    let dt = chrono::DateTime::parse_from_rfc3339(timestamp)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .or_else(|_| {
            chrono::NaiveDateTime::parse_from_str(timestamp, "%Y-%m-%d %H:%M:%S")
                .map(|ndt| ndt.and_utc())
        })
        .unwrap_or_else(|_| chrono::Utc::now());
    Ok(RunEventV2 {
        event_id: format!("legacy:{run_id}:{sequence}"),
        global_sequence: 0,
        run_id: run_id.to_string(),
        run_sequence: sequence,
        timestamp: dt,
        payload,
    })
}

pub fn decode_payload_v2(event_type: &str, payload_str: &str) -> Result<RunEventKind, String> {
    if let Ok(payload) = serde_json::from_str::<RunEventKind>(payload_str) {
        return Ok(payload);
    }
    let normalized = normalize_stored_payload(event_type, payload_str);
    serde_json::from_str(&normalized).or_else(|_| {
        // Unknown/unregistered event type: surface as the forward-compatibility
        // Unknown variant instead of failing the whole replay for one row.
        serde_json::from_str::<RunEventKind>(
            &serde_json::json!({"type": "unknown", "raw": {"event_type": event_type, "payload": payload_str}}).to_string(),
        )
        .map_err(|e| format!("decode v2 payload: {e}"))
    })
}

/// Get the event type name from a RunEventPayload.
pub fn event_type_name(payload: &RunEventPayload) -> &'static str {
    match payload {
        RunEventPayload::Queued => "queued",
        RunEventPayload::Preparing => "preparing",
        RunEventPayload::Started => "started",
        RunEventPayload::TextDelta { .. } => "text_delta",
        RunEventPayload::ReasoningDelta { .. } => "reasoning_delta",
        RunEventPayload::ToolCallRequested { .. } => "tool_call_requested",
        RunEventPayload::ToolCallStarted { .. } => "tool_call_started",
        RunEventPayload::ToolCallCompleted { .. } => "tool_call_completed",
        RunEventPayload::PermissionRequested { .. } => "permission_requested",
        RunEventPayload::PermissionResponded { .. } => "permission_responded",
        RunEventPayload::FileChanged { .. } => "file_changed",
        RunEventPayload::UsageUpdated { .. } => "usage_updated",
        RunEventPayload::Completed { .. } => "completed",
        RunEventPayload::Failed { .. } => "failed",
        RunEventPayload::Interrupted { .. } => "interrupted",
        RunEventPayload::WaitingPermission => "waiting_permission",
        RunEventPayload::ContextCompressed { .. } => "context_compressed",
        RunEventPayload::CheckpointCreated { .. } => "checkpoint_created",
        RunEventPayload::SubAgentCreated { .. } => "sub_agent_created",
        RunEventPayload::SubAgentCompleted { .. } => "sub_agent_completed",
        RunEventPayload::SubAgentFailed { .. } => "sub_agent_failed",
        RunEventPayload::Progress { .. } => "progress",
        RunEventPayload::Unknown { .. } => "unknown",
    }
}
