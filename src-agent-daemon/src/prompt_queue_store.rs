//! Daemon-owned prompt queue persistence + SessionHarness wiring (Phase 2).
//!
//! Writes to the daemon `prompt_queue` table. In-memory harness tracks
//! interjection / send_now / drain-on-finish per conversation. Host may still
//! keep its own `assistant_prompt_queue` handlers during migration.

use crate::conversation_store;
use crate::run_manager::global_run_manager;
use crate::storage::DataStore;
use agent_core::{
    HarnessAction, PromptSource, QueueItem, SafePoint, SessionHarness,
};
use assistant_protocol::v2::methods::names;
use assistant_protocol::v2::{CancelRunRequest, StartRunRequest};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use uuid::Uuid;

static GLOBAL_HARNESS: OnceLock<Arc<SessionHarness>> = OnceLock::new();

pub fn global_harness() -> Arc<SessionHarness> {
    GLOBAL_HARNESS
        .get_or_init(|| SessionHarness::shared())
        .clone()
}

fn store() -> Result<DataStore, String> {
    let db_path = std::env::var("NATIVES_ASSISTANT_DB_PATH")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("NATIVES_DB_PATH")
                .ok()
                .filter(|s| !s.trim().is_empty())
                .map(PathBuf::from)
        })
        .unwrap_or_else(crate::default_assistant_db_path);
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
    }))
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
    let mut stmt = conn
        .prepare(
            "SELECT id, conversation_id, content, source, attachments, position,
                    client_temp_id, created_at, updated_at
             FROM prompt_queue
             WHERE conversation_id = ?1
             ORDER BY position ASC, created_at ASC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![conversation_id], row_to_item)
        .map_err(|e| e.to_string())?;
    let items: Vec<Value> = rows.filter_map(Result::ok).collect();

    // Keep harness in sync with DB order (best-effort for this process).
    let harness = global_harness();
    // Rebuild harness queue from DB so list is source of truth after restart.
    // Clear by removing known ids then re-enqueue is heavy; for skeleton we only
    // mirror when harness is empty for this conversation.
    if harness.queue_len(conversation_id) == 0 {
        for item in &items {
            let id = item.get("id").and_then(Value::as_str).unwrap_or("");
            let content = item.get("content").and_then(Value::as_str).unwrap_or("");
            let source = item
                .get("source")
                .and_then(Value::as_str)
                .map(PromptSource::parse)
                .unwrap_or(PromptSource::User);
            let client_temp = item
                .get("client_temp_id")
                .and_then(Value::as_str)
                .map(str::to_string);
            if !id.is_empty() {
                harness.enqueue(
                    conversation_id,
                    content,
                    source,
                    client_temp,
                    Some(id.to_string()),
                );
            }
        }
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
    .map_err(|e| format!("prompt_queue insert: {e}"))?;

    let item = global_harness().enqueue(
        conversation_id,
        content,
        PromptSource::parse(source),
        client_temp_id.clone(),
        Some(id.clone()),
    );

    Ok(json!({
        "id": item.id,
        "conversation_id": conversation_id,
        "content": content,
        "source": source,
        "order": position,
        "position": position,
        "client_temp_id": client_temp_id,
        "created_at": now,
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

    global_harness().interject(conversation_id, content);
    // Also persist as a high-priority queue item with interjection source so
    // list/reload survives process restart (optional; harness is source for injection).
    Ok(json!({
        "conversation_id": conversation_id,
        "interjected": true,
        "content": content,
        "note": "pending until next SafePoint (provider batch / tool / permission)",
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

    // Ensure harness knows about this item (may already).
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

    // Cancel active runs when needed.
    if matches!(action, HarnessAction::CancelThenStart { .. }) {
        let rm = global_run_manager();
        let runs = rm.list_runs(Some(&conversation_id));
        for run in runs.into_iter().filter(|r| !r.status.is_terminal()) {
            let _ = rm
                .cancel(CancelRunRequest {
                    run_id: run.id.clone(),
                })
                .await;
            // Wait briefly for terminal (skeleton: poll a few times).
            for _ in 0..20 {
                if rm
                    .get_run(&run.id)
                    .map(|r| r.status.is_terminal())
                    .unwrap_or(true)
                {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
        }
        let _ = harness.mark_finished(&conversation_id);
    }

    // Delete from DB before starting so list no longer shows it.
    {
        let conn = store.conn()?;
        conn.execute("DELETE FROM prompt_queue WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
    }

    let attachments = attachments_raw
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
        idempotency_key: Some(format!("prompt-queue-{id}")),
        effort: None,
        runtime_id: None,
    };
    // attachments field on StartRunRequest is typed; ignore raw JSON for skeleton.
    let _ = attachments;

    let run = crate::run_manager::RunManager::start_detached_global(start_req)?;
    harness.mark_running(&conversation_id, &run.id, &content);

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
    global_harness().on_safe_point(conversation_id, point)
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    fn with_temp_db<F: FnOnce()>(f: F) {
        let _guard = env_lock();
        let dir = tempfile::tempdir().unwrap();
        let prev_db = std::env::var("NATIVES_DB_PATH").ok();
        let prev_asst = std::env::var("NATIVES_ASSISTANT_DB_PATH").ok();
        let prev_rt = std::env::var("NATIVES_RUNTIME_DIR").ok();
        let db = dir.path().join("assistant.db");
        // store() prefers NATIVES_ASSISTANT_DB_PATH over NATIVES_DB_PATH.
        std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db);
        std::env::set_var("NATIVES_DB_PATH", &db);
        std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
        // Fresh harness isolation via unique conversation ids in each test.
        f();
        if let Some(v) = prev_db {
            std::env::set_var("NATIVES_DB_PATH", v);
        } else {
            std::env::remove_var("NATIVES_DB_PATH");
        }
        if let Some(v) = prev_asst {
            std::env::set_var("NATIVES_ASSISTANT_DB_PATH", v);
        } else {
            std::env::remove_var("NATIVES_ASSISTANT_DB_PATH");
        }
        if let Some(v) = prev_rt {
            std::env::set_var("NATIVES_RUNTIME_DIR", v);
        } else {
            std::env::remove_var("NATIVES_RUNTIME_DIR");
        }
    }

    #[test]
    fn db_enqueue_list_order() {
        with_temp_db(|| {
            let cid = format!("pq-{}", Uuid::new_v4());
            conversation_store::ensure_conversation_stub(
                &cid, "openai", "gpt-4o", None, None,
            )
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
            conversation_store::ensure_conversation_stub(
                &cid, "openai", "gpt-4o", None, None,
            )
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
}
