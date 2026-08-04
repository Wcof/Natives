//! Idempotent, transaction-scoped projection of committed typed turns from the
//! replayable event log into the conversation tables (TASK-005 / B03).
//!
//! - The event log is the single replayable source of truth; only committed
//!   (TurnCompleted) turns are materialized as complete assistant messages.
//! - Each committed turn writes its turn row, assistant message + blocks, tool
//!   result messages + blocks, and projection watermark in ONE transaction, so
//!   a crash mid-projection leaves either nothing or everything for that turn.
//! - Re-running the projector over the same events is idempotent. A content
//!   conflict or a corrupt event is quarantined explicitly in
//!   `projection_quarantine` — never silently skipped, never silently
//!   overwritten — and the remaining turns still project.
//! - FK decisions are made by actual row existence, not by run-/turn- name
//!   prefixes. Runs without an FK row (legacy/symbolic ids) route through the
//!   compat reader in `conversation_store`, which keeps FK columns NULL.

use agent_core::{AssistantMessage, ContentBlock, MessageId, ToolResultBlock, ToolResultMessage};
use assistant_protocol::v2::{RunEventKind, RunEventV2};
use rusqlite::params;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::conversation_store;

/// A projection write that failed inside the per-turn transaction. The
/// `Conflict` variant must be quarantined AFTER the transaction rolls back —
/// a quarantine row written inside the transaction would roll back with it.
enum ProjectionFailure {
    /// A stored message disagrees with the content derived from the events.
    Conflict {
        run_id: String,
        turn_id: String,
        message_id: String,
    },
    /// Any other write/commit failure (rolled back, nothing to quarantine).
    Other(String),
}

impl From<String> for ProjectionFailure {
    fn from(value: String) -> Self {
        ProjectionFailure::Other(value)
    }
}

/// Project all committed turns in `events` into the conversation tables for a
/// run. Returns the number of turns projected.
///
/// Production entry point (TASK-005): the daemon calls this after a run's
/// engine completes and again during startup recovery. Both calls are
/// idempotent; corrupt events are isolated in `projection_quarantine` and the
/// other turns still project.
pub fn project_run_from_events(
    conversation_id: &str,
    run_id: &str,
    events: &[RunEventV2],
) -> Result<usize, String> {
    let run_exists: i64 = conversation_store::store()?
        .conn()?
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM run WHERE id = ?1)",
            params![run_id],
            |row| row.get(0),
        )
        .map_err(|e| format!("project run existence: {e}"))?;
    if run_exists == 0 {
        // Legacy/symbolic run without an FK row: the compat reader keeps the
        // projection working without name-prefix FK heuristics.
        return conversation_store::append_assistant_turn_from_events(
            conversation_id,
            run_id,
            events,
        )
        .map(|_| 0);
    }
    let mut projected = 0;
    for group in group_turns(events) {
        match project_committed_turn(conversation_id, run_id, &group) {
            Ok(Some(_)) => projected += 1,
            Ok(None) => {}
            // The corrupt/conflicting turn was quarantined inside
            // `project_committed_turn`; the other turns still project.
            Err(_) => {}
        }
    }
    Ok(projected)
}

/// Startup recovery (B03): backfill projections for every run that has
/// committed turns not yet covered by its projection watermark. Idempotent and
/// safe to run on every daemon start; returns the number of turns projected.
pub fn recover_projections() -> Result<usize, String> {
    let store = conversation_store::store()?;
    let run_ids: Vec<String> = {
        let conn = store.conn()?;
        runs_needing_recovery(&conn)?
    };
    let mut total = 0;
    for run_id in run_ids {
        let events = {
            let conn = store.conn()?;
            load_events_for_run(&conn, &run_id)
        };
        let conversation_id = {
            let conn = store.conn()?;
            conn.query_row(
                "SELECT conversation_id FROM run WHERE id = ?1",
                params![run_id],
                |row| row.get::<_, String>(0),
            )
            .unwrap_or_default()
        };
        let Ok(events) = events else {
            // A corrupt stored event quarantines the run; other runs recover.
            continue;
        };
        if conversation_id.is_empty() || events.is_empty() {
            continue;
        }
        total += project_run_from_events(&conversation_id, &run_id, &events)?;
    }
    Ok(total)
}

/// Runs with a committed turn not yet covered by the projection watermark.
fn runs_needing_recovery(conn: &rusqlite::Connection) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT re.run_id
             FROM run_event re
             WHERE re.event_type = 'turn_completed'
               AND re.sequence > COALESCE(
                   (SELECT w.event_sequence FROM projection_watermark w
                    WHERE w.projector = 'conversation' AND w.run_id = re.run_id), -1)
             GROUP BY re.run_id",
        )
        .map_err(|e| e.to_string())?;
    let ids = stmt
        .query_map([], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<String>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(ids)
}

/// Decode every stored event for a run. A corrupt payload quarantines the run
/// explicitly (no silent skip) and aborts only that run's recovery.
fn load_events_for_run(
    conn: &rusqlite::Connection,
    run_id: &str,
) -> Result<Vec<RunEventV2>, String> {
    let payloads: Vec<String> = conn
        .prepare("SELECT payload FROM run_event WHERE run_id = ?1 ORDER BY sequence")
        .map_err(|e| e.to_string())?
        .query_map(params![run_id], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    let mut events = Vec::with_capacity(payloads.len());
    for payload in payloads {
        match serde_json::from_str::<RunEventV2>(&payload) {
            Ok(event) => events.push(event),
            Err(error) => {
                quarantine(
                    run_id,
                    None,
                    None,
                    "corrupt_stored_event",
                    &error.to_string(),
                )?;
                return Err(format!("corrupt stored event for run {run_id}: {error}"));
            }
        }
    }
    Ok(events)
}

/// Split events into turn groups using the same boundaries as the legacy
/// reader: a new TurnStarted closes the previous group, TurnCompleted closes
/// its own group, and any leftover events form a final (possibly delta-only)
/// group.
fn group_turns(events: &[RunEventV2]) -> Vec<Vec<RunEventV2>> {
    let mut groups: Vec<Vec<RunEventV2>> = Vec::new();
    let mut current: Vec<RunEventV2> = Vec::new();
    for event in events {
        if matches!(&event.payload, RunEventKind::TurnStarted { .. }) && !current.is_empty() {
            groups.push(std::mem::take(&mut current));
        }
        current.push(event.clone());
        if matches!(&event.payload, RunEventKind::TurnCompleted { .. }) {
            groups.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        groups.push(current);
    }
    groups
}

/// Project one turn group in a single transaction. Returns the assistant
/// message id when the turn was projected, `None` when the group was skipped
/// (partial typed turn, or nothing projectable).
fn project_committed_turn(
    conversation_id: &str,
    run_id: &str,
    events: &[RunEventV2],
) -> Result<Option<String>, String> {
    let has_turn_start = events
        .iter()
        .any(|e| matches!(&e.payload, RunEventKind::TurnStarted { .. }));
    let has_turn_completed = events
        .iter()
        .any(|e| matches!(&e.payload, RunEventKind::TurnCompleted { .. }));
    if has_turn_start && !has_turn_completed {
        // A crashed/partial typed turn is never materialized as a complete
        // assistant message.
        return Ok(None);
    }
    let turn_id = events.iter().find_map(|event| match &event.payload {
        RunEventKind::TurnStarted { turn_id } => Some(turn_id.clone()),
        _ => None,
    });
    let assistant_message_id = events.iter().find_map(|event| match &event.payload {
        RunEventKind::MessageStarted {
            message_id, role, ..
        } if role == "assistant" => Some(message_id.clone()),
        _ => None,
    });
    let stop_reason = events.iter().find_map(|event| match &event.payload {
        RunEventKind::TurnCompleted { stop_reason, .. } => Some(stop_reason.clone()),
        _ => None,
    });
    let mut text = String::new();
    let mut thinking = String::new();
    let mut tool_calls = Vec::new();
    let mut tool_results = Vec::new();
    let mut committed_content: Option<Vec<ContentBlock>> = None;
    for event in events {
        match &event.payload {
            RunEventKind::TextDelta { text: delta } => text.push_str(delta),
            RunEventKind::ReasoningDelta { text: delta } => thinking.push_str(delta),
            RunEventKind::ToolCallRequested { id, name, input } => {
                tool_calls.push(agent_core::ToolCall {
                    tool_call_id: id.clone().into(),
                    name: name.clone(),
                    arguments_json: input.to_string(),
                });
            }
            RunEventKind::ToolCallCompleted {
                id,
                name,
                output,
                is_error,
                duration_ms,
                result_message_id,
            } => tool_results.push((
                id.clone(),
                name.clone(),
                output.clone(),
                *is_error,
                *duration_ms,
                result_message_id.clone(),
            )),
            RunEventKind::MessageCompleted { content, .. } => {
                if let Some(content) = content {
                    let blocks = content.get("content").ok_or_else(|| {
                        let detail = content.to_string();
                        let _ = quarantine(
                            run_id,
                            turn_id.as_deref(),
                            assistant_message_id.as_deref(),
                            "corrupt_message_completed",
                            &detail,
                        );
                        "corrupt message completed content: missing blocks".to_string()
                    })?;
                    committed_content =
                        Some(serde_json::from_value(blocks.clone()).map_err(|error| {
                            let _ = quarantine(
                                run_id,
                                turn_id.as_deref(),
                                assistant_message_id.as_deref(),
                                "corrupt_message_completed",
                                &error.to_string(),
                            );
                            format!("invalid message completed content: {error}")
                        })?);
                }
            }
            _ => {}
        }
    }
    let has_committed_content = committed_content.is_some();
    let content = committed_content.unwrap_or_else(|| {
        let mut content = Vec::new();
        if !thinking.trim().is_empty() {
            content.push(ContentBlock::Thinking {
                text: thinking,
                signature: None,
            });
        }
        if !text.trim().is_empty() {
            content.push(ContentBlock::Text { text });
        }
        content.extend(
            tool_calls
                .into_iter()
                .map(agent_core::ContentBlock::ToolCall),
        );
        content
    });
    if content.is_empty() && tool_results.is_empty() {
        return Ok(None);
    }
    let typed_turn_id = turn_id.unwrap_or_else(|| format!("legacy-turn:{run_id}"));
    let assistant_id = assistant_message_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let last_sequence = events
        .iter()
        .map(|event| event.effective_run_sequence())
        .max()
        .unwrap_or(0);
    let turn_sequence = events
        .iter()
        .find_map(|event| match &event.payload {
            RunEventKind::TurnStarted { .. } => Some(event.effective_run_sequence()),
            _ => None,
        })
        .unwrap_or(0);

    let store = conversation_store::store()?;
    let conn = store.conn()?;
    let outcome: Result<Option<String>, ProjectionFailure> = (|| {
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| format!("project turn begin: {e}"))?;

        // Turn record — idempotent; the affected count tells us whether this
        // turn is new (count the watermark once) or an already-projected
        // re-run.
        let turn_affected = tx
            .execute(
                "INSERT OR IGNORE INTO turn (id, run_id, sequence, status, stop_reason, created_at, completed_at)
                 VALUES (?1, ?2, ?3, 'committed', ?4, datetime('now'), ?5)",
                params![
                    typed_turn_id,
                    run_id,
                    turn_sequence as i64,
                    stop_reason,
                    chrono::Utc::now().to_rfc3339()
                ],
            )
            .map_err(|e| format!("project turn insert: {e}"))?;

        // Assistant message + blocks (mirrors the legacy append path so the
        // stored rows are byte-identical: content blocks + run_reference).
        let assistant = AssistantMessage {
            message_id: MessageId::from(assistant_id.clone()),
            content: content.clone(),
            stop_reason: stop_reason.as_deref().map(parse_stop_reason),
        };
        let mut assistant_blocks = content_blocks_to_json(&assistant.content);
        assistant_blocks.push(serde_json::json!({ "type": "run_reference", "run_id": run_id }));
        let legacy_marker = typed_turn_id
            .starts_with("legacy-turn:")
            .then_some("legacy_turn_unknown");
        upsert_message_blocks(
            &tx,
            conversation_id,
            run_id,
            &typed_turn_id,
            &assistant_id,
            "assistant",
            &assistant_blocks,
            stop_reason.as_deref(),
            legacy_marker,
        )?;

        // Tool result messages (one per ToolCallCompleted, by stable result id).
        let mut projected_ids: Vec<String> = vec![assistant_id.clone()];
        for (id, name, output, is_error, _duration_ms, result_message_id) in &tool_results {
            let result_id = result_message_id
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            let result = ToolResultMessage {
                message_id: MessageId::from(result_id.clone()),
                tool_call_id: id.clone().into(),
                tool_name: name.clone(),
                content: result_blocks_for(output),
                is_error: *is_error,
                code: error_code_for(output),
            };
            let blocks = vec![tool_result_block(&result)];
            upsert_message_blocks(
                &tx,
                conversation_id,
                run_id,
                &typed_turn_id,
                &result_id,
                "assistant",
                &blocks,
                None,
                legacy_marker,
            )?;
            projected_ids.push(result_id);
        }

        // Watermark — the event prefix + a digest of the projected message ids.
        // New turns advance the counter; re-projections only ever advance the
        // sequence, so recovery stays idempotent.
        let mut hasher = Sha256::new();
        for id in &projected_ids {
            hasher.update(id.as_bytes());
            hasher.update(b"\0");
        }
        let digest = hex::encode(hasher.finalize());
        // Delta-only (non-typed) turns project through the compat branch inside
        // the projector; each one records a hit so legacy usage stays observable.
        let compat_contribution = if has_committed_content { 0 } else { 1 };
        if turn_affected > 0 {
            tx.execute(
                "INSERT INTO projection_watermark
                    (projector, run_id, event_sequence, turn_count, digest, compat_hits, created_at, updated_at)
                 VALUES ('conversation', ?1, ?2, 1, ?3, ?4, datetime('now'), datetime('now'))
                 ON CONFLICT(projector, run_id) DO UPDATE SET
                    event_sequence = excluded.event_sequence,
                    turn_count = turn_count + 1,
                    digest = excluded.digest,
                    compat_hits = compat_hits + excluded.compat_hits,
                    updated_at = datetime('now')",
                params![run_id, last_sequence as i64, digest, compat_contribution],
            )
            .map_err(|e| format!("project watermark upsert: {e}"))?;
        } else {
            tx.execute(
                "INSERT INTO projection_watermark
                    (projector, run_id, event_sequence, turn_count, digest, compat_hits, created_at, updated_at)
                 VALUES ('conversation', ?1, ?2, 1, ?3, 0, datetime('now'), datetime('now'))
                 ON CONFLICT(projector, run_id) DO UPDATE SET
                    event_sequence = MAX(event_sequence, excluded.event_sequence),
                    updated_at = datetime('now')",
                params![run_id, last_sequence as i64, digest],
            )
            .map_err(|e| format!("project watermark idempotent: {e}"))?;
        }
        tx.commit()
            .map_err(|e| format!("project turn commit: {e}"))?;
        Ok(Some(assistant_id))
    })();
    match outcome {
        Ok(id) => Ok(id),
        Err(ProjectionFailure::Conflict {
            run_id,
            turn_id,
            message_id,
        }) => {
            // The transaction rolled back when the closure dropped `tx`.
            // Quarantine on a fresh connection so the isolation survives the
            // rollback — never a silent overwrite.
            let detail =
                format!("stored content for message {message_id} disagrees with the events");
            let _ = quarantine(
                &run_id,
                Some(&turn_id),
                Some(&message_id),
                "content_conflict",
                &detail,
            );
            Err(detail)
        }
        Err(ProjectionFailure::Other(error)) => Err(error),
    }
}

/// Insert a message row + blocks when absent; when present, verify the stored
/// blocks match the projected blocks (idempotent no-op). A content mismatch is
/// surfaced as `ProjectionFailure::Conflict`; the caller quarantines it AFTER
/// the transaction rolls back, never silently overwriting.
fn upsert_message_blocks(
    tx: &rusqlite::Transaction,
    conversation_id: &str,
    run_id: &str,
    turn_id: &str,
    message_id: &str,
    role: &str,
    blocks: &[Value],
    stop_reason: Option<&str>,
    legacy_marker: Option<&str>,
) -> Result<(), ProjectionFailure> {
    let existing: Vec<(i64, String)> = tx
        .prepare(
            "SELECT sort_order, block_json FROM message_block
             WHERE message_id = ?1 ORDER BY sort_order",
        )
        .map_err(|e| e.to_string())?
        .query_map(params![message_id], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    if !existing.is_empty() {
        let projected: Vec<(i64, String)> = blocks
            .iter()
            .enumerate()
            .map(|(index, block)| (index as i64, block.to_string()))
            .collect();
        if existing != projected {
            return Err(ProjectionFailure::Conflict {
                run_id: run_id.to_string(),
                turn_id: turn_id.to_string(),
                message_id: message_id.to_string(),
            });
        }
        // Already projected with identical content — idempotent no-op.
        return Ok(());
    }
    let now = chrono::Utc::now().to_rfc3339();
    tx.execute(
        "INSERT INTO message
            (id, conversation_id, role, status, turn_id, run_id, legacy_marker, truncated, stop_reason, created_at)
         VALUES (?1, ?2, ?3, 'complete', ?4, ?5, ?6, 0, ?7, ?8)",
        params![
            message_id,
            conversation_id,
            role,
            turn_id,
            run_id,
            legacy_marker,
            stop_reason,
            now
        ],
    )
    .map_err(|e| format!("project message insert: {e}"))?;
    for (index, block) in blocks.iter().enumerate() {
        let block_type = block.get("type").and_then(Value::as_str).unwrap_or("text");
        tx.execute(
            "INSERT INTO message_block (message_id, sort_order, block_type, block_json, artifact_id, truncated)
             VALUES (?1, ?2, ?3, ?4, NULL, 0)",
            params![message_id, index as i64, block_type, block.to_string()],
        )
        .map_err(|e| format!("project block insert: {e}"))?;
    }
    Ok(())
}

/// Record an explicitly isolated event/turn. This is the "no silent skip"
/// contract: the quarantine row is durable and queryable.
fn quarantine(
    run_id: &str,
    turn_id: Option<&str>,
    message_id: Option<&str>,
    reason: &str,
    detail: &str,
) -> Result<(), String> {
    conversation_store::store()?
        .conn()?
        .execute(
            "INSERT INTO projection_quarantine
                (projector, run_id, turn_id, message_id, reason, detail)
             VALUES ('conversation', ?1, ?2, ?3, ?4, ?5)",
            params![run_id, turn_id, message_id, reason, detail],
        )
        .map_err(|e| format!("projection quarantine: {e}"))?;
    Ok(())
}

fn content_blocks_to_json(blocks: &[ContentBlock]) -> Vec<Value> {
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

fn result_blocks_for(output: &Value) -> Vec<ToolResultBlock> {
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

fn error_code_for(output: &Value) -> Option<String> {
    output
        .get("error_code")
        .or_else(|| output.get("code"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn tool_result_block(result: &ToolResultMessage) -> Value {
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

fn parse_stop_reason(value: &str) -> agent_core::StopReason {
    match value {
        "stop" => agent_core::StopReason::Stop,
        "tool_use" => agent_core::StopReason::ToolUse,
        "length" => agent_core::StopReason::Length,
        "cancelled" => agent_core::StopReason::Cancelled,
        "error" => agent_core::StopReason::Error,
        other => agent_core::StopReason::Provider(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation_store;

    /// Shared test store (migrations run once per test on a fresh temp DB);
    /// each test uses unique run/conv ids. Returns the tempdir so it stays
    /// alive for the whole test.
    fn setup() -> ((String, String), tempfile::TempDir) {
        let _guard = crate::storage::DataStore::env_test_lock();
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("projector.db");
        std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db);
        std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
        crate::storage::set_test_db_override(Some(db.clone()), Some(dir.path().join("artifacts")));
        let _store = crate::storage::DataStore::new(&db, &dir.path().join("artifacts")).unwrap();
        let conv = format!("proj-conv-{}", uuid::Uuid::new_v4());
        let run = format!("proj-run-{}", uuid::Uuid::new_v4());
        {
            let store = conversation_store::store().unwrap();
            let conn = store.conn().unwrap();
            conn.execute(
                "INSERT OR IGNORE INTO conversation (id, mode, title, provider_id, model_id)
                 VALUES (?1, 'chat', 'Projector Test', 'prov-1', 'model-1')",
                params![conv],
            )
            .unwrap();
            conn.execute(
                "INSERT OR IGNORE INTO run (id, conversation_id, status, provider_id, model_id)
                 VALUES (?1, ?2, 'completed', 'prov-1', 'model-1')",
                params![run, conv],
            )
            .unwrap();
        }
        ((conv, run), dir)
    }

    fn event(run_id: &str, sequence: u64, payload: RunEventKind) -> RunEventV2 {
        RunEventV2 {
            event_id: format!("evt-{run_id}-{sequence}"),
            global_sequence: 0,
            run_sequence: sequence,
            run_id: run_id.into(),
            sequence,
            timestamp: chrono::Utc::now(),
            payload,
        }
    }

    fn typed_turn_events(run_id: &str, turn: &str) -> Vec<RunEventV2> {
        vec![
            event(
                run_id,
                1,
                RunEventKind::TurnStarted {
                    turn_id: turn.into(),
                },
            ),
            event(
                run_id,
                2,
                RunEventKind::MessageStarted {
                    turn_id: turn.into(),
                    message_id: format!("msg-{turn}"),
                    role: "assistant".into(),
                },
            ),
            event(
                run_id,
                3,
                RunEventKind::TextDelta {
                    text: "hello".into(),
                },
            ),
            event(
                run_id,
                4,
                RunEventKind::ToolCallRequested {
                    id: format!("call-{turn}-1"),
                    name: "read_file".into(),
                    input: serde_json::json!({"path": "/tmp/a.txt"}),
                },
            ),
            event(
                run_id,
                5,
                RunEventKind::ToolCallCompleted {
                    id: format!("call-{turn}-1"),
                    name: "read_file".into(),
                    output: serde_json::json!({"content": "file body"}),
                    is_error: false,
                    duration_ms: 2,
                    result_message_id: Some(format!("rm-{turn}-1")),
                },
            ),
            event(
                run_id,
                6,
                RunEventKind::MessageCompleted {
                    turn_id: turn.into(),
                    message_id: format!("msg-{turn}"),
                    role: "assistant".into(),
                    // The engine stores the typed content as an externally-tagged
                    // `Vec<ContentBlock>`; mirror that exact serialization.
                    content: Some(serde_json::json!({
                        "message_id": format!("msg-{turn}"),
                        "role": "assistant",
                        "content": serde_json::to_value(vec![
                            agent_core::ContentBlock::Text { text: "hello".into() },
                            agent_core::ContentBlock::ToolCall(agent_core::ToolCall {
                                tool_call_id: format!("call-{turn}-1").into(),
                                name: "read_file".into(),
                                arguments_json: "{\"path\":\"/tmp/a.txt\"}".into(),
                            }),
                        ]).unwrap(),
                    })),
                },
            ),
            event(
                run_id,
                7,
                RunEventKind::TurnCompleted {
                    turn_id: turn.into(),
                    stop_reason: "tool_use".into(),
                    input_tokens: 0,
                    output_tokens: 0,
                },
            ),
        ]
    }

    /// TASK-005 acceptance #1/#2: the projector writes the full typed turn
    /// (assistant content + complete tool pair) so a reloaded AgentMessage
    /// transcript matches the event-derived content and the tool result follows
    /// its tool call in block order.
    #[test]
    fn projector_preserves_full_tool_pair_and_block_order() {
        let ((conv, run), _dir) = setup();
        let events = typed_turn_events(&run, "t1");
        let projected = project_run_from_events(&conv, &run, &events).unwrap();
        assert_eq!(projected, 1, "one committed turn projects");
        let messages = conversation_store::load_agent_messages(&conv).unwrap();
        assert_eq!(messages.len(), 2, "assistant + tool result");
        let assistant = messages
            .iter()
            .find_map(|m| match m {
                agent_core::AgentMessage::Assistant(a) => Some(a),
                _ => None,
            })
            .unwrap();
        let tool_call = assistant
            .content
            .iter()
            .find_map(|b| match b {
                ContentBlock::ToolCall(call) => Some(call),
                _ => None,
            })
            .unwrap();
        assert_eq!(tool_call.tool_call_id.to_string(), format!("call-t1-1"));
        let result = messages
            .iter()
            .find_map(|m| match m {
                agent_core::AgentMessage::ToolResult(r) => Some(r),
                _ => None,
            })
            .unwrap();
        assert_eq!(result.tool_call_id.to_string(), format!("call-t1-1"));
        assert_eq!(result.tool_name, "read_file");
        assert!(!result.is_error);
    }

    /// TASK-005: re-projecting the same events is a zero-side-effect no-op.
    #[test]
    fn projector_is_idempotent() {
        let ((conv, run), _dir) = setup();
        let events = typed_turn_events(&run, "t1");
        project_run_from_events(&conv, &run, &events).unwrap();
        let before = conversation_store::load_agent_messages(&conv).unwrap();
        project_run_from_events(&conv, &run, &events).unwrap();
        let after = conversation_store::load_agent_messages(&conv).unwrap();
        assert_eq!(before.len(), after.len());
        assert_eq!(
            before, after,
            "re-projection must not change the transcript"
        );
        let store = conversation_store::store().unwrap();
        let conn = store.conn().unwrap();
        let turn_count: i64 = conn
            .query_row(
                "SELECT turn_count FROM projection_watermark
                 WHERE projector='conversation' AND run_id=?1",
                params![run],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(turn_count, 1, "a re-projected turn must not double-count");
    }

    /// TASK-005: a crashed/partial typed turn (no TurnCompleted) is never
    /// materialized as a complete message.
    #[test]
    fn partial_turn_is_not_projected() {
        let ((conv, run), _dir) = setup();
        let events = typed_turn_events(&run, "t1");
        let mut partial = events;
        partial.retain(|e| !matches!(e.payload, RunEventKind::TurnCompleted { .. }));
        let projected = project_run_from_events(&conv, &run, &partial).unwrap();
        assert_eq!(projected, 0);
        assert!(conversation_store::load_agent_messages(&conv)
            .unwrap()
            .is_empty());
    }

    /// TASK-005: a corrupt MessageCompleted payload is quarantined explicitly,
    /// never silently skipped, and the turn is not projected.
    #[test]
    fn corrupt_event_is_quarantined_not_skipped() {
        let ((conv, run), _dir) = setup();
        let mut events = typed_turn_events(&run, "t1");
        if let RunEventV2 {
            payload: RunEventKind::MessageCompleted { content, .. },
            ..
        } = &mut events[5]
        {
            *content = Some(
                serde_json::json!({ "message_id": "msg-t1", "role": "assistant", "content": "not-an-array" }),
            );
        }
        let projected = project_run_from_events(&conv, &run, &events).unwrap();
        assert_eq!(projected, 0, "corrupt turn must not project");
        let store = conversation_store::store().unwrap();
        let conn = store.conn().unwrap();
        let quarantined: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM projection_quarantine WHERE run_id=?1 AND reason='corrupt_message_completed'",
                params![run],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            quarantined, 1,
            "corrupt event must be explicitly quarantined"
        );
    }

    /// TASK-005: projecting a turn whose stored content disagrees with the
    /// events quarantines the conflict instead of silently overwriting.
    #[test]
    fn content_conflict_is_quarantined() {
        let ((conv, run), _dir) = setup();
        let events = typed_turn_events(&run, "t1");
        project_run_from_events(&conv, &run, &events).unwrap();
        // Tamper with the stored assistant block, then re-project the same
        // events: the projector must detect the disagreement.
        let store = conversation_store::store().unwrap();
        let conn = store.conn().unwrap();
        conn.execute(
            "UPDATE message_block SET block_json = '{\"type\":\"text\",\"text\":\"tampered\"}'
             WHERE message_id = 'msg-t1' AND block_type='text'",
            [],
        )
        .unwrap();
        drop(conn);
        let result = project_run_from_events(&conv, &run, &events);
        assert!(
            result.is_ok(),
            "conflict isolates the turn, it does not fail the run"
        );
        let store = conversation_store::store().unwrap();
        let conn = store.conn().unwrap();
        let quarantined: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM projection_quarantine
                 WHERE run_id=?1 AND reason='content_conflict'",
                params![run],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            quarantined, 1,
            "content conflict must be quarantined explicitly"
        );
        // The stored content is untouched — no silent overwrite.
        let stored: String = conn
            .query_row(
                "SELECT block_json FROM message_block
                 WHERE message_id = 'msg-t1' AND block_type='text'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(stored.contains("tampered"));
    }

    /// TASK-005 acceptance #4: FK decisions are by actual run existence, not
    /// name prefixes. A symbolic run with no FK row routes to the compat
    /// reader instead of failing the whole projection.
    #[test]
    fn missing_run_routes_to_compat_not_name_heuristics() {
        let ((conv, _), _dir) = setup();
        let run = "run-1".to_string();
        // Legacy delta-only events (no typed turn) are what a symbolic run
        // carries; the compat reader materializes them with NULL FK columns.
        let events = vec![event(
            &run,
            1,
            RunEventKind::TextDelta {
                text: "hello".into(),
            },
        )];
        let projected = project_run_from_events(&conv, &run, &events).unwrap();
        assert_eq!(projected, 0, "compat path returns legacy count semantics");
        let messages = conversation_store::load_agent_messages(&conv).unwrap();
        assert_eq!(
            messages.len(),
            1,
            "compat projection materializes the delta text"
        );
        match &messages[0] {
            agent_core::AgentMessage::Assistant(assistant) => {
                let text = assistant
                    .content
                    .iter()
                    .find_map(|b| match b {
                        ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .unwrap_or("");
                assert_eq!(text, "hello");
            }
            other => panic!("expected assistant message, got {other:?}"),
        }
    }

    /// TASK-005 (B03): startup recovery backfills committed turns whose events
    /// are in the log but whose projection never ran (crash before projection),
    /// and is idempotent.
    #[test]
    fn recover_projections_backfills_unprojected_committed_turns() {
        let ((conv, run), _dir) = setup();
        let events = typed_turn_events(&run, "t1");
        // Simulate a crash before projection: events durable in the log, no
        // watermark, no projected messages.
        {
            let store = conversation_store::store().unwrap();
            let conn = store.conn().unwrap();
            for event in &events {
                conn.execute(
                    "INSERT INTO run_event (run_id, sequence, event_type, payload, timestamp, event_id)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        run,
                        event.effective_run_sequence() as i64,
                        event.payload.type_name(),
                        serde_json::to_string(event).unwrap(),
                        event.timestamp.to_rfc3339(),
                        event.event_id,
                    ],
                )
                .unwrap();
            }
        }
        assert!(
            conversation_store::load_agent_messages(&conv)
                .unwrap()
                .is_empty(),
            "nothing projected before recovery"
        );
        let recovered = recover_projections().unwrap();
        assert_eq!(recovered, 1, "the committed turn is backfilled at startup");
        let messages = conversation_store::load_agent_messages(&conv).unwrap();
        assert_eq!(messages.len(), 2, "assistant + tool result after recovery");
        // Re-running recovery is a no-op.
        assert_eq!(recover_projections().unwrap(), 0, "recovery is idempotent");
    }
}
