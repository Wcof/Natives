//! Daemon-owned prompt queue + SessionCoordinator persistence.
//!
//! **Single write authority for Assistant execution data (queue / actor):**
//! this module writes `prompt_queue` + `session_actor` on the Daemon's
//! `assistant.db`. Host must not dual-write queue rows in UDS production.
//!
//! In-memory [`SessionCoordinator`] (agent-core) is the live coordination
//! actor; SQLite is the durable source of truth across restarts.

use crate::conversation_store;
use crate::run_manager::global_run_manager;
use crate::storage::DataStore;
use agent_core::{
    CoordinatorAction, HarnessAction, PromptSource, QueueItem, QueueItemStatus, SafePoint,
    SessionActorSnapshot, SessionCoordinator,
};
use assistant_protocol::v2::methods::names;
use assistant_protocol::v2::{CancelRunRequest, StartRunRequest};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use uuid::Uuid;

static GLOBAL_HARNESS: OnceLock<Arc<SessionCoordinator>> = OnceLock::new();

pub fn global_harness() -> Arc<SessionCoordinator> {
    GLOBAL_HARNESS
        .get_or_init(|| SessionCoordinator::shared())
        .clone()
}

/// Alias for clarity at call sites.
pub fn global_coordinator() -> Arc<SessionCoordinator> {
    global_harness()
}

fn store() -> Result<DataStore, String> {
    #[cfg(test)]
    let _env_guard = crate::storage::DataStore::env_test_lock();
    #[cfg(test)]
    if let Some((db_path, artifact_dir)) = crate::storage::test_db_override() {
        if let Some(parent) = db_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        return DataStore::new(&db_path, &artifact_dir);
    }
    let db_path = std::env::var("NATIVES_ASSISTANT_DB_PATH")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("NATIVES_DB_PATH")
                .ok()
                .filter(|s| !s.trim().is_empty())
                .map(PathBuf::from)
        });
    #[cfg(test)]
    let db_path = db_path.ok_or_else(|| {
        "test store() requires NATIVES_ASSISTANT_DB_PATH or NATIVES_DB_PATH (refusing ~/.natives default)".to_string()
    })?;
    #[cfg(not(test))]
    let db_path = db_path.unwrap_or_else(crate::default_assistant_db_path);
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let artifact_dir = std::env::var("NATIVES_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            PathBuf::from(home).join(".natives").join("runtime")
        })
        .join("artifacts");
    DataStore::new(&db_path, &artifact_dir)
}

fn ensure_conversation_for_queue(conversation_id: &str, params: &Value) -> Result<(), String> {
    let provider = params
        .get("provider_id")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let model = params
        .get("model_id")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let project = params.get("project_id").and_then(Value::as_str);
    conversation_store::ensure_conversation_stub(
        conversation_id,
        provider,
        model,
        None,
        project,
    )
}

fn row_to_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    let attachments: Option<String> = row.get(4)?;
    // status column may be missing on pre-migration-012 DBs mid-upgrade.
    let status: String = row.get::<_, String>(9).unwrap_or_else(|_| "queued".into());
    Ok(json!({
        "id": row.get::<_, String>(0)?,
        "conversation_id": row.get::<_, String>(1)?,
        "content": row.get::<_, String>(2)?,
        "source": row.get::<_, String>(3)?,
        "attachments": attachments.and_then(|raw| serde_json::from_str::<Value>(&raw).ok()),
        "order": row.get::<_, i64>(5)?,
        "position": row.get::<_, i64>(5)?,
        "client_temp_id": row.get::<_, Option<String>>(6)?,
        "created_at": row.get::<_, String>(7)?,
        "updated_at": row.get::<_, String>(8)?,
        "status": status,
    }))
}

fn value_to_queue_item(item: &Value) -> Option<QueueItem> {
    let id = item.get("id")?.as_str()?.to_string();
    let conversation_id = item.get("conversation_id")?.as_str()?.to_string();
    let content = item.get("content")?.as_str()?.to_string();
    let source = item
        .get("source")
        .and_then(Value::as_str)
        .map(PromptSource::parse)
        .unwrap_or(PromptSource::User);
    let position = item
        .get("position")
        .or_else(|| item.get("order"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let client_temp_id = item
        .get("client_temp_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    let created_at = item
        .get("created_at")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let status = item
        .get("status")
        .and_then(Value::as_str)
        .map(QueueItemStatus::parse)
        .unwrap_or(QueueItemStatus::Queued);
    Some(QueueItem {
        id,
        conversation_id,
        content,
        source,
        position,
        client_temp_id,
        created_at,
        status,
    })
}

/// Persist SessionCoordinator snapshot for one conversation.
/// Returns error on store/SQL failure so callers can fail closed.
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

/// Best-effort wrapper for non-critical paths that historically ignored errors.
fn persist_actor_snapshot_best_effort(conversation_id: &str) {
    if let Err(e) = persist_actor_snapshot(conversation_id) {
        eprintln!("[prompt_queue] persist_actor_snapshot: {e}");
    }
}

fn load_actor_snapshot(conversation_id: &str) -> Option<SessionActorSnapshot> {
    let store = store().ok()?;
    let conn = store.conn().ok()?;
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
                cancel_requested: row.get::<_, i64>(6).unwrap_or(0) != 0,
                drain_on_finish: row.get::<_, i64>(7).unwrap_or(1) != 0,
                version: row.get::<_, i64>(8).unwrap_or(0) as u64,
            })
        },
    )
    .optional()
    .ok()
    .flatten()
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
    for row in rows.flatten() {
        if let Some(item) = value_to_queue_item(&row) {
            items.push(item);
        }
    }
    let harness = global_harness();
    harness.reload_queue(conversation_id, items);
    if let Some(snap) = load_actor_snapshot(conversation_id) {
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
    if let Ok(mut stmt) = conn.prepare(
        "SELECT DISTINCT conversation_id FROM prompt_queue
         WHERE COALESCE(status, 'queued') IN ('queued', 'running')",
    ) {
        let rows = stmt.query_map([], |row| row.get::<_, String>(0));
        if let Ok(rows) = rows {
            for id in rows.flatten() {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
    }
    if let Ok(mut stmt) = conn.prepare("SELECT conversation_id FROM session_actor") {
        let rows = stmt.query_map([], |row| row.get::<_, String>(0));
        if let Ok(rows) = rows {
            for id in rows.flatten() {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
    }
    let _ = conn.execute(
        "UPDATE prompt_queue SET status = 'queued', updated_at = ?1
         WHERE status = 'running'",
        params![chrono::Utc::now().to_rfc3339()],
    );
    let _ = conn.execute(
        "UPDATE session_actor SET active_run_id = NULL, running_prompt_id = NULL,
            cancel_requested = 0, updated_at = ?1",
        params![chrono::Utc::now().to_rfc3339()],
    );
    let n = ids.len();
    for id in ids {
        let _ = hydrate_conversation(&id);
    }
    Ok(n)
}

/// RPC entry: `promptQueue.*` methods.
pub async fn request(method: &str, params: Value) -> Result<Value, String> {
    match method {
        names::PROMPT_QUEUE_LIST => list(params),
        names::PROMPT_QUEUE_ENQUEUE => enqueue(params),
        names::PROMPT_QUEUE_UPDATE => update(params),
        names::PROMPT_QUEUE_REMOVE => remove(params),
        names::PROMPT_QUEUE_REORDER => reorder(params),
        names::PROMPT_QUEUE_SEND_NOW => send_now(params).await,
        names::PROMPT_QUEUE_INTERJECT => interject(params),
        _ => Err(format!("unsupported promptQueue method: {method}")),
    }
}

fn list(params: Value) -> Result<Value, String> {
    let conversation_id = params
        .get("conversation_id")
        .or_else(|| params.get("conversationId"))
        .and_then(Value::as_str)
        .ok_or_else(|| "conversation_id is required".to_string())?;
    let store = store()?;
    let conn = store.conn()?;
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
    let items: Vec<Value> = rows.filter_map(Result::ok).collect();

    // SQLite is source of truth — always rehydrate coordinator from durable rows.
    let q_items: Vec<QueueItem> = items.iter().filter_map(value_to_queue_item).collect();
    let harness = global_harness();
    harness.reload_queue(conversation_id, q_items);
    if let Some(snap) = load_actor_snapshot(conversation_id) {
        harness.restore_snapshot(snap);
    }

    Ok(Value::Array(items))
}

fn enqueue(params: Value) -> Result<Value, String> {
    let conversation_id = params
        .get("conversation_id")
        .or_else(|| params.get("conversationId"))
        .and_then(Value::as_str)
        .ok_or_else(|| "conversation_id is required".to_string())?;
    let content = params
        .get("content")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| "content is required".to_string())?;
    let source = params
        .get("source")
        .and_then(Value::as_str)
        .unwrap_or("user");
    let client_temp_id = params
        .get("client_temp_id")
        .or_else(|| params.get("clientTempId"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let attachments = params
        .get("attachments")
        .map(|v| v.to_string())
        .unwrap_or_else(|| "null".into());

    ensure_conversation_for_queue(conversation_id, &params)?;

    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let store = store()?;
    let conn = store.conn()?;
    let position: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM prompt_queue WHERE conversation_id = ?1",
            params![conversation_id],
            |row| row.get(0),
        )
        .unwrap_or(0);

    conn.execute(
        "INSERT INTO prompt_queue
            (id, conversation_id, content, source, attachments, position, client_temp_id, created_at, updated_at, status)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, 'queued')",
        params![
            id,
            conversation_id,
            content,
            source,
            attachments,
            position,
            client_temp_id,
            now
        ],
    )
    .or_else(|e| {
        if e.to_string().contains("no such column: status") {
            conn.execute(
                "INSERT INTO prompt_queue
                    (id, conversation_id, content, source, attachments, position, client_temp_id, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
                params![
                    id,
                    conversation_id,
                    content,
                    source,
                    attachments,
                    position,
                    client_temp_id,
                    now
                ],
            )
        } else {
            Err(e)
        }
    })
    .map_err(|e| format!("prompt_queue insert: {e}"))?;

    let item = global_harness().enqueue(
        conversation_id,
        content,
        PromptSource::parse(source),
        client_temp_id.clone(),
        Some(id.clone()),
    );
    persist_actor_snapshot(conversation_id)?;

    Ok(json!({
        "id": item.id,
        "conversation_id": conversation_id,
        "content": content,
        "source": source,
        "order": position,
        "position": position,
        "client_temp_id": client_temp_id,
        "created_at": now,
        "status": "queued",
    }))
}

fn update(params: Value) -> Result<Value, String> {
    let id = params
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "id is required".to_string())?;
    let content = params
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| "content is required".to_string())?;
    let now = chrono::Utc::now().to_rfc3339();
    let store = store()?;
    let conn = store.conn()?;
    let conversation_id: String = conn
        .query_row(
            "SELECT conversation_id FROM prompt_queue WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "prompt queue item not found".to_string())?;

    let n = conn
        .execute(
            "UPDATE prompt_queue SET content = ?1, updated_at = ?2 WHERE id = ?3",
            params![content, now, id],
        )
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Err("prompt queue item not found".into());
    }

    let _ = global_harness().update(&conversation_id, id, content);
    persist_actor_snapshot(&conversation_id)?;
    Ok(json!({ "id": id, "updated": true }))
}

fn remove(params: Value) -> Result<Value, String> {
    let id = params
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "id is required".to_string())?;
    let store = store()?;
    let conn = store.conn()?;
    let conversation_id: Option<String> = conn
        .query_row(
            "SELECT conversation_id FROM prompt_queue WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;

    let n = conn
        .execute("DELETE FROM prompt_queue WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    if n == 0 {
        return Err("prompt queue item not found".into());
    }
    if let Some(cid) = conversation_id {
        let _ = global_harness().remove(&cid, id);
        persist_actor_snapshot(&cid)?;
    }
    Ok(json!({ "id": id, "removed": true }))
}

fn reorder(params: Value) -> Result<Value, String> {
    let conversation_id = params
        .get("conversation_id")
        .or_else(|| params.get("conversationId"))
        .and_then(Value::as_str)
        .ok_or_else(|| "conversation_id is required".to_string())?;
    let ids = params
        .get("ids")
        .and_then(Value::as_array)
        .ok_or_else(|| "ids is required".to_string())?;
    let id_list: Vec<String> = ids
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect();

    let store = store()?;
    let conn = store.conn()?;
    let now = chrono::Utc::now().to_rfc3339();
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| e.to_string())?;
    for (position, id) in id_list.iter().enumerate() {
        tx.execute(
            "UPDATE prompt_queue SET position = ?1, updated_at = ?2
             WHERE id = ?3 AND conversation_id = ?4",
            params![position as i64, now, id, conversation_id],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())?;

    let _ = global_harness().reorder(conversation_id, &id_list);
    persist_actor_snapshot(conversation_id)?;
    Ok(json!({ "conversation_id": conversation_id, "reordered": true }))
}

fn interject(params: Value) -> Result<Value, String> {
    let conversation_id = params
        .get("conversation_id")
        .or_else(|| params.get("conversationId"))
        .and_then(Value::as_str)
        .ok_or_else(|| "conversation_id is required".to_string())?;
    let content = params
        .get("content")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| "content is required".to_string())?;

    ensure_conversation_for_queue(conversation_id, &params)?;
    global_harness().interject(conversation_id, content);
    // Durable: session_actor.pending_interjection (latest wins across restart).
    persist_actor_snapshot(conversation_id)?;
    Ok(json!({
        "conversation_id": conversation_id,
        "interjected": true,
        "content": content,
        "note": "pending until next SafePoint (provider batch / tool / permission); durable in session_actor",
    }))
}

async fn send_now(params: Value) -> Result<Value, String> {
    let id = params
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "id is required".to_string())?;

    let store = store()?;
    let (
        conversation_id,
        content,
        attachments_raw,
        provider_id,
        model_id,
        project_path,
    ) = {
        let conn = store.conn()?;
        conn.query_row(
            "SELECT q.conversation_id, q.content, q.attachments,
                    c.provider_id, c.model_id, c.project_id
             FROM prompt_queue q
             JOIN conversation c ON c.id = q.conversation_id
             WHERE q.id = ?1",
            params![id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            },
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "prompt queue item not found".to_string())?
    };

    // Ensure coordinator knows about this item (may already).
    let harness = global_harness();
    if !harness.list(&conversation_id).iter().any(|i| i.id == id) {
        harness.enqueue(
            &conversation_id,
            &content,
            PromptSource::User,
            None,
            Some(id.to_string()),
        );
    }

    let action = harness
        .send_now(&conversation_id, id)
        .map_err(|e| e)?;
    persist_actor_snapshot(&conversation_id)?;

    // Cancel active runs when needed. Only the winner of finish_run(expected_run_id)
    // may start the next prompt — either us (after cancel settles) or on_run_terminal.
    if matches!(action, CoordinatorAction::CancelThenStart { .. }) {
        let rm = global_run_manager();
        let active_ids: Vec<String> = rm
            .list_runs(Some(&conversation_id))
            .into_iter()
            .filter(|r| !r.status.is_terminal())
            .map(|r| r.id)
            .collect();
        for run_id in &active_ids {
            let _ = rm
                .cancel(CancelRunRequest {
                    run_id: run_id.clone(),
                })
                .await;
            for _ in 0..20 {
                if rm
                    .get_run(run_id)
                    .map(|r| r.status.is_terminal())
                    .unwrap_or(true)
                {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
        }

        // Prefer the coordinator's recorded active run id (exact match for finish_run).
        let expected = harness
            .snapshot(&conversation_id)
            .active_run_id
            .or_else(|| active_ids.first().cloned());

        if let Some(expected_run_id) = expected {
            if let Some(next) = harness.finish_run(&conversation_id, &expected_run_id, false) {
                if let CoordinatorAction::StartPrompt { item } = next {
                    {
                        let conn = store.conn()?;
                        let now = chrono::Utc::now().to_rfc3339();
                        let _ = conn.execute(
                            "UPDATE prompt_queue SET status = 'sent', updated_at = ?1 WHERE id = ?2",
                            params![now, item.id],
                        );
                        let _ =
                            conn.execute("DELETE FROM prompt_queue WHERE id = ?1", params![item.id]);
                    }
                    let start_req = StartRunRequest {
                        run_id: None,
                        conversation_id: Some(conversation_id.clone()),
                        provider_id: Some(provider_id.clone()),
                        model_id: Some(model_id.clone()),
                        key_id: None,
                        content: Some(item.content.clone()),
                        attachments: None,
                        trigger_message_id: None,
                        permission_profile: None,
                        max_steps: None,
                        project_path: project_path.clone(),
                        idempotency_key: Some(format!("prompt-queue:{}", item.id)),
                        effort: None,
                        runtime_id: None,
                    };
                    let run = crate::run_manager::RunManager::start_detached_global(start_req)?;
                    harness.mark_running_item(
                        &conversation_id,
                        &run.id,
                        Some(&item.id),
                        &item.content,
                    );
                    persist_actor_snapshot(&conversation_id)?;
                    return Ok(serde_json::to_value(run).unwrap_or_else(|_| {
                        json!({
                            "conversation_id": conversation_id,
                            "content": item.content,
                            "started": true,
                            "queue_item_id": item.id,
                        })
                    }));
                }
            }
        }

        // on_run_terminal already claimed finish and started next (or still mid-cancel).
        persist_actor_snapshot(&conversation_id)?;
        return Ok(json!({
            "conversation_id": conversation_id,
            "content": content,
            "started": false,
            "cancelling": true,
            "queue_item_id": id,
            "note": "cancel requested; next prompt starts via sole finish_run winner",
        }));
    }

    // Idle path: StartPrompt immediately.
    {
        let conn = store.conn()?;
        let now = chrono::Utc::now().to_rfc3339();
        let _ = conn.execute(
            "UPDATE prompt_queue SET status = 'sent', updated_at = ?1 WHERE id = ?2",
            params![now, id],
        );
        conn.execute("DELETE FROM prompt_queue WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
    }

    let _ = attachments_raw
        .as_ref()
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok());

    let start_req = StartRunRequest {
        run_id: None,
        conversation_id: Some(conversation_id.clone()),
        provider_id: Some(provider_id),
        model_id: Some(model_id),
        key_id: None,
        content: Some(content.clone()),
        attachments: None,
        trigger_message_id: None,
        permission_profile: None,
        max_steps: None,
        project_path,
        // Unified queue idempotency key (also used by drain / cancel-and-send).
        idempotency_key: Some(format!("prompt-queue:{id}")),
        effort: None,
        runtime_id: None,
    };

    let run = crate::run_manager::RunManager::start_detached_global(start_req)?;
    harness.mark_running_item(&conversation_id, &run.id, Some(id), &content);
    persist_actor_snapshot(&conversation_id)?;

    Ok(serde_json::to_value(run).unwrap_or_else(|_| {
        json!({
            "conversation_id": conversation_id,
            "content": content,
            "started": true,
            "queue_item_id": id,
        })
    }))
}

/// Engine / permission hook: process a safe point for a conversation.
pub fn on_safe_point(conversation_id: &str, point: SafePoint) -> HarnessAction {
    let action = global_harness().on_safe_point(conversation_id, point);
    if !matches!(action, CoordinatorAction::None) {
        persist_actor_snapshot_best_effort(conversation_id);
    }
    action
}

/// Called when a run reaches a real terminal state.
/// Atomically claims finish via `finish_run(expected_run_id)` so concurrent
/// send_now / duplicate terminals never double-advance the queue.
pub async fn on_run_terminal(
    conversation_id: &str,
    run_id: &str,
    success: bool,
) -> Result<Option<String>, String> {
    let harness = global_harness();
    let Some(action) = harness.finish_run(conversation_id, run_id, success) else {
        // Stale terminal (wrong run id, already finished, or empty active).
        return Ok(None);
    };
    persist_actor_snapshot(conversation_id)?;

    match action {
        CoordinatorAction::StartPrompt { item } => {
            let store = store()?;
            let (provider_id, model_id, project_path) = {
                let conn = store.conn()?;
                conn.query_row(
                    "SELECT provider_id, model_id, project_id FROM conversation WHERE id = ?1",
                    params![conversation_id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<String>>(2)?,
                        ))
                    },
                )
                .optional()
                .map_err(|e| e.to_string())?
                .unwrap_or_else(|| ("unknown".into(), "unknown".into(), None))
            };

            {
                let conn = store.conn()?;
                let now = chrono::Utc::now().to_rfc3339();
                let _ = conn.execute(
                    "UPDATE prompt_queue SET status = 'sent', updated_at = ?1 WHERE id = ?2",
                    params![now, item.id],
                );
                let _ = conn.execute(
                    "DELETE FROM prompt_queue WHERE id = ?1",
                    params![item.id],
                );
            }

            let start_req = StartRunRequest {
                run_id: None,
                conversation_id: Some(conversation_id.to_string()),
                provider_id: Some(provider_id),
                model_id: Some(model_id),
                key_id: None,
                content: Some(item.content.clone()),
                attachments: None,
                trigger_message_id: None,
                permission_profile: None,
                max_steps: None,
                project_path,
                // Unified with send_now: prompt-queue:{item_id}
                idempotency_key: Some(format!("prompt-queue:{}", item.id)),
                effort: None,
                runtime_id: None,
            };
            let run = crate::run_manager::RunManager::start_detached_global(start_req)?;
            harness.mark_running_item(
                conversation_id,
                &run.id,
                Some(&item.id),
                &item.content,
            );
            persist_actor_snapshot(conversation_id)?;
            Ok(Some(run.id))
        }
        _ => Ok(None),
    }
}

/// Convert harness QueueItem to JSON (tests / diagnostics).
pub fn queue_item_json(item: &QueueItem) -> Value {
    json!({
        "id": item.id,
        "conversation_id": item.conversation_id,
        "content": item.content,
        "source": item.source.as_str(),
        "order": item.position,
        "client_temp_id": item.client_temp_id,
        "created_at": item.created_at,
        "status": item.status.as_str(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    
    fn env_lock() -> crate::storage::EnvTestGuard {
        crate::storage::DataStore::env_test_lock()
    }

    struct EnvRestore {
        db: Option<String>,
        asst: Option<String>,
        rt: Option<String>,
    }
    impl Drop for EnvRestore {
        fn drop(&mut self) {
            if let Some(v) = self.db.take() {
                std::env::set_var("NATIVES_DB_PATH", v);
            } else {
                std::env::remove_var("NATIVES_DB_PATH");
            }
            if let Some(v) = self.asst.take() {
                std::env::set_var("NATIVES_ASSISTANT_DB_PATH", v);
            } else {
                std::env::remove_var("NATIVES_ASSISTANT_DB_PATH");
            }
            if let Some(v) = self.rt.take() {
                std::env::set_var("NATIVES_RUNTIME_DIR", v);
            } else {
                std::env::remove_var("NATIVES_RUNTIME_DIR");
            }
        }
    }

    fn with_temp_db<F: FnOnce()>(f: F) {
        let _guard = env_lock();
        let _restore = EnvRestore {
            db: std::env::var("NATIVES_DB_PATH").ok(),
            asst: std::env::var("NATIVES_ASSISTANT_DB_PATH").ok(),
            rt: std::env::var("NATIVES_RUNTIME_DIR").ok(),
        };
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join(format!("assistant-{}.db", Uuid::new_v4()));
        std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db);
        std::env::set_var("NATIVES_DB_PATH", &db);
        std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
        let art = dir.path().join("artifacts");
        crate::storage::set_test_db_override(Some(db.clone()), Some(art.clone()));
        let warm = crate::storage::DataStore::new(&db, &art)
            .expect("prompt_queue temp db migrate");
        assert!(
            warm.has_table("conversation") && warm.has_table("prompt_queue"),
            "temp db missing tables: {}",
            db.display()
        );
        f();
        drop(warm);
        crate::storage::set_test_db_override(None, None);
        drop(dir);
        // _restore drops here even on panic
    }

    #[test]
    fn db_enqueue_list_order() {
        with_temp_db(|| {
            let cid = format!("pq-{}", Uuid::new_v4());
            conversation_store::ensure_conversation_stub(&cid, "openai", "gpt-4o", None, None)
                .unwrap();
            let a = enqueue(json!({
                "conversation_id": cid,
                "content": "one",
            }))
            .unwrap();
            let b = enqueue(json!({
                "conversation_id": cid,
                "content": "two",
            }))
            .unwrap();
            assert_eq!(a["order"], 0);
            assert_eq!(b["order"], 1);
            let list = list(json!({ "conversation_id": cid })).unwrap();
            let arr = list.as_array().unwrap();
            assert_eq!(arr.len(), 2);
            assert_eq!(arr[0]["content"], "one");
            assert_eq!(arr[1]["content"], "two");
        });
    }

    #[test]
    fn db_update_remove_reorder() {
        with_temp_db(|| {
            let cid = format!("pq-{}", Uuid::new_v4());
            conversation_store::ensure_conversation_stub(&cid, "openai", "gpt-4o", None, None)
                .unwrap();
            let a = enqueue(json!({"conversation_id": cid, "content": "a"})).unwrap();
            let b = enqueue(json!({"conversation_id": cid, "content": "b"})).unwrap();
            let id_a = a["id"].as_str().unwrap().to_string();
            let id_b = b["id"].as_str().unwrap().to_string();
            update(json!({"id": id_a, "content": "a2"})).unwrap();
            reorder(json!({
                "conversation_id": cid,
                "ids": [id_b, id_a],
            }))
            .unwrap();
            let listed = list(json!({"conversation_id": cid})).unwrap();
            let arr = listed.as_array().unwrap();
            assert_eq!(arr[0]["id"], id_b);
            assert_eq!(arr[1]["content"], "a2");
            remove(json!({"id": id_b})).unwrap();
            let listed = list(json!({"conversation_id": cid})).unwrap();
            assert_eq!(listed.as_array().unwrap().len(), 1);
        });
    }

    #[test]
    fn interject_marks_harness_pending() {
        with_temp_db(|| {
            let cid = format!("pq-{}", Uuid::new_v4());
            interject(json!({
                "conversation_id": cid,
                "content": "inject me",
            }))
            .unwrap();
            assert_eq!(
                global_harness().pending_interjection(&cid).as_deref(),
                Some("inject me")
            );
            let action = on_safe_point(&cid, SafePoint::AfterPermissionResolved);
            assert!(matches!(
                action,
                HarnessAction::InjectInterjection { content } if content == "inject me"
            ));
        });
    }

    #[test]
    fn interject_survives_coordinator_rehydrate() {
        with_temp_db(|| {
            let cid = format!("pq-{}", Uuid::new_v4());
            interject(json!({
                "conversation_id": cid,
                "content": "durable inject",
            }))
            .unwrap();
            // Simulate process restart: clear memory then hydrate from SQLite.
            global_harness().clear_conversation(&cid);
            assert!(global_harness().pending_interjection(&cid).is_none());
            hydrate_conversation(&cid).unwrap();
            assert_eq!(
                global_harness().pending_interjection(&cid).as_deref(),
                Some("durable inject")
            );
        });
    }

    #[test]
    fn migration_012_session_actor_table_exists() {
        with_temp_db(|| {
            let s = store().unwrap();
            assert!(
                s.has_table("session_actor"),
                "migration 012 must create session_actor"
            );
            assert!(s.has_table("prompt_queue"));
            let conn = s.conn().unwrap();
            let n: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM pragma_table_info('prompt_queue') WHERE name = 'status'",
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            assert_eq!(n, 1, "prompt_queue.status column required");
        });
    }

    #[test]
    fn send_now_cancels_active_and_starts_new_run() {
        with_temp_db(|| {
            // Fixture provider + same global RunManager that send_now cancels/starts.
            std::env::set_var("NATIVES_DAEMON_FIXTURE", "1");
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async {
                let cid = format!("pq-sendnow-{}", Uuid::new_v4());
                conversation_store::ensure_conversation_stub(
                    &cid, "openai", "gpt-4o", None, Some("/tmp"),
                )
                .unwrap();
                // Use process-global manager: send_now cancels via global_run_manager().
                // start_detached requires &Arc<Self>; use start_detached_global.
                let rm = crate::run_manager::global_run_manager();
                let active = rm
                    .create_run(assistant_protocol::v2::CreateRunRequest {
                        conversation_id: cid.clone(),
                        provider_id: "openai".into(),
                        model_id: "gpt-4o".into(),
                        key_id: Some("k".into()),
                        agent_profile_id: None,
                        permission_profile: Some("ask".into()),
                        content: Some("running".into()),
                        attachments: None,
                        max_steps: Some(3),
                        parent_run_id: None,
                        project_path: Some("/tmp".into()),
                        idempotency_key: Some(format!("active-{}", Uuid::new_v4())),
                        effort: None,
                        runtime_id: Some("native".into()),
                    })
                    .unwrap();
                let _ = crate::run_manager::RunManager::start_detached_global(
                    assistant_protocol::v2::StartRunRequest {
                        run_id: Some(active.id.clone()),
                        conversation_id: Some(cid.clone()),
                        provider_id: Some("openai".into()),
                        model_id: Some("gpt-4o".into()),
                        key_id: Some("k".into()),
                        content: Some("running".into()),
                        attachments: None,
                        trigger_message_id: None,
                        permission_profile: Some("ask".into()),
                        max_steps: Some(3),
                        project_path: Some("/tmp".into()),
                        idempotency_key: None,
                        effort: None,
                        runtime_id: Some("native".into()),
                    },
                )
                .unwrap();
                global_harness().mark_running(&cid, &active.id, "running");

                let queued = enqueue(json!({
                    "conversation_id": cid,
                    "content": "send me now",
                }))
                .unwrap();
                let qid = queued["id"].as_str().unwrap().to_string();

                let started = send_now(json!({ "id": qid })).await.unwrap();
                assert!(
                    started.get("id").and_then(|v| v.as_str()).is_some(),
                    "send_now should return a run: {started}"
                );
                let new_id = started["id"].as_str().unwrap().to_string();
                assert_ne!(new_id, active.id, "should start a new run id");

                // Poll until original is terminal (cancel is async via global manager).
                let mut orig_terminal = false;
                for _ in 0..80 {
                    if let Some(orig) = rm.get_run(&active.id) {
                        if orig.status.is_terminal() {
                            orig_terminal = true;
                            break;
                        }
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                }
                assert!(
                    orig_terminal,
                    "active run should become terminal after send_now"
                );

                let listed = list(json!({ "conversation_id": cid })).unwrap();
                let arr = listed.as_array().cloned().unwrap_or_default();
                assert!(
                    arr.iter()
                        .all(|i| i.get("id").and_then(|v| v.as_str()) != Some(qid.as_str())),
                    "queue should not contain send_now item: {listed}"
                );
            });
        });
    }


}
