//! Per-turn commit projection: group committed turn events and, in one
//! transaction, write the turn row, assistant message + blocks, tool result
//! messages + blocks, and the projection watermark (TASK-005 / A8).

use super::conversation_projector_blocks::{
    content_blocks_to_json, error_code_for, parse_stop_reason, result_blocks_for, tool_result_block,
};
use super::*;
use crate::conversation_store;
use agent_core::{AssistantMessage, ContentBlock, MessageId, ToolResultMessage};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Split events into turn groups: a new TurnStarted closes the previous group,
/// TurnCompleted closes its own group, and any leftover events form a final
/// (possibly partial) group.
pub(super) fn group_turns(events: &[RunEventV2]) -> Vec<Vec<RunEventV2>> {
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

/// Project one turn group in a single transaction. Returns:
///
/// - `Ok(None)` when the group is skipped (partial typed turn, or nothing
///   projectable);
/// - `Ok(Some(status))` when the group was handled with that status;
/// - `Err(ProjectionError)` when a DB/FK/commit error aborted the group — the
///   watermark is untouched so the retry re-attempts it.
pub(super) fn project_committed_turn(
    conversation_id: &str,
    run_id: &str,
    events: &[RunEventV2],
) -> Result<Option<ProjectionStatus>, ProjectionError> {
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
    // Retired: a turn group without TurnStarted is a pre-typed delta-only
    // batch (the engine always emits typed turns now). It is not materialized.
    let Some(typed_turn_id) = turn_id else {
        return Ok(None);
    };
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
            RunEventKind::MessageCompleted {
                content: Some(content),
                ..
            } => {
                // A corrupt committed payload is quarantined explicitly, never
                // swallowed. If the quarantine row itself cannot be written,
                // that is a retryable failure — the next recovery re-attempts.
                let blocks = match content
                    .get("content")
                    .ok_or_else(|| content.to_string())
                    .and_then(|blocks| {
                        serde_json::from_value::<Vec<ContentBlock>>(blocks.clone())
                            .map_err(|e| e.to_string())
                    }) {
                    Ok(blocks) => blocks,
                    Err(detail) => {
                        quarantine(
                            run_id,
                            Some(&typed_turn_id),
                            assistant_message_id.as_deref(),
                            "corrupt_message_completed",
                            &detail,
                        )
                        .map_err(|e| {
                            ProjectionError::retryable(format!(
                                "corrupt turn could not be quarantined: {e}"
                            ))
                        })?;
                        return Ok(Some(ProjectionStatus::Quarantined));
                    }
                };
                committed_content = Some(blocks);
            }
            RunEventKind::MessageCompleted { content: None, .. } => {}
            _ => {}
        }
    }
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
    let assistant_id = assistant_message_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let last_sequence = events
        .iter()
        .map(|event| event.run_sequence)
        .max()
        .unwrap_or(0);
    let turn_sequence = events
        .iter()
        .find_map(|event| match &event.payload {
            RunEventKind::TurnStarted { .. } => Some(event.run_sequence),
            _ => None,
        })
        .unwrap_or(0);

    let store = conversation_store::store()?;
    let conn = store.conn()?;
    let outcome: Result<TurnWrite, ProjectionFailure> = (|| {
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| sqlite_failure("project turn begin", e))?;

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
            .map_err(|e| sqlite_failure("project turn insert", e))?;

        // Assistant message + blocks: content blocks + run_reference.
        let assistant = AssistantMessage {
            message_id: MessageId::from(assistant_id.clone()),
            content: content.clone(),
            stop_reason: stop_reason.as_deref().map(parse_stop_reason),
        };
        let mut assistant_blocks = content_blocks_to_json(&assistant.content);
        assistant_blocks.push(serde_json::json!({ "type": "run_reference", "run_id": run_id }));
        upsert_message_blocks(
            &tx,
            conversation_id,
            run_id,
            &typed_turn_id,
            &assistant_id,
            "assistant",
            &assistant_blocks,
            stop_reason.as_deref(),
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
            )?;
            projected_ids.push(result_id);
        }

        // Watermark — the event prefix + a digest of the projected message ids.
        // New turns advance the counter; re-projections only ever advance the
        // sequence, so recovery stays idempotent. (The `compat_hits` column is
        // legacy observability and stays 0 — the compat path is retired.)
        let mut hasher = Sha256::new();
        for id in &projected_ids {
            hasher.update(id.as_bytes());
            hasher.update(b"\0");
        }
        let digest = hex::encode(hasher.finalize());
        if turn_affected > 0 {
            tx.execute(
                "INSERT INTO projection_watermark
                    (projector, run_id, event_sequence, turn_count, digest, created_at, updated_at)
                 VALUES ('conversation', ?1, ?2, 1, ?3, datetime('now'), datetime('now'))
                 ON CONFLICT(projector, run_id) DO UPDATE SET
                    event_sequence = excluded.event_sequence,
                    turn_count = turn_count + 1,
                    digest = excluded.digest,
                    updated_at = datetime('now')",
                params![run_id, last_sequence as i64, digest],
            )
            .map_err(|e| sqlite_failure("project watermark upsert", e))?;
        } else {
            tx.execute(
                "INSERT INTO projection_watermark
                    (projector, run_id, event_sequence, turn_count, digest, created_at, updated_at)
                 VALUES ('conversation', ?1, ?2, 1, ?3, datetime('now'), datetime('now'))
                 ON CONFLICT(projector, run_id) DO UPDATE SET
                    event_sequence = MAX(event_sequence, excluded.event_sequence),
                    updated_at = datetime('now')",
                params![run_id, last_sequence as i64, digest],
            )
            .map_err(|e| sqlite_failure("project watermark idempotent", e))?;
        }
        tx.commit()
            .map_err(|e| sqlite_failure("project turn commit", e))?;
        if turn_affected > 0 {
            Ok(TurnWrite::Projected)
        } else {
            Ok(TurnWrite::AlreadyProjected)
        }
    })();
    match outcome {
        Ok(TurnWrite::Projected) => Ok(Some(ProjectionStatus::Projected)),
        Ok(TurnWrite::AlreadyProjected) => Ok(Some(ProjectionStatus::AlreadyProjected)),
        Err(ProjectionFailure::Conflict {
            run_id,
            turn_id,
            message_id,
        }) => {
            // The transaction rolled back when the closure dropped `tx`.
            // Quarantine on a fresh connection so the isolation survives the
            // rollback — never a silent overwrite. A quarantine write failure
            // is surfaced as retryable; the conflict was not silently dropped.
            let detail =
                format!("stored content for message {message_id} disagrees with the events");
            quarantine(
                &run_id,
                Some(&turn_id),
                Some(&message_id),
                "content_conflict",
                &detail,
            )
            .map_err(|e| {
                ProjectionError::retryable(format!(
                    "content conflict could not be quarantined: {e}"
                ))
            })?;
            Ok(Some(ProjectionStatus::Quarantined))
        }
        Err(ProjectionFailure::Retryable(message)) => Err(ProjectionError::retryable(message)),
        Err(ProjectionFailure::Fatal(message)) => Err(ProjectionError::fatal(message)),
    }
}

/// Result of a successful per-turn write.
enum TurnWrite {
    /// The turn row was newly inserted (watermark counted once).
    Projected,
    /// The turn row already existed (idempotent re-projection).
    AlreadyProjected,
}

/// Insert a message row + blocks when absent; when present, verify the stored
/// blocks match the projected blocks (idempotent no-op). A content mismatch is
/// surfaced as `ProjectionFailure::Conflict`; the caller quarantines it AFTER
/// the transaction rolls back, never silently overwriting.
#[allow(clippy::too_many_arguments)] // pre-existing: parameter list is fixed
fn upsert_message_blocks(
    tx: &rusqlite::Transaction,
    conversation_id: &str,
    run_id: &str,
    turn_id: &str,
    message_id: &str,
    role: &str,
    blocks: &[Value],
    stop_reason: Option<&str>,
) -> Result<(), ProjectionFailure> {
    let existing: Vec<(i64, String)> = tx
        .prepare(
            "SELECT sort_order, block_json FROM message_block
             WHERE message_id = ?1 ORDER BY sort_order",
        )
        .map_err(|e| sqlite_failure("project existing blocks read", e))?
        .query_map(params![message_id], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|e| sqlite_failure("project existing blocks map", e))?
        .collect::<Result<_, _>>()
        .map_err(|e| sqlite_failure("project existing blocks collect", e))?;
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
         VALUES (?1, ?2, ?3, 'complete', ?4, ?5, NULL, 0, ?6, ?7)",
        params![
            message_id,
            conversation_id,
            role,
            turn_id,
            run_id,
            stop_reason,
            now
        ],
    )
    .map_err(|e| sqlite_failure("project message insert", e))?;
    for (index, block) in blocks.iter().enumerate() {
        let block_type = block.get("type").and_then(Value::as_str).unwrap_or("text");
        tx.execute(
            "INSERT INTO message_block (message_id, sort_order, block_type, block_json, artifact_id, truncated)
             VALUES (?1, ?2, ?3, ?4, NULL, 0)",
            params![message_id, index as i64, block_type, block.to_string()],
        )
        .map_err(|e| sqlite_failure("project block insert", e))?;
    }
    Ok(())
}

/// Record an explicitly isolated event/turn. This is the "no silent skip"
/// contract: the quarantine row is durable and queryable.
pub(super) fn quarantine(
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
