//! Daemon-owned task persistence (terminal tasks + subagent tasks).
//!
//! Rows live in the `task` table. Used by:
//! - `task.list` / `task.wait` / `task.cancel` RPC
//! - subagent spawn and terminal task tracking

use crate::storage::DataStore;
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};

fn store() -> Result<DataStore, String> {
    // W2: single Daemon DataStore open path (assistant.db authority + test hook).
    crate::storage::open_daemon_store()
}

/// Insert a new task record.
pub fn insert_task(
    id: &str,
    conversation_id: Option<&str>,
    parent_run_id: Option<&str>,
    agent_profile_id: Option<&str>,
    title: &str,
    input: Option<&Value>,
) -> Result<(), String> {
    let s = store()?;
    let conn = s.conn().map_err(|e| format!("conn lock: {e}"))?;
    conn.execute(
        "INSERT INTO task (id, conversation_id, parent_run_id, agent_profile_id, status, title, input)
         VALUES (?1, ?2, ?3, ?4, 'pending', ?5, ?6)",
        params![
            id,
            conversation_id,
            parent_run_id,
            agent_profile_id,
            title,
            input.map(|v| v.to_string()),
        ],
    )
    .map_err(|e| format!("insert_task failed: {e}"))?;
    Ok(())
}

/// Update task status.
pub fn update_task_status(id: &str, status: &str, result: Option<&Value>) -> Result<(), String> {
    let s = store()?;
    let conn = s.conn().map_err(|e| format!("conn lock: {e}"))?;
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE task SET status = ?1, result = ?2, updated_at = ?3,
         finished_at = CASE WHEN ?1 IN ('completed', 'failed', 'cancelled', 'interrupted') THEN ?3 ELSE finished_at END
         WHERE id = ?4",
        params![status, result.map(|v| v.to_string()), now, id],
    )
    .map_err(|e| format!("update_task_status failed: {e}"))?;
    Ok(())
}

/// Get a task by ID.
pub fn get_task(id: &str) -> Result<Option<Value>, String> {
    let s = store()?;
    let conn = s.conn().map_err(|e| format!("conn lock: {e}"))?;
    let result = conn.query_row(
        "SELECT id, conversation_id, parent_run_id, agent_profile_id, status, title, input, result, created_at, updated_at, finished_at
         FROM task WHERE id = ?1",
        params![id],
        |row| {
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "conversation_id": row.get::<_, Option<String>>(1)?,
                "parent_run_id": row.get::<_, Option<String>>(2)?,
                "agent_profile_id": row.get::<_, Option<String>>(3)?,
                "status": row.get::<_, String>(4)?,
                "title": row.get::<_, String>(5)?,
                "input": row.get::<_, Option<String>>(6)?.and_then(|s| serde_json::from_str::<Value>(&s).ok()).unwrap_or_default(),
                "result": row.get::<_, Option<String>>(7)?.and_then(|s| serde_json::from_str::<Value>(&s).ok()).unwrap_or_default(),
                "created_at": row.get::<_, String>(8)?,
                "updated_at": row.get::<_, String>(9)?,
                "finished_at": row.get::<_, Option<String>>(10)?,
            }))
        },
    ).optional().map_err(|e| format!("get_task failed: {e}"))?;
    Ok(result)
}

/// List tasks for a conversation.
pub fn list_tasks(conversation_id: &str) -> Result<Vec<Value>, String> {
    let s = store()?;
    let conn = s.conn().map_err(|e| format!("conn lock: {e}"))?;
    let mut stmt = conn
        .prepare(
            "SELECT id, conversation_id, parent_run_id, agent_profile_id, status, title, input, result, created_at, updated_at, finished_at
             FROM task WHERE conversation_id = ?1 ORDER BY created_at DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![conversation_id], |row| {
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "conversation_id": row.get::<_, Option<String>>(1)?,
                "parent_run_id": row.get::<_, Option<String>>(2)?,
                "agent_profile_id": row.get::<_, Option<String>>(3)?,
                "status": row.get::<_, String>(4)?,
                "title": row.get::<_, String>(5)?,
                "input": row.get::<_, Option<String>>(6)?.and_then(|s| serde_json::from_str::<Value>(&s).ok()).unwrap_or_default(),
                "result": row.get::<_, Option<String>>(7)?.and_then(|s| serde_json::from_str::<Value>(&s).ok()).unwrap_or_default(),
                "created_at": row.get::<_, String>(8)?,
                "updated_at": row.get::<_, String>(9)?,
                "finished_at": row.get::<_, Option<String>>(10)?,
            }))
        })
        .map_err(|e| e.to_string())?;
    let mut tasks = Vec::new();
    for row in rows {
        tasks.push(row.map_err(|e| e.to_string())?);
    }
    Ok(tasks)
}

/// List all tasks (for task.list RPC).
pub fn list_all_tasks() -> Result<Vec<Value>, String> {
    let s = store()?;
    let conn = s.conn().map_err(|e| format!("conn lock: {e}"))?;
    let mut stmt = conn
        .prepare(
            "SELECT id, conversation_id, parent_run_id, agent_profile_id, status, title, input, result, created_at, updated_at, finished_at
             FROM task ORDER BY created_at DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "conversation_id": row.get::<_, Option<String>>(1)?,
                "parent_run_id": row.get::<_, Option<String>>(2)?,
                "agent_profile_id": row.get::<_, Option<String>>(3)?,
                "status": row.get::<_, String>(4)?,
                "title": row.get::<_, String>(5)?,
                "input": row.get::<_, Option<String>>(6)?.and_then(|s| serde_json::from_str::<Value>(&s).ok()).unwrap_or_default(),
                "result": row.get::<_, Option<String>>(7)?.and_then(|s| serde_json::from_str::<Value>(&s).ok()).unwrap_or_default(),
                "created_at": row.get::<_, String>(8)?,
                "updated_at": row.get::<_, String>(9)?,
                "finished_at": row.get::<_, Option<String>>(10)?,
            }))
        })
        .map_err(|e| e.to_string())?;
    let mut tasks = Vec::new();
    for row in rows {
        tasks.push(row.map_err(|e| e.to_string())?);
    }
    Ok(tasks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn with_temp_db<F: FnOnce()>(f: F) {
        let _guard = crate::storage::DataStore::env_test_lock();
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("task-test.db");
        let art = dir.path().join("artifacts");
        crate::storage::set_test_db_override(Some(db.clone()), Some(art.clone()));
        let _warm = crate::storage::DataStore::new(&db, &art).expect("task temp db migrate");
        f();
        crate::storage::set_test_db_override(None, None);
    }

    #[test]
    fn insert_get_update_roundtrip() {
        with_temp_db(|| {
            let id = format!("task-{}", uuid::Uuid::new_v4());
            insert_task(
                &id,
                None,
                None,
                None,
                "Test task",
                Some(&json!({"cmd": "ls"})),
            )
            .unwrap();

            let task = get_task(&id).unwrap().expect("task should exist");
            assert_eq!(task["status"], "pending");
            assert_eq!(task["title"], "Test task");

            update_task_status(&id, "completed", Some(&json!({"output": "ok"}))).unwrap();

            let task = get_task(&id).unwrap().expect("task should exist");
            assert_eq!(task["status"], "completed");
            assert!(task["finished_at"].as_str().is_some());
        });
    }

    #[test]
    fn list_tasks_for_conversation() {
        with_temp_db(|| {
            let cid = format!("conv-{}", uuid::Uuid::new_v4());
            crate::conversation_store::ensure_conversation_stub(
                &cid, "openai", "gpt-4o", None, None,
            )
            .unwrap();

            let id1 = format!("task-{}", uuid::Uuid::new_v4());
            let id2 = format!("task-{}", uuid::Uuid::new_v4());
            insert_task(&id1, Some(&cid), None, None, "Task 1", None).unwrap();
            insert_task(&id2, Some(&cid), None, None, "Task 2", None).unwrap();

            let tasks = list_tasks(&cid).unwrap();
            assert_eq!(tasks.len(), 2);
        });
    }
}
