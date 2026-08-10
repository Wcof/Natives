//! Conversation message domain: durable typed transcript load/store.
//!
//! Split out of `conversation_store` to keep each file under 1000 lines. Public
//! items are re-exported from `conversation_store`; shared helpers (`store`,
//! `required_str`, content-block parsers) come from the parent via `super::*`.

use agent_core::{AgentMessage, ContentBlock, EngineMessage, ToolResultBlock};
use assistant_protocol::v2::{AttachmentRef, RunEventV2};
use rusqlite::params;
use serde_json::Value;

use super::*;

pub(crate) fn get_messages(params: Value) -> Result<Value, String> {
    let conversation_id = required_str(&params, "conversation_id")?;
    let page_limit = params
        .get("limit")
        .and_then(Value::as_i64)
        .map(|n| n.clamp(20, 200));
    let cursor = params.get("cursor");
    let cursor_created = cursor
        .and_then(|v| v.get("createdAt").or_else(|| v.get("created_at")))
        .and_then(Value::as_str);
    let cursor_id = cursor.and_then(|v| v.get("id")).and_then(Value::as_str);
    let store = store()?;
    let conn = store.conn()?;
    let mut messages: Vec<Value> = if let Some(limit) = page_limit {
        let mut stmt = conn.prepare(
            "SELECT id, role, conversation_id, parent_message_id, status, input_tokens, output_tokens, created_at,
                    turn_id, run_id, stop_reason, legacy_marker, truncated
             FROM message WHERE conversation_id = ?1
               AND (?2 IS NULL OR created_at < ?2 OR (created_at = ?2 AND id < ?3))
             ORDER BY created_at DESC, id DESC LIMIT ?4",
        ).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(
                params![conversation_id, cursor_created, cursor_id, limit + 1],
                |row| {
                    Ok(serde_json::json!({
                        "id": row.get::<_, String>(0)?,
                        "role": row.get::<_, String>(1)?,
                        "conversation_id": row.get::<_, String>(2)?,
                        "parent_message_id": row.get::<_, Option<String>>(3)?,
                        "status": row.get::<_, String>(4)?,
                        "input_tokens": row.get::<_, Option<i64>>(5)?,
                        "output_tokens": row.get::<_, Option<i64>>(6)?,
                        "created_at": row.get::<_, String>(7)?,
                        "turn_id": row.get::<_, Option<String>>(8)?,
                        "run_id": row.get::<_, Option<String>>(9)?,
                        "stop_reason": row.get::<_, Option<String>>(10)?,
                        "legacy_marker": row.get::<_, Option<String>>(11)?,
                        "truncated": row.get::<_, i64>(12).unwrap_or(0) != 0,
                    }))
                },
            )
            .map_err(|e| e.to_string())?;
        let mut rows: Vec<Value> = rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        rows.reverse();
        rows
    } else {
        let mut stmt = conn.prepare(
            "SELECT id, role, conversation_id, parent_message_id, status, input_tokens, output_tokens, created_at,
                    turn_id, run_id, stop_reason, legacy_marker, truncated
             FROM message WHERE conversation_id = ?1 ORDER BY created_at ASC, id ASC",
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map(params![conversation_id], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?, "role": row.get::<_, String>(1)?,
                "conversation_id": row.get::<_, String>(2)?, "parent_message_id": row.get::<_, Option<String>>(3)?,
                "status": row.get::<_, String>(4)?, "input_tokens": row.get::<_, Option<i64>>(5)?,
                "output_tokens": row.get::<_, Option<i64>>(6)?, "created_at": row.get::<_, String>(7)?,
                "turn_id": row.get::<_, Option<String>>(8)?, "run_id": row.get::<_, Option<String>>(9)?,
                "stop_reason": row.get::<_, Option<String>>(10)?, "legacy_marker": row.get::<_, Option<String>>(11)?,
                "truncated": row.get::<_, i64>(12).unwrap_or(0) != 0,
            }))
        }).map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    };

    let mut blocks = conn
        .prepare(
            "SELECT message_id, block_type, sort_order, block_json
             FROM message_block
             WHERE message_id IN (SELECT id FROM message WHERE conversation_id = ?1)
             ORDER BY sort_order ASC",
        )
        .map_err(|e| e.to_string())?;
    let block_rows = blocks
        .query_map(params![conversation_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut by_message = std::collections::HashMap::<String, Vec<Value>>::new();
    for (message_id, block_type, index, json) in block_rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?
    {
        let content = serde_json::from_str::<Value>(&json)
            .map_err(|e| format!("invalid message block JSON: {e}"))?;
        by_message
            .entry(message_id)
            .or_default()
            .push(serde_json::json!({
                "type": block_type,
                "index": index,
                "content": content,
            }));
    }
    for message in &mut messages {
        let id = message
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let mut content_blocks = by_message.remove(id).unwrap_or_default();
        let run_id = content_blocks.iter().find_map(|block| {
            (block.get("type").and_then(Value::as_str) == Some("run_reference"))
                .then(|| block.get("content")?.get("run_id")?.as_str())
                .flatten()
        });
        if let Some(run_id) = run_id {
            message["run_id"] = serde_json::json!(run_id);
            if !content_blocks
                .iter()
                .any(|block| block.get("type").and_then(Value::as_str) == Some("reasoning"))
            {
                let mut event_stmt = conn
                    .prepare(
                        "SELECT payload FROM run_event WHERE run_id = ?1 ORDER BY sequence ASC",
                    )
                    .map_err(|e| e.to_string())?;
                let event_rows = event_stmt
                    .query_map(params![run_id], |row| row.get::<_, String>(0))
                    .map_err(|e| e.to_string())?;
                let event_payloads = event_rows
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| e.to_string())?;
                let events = event_payloads
                    .iter()
                    .map(|payload| {
                        serde_json::from_str::<RunEventV2>(payload)
                            .map_err(|e| format!("invalid run event for message history: {e}"))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                if let Some(reasoning) = reasoning_block_from_events(&events) {
                    content_blocks.insert(
                        0,
                        serde_json::json!({
                            "type": "reasoning",
                            "index": 0,
                            "content": reasoning,
                        }),
                    );
                }
            }
        }
        message["content_blocks"] = serde_json::json!(content_blocks);
    }
    Ok(Value::Array(messages))
}

pub(crate) fn get_messages_page(params: Value) -> Result<Value, String> {
    let conversation_id = required_str(&params, "conversation_id")?;
    let limit = params
        .get("limit")
        .and_then(Value::as_i64)
        .unwrap_or(100)
        .clamp(20, 200);
    let cursor = params.get("cursor");
    let cursor_created = cursor
        .and_then(|v| v.get("createdAt").or_else(|| v.get("created_at")))
        .and_then(Value::as_str);
    let cursor_id = cursor.and_then(|v| v.get("id")).and_then(Value::as_str);
    let all = get_messages(
        serde_json::json!({ "conversation_id": conversation_id, "limit": limit, "cursor": { "createdAt": cursor_created, "id": cursor_id } }),
    )?;
    let mut messages: Vec<Value> = all.as_array().cloned().unwrap_or_default();
    let has_more = messages.len() > limit as usize;
    messages.truncate(limit as usize);
    let next_cursor = has_more
        .then(|| messages.first())
        .flatten()
        .and_then(|row| {
            Some(serde_json::json!({
                "createdAt": row.get("created_at")?, "id": row.get("id")?
            }))
        });
    messages.reverse();
    Ok(serde_json::json!({ "messages": messages, "nextCursor": next_cursor }))
}

pub fn engine_history(conversation_id: &str) -> Result<Vec<EngineMessage>, String> {
    let messages = get_messages(serde_json::json!({ "conversation_id": conversation_id }))?;
    let Some(rows) = messages.as_array() else {
        return Ok(Vec::new());
    };
    let raw_history: Vec<_> = rows
        .iter()
        .filter_map(|message| {
            let role = message.get("role")?.as_str()?.to_string();
            let blocks = message.get("content_blocks").and_then(Value::as_array);
            let content = blocks
                .map(|blocks| {
                    blocks
                        .iter()
                        .filter_map(block_text)
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default();
            let images: Vec<_> = blocks
                .map(|blocks| blocks.iter().filter_map(block_image).collect())
                .unwrap_or_default();
            // An image-only turn carries no text, and dropping it here would
            // silently rewrite history — the model would see the reply to a
            // picture it was never shown.
            if content.trim().is_empty() && images.is_empty() {
                return None;
            }
            Some(EngineMessage {
                role,
                content,
                images,
                ..Default::default()
            })
        })
        .collect();
    // Legacy EngineMessage consumers expect the tool result summary adjacent
    // to the assistant call. Typed persistence keeps it as its own message;
    // this adapter only folds the display text at the compatibility edge.
    let mut history: Vec<EngineMessage> = Vec::with_capacity(raw_history.len());
    for message in raw_history {
        if message.role == "assistant" && message.content.contains("[tool result:") {
            if let Some(previous) = history.last_mut() {
                if previous.role == "assistant" {
                    if !previous.content.is_empty() {
                        previous.content.push('\n');
                    }
                    previous.content.push_str(&message.content);
                    continue;
                }
            }
        }
        history.push(message);
    }
    if let Some(summary) = latest_context_summary(conversation_id)? {
        history.insert(0, EngineMessage::text("system", summary));
    }
    Ok(history)
}

/// Load the durable typed transcript used by Core replay.  The older
/// `engine_history` helper intentionally remains for RPC/fixture compatibility;
/// production callers should prefer this lossless representation.
pub fn load_agent_messages(conversation_id: &str) -> Result<Vec<AgentMessage>, String> {
    let raw = get_messages(serde_json::json!({ "conversation_id": conversation_id }))?;
    let Some(rows) = raw.as_array() else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for row in rows {
        let id = row
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| "message id missing".to_string())?;
        let role = row
            .get("role")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("message {id} role missing"))?;
        let blocks = row
            .get("content_blocks")
            .and_then(Value::as_array)
            .cloned()
            .ok_or_else(|| format!("message {id} content_blocks missing"))?;
        let stop_reason =
            row.get("stop_reason")
                .and_then(Value::as_str)
                .map(|reason| match reason {
                    "stop" => agent_core::StopReason::Stop,
                    "tool_use" => agent_core::StopReason::ToolUse,
                    "length" => agent_core::StopReason::Length,
                    "cancelled" => agent_core::StopReason::Cancelled,
                    "error" => agent_core::StopReason::Error,
                    other => agent_core::StopReason::Provider(other.to_string()),
                });
        if let Some(tool_block) = blocks
            .iter()
            .find(|block| block.get("type").and_then(Value::as_str) == Some("tool_result"))
        {
            let payload = tool_block.get("content").unwrap_or(tool_block);
            let content = payload
                .get("content")
                .and_then(Value::as_array)
                .ok_or_else(|| format!("tool result {id} content blocks missing"))
                .and_then(|blocks| parse_tool_result_blocks(blocks))?;
            let tool_call_id = payload
                .get("tool_call_id")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .map(ToString::to_string)
                .unwrap_or_else(|| format!("legacy-missing-tool-call:{id}"));
            let tool_name = tool_block
                .get("content")
                .and_then(|v| v.get("name"))
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("legacy-tool")
                .to_string();
            out.push(AgentMessage::ToolResult(agent_core::ToolResultMessage {
                message_id: agent_core::MessageId::from(id),
                tool_call_id: agent_core::ToolCallId::from(tool_call_id),
                tool_name,
                content,
                is_error: payload
                    .get("is_error")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                    || payload.get("tool_call_id").is_none(),
                code: payload
                    .get("error_code")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| {
                        payload
                            .get("tool_call_id")
                            .is_none()
                            .then_some("LEGACY_TOOL_RESULT_ID_MISSING".into())
                    }),
            }));
            continue;
        }
        // Custom messages carry a single `custom` block; reloading them restores
        // the kind + payload exactly, unlike the old free-form block type which
        // parse_content_block could not round-trip.
        if let Some(custom_block) = blocks
            .iter()
            .find(|block| block.get("type").and_then(Value::as_str) == Some("custom"))
        {
            let content = custom_block.get("content").unwrap_or(custom_block);
            out.push(AgentMessage::Custom(agent_core::CustomMessage {
                message_id: agent_core::MessageId::from(id),
                kind: content
                    .get("kind")
                    .and_then(Value::as_str)
                    .filter(|kind| !kind.trim().is_empty())
                    .unwrap_or("custom")
                    .to_string(),
                payload: content.get("payload").cloned().unwrap_or(Value::Null),
            }));
            continue;
        }
        let content = blocks
            .iter()
            // run_reference is provenance metadata appended to assistant
            // messages; it is not part of the typed content and must not reach
            // parse_content_block.
            .filter(|block| block.get("type").and_then(Value::as_str) != Some("run_reference"))
            .enumerate()
            .map(|(index, block)| {
                parse_content_block(block).map_err(|error| {
                    format!("message {id} content block {index} is malformed: {error}")
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        match role {
            "user" => out.push(AgentMessage::User(agent_core::UserMessage {
                message_id: agent_core::MessageId::from(id),
                content,
            })),
            "system" => out.push(AgentMessage::System(agent_core::SystemMessage {
                message_id: agent_core::MessageId::from(id),
                text: content
                    .iter()
                    .filter_map(|block| match block {
                        ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            })),
            _ => out.push(AgentMessage::Assistant(agent_core::AssistantMessage {
                message_id: agent_core::MessageId::from(id),
                content,
                stop_reason,
            })),
        }
    }
    Ok(out)
}

pub(crate) fn append_message(params: Value) -> Result<Value, String> {
    let conversation_id = required_str(&params, "conversation_id")?;
    let role = required_str(&params, "role")?;
    if !matches!(role, "system" | "user" | "assistant") {
        return Err("role must be user, assistant, or system".into());
    }
    let content = params
        .get("content")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty());
    let mut blocks = params
        .get("blocks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if blocks.is_empty() {
        let content = content.ok_or_else(|| "content or blocks is required".to_string())?;
        blocks.push(serde_json::json!({ "type": "text", "text": content }));
    }
    let status = params
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("complete");
    let id = params
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let turn_id = params.get("turn_id").and_then(Value::as_str);
    let run_id = params.get("run_id").and_then(Value::as_str);
    let legacy_marker = params.get("legacy_marker").and_then(Value::as_str);
    let stop_reason = params.get("stop_reason").and_then(Value::as_str);
    let truncated = params
        .get("truncated")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let now = chrono::Utc::now().to_rfc3339();
    let store = store()?;
    let conn = store.conn()?;
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT INTO message (id, conversation_id, role, status, turn_id, run_id, legacy_marker, truncated, stop_reason, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![id, conversation_id, role, status, turn_id, run_id, legacy_marker, if truncated { 1 } else { 0 }, stop_reason, now],
    )
    .map_err(|e| e.to_string())?;
    for (index, block) in blocks.iter().enumerate() {
        let block_type = block
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| "block type is required".to_string())?;
        let content = if block_type == "text" {
            serde_json::json!({ "text": block.get("text").and_then(Value::as_str).unwrap_or_default() })
        } else {
            block.clone()
        };
        tx.execute(
            "INSERT INTO message_block (message_id, sort_order, block_type, block_json, artifact_id, truncated)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, index as i64, block_type, content.to_string(), block.get("artifact_id").and_then(Value::as_str), if block.get("truncated").and_then(Value::as_bool).unwrap_or(false) { 1 } else { 0 }],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.execute(
        "UPDATE conversation SET updated_at = ?1 WHERE id = ?2",
        params![now, conversation_id],
    )
    .and_then(|_| tx.commit())
    .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "id": id, "created_at": now }))
}

/// Persist the typed Core message without passing through an EngineMessage
/// transcript. The JSON block representation is the durable compatibility
/// boundary for old renderers; identity and ordering stay typed and stable.
pub fn append_agent_message(
    conversation_id: &str,
    run_id: Option<&str>,
    turn_id: Option<&str>,
    message: &AgentMessage,
) -> Result<String, String> {
    // Legacy fixtures use symbolic ids without corresponding FK rows. Real
    // daemon runs are UUIDs and keep the binding; the compatibility marker is
    // still written below for symbolic test/replay events.
    let durable_run_id = run_id.filter(|value| !value.starts_with("run-"));
    let durable_turn_id =
        turn_id.filter(|value| !value.starts_with("turn-") && !value.starts_with("legacy-turn:"));
    let (id, role, blocks, stop_reason) = agent_message_parts(message);
    let mut payload = serde_json::json!({
        "id": id,
        "conversation_id": conversation_id,
        "role": role,
        "run_id": durable_run_id,
        "turn_id": durable_turn_id,
        "blocks": blocks,
    });
    if role == "assistant" {
        if let Some(run_id) = run_id {
            payload["blocks"]
                .as_array_mut()
                .expect("typed message blocks are an array")
                .push(serde_json::json!({
                    "type": "run_reference",
                    "run_id": run_id,
                }));
        }
    }
    if let Some(reason) = stop_reason {
        payload["stop_reason"] = serde_json::Value::String(reason);
    }
    if turn_id.is_some_and(|value| value.starts_with("legacy-turn:")) {
        payload["legacy_marker"] = serde_json::Value::String("legacy_turn_unknown".into());
    }
    let row = append_message(payload)?;
    row.get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "typed message append returned no id".into())
}

fn agent_message_parts(
    message: &AgentMessage,
) -> (String, &'static str, Vec<Value>, Option<String>) {
    match message {
        AgentMessage::User(user) => (
            user.message_id.0.clone(),
            "user",
            content_blocks_to_json(&user.content),
            None,
        ),
        AgentMessage::Assistant(assistant) => (
            assistant.message_id.0.clone(),
            "assistant",
            content_blocks_to_json(&assistant.content),
            assistant.stop_reason.as_ref().map(ToString::to_string),
        ),
        AgentMessage::ToolResult(result) => (
            result.message_id.0.clone(),
            "assistant",
            vec![serde_json::json!({
                "type": "tool_result",
                "tool_call_id": result.tool_call_id,
                "name": result.tool_name,
                "is_error": result.is_error,
                "error_code": result.code,
                "content": result_blocks_to_json(&result.content),
            })],
            None,
        ),
        AgentMessage::System(system) => (
            system.message_id.0.clone(),
            "system",
            vec![serde_json::json!({ "type": "text", "text": system.text })],
            None,
        ),
        AgentMessage::Custom(custom) => (
            custom.message_id.0.clone(),
            "assistant",
            vec![serde_json::json!({
                "type": "custom",
                "kind": custom.kind,
                "payload": custom.payload,
            })],
            None,
        ),
    }
}

fn content_blocks_to_json(blocks: &[ContentBlock]) -> Vec<Value> {
    blocks
        .iter()
        .map(|block| match block {
            ContentBlock::Text { text } => serde_json::json!({ "type": "text", "text": text }),
            ContentBlock::Thinking { text, signature } => serde_json::json!({ "type": "thinking", "text": text, "signature": signature }),
            ContentBlock::Image { source } => serde_json::json!({ "type": "image", "source": source }),
            ContentBlock::ToolCall(call) => serde_json::json!({ "type": "tool_call", "tool_call_id": call.tool_call_id, "name": call.name, "arguments": call.arguments_json }),
        })
        .collect()
}

fn result_blocks_to_json(blocks: &[ToolResultBlock]) -> Vec<Value> {
    blocks
        .iter()
        .map(|block| match block {
            ToolResultBlock::Text { text } => serde_json::json!({ "type": "text", "text": text }),
            ToolResultBlock::Json { value } => serde_json::json!({ "type": "json", "value": value }),
            ToolResultBlock::Artifact { artifact_id, preview } => serde_json::json!({ "type": "artifact", "artifact_id": artifact_id, "preview": preview }),
        })
        .collect()
}

pub fn append_trigger_message(
    conversation_id: &str,
    content: Option<&str>,
    attachments: Option<&[AttachmentRef]>,
) -> Result<Option<String>, String> {
    let mut blocks = Vec::new();
    if let Some(content) = content.filter(|s| !s.trim().is_empty()) {
        blocks.push(serde_json::json!({ "type": "text", "text": content }));
    }
    let attachments = attachments.unwrap_or(&[]);
    if attachments.len() > 10 {
        return Err("At most 10 attachments are allowed".into());
    }
    for attachment in attachments {
        if attachment.path.trim().is_empty() {
            return Err("attachment path is required".into());
        }
        blocks.push(serde_json::json!({
            "type": "file_reference",
            "path": attachment.path,
            "name": attachment.name.clone(),
            "mime_type": attachment.mime_type.clone(),
            "size": attachment.size,
        }));
    }
    if blocks.is_empty() {
        return Ok(None);
    }
    let appended = append_message(serde_json::json!({
        "conversation_id": conversation_id,
        "role": "user",
        "blocks": blocks,
    }))?;
    Ok(appended
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string))
}

pub fn delete_message(message_id: &str) -> Result<(), String> {
    if message_id.trim().is_empty() {
        return Ok(());
    }
    let store = store()?;
    let conn = store.conn()?;
    conn.execute("DELETE FROM message WHERE id = ?1", params![message_id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation_store::test_support::*;

    #[tokio::test]
    async fn conversation_round_trip_uses_daemon_tables() {
        let _guard = env_lock();
        let _restore = EnvRestore {
            db: std::env::var("NATIVES_DB_PATH").ok(),
            asst: std::env::var("NATIVES_ASSISTANT_DB_PATH").ok(),
            rt: std::env::var("NATIVES_RUNTIME_DIR").ok(),
        };
        let _clear_db = ClearTestDb;
        let dir = tempfile::tempdir().unwrap();
        let db = dir
            .path()
            .join(format!("natives-{}.db", uuid::Uuid::new_v4()));
        std::env::set_var("NATIVES_DB_PATH", &db);
        std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db);
        std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
        let art = dir.path().join("artifacts");
        crate::storage::set_test_db_override(Some(db.clone()), Some(art.clone()));
        let _warm = crate::storage::DataStore::new(&db, &art).expect("migrate");

        let created = request(
            names::CONVERSATION_CREATE,
            serde_json::json!({
                "mode": "agent",
                "title": "Test",
                "project_id": "/project/test",
                "provider_id": "p",
                "model_id": "m",
                "permission_profile_id": "readonly"
            }),
        )
        .await
        .unwrap();
        let id = created["id"].as_str().unwrap();
        request(
            names::CONVERSATION_APPEND_MESSAGE,
            serde_json::json!({
                "conversation_id": id,
                "role": "user",
                "blocks": [{ "type": "text", "text": "hello" }]
            }),
        )
        .await
        .unwrap();

        let list = request(names::CONVERSATION_LIST, serde_json::json!({}))
            .await
            .unwrap();
        assert_eq!(list[0]["permission_profile_id"], "readonly");
        let messages = request(
            names::CONVERSATION_GET_MESSAGES,
            serde_json::json!({ "conversation_id": id }),
        )
        .await
        .unwrap();
        assert_eq!(messages[0]["content_blocks"][0]["content"]["text"], "hello");
        let history = engine_history(id).unwrap();
        assert_eq!(history[0].content, "hello");
    }
    #[test]
    fn typed_message_round_trip_preserves_tool_call_identity() {
        let _guard = env_lock();
        let _restore = EnvRestore {
            db: std::env::var("NATIVES_DB_PATH").ok(),
            asst: std::env::var("NATIVES_ASSISTANT_DB_PATH").ok(),
            rt: std::env::var("NATIVES_RUNTIME_DIR").ok(),
        };
        let _clear_db = ClearTestDb;
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("typed-roundtrip.db");
        std::env::set_var("NATIVES_DB_PATH", &db);
        std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db);
        std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
        crate::storage::set_test_db_override(Some(db.clone()), Some(dir.path().join("artifacts")));
        let _store = crate::storage::DataStore::new(&db, &dir.path().join("artifacts")).unwrap();
        ensure_conversation_stub("typed-conv", "openai", "gpt-4o", None, None).unwrap();
        let message = AgentMessage::ToolResult(agent_core::ToolResultMessage {
            message_id: agent_core::MessageId::from("message-1"),
            tool_call_id: agent_core::ToolCallId::from("call-1"),
            tool_name: "read_file".into(),
            content: vec![ToolResultBlock::Json {
                value: serde_json::json!({"ok": true}),
            }],
            is_error: false,
            code: None,
        });
        append_agent_message("typed-conv", None, None, &message).unwrap();
        let loaded = load_agent_messages("typed-conv").unwrap();
        assert_eq!(loaded, vec![message]);
    }
    #[test]
    fn custom_message_round_trips_through_sqlite() {
        let _guard = env_lock();
        let _restore = EnvRestore {
            db: std::env::var("NATIVES_DB_PATH").ok(),
            asst: std::env::var("NATIVES_ASSISTANT_DB_PATH").ok(),
            rt: std::env::var("NATIVES_RUNTIME_DIR").ok(),
        };
        let _clear_db = ClearTestDb;
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("custom-roundtrip.db");
        std::env::set_var("NATIVES_DB_PATH", &db);
        std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db);
        std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
        crate::storage::set_test_db_override(Some(db.clone()), Some(dir.path().join("artifacts")));
        let _store = crate::storage::DataStore::new(&db, &dir.path().join("artifacts")).unwrap();
        ensure_conversation_stub("custom-conv", "openai", "gpt-4o", None, None).unwrap();
        let message = AgentMessage::Custom(agent_core::CustomMessage {
            message_id: agent_core::MessageId::from("custom-1"),
            kind: "recipe".into(),
            payload: serde_json::json!({"steps": 3, "tag": "chef"}),
        });
        append_agent_message("custom-conv", None, None, &message).unwrap();
        let loaded = load_agent_messages("custom-conv").unwrap();
        assert_eq!(
            loaded,
            vec![message],
            "Custom kind + payload must round-trip"
        );
    }
    #[test]
    fn file_reference_injects_content_or_degrades_explicitly() {
        let _guard = env_lock();
        let _restore = EnvRestore {
            db: std::env::var("NATIVES_DB_PATH").ok(),
            asst: std::env::var("NATIVES_ASSISTANT_DB_PATH").ok(),
            rt: std::env::var("NATIVES_RUNTIME_DIR").ok(),
        };
        let _clear_db = ClearTestDb;
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("attachment-boundary.db");
        std::env::set_var("NATIVES_DB_PATH", &db);
        std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db);
        std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
        crate::storage::set_test_db_override(Some(db.clone()), Some(dir.path().join("artifacts")));
        let _store = crate::storage::DataStore::new(&db, &dir.path().join("artifacts")).unwrap();
        ensure_conversation_stub("attach-conv", "openai", "gpt-4o", None, None).unwrap();

        // Create a real text attachment so its content is injected.
        let real_path = dir.path().join("a.txt");
        std::fs::write(&real_path, "hello attachment content").unwrap();
        append_trigger_message(
            "attach-conv",
            Some("look at this"),
            Some(&[AttachmentRef {
                path: real_path.to_string_lossy().to_string(),
                name: Some("a.txt".into()),
                mime_type: Some("text/plain".into()),
                size: None,
            }]),
        )
        .unwrap();
        let loaded = load_agent_messages("attach-conv").unwrap();
        // T209 (P1-036): the attachment CONTENT must reach the model — not a
        // bare `[attachment: name at path]` marker.
        assert!(matches!(
            &loaded[0],
            AgentMessage::User(user)
                if user.content.iter().any(|block| matches!(
                    block,
                    ContentBlock::Text { text } if text.contains("hello attachment content")
                ))
        ));
        assert!(
            !matches!(
                &loaded[0],
                AgentMessage::User(user)
                    if user.content.iter().any(|block| matches!(
                        block,
                        ContentBlock::Text { text } if text.contains("[attachment: a.txt at")
                    ))
            ),
            "path-only marker must not be produced for a readable attachment"
        );
    }
}
