//! Pure content-block conversion helpers shared by turn-commit projection
//! (and tests): assistant/tool-result JSON block serialization and stop-reason
//! parsing. No I/O; no transaction state.

use super::*;
use agent_core::{ContentBlock, ToolResultBlock, ToolResultMessage};
use serde_json::Value;

pub(super) fn content_blocks_to_json(blocks: &[ContentBlock]) -> Vec<Value> {
    blocks
        .iter()
        .map(|block| match block {
            ContentBlock::Text { text } => serde_json::json!({ "type": "text", "text": text }),
            ContentBlock::Thinking { text, signature } => {
                serde_json::json!({ "type": "thinking", "text": text, "signature": signature })
            }
            ContentBlock::Image { source } => {
                serde_json::json!({ "type": "image", "source": source })
            }
            ContentBlock::ToolCall(call) => serde_json::json!({
                "type": "tool_call",
                "tool_call_id": call.tool_call_id,
                "name": call.name,
                "arguments": call.arguments_json,
            }),
        })
        .collect()
}

pub(super) fn result_blocks_for(output: &Value) -> Vec<ToolResultBlock> {
    if let Some(artifact_id) = output.get("artifact_id").and_then(Value::as_str) {
        return vec![ToolResultBlock::Artifact {
            artifact_id: artifact_id.to_string(),
            preview: output
                .get("preview")
                .and_then(Value::as_str)
                .map(str::to_string),
        }];
    }
    vec![ToolResultBlock::Json {
        value: output.clone(),
    }]
}

pub(super) fn error_code_for(output: &Value) -> Option<String> {
    output
        .get("error_code")
        .or_else(|| output.get("code"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

pub(super) fn tool_result_block(result: &ToolResultMessage) -> Value {
    let content: Vec<Value> = result
        .content
        .iter()
        .map(|block| match block {
            ToolResultBlock::Text { text } => serde_json::json!({ "type": "text", "text": text }),
            ToolResultBlock::Json { value } => {
                serde_json::json!({ "type": "json", "value": value })
            }
            ToolResultBlock::Artifact {
                artifact_id,
                preview,
            } => serde_json::json!({
                "type": "artifact",
                "artifact_id": artifact_id,
                "preview": preview,
            }),
        })
        .collect();
    serde_json::json!({
        "type": "tool_result",
        "tool_call_id": result.tool_call_id,
        "name": result.tool_name,
        "is_error": result.is_error,
        "error_code": result.code,
        "content": content,
    })
}

pub(super) fn parse_stop_reason(value: &str) -> agent_core::StopReason {
    match value {
        "stop" => agent_core::StopReason::Stop,
        "tool_use" => agent_core::StopReason::ToolUse,
        "length" => agent_core::StopReason::Length,
        "cancelled" => agent_core::StopReason::Cancelled,
        "error" => agent_core::StopReason::Error,
        other => agent_core::StopReason::Provider(other.to_string()),
    }
}
