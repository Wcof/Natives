//! SessionCoordinator snapshot persistence (W9 split from prompt_queue_store.rs).

use super::{global_harness, row_to_item, store, value_to_queue_item};
use agent_core::SessionActorSnapshot;
use rusqlite::{params, OptionalExtension};

pub fn persist_actor_snapshot(conversation_id: &str) -> Result<(), String> {
    let snap = global_harness().snapshot(conversation_id);
    let store = store()?;
    let conn = store.conn()?;
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO session_actor (
            conversation_id, active_run_id, running_prompt_id, pending_interjection,
            pending_interaction_id, cancel_and_send_id, cancel_requested, drain_on_finish,
            version, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(conversation_id) DO UPDATE SET
            active_run_id = excluded.active_run_id,
            running_prompt_id = excluded.running_prompt_id,
            pending_interjection = excluded.pending_interjection,
            pending_interaction_id = excluded.pending_interaction_id,
            cancel_and_send_id = excluded.cancel_and_send_id,
            cancel_requested = excluded.cancel_requested,
            drain_on_finish = excluded.drain_on_finish,
            version = excluded.version,
            updated_at = excluded.updated_at",
        params![
            snap.conversation_id,
            snap.active_run_id,
            snap.running_prompt_id,
            snap.pending_interjection,
            snap.pending_interaction_id,
            snap.cancel_and_send_id,
            if snap.cancel_requested { 1 } else { 0 },
            if snap.drain_on_finish { 1 } else { 0 },
            snap.version as i64,
            now,
        ],
    )
    .map_err(|e| format!("persist session_actor failed: {e}"))?;
    Ok(())
}

pub(crate) fn load_actor_snapshot(conversation_id: &str) -> Result<Option<SessionActorSnapshot>, String> {
    let store = store()?;
    let conn = store.conn()?;
    conn.query_row(
        "SELECT conversation_id, active_run_id, running_prompt_id, pending_interjection,
                pending_interaction_id, cancel_and_send_id, cancel_requested, drain_on_finish,
                version
         FROM session_actor WHERE conversation_id = ?1",
        params![conversation_id],
        |row| {
            Ok(SessionActorSnapshot {
                conversation_id: row.get(0)?,
                active_run_id: row.get(1)?,
                running_prompt_id: row.get(2)?,
                pending_interjection: row.get(3)?,
                pending_interaction_id: row.get(4)?,
                cancel_and_send_id: row.get(5)?,
                cancel_requested: row.get::<_, i64>(6)? != 0,
                drain_on_finish: row.get::<_, i64>(7)? != 0,
                version: row.get::<_, i64>(8)? as u64,
            })
        },
    )
    .optional()
    .map_err(|e| e.to_string())
}

/// Rebuild in-memory coordinator for a conversation from SQLite (queue + actor).
pub fn hydrate_conversation(conversation_id: &str) -> Result<(), String> {
    let store = store()?;
    let conn = store.conn()?;
    let mut items = Vec::new();
    let sql_with_status = "SELECT id, conversation_id, content, source, attachments, position,
                                  client_temp_id, created_at, updated_at, status
                           FROM prompt_queue
                           WHERE conversation_id = ?1
                           ORDER BY position ASC, created_at ASC";
    let sql_legacy = "SELECT id, conversation_id, content, source, attachments, position,
                             client_temp_id, created_at, updated_at, 'queued' AS status
                      FROM prompt_queue
                      WHERE conversation_id = ?1
                      ORDER BY position ASC, created_at ASC";
    let mut stmt = conn
        .prepare(sql_with_status)
        .or_else(|_| conn.prepare(sql_legacy))
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![conversation_id], row_to_item)
        .map_err(|e| e.to_string())?;
    for row in rows {
        let row = row.map_err(|e| e.to_string())?;
        items.push(value_to_queue_item(&row)?);
    }
    let harness = global_harness();
    harness.reload_queue(conversation_id, items);
    if let Some(snap) = load_actor_snapshot(conversation_id)? {
        harness.restore_snapshot(snap);
    }
    Ok(())
}

/// On daemon start: hydrate all conversations that have queue or actor state.
/// Does **not** auto-start runs (no silent re-execution after crash).
pub fn recover_session_actors_on_startup() -> Result<usize, String> {
    let store = store()?;
    let conn = store.conn()?;
    let mut ids: Vec<String> = Vec::new();
    {
        let mut stmt = conn
            .prepare(
                "SELECT DISTINCT conversation_id FROM prompt_queue
                 WHERE COALESCE(status, 'queued') IN ('queued', 'running', 'leased')",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        for id in rows {
            let id = id.map_err(|e| e.to_string())?;
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    {
        let mut stmt = conn
            .prepare("SELECT conversation_id FROM session_actor")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        for id in rows {
            let id = id.map_err(|e| e.to_string())?;
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    conn.execute(
        "UPDATE prompt_queue SET status = 'queued', lease_token = NULL,
            lease_run_id = NULL, leased_at = NULL, updated_at = ?1
         WHERE status IN ('running', 'leased')",
        params![chrono::Utc::now().to_rfc3339()],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE session_actor SET active_run_id = NULL, running_prompt_id = NULL,
            cancel_requested = 0, updated_at = ?1",
        params![chrono::Utc::now().to_rfc3339()],
    )
    .map_err(|e| e.to_string())?;
    let n = ids.len();
    for id in ids {
        hydrate_conversation(&id)?;
    }
    Ok(n)
}
