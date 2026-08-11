//! Prompt-queue ack + message persistence for leased steering/follow-up inputs.
//!
//! Split out of `conversation_store` to keep each file under 1000 lines (W10).
//! Shared helpers (`store`) come from the parent via `super::*`.

use super::*;

/// Atomically persist a leased steering/follow-up input and acknowledge the
/// queue row. A crash before commit leaves the lease recoverable; a crash after
/// commit cannot duplicate the user message because its id is queue-derived.
pub fn persist_queued_input_and_ack(
    conversation_id: &str,
    run_id: &str,
    input_id: &str,
    content: &str,
    turn_id: Option<&str>,
    lease_token: Option<&str>,
) -> Result<(), String> {
    let store = store()?;
    let conn = store.conn()?;
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();
    let message_id = format!("queue:{input_id}");
    let expected_marker = format!("prompt_queue:{input_id}");
    #[allow(clippy::type_complexity)] // pre-existing: factored type alias deferred
    let existing: Option<(String, String, String, Option<String>, Option<String>)> = tx
        .query_row(
            "SELECT conversation_id, role, status, run_id, turn_id
             FROM message WHERE id = ?1",
            params![message_id],
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
        .map_err(|e| e.to_string())?;
    if let Some((existing_conversation, role, status, existing_run, existing_turn)) = existing {
        if existing_conversation != conversation_id
            || role != "user"
            || status != "complete"
            || existing_run.as_deref() != Some(run_id)
            || existing_turn.as_deref() != turn_id
        {
            return Err(format!("queue message identity collision: {message_id}"));
        }
        let stored: String = tx
            .query_row(
                "SELECT block_json FROM message_block
                 WHERE message_id = ?1 AND sort_order = 0 AND block_type = 'text'",
                params![message_id],
                |row| row.get(0),
            )
            .map_err(|e| format!("load queued message block: {e}"))?;
        let expected = serde_json::json!({ "text": content }).to_string();
        if stored != expected {
            return Err(format!("queue message content collision: {message_id}"));
        }
    } else {
        tx.execute(
            "INSERT INTO message
             (id, conversation_id, role, status, run_id, turn_id, legacy_marker, created_at)
             VALUES (?1, ?2, 'user', 'complete', ?3, ?4, ?5, ?6)",
            params![
                message_id,
                conversation_id,
                run_id,
                turn_id,
                expected_marker,
                now
            ],
        )
        .map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO message_block
             (message_id, sort_order, block_type, block_json)
             VALUES (?1, 0, 'text', ?2)",
            params![
                message_id,
                serde_json::json!({ "text": content }).to_string()
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    let changed = tx
        .execute(
            "UPDATE prompt_queue SET status = 'sent', consumed_turn_id = ?1,
             lease_token = NULL, lease_run_id = NULL, leased_at = NULL, updated_at = ?2
             WHERE id = ?3 AND conversation_id = ?4 AND lease_run_id = ?5
               AND lease_token = ?6 AND status = 'leased'",
            params![
                turn_id.unwrap_or(run_id),
                now,
                input_id,
                conversation_id,
                run_id,
                lease_token
            ],
        )
        .map_err(|e| e.to_string())?;
    if changed != 1 {
        return Err("queue lease was lost before message commit".into());
    }
    tx.execute(
        "UPDATE conversation SET updated_at = ?1 WHERE id = ?2",
        params![now, conversation_id],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}
