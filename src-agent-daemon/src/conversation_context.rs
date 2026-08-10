//! Active-context snapshot domain: compaction snapshots, context summaries,
//! and content-block parsing/replay helpers.
//!
//! Split out of `conversation_store` to keep each file under 1000 lines. Public
//! items are re-exported from `conversation_store`; `store`, `MAX_ATTACHMENT_BYTES`
//! and `base64_engine` come from the parent module via `super::*`.

use agent_core::{AgentMessage, ContentBlock, ToolResultBlock};
use assistant_protocol::v2::{RunEventKind, RunEventV2};
use base64::Engine as _;
use rusqlite::{params, OptionalExtension};
use serde_json::Value;
use std::collections::HashSet;

use super::*;

/// Lossless active-context snapshot plus the durable message ids it replaced.
/// The daemon uses the id set to append messages written after compaction
/// without deleting or mutating the full transcript.
#[derive(Debug, Clone)]
pub struct ActiveContextSnapshot {
    pub messages: Vec<AgentMessage>,
    pub input_message_ids: HashSet<String>,
}

/// Load the newest lossless active-context snapshot when one exists. A missing
/// or malformed snapshot is a normal cache miss: callers fall back to the
/// durable full message history.
pub fn load_active_context_snapshot(
    conversation_id: &str,
) -> Result<Option<ActiveContextSnapshot>, String> {
    let store = store()?;
    let conn = store.conn()?;
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT snapshot_json, input_message_ids FROM context_snapshot
             WHERE conversation_id = ?1 AND snapshot_type = 'compaction'
             ORDER BY sequence DESC LIMIT 1",
            params![conversation_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some((raw, input_ids)) = row else {
        return Ok(None);
    };
    let value = serde_json::from_str::<Value>(&raw).map_err(|e| e.to_string())?;
    let ids = serde_json::from_str::<Vec<String>>(&input_ids)
        .map_err(|e| e.to_string())?
        .into_iter()
        .collect();
    Ok(Some(ActiveContextSnapshot {
        messages: agent_core::try_agent_messages_from_json(&value)?,
        input_message_ids: ids,
    }))
}

/// Load the snapshot explicitly attached to a checkpoint. Continue/Resume
/// must not silently jump to a newer conversation snapshot.
pub fn load_active_context_snapshot_for_checkpoint(
    conversation_id: &str,
    checkpoint_id: &str,
) -> Result<Option<ActiveContextSnapshot>, String> {
    let store = store()?;
    let conn = store.conn()?;
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT cs.snapshot_json, cs.input_message_ids
             FROM checkpoint cp
             JOIN context_snapshot cs ON cs.id = cp.active_context_snapshot_id
             WHERE cp.id = ?1 AND cp.conversation_id = ?2
             LIMIT 1",
            params![checkpoint_id, conversation_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some((raw, input_ids)) = row else {
        return Ok(None);
    };
    let value = serde_json::from_str::<Value>(&raw).map_err(|e| e.to_string())?;
    let ids = serde_json::from_str::<Vec<String>>(&input_ids)
        .map_err(|e| e.to_string())?
        .into_iter()
        .collect();
    Ok(Some(ActiveContextSnapshot {
        messages: agent_core::try_agent_messages_from_json(&value)?,
        input_message_ids: ids,
    }))
}

pub fn load_active_context_messages(conversation_id: &str) -> Result<Vec<AgentMessage>, String> {
    Ok(load_active_context_snapshot(conversation_id)?
        .map(|snapshot| snapshot.messages)
        .unwrap_or_default())
}

pub(crate) fn parse_content_block(block: &Value) -> Result<ContentBlock, String> {
    let kind = block
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| "block type is required".to_string())?;
    let content = block.get("content").unwrap_or(block);
    match kind {
        "text" => Ok(ContentBlock::Text {
            text: content
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| "text block has no text".to_string())?
                .to_string(),
        }),
        "thinking" | "reasoning" => Ok(ContentBlock::Thinking {
            text: content
                .get("text")
                .or_else(|| content.get("reasoning"))
                .and_then(Value::as_str)
                .ok_or_else(|| "thinking block has no text".to_string())?
                .to_string(),
            signature: content
                .get("signature")
                .and_then(Value::as_str)
                .map(str::to_string),
        }),
        "tool_call" => {
            let name = content
                .get("name")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| "tool call has no name".to_string())?;
            let tool_call_id = content
                .get("tool_call_id")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| "tool call has no tool_call_id".to_string())?;
            let arguments_json = content
                .get("arguments")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| "tool call has no arguments".to_string())?;
            serde_json::from_str::<Value>(arguments_json)
                .map_err(|error| format!("tool call arguments are invalid JSON: {error}"))?;
            Ok(ContentBlock::ToolCall(agent_core::ToolCall {
                tool_call_id: agent_core::ToolCallId::from(tool_call_id),
                name: name.to_string(),
                arguments_json: arguments_json.to_string(),
            }))
        }
        "image" => {
            let source = serde_json::from_value::<agent_core::ImageSource>(
                content
                    .get("source")
                    .cloned()
                    .unwrap_or_else(|| content.clone()),
            )
            .map_err(|error| format!("invalid image source: {error}"))?;
            if source.url.trim().is_empty() {
                return Err("image source url is empty".into());
            }
            Ok(ContentBlock::Image { source })
        }
        "file_reference" => {
            let path = content
                .get("path")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| "file reference has no path".to_string())?;
            let name = content
                .get("name")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or(path);
            let mime_type = content
                .get("mime_type")
                .or_else(|| content.get("mimeType"))
                .and_then(Value::as_str)
                .unwrap_or("");
            // T209 (P1-036): attachment content must actually reach the model,
            // not a bare `[attachment: name at path]` marker. Images become an
            // Image block (data URL); text/document attachments become a
            // controlled text block. Oversized / unreadable / binary files are
            // degraded explicitly — never silently dropped, never presented as
            // the path alone.
            if mime_type.starts_with("image/") {
                match std::fs::read(path) {
                    Ok(bytes) if bytes.len() <= MAX_ATTACHMENT_BYTES => {
                        let b64 = base64_engine().encode(bytes);
                        Ok(ContentBlock::Image {
                            source: agent_core::ImageSource {
                                url: format!("data:{mime_type};base64,{b64}"),
                                media_type: Some(mime_type.to_string()),
                                detail: None,
                            },
                        })
                    }
                    Ok(bytes) => Ok(ContentBlock::Text {
                        text: format!(
                            "[attachment {name}: image too large ({} bytes, limit {MAX_ATTACHMENT_BYTES})]",
                            bytes.len()
                        ),
                    }),
                    Err(e) => Ok(ContentBlock::Text {
                        text: format!("[attachment {name}: unreadable image ({e})]"),
                    }),
                }
            } else {
                match std::fs::read(path) {
                    Ok(bytes) if bytes.len() <= MAX_ATTACHMENT_BYTES => {
                        let text = String::from_utf8_lossy(&bytes).to_string();
                        if text.trim().is_empty() {
                            Ok(ContentBlock::Text {
                                text: format!("[attachment {name}: empty file]"),
                            })
                        } else {
                            Ok(ContentBlock::Text { text })
                        }
                    }
                    Ok(bytes) => Ok(ContentBlock::Text {
                        text: format!(
                            "[attachment {name}: too large ({} bytes, limit {MAX_ATTACHMENT_BYTES})]",
                            bytes.len()
                        ),
                    }),
                    Err(e) => Ok(ContentBlock::Text {
                        text: format!("[attachment {name}: unreadable ({e})]"),
                    }),
                }
            }
        }
        other => Err(format!("unsupported block type {other}")),
    }
}

pub(crate) fn parse_tool_result_blocks(blocks: &[Value]) -> Result<Vec<ToolResultBlock>, String> {
    blocks
        .iter()
        .enumerate()
        .map(
            |(index, block)| match block.get("type").and_then(Value::as_str) {
                Some("text") => Ok(ToolResultBlock::Text {
                    text: block
                        .get("text")
                        .and_then(Value::as_str)
                        .ok_or_else(|| format!("tool result block {index} text missing"))?
                        .to_string(),
                }),
                Some("json") => Ok(ToolResultBlock::Json {
                    value: block
                        .get("value")
                        .cloned()
                        .ok_or_else(|| format!("tool result block {index} value missing"))?,
                }),
                Some("artifact") => Ok(ToolResultBlock::Artifact {
                    artifact_id: block
                        .get("artifact_id")
                        .and_then(Value::as_str)
                        .filter(|id| !id.trim().is_empty())
                        .ok_or_else(|| format!("tool result block {index} artifact_id missing"))?
                        .to_string(),
                    preview: block
                        .get("preview")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                }),
                Some(other) => Err(format!(
                    "tool result block {index} type {other} unsupported"
                )),
                None => Err(format!("tool result block {index} type missing")),
            },
        )
        .collect()
}

pub(crate) fn latest_context_summary(conversation_id: &str) -> Result<Option<String>, String> {
    let store = store()?;
    let conn = store.conn()?;
    conn.query_row(
        "SELECT cs.summary
         FROM context_snapshot cs
         JOIN run r ON r.id = cs.run_id
         WHERE r.conversation_id = ?1
           AND cs.snapshot_type = 'compaction'
           AND COALESCE(cs.summary, '') <> ''
         ORDER BY cs.created_at DESC, cs.sequence DESC
         LIMIT 1",
        params![conversation_id],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(|e| e.to_string())
}

/// Recover a stored `image` content block for replay into a provider request.
///
/// Returns `None` for every other block type, and for an image block whose URL
/// is absent — a placeholder [`EngineImage`] would reach the adapters and be
/// announced to the model as a degraded image, claiming a picture existed where
/// the record has none.
pub(crate) fn block_image(block: &Value) -> Option<agent_core::EngineImage> {
    if block.get("type").and_then(Value::as_str)? != "image" {
        return None;
    }
    let content = block.get("content").unwrap_or(block);
    let url = content
        .get("image_url")
        .or_else(|| content.get("imageUrl"))
        .or_else(|| content.get("url"))
        .and_then(Value::as_str)
        .filter(|url| !url.trim().is_empty())?;
    Some(agent_core::EngineImage {
        url: url.to_string(),
        media_type: content
            .get("mime_type")
            .or_else(|| content.get("mimeType"))
            .and_then(Value::as_str)
            .map(str::to_string),
        detail: None,
    })
}

pub(crate) fn block_text(block: &Value) -> Option<String> {
    match block.get("type").and_then(Value::as_str)? {
        "text" => block
            .get("content")
            .and_then(|c| c.get("text"))
            .and_then(Value::as_str)
            .map(str::to_string),
        "file_reference" => {
            let content = block.get("content").unwrap_or(block);
            let path = content.get("path").and_then(Value::as_str).unwrap_or("");
            let name = content.get("name").and_then(Value::as_str).unwrap_or(path);
            Some(format!("[attachment: {name} at {path}]"))
        }
        "tool_result" => {
            let metadata = block
                .get("content")
                .filter(|value| value.is_object())
                .unwrap_or(block);
            let name = metadata
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("tool");
            let output = metadata
                .get("output")
                .cloned()
                .or_else(|| metadata.get("content").cloned())
                .unwrap_or(Value::Null);
            Some(format!("[tool result: {name} => {output}]"))
        }
        _ => None,
    }
}

pub(crate) fn reasoning_block_from_events(events: &[RunEventV2]) -> Option<Value> {
    let mut reasoning = String::new();
    let mut started_at = None;
    let mut finished_at = None;
    for event in events {
        if let RunEventKind::ReasoningDelta { text } = &event.payload {
            started_at.get_or_insert(event.timestamp);
            reasoning.push_str(text);
        }
        if started_at.is_some()
            && finished_at.is_none()
            && matches!(
                &event.payload,
                RunEventKind::TextDelta { .. }
                    | RunEventKind::ToolCallRequested { .. }
                    | RunEventKind::ToolCallStarted { .. }
                    | RunEventKind::ToolCallDelta { .. }
                    | RunEventKind::ToolCallCompleted { .. }
                    | RunEventKind::Completed { .. }
                    | RunEventKind::Failed { .. }
                    | RunEventKind::Cancelled { .. }
                    | RunEventKind::Interrupted { .. }
            )
        {
            finished_at = Some(event.timestamp);
        }
    }
    let reasoning = reasoning.trim();
    if reasoning.is_empty() {
        return None;
    }
    let duration_ms = started_at
        .zip(finished_at.or_else(|| events.last().map(|e| e.timestamp)))
        .map(|(start, end)| (end - start).num_milliseconds().max(0) as u64)
        .unwrap_or(0);
    Some(serde_json::json!({
        "reasoning": reasoning,
        "duration_ms": duration_ms,
    }))
}

fn persist_context_snapshots_from_events(
    run_id: &str,
    events: &[RunEventV2],
) -> Result<(), String> {
    let store = store()?;
    let conn = store.conn()?;
    let conversation_id: Option<String> = conn
        .query_row(
            "SELECT conversation_id FROM run WHERE id = ?1",
            params![run_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let branch_id: Option<String> = match conversation_id.as_deref() {
        Some(conversation_id) => conn
            .query_row(
                "SELECT branch_id FROM conversation WHERE id = ?1",
                params![conversation_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .map_err(|e| e.to_string())?,
        None => None,
    };
    for event in events {
        let (snapshot_id, _before_tokens, after_tokens, summary, committed) = match &event.payload {
            RunEventKind::ContextCompressed {
                before_tokens,
                after_tokens,
                summary,
            } => (None, *before_tokens, *after_tokens, summary.as_str(), None),
            RunEventKind::ContextSnapshotCommitted {
                snapshot_id,
                input_message_ids,
                summary_message_id,
                replaced_range,
                algorithm_version,
                provider_context_window,
                artifact_reference,
                turn_id,
                source_revision,
                snapshot_json,
            } => (
                Some(snapshot_id.as_str()),
                0,
                0,
                "",
                Some((
                    input_message_ids,
                    summary_message_id.as_deref(),
                    replaced_range.as_deref(),
                    algorithm_version.as_str(),
                    *provider_context_window,
                    artifact_reference.as_deref(),
                    turn_id.as_deref(),
                    *source_revision,
                    snapshot_json,
                )),
            ),
            _ => continue,
        };
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM context_snapshot
                 WHERE run_id = ?1 AND sequence = ?2 AND snapshot_type = 'compaction'",
                params![run_id, event.run_sequence as i64],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        if exists > 0 {
            if let Some(committed) = committed.as_ref() {
                persist_summary_message(
                    &conn,
                    conversation_id.as_deref(),
                    run_id,
                    committed.6,
                    committed.1,
                    committed.8,
                )?;
            } else if !summary.trim().is_empty() {
                persist_summary_text_message(
                    &conn,
                    conversation_id.as_deref(),
                    run_id,
                    None,
                    &format!("context-summary-{run_id}-{}", event.run_sequence),
                    summary,
                )?;
            }
            continue;
        }
        let event_turn_id = events[..events
            .iter()
            .position(|candidate| candidate.run_sequence == event.run_sequence)
            .unwrap_or(0)]
            .iter()
            .rev()
            .find_map(|candidate| match &candidate.payload {
                RunEventKind::TurnCompleted { turn_id, .. }
                | RunEventKind::TurnStarted { turn_id } => Some(turn_id.clone()),
                _ => None,
            });
        let turn_id = committed
            .as_ref()
            .and_then(|value| value.6.map(str::to_string))
            .or(event_turn_id);
        let mechanical_summary_id = format!("context-summary-{run_id}-{}", event.run_sequence);
        let snapshot_id = snapshot_id
            .map(str::to_string)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let (
            input_message_ids,
            summary_message_id,
            replaced_range,
            algorithm_version,
            provider_context_window,
            artifact_reference,
            source_revision,
            snapshot_json,
        ) = if let Some(value) = committed.as_ref() {
            (
                serde_json::to_string(value.0)
                    .map_err(|e| format!("serialize snapshot input ids: {e}"))?,
                value.1.map(str::to_string),
                value.2.map(str::to_string),
                value.3.to_string(),
                value.4,
                value.5.map(str::to_string),
                value.7,
                value.8.clone(),
            )
        } else {
            (
                "[]".into(),
                None,
                None,
                "mechanical-v1".into(),
                None,
                None,
                event.run_sequence,
                serde_json::json!([{
                    "message_id": mechanical_summary_id,
                    "role": "system",
                    "content": summary,
                }]),
            )
        };
        let estimated_tokens = if after_tokens > 0 {
            after_tokens as i64
        } else {
            (snapshot_json.to_string().len() as i64 / 4).max(1)
        };
        conn.execute(
            "INSERT INTO context_snapshot (
                id, run_id, conversation_id, branch_id, turn_id, sequence, snapshot_type, token_count, summary,
                source_revision, input_message_ids, summary_message_id, replaced_range,
                algorithm_version, provider_context_window, artifact_reference, snapshot_json
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'compaction', ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                snapshot_id,
                run_id,
                conversation_id,
                branch_id,
                turn_id,
                event.run_sequence as i64,
                estimated_tokens,
                summary,
                source_revision as i64,
                input_message_ids,
                summary_message_id,
                replaced_range,
                algorithm_version,
                provider_context_window.map(|value| value as i64),
                artifact_reference,
                snapshot_json.to_string()
            ],
        )
        .map_err(|e| e.to_string())?;
        if let Some(committed) = committed.as_ref() {
            persist_summary_message(
                &conn,
                conversation_id.as_deref(),
                run_id,
                turn_id.as_deref(),
                committed.1,
                committed.8,
            )?;
        } else if !summary.trim().is_empty() {
            persist_summary_text_message(
                &conn,
                conversation_id.as_deref(),
                run_id,
                turn_id.as_deref(),
                &mechanical_summary_id,
                summary,
            )?;
        }
    }
    Ok(())
}

/// Daemon-startup repair for the crash gap between a committed
/// `ContextSnapshotCommitted` event and its run-end `context_snapshot` row
/// projection. When the engine commits the event but the process dies before
/// the run's context-snapshot projection materializes the row, a restart leaves
/// an event without a queryable snapshot. This scans every run that committed
/// such an event and idempotently projects any missing rows (the projection is
/// deduplicated per `run_id` + sequence, and summary messages are identity
/// checked), so it is safe to run on every daemon start.
pub fn backfill_context_snapshots() -> Result<usize, String> {
    let run_ids: Vec<String> = {
        let store = store()?;
        let conn = store.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT DISTINCT run_id FROM run_event
                 WHERE event_type = 'context_snapshot_committed'",
            )
            .map_err(|e| e.to_string())?;
        let ids = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        ids
    };
    let mut touched = 0usize;
    for run_id in run_ids {
        let events: Vec<RunEventV2> = {
            let store = store()?;
            let conn = store.conn()?;
            let mut stmt = conn
                .prepare(
                    "SELECT payload FROM run_event
                     WHERE run_id = ?1 ORDER BY sequence ASC",
                )
                .map_err(|e| e.to_string())?;
            let mut events = Vec::new();
            {
                let rows = stmt
                    .query_map(rusqlite::params![run_id], |row| row.get::<_, String>(0))
                    .map_err(|e| e.to_string())?;
                for payload in rows {
                    let payload = payload.map_err(|e| e.to_string())?;
                    if let Ok(event) = serde_json::from_str::<RunEventV2>(&payload) {
                        events.push(event);
                    }
                }
            }
            events
        };
        if !events.is_empty() {
            persist_context_snapshots_from_events(&run_id, &events)?;
            touched += 1;
        }
    }
    Ok(touched)
}

/// Keep a model-written compaction summary as a first-class system message.
/// The active snapshot still carries the complete provider-valid view, but a
/// durable conversation must be able to reload and audit the summary without
/// depending on the snapshot JSON alone.
fn persist_summary_message(
    conn: &rusqlite::Connection,
    conversation_id: Option<&str>,
    run_id: &str,
    turn_id: Option<&str>,
    summary_message_id: Option<&str>,
    snapshot_json: &Value,
) -> Result<(), String> {
    let Some(conversation_id) = conversation_id else {
        return Ok(());
    };
    let Some(summary_message_id) = summary_message_id else {
        return Ok(());
    };
    let summary = snapshot_json
        .as_array()
        .and_then(|messages| {
            messages.iter().find(|message| {
                message.get("message_id").and_then(Value::as_str) == Some(summary_message_id)
                    && message.get("role").and_then(Value::as_str) == Some("system")
            })
        })
        .and_then(|message| message.get("content").and_then(Value::as_str))
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| {
            format!("context snapshot references missing summary message {summary_message_id}")
        })?;
    persist_summary_text_message(
        conn,
        Some(conversation_id),
        run_id,
        turn_id,
        summary_message_id,
        summary,
    )
}

fn persist_summary_text_message(
    conn: &rusqlite::Connection,
    conversation_id: Option<&str>,
    run_id: &str,
    turn_id: Option<&str>,
    summary_message_id: &str,
    summary: &str,
) -> Result<(), String> {
    let Some(conversation_id) = conversation_id else {
        return Ok(());
    };
    let now = chrono::Utc::now().to_rfc3339();
    #[allow(clippy::type_complexity)] // pre-existing: factored type alias deferred
    let existing: Option<(String, String, String, Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT conversation_id, role, status, run_id, turn_id
             FROM message WHERE id = ?1",
            rusqlite::params![summary_message_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional()
        .map_err(|e| format!("load summary message: {e}"))?;
    if let Some((existing_conversation, role, status, existing_run, existing_turn)) = existing {
        if existing_conversation != conversation_id
            || role != "system"
            || status != "complete"
            || existing_run.as_deref() != Some(run_id)
            || existing_turn.as_deref() != turn_id
        {
            return Err(format!(
                "summary message identity collision: {summary_message_id}"
            ));
        }
        let stored: String = conn
            .query_row(
                "SELECT block_json FROM message_block
                 WHERE message_id = ?1 AND sort_order = 0 AND block_type = 'text'",
                rusqlite::params![summary_message_id],
                |row| row.get(0),
            )
            .map_err(|e| format!("load summary block: {e}"))?;
        let expected = serde_json::json!({"text": summary}).to_string();
        if stored != expected {
            return Err(format!(
                "summary message content collision: {summary_message_id}"
            ));
        }
        return Ok(());
    }
    conn.execute(
        "INSERT INTO message
         (id, conversation_id, role, status, run_id, turn_id, legacy_marker, created_at)
         VALUES (?1, ?2, 'system', 'complete', ?3, ?4, 'context_summary', ?5)",
        rusqlite::params![summary_message_id, conversation_id, run_id, turn_id, now],
    )
    .map_err(|e| format!("persist summary message: {e}"))?;
    conn.execute(
        "INSERT INTO message_block
         (message_id, sort_order, block_type, block_json)
         VALUES (?1, 0, 'text', ?2)",
        rusqlite::params![
            summary_message_id,
            serde_json::json!({"text": summary}).to_string()
        ],
    )
    .map_err(|e| format!("persist summary block: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation_store::test_support::*;

    #[test]
    fn stored_image_blocks_are_recovered_for_replay() {
        let block = serde_json::json!({
            "type": "image",
            "content": { "imageUrl": "data:image/webp;base64,UklGRg==", "mimeType": "image/webp" }
        });
        let image = block_image(&block).expect("image block should be recovered");
        assert_eq!(image.url, "data:image/webp;base64,UklGRg==");
        assert_eq!(image.media_type.as_deref(), Some("image/webp"));

        // Flat spelling (no nested `content`) is what some writers persist.
        let flat = serde_json::json!({ "type": "image", "image_url": "https://a.test/b.png" });
        assert_eq!(
            block_image(&flat).expect("flat image block").url,
            "https://a.test/b.png"
        );
    }
    #[test]
    fn non_image_blocks_and_urlless_images_yield_nothing() {
        assert!(block_image(&serde_json::json!({"type": "text"})).is_none());
        assert!(block_image(&serde_json::json!({"type": "image"})).is_none());
        assert!(block_image(&serde_json::json!({
            "type": "image",
            "content": { "imageUrl": "  " }
        }))
        .is_none());
    }
    #[test]
    fn typed_loader_handles_attachments_and_rejects_malformed_tool_calls() {
        // T209: file_reference must NOT become a bare `[attachment: name at
        // path]` marker — the content is read and injected (here the file does
        // not exist, so it degrades to an explicit unreadable marker, never a
        // path-only fake).
        let attachment = parse_content_block(&serde_json::json!({
            "type": "file_reference",
            "content": {"path": "/tmp/nonexistent-natives-attach.txt", "name": "example.txt"}
        }))
        .unwrap();
        assert!(
            matches!(attachment, ContentBlock::Text { ref text } if text.contains("[attachment example.txt: unreadable")),
            "missing attachment must degrade explicitly, got {attachment:?}"
        );
        assert!(
            !matches!(attachment, ContentBlock::Text { ref text } if text.contains("[attachment: example.txt at /tmp/nonexistent-natives-attach.txt]")),
            "path-only marker must not be produced"
        );

        let malformed = parse_content_block(&serde_json::json!({
            "type": "tool_call",
            "content": {"name": "read_file", "arguments": "{}"}
        }))
        .unwrap_err();
        assert!(malformed.contains("tool_call_id"));
    }
    #[test]
    fn context_compression_events_persist_snapshot_and_reenter_history() {
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

        let store = store().unwrap();
        store
            .conn()
            .unwrap()
            .execute(
                "INSERT INTO conversation (id, mode, title, provider_id, model_id)
             VALUES ('compact-conv', 'agent', 'Compact', 'openai', 'gpt-4o')",
                [],
            )
            .unwrap();
        store
            .conn()
            .unwrap()
            .execute(
                "INSERT INTO run (id, conversation_id, status, provider_id, model_id)
             VALUES ('compact-run', 'compact-conv', 'completed', 'openai', 'gpt-4o')",
                [],
            )
            .unwrap();
        persist_context_snapshots_from_events(
            "compact-run",
            &[RunEventV2 {
                event_id: uuid::Uuid::new_v4().to_string(),
                global_sequence: 0,
                run_sequence: 7,
                run_id: "compact-run".into(),
                timestamp: chrono::Utc::now(),
                payload: RunEventKind::ContextCompressed {
                    before_tokens: 100,
                    after_tokens: 20,
                    summary: "Previous compacted facts: alpha survives.".into(),
                },
            }],
        )
        .unwrap();

        let count: i64 = store
            .conn()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM context_snapshot WHERE run_id = 'compact-run'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        let summary_count: i64 = store
            .conn()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM message
                 WHERE conversation_id = 'compact-conv' AND legacy_marker = 'context_summary'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(summary_count, 1);
        let active = load_active_context_snapshot("compact-conv")
            .unwrap()
            .expect("mechanical compaction snapshot");
        assert!(matches!(
            &active.messages[0],
            AgentMessage::System(message) if message.text.contains("alpha survives")
        ));
        let history = engine_history("compact-conv").unwrap();
        assert_eq!(history[0].role, "system");
        assert!(history[0].content.contains("alpha survives"));
    }
}
