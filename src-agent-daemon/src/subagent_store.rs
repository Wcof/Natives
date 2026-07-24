//! Subagent route policy + session registry (migration 010).
//!
//! Stores only provider/key/model **IDs** — never plaintext credentials.

use crate::storage::DataStore;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use uuid::Uuid;

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
        let _ = std::fs::create_dir_all(parent);
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

/// One credential binding (IDs only).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouteBinding {
    pub provider_id: String,
    pub key_id: String,
    pub model_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RoutePolicy {
    pub parent_conversation_id: String,
    pub mode: String,
    pub bindings: Vec<RouteBinding>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentSession {
    pub id: String,
    pub parent_conversation_id: String,
    pub child_conversation_id: String,
    pub parent_run_id: Option<String>,
    pub task_call_id: Option<String>,
    pub name: String,
    pub task: String,
    pub status: String,
    pub provider_id: String,
    pub key_id: String,
    pub model_id: String,
    pub attempted_bindings: Vec<RouteBinding>,
    pub last_activity_at: String,
    pub closed_at: Option<String>,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

fn parse_bindings(raw: &str) -> Vec<RouteBinding> {
    serde_json::from_str(raw).unwrap_or_default()
}

/// Upsert route policy for a parent conversation.
pub fn upsert_route_policy(
    parent_conversation_id: &str,
    mode: &str,
    bindings: &[RouteBinding],
) -> Result<RoutePolicy, String> {
    let parent = parent_conversation_id.trim();
    if parent.is_empty() {
        return Err("parent_conversation_id required".into());
    }
    let mode = match mode.trim() {
        "random" => "random",
        "custom" => "custom",
        _ => "default",
    };
    if bindings.is_empty() {
        return Err("bindings must be non-empty".into());
    }
    for b in bindings {
        if b.provider_id.trim().is_empty()
            || b.key_id.trim().is_empty()
            || b.model_id.trim().is_empty()
        {
            return Err("each binding needs provider_id + key_id + model_id".into());
        }
        if b.key_id.eq_ignore_ascii_case("auto") {
            return Err("key_id must not be 'auto'".into());
        }
    }
    let bindings_json =
        serde_json::to_string(bindings).map_err(|e| format!("bindings serialize: {e}"))?;
    let now = chrono::Utc::now().to_rfc3339();
    let s = store()?;
    let conn = s.conn().map_err(|e| format!("conn lock: {e}"))?;
    conn.execute(
        "INSERT INTO subagent_route_policy (parent_conversation_id, mode, bindings_json, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?4)
         ON CONFLICT(parent_conversation_id) DO UPDATE SET
           mode = excluded.mode,
           bindings_json = excluded.bindings_json,
           updated_at = excluded.updated_at",
        params![parent, mode, bindings_json, now],
    )
    .map_err(|e| format!("upsert_route_policy failed: {e}"))?;
    get_route_policy(parent)?.ok_or_else(|| "route policy missing after upsert".into())
}

pub fn get_route_policy(parent_conversation_id: &str) -> Result<Option<RoutePolicy>, String> {
    let parent = parent_conversation_id.trim();
    if parent.is_empty() {
        return Ok(None);
    }
    let s = store()?;
    let conn = s.conn().map_err(|e| format!("conn lock: {e}"))?;
    conn.query_row(
        "SELECT parent_conversation_id, mode, bindings_json, created_at, updated_at
         FROM subagent_route_policy WHERE parent_conversation_id = ?1",
        params![parent],
        |row| {
            let bindings_raw: String = row.get(2)?;
            Ok(RoutePolicy {
                parent_conversation_id: row.get(0)?,
                mode: row.get(1)?,
                bindings: parse_bindings(&bindings_raw),
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
            })
        },
    )
    .optional()
    .map_err(|e| format!("get_route_policy failed: {e}"))
}

/// Pick next binding from policy (default=first unused then wrap; random=rand; custom=first).
pub fn pick_binding(
    policy: &RoutePolicy,
    attempted: &[RouteBinding],
) -> Result<RouteBinding, String> {
    if policy.bindings.is_empty() {
        return Err("route policy has empty bindings".into());
    }
    let remaining: Vec<_> = policy
        .bindings
        .iter()
        .filter(|b| {
            !attempted.iter().any(|a| {
                a.provider_id == b.provider_id && a.key_id == b.key_id && a.model_id == b.model_id
            })
        })
        .cloned()
        .collect();
    let pool = if remaining.is_empty() {
        return Err("route binding pool exhausted".into());
    } else {
        remaining
    };
    match policy.mode.as_str() {
        "random" => {
            let idx = (uuid::Uuid::new_v4().as_u128() as usize) % pool.len();
            Ok(pool[idx].clone())
        }
        _ => Ok(pool[0].clone()),
    }
}

/// Create a hidden child conversation under parent + subagent_session row.
/// Returns (session_id, child_conversation_id).
pub fn create_hidden_child_session(
    parent_conversation_id: &str,
    parent_run_id: Option<&str>,
    task_call_id: Option<&str>,
    name: &str,
    task: &str,
    binding: &RouteBinding,
    permission_profile: Option<&str>,
    project_id: Option<&str>,
) -> Result<(String, String), String> {
    let parent = parent_conversation_id.trim();
    if parent.is_empty() {
        return Err("parent_conversation_id required".into());
    }
    if binding.key_id.eq_ignore_ascii_case("auto") {
        return Err("key_id must not be 'auto'".into());
    }
    let child_id = Uuid::new_v4().to_string();
    let session_id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let permission = permission_profile
        .filter(|p| matches!(*p, "readonly" | "ask" | "full_access"))
        .unwrap_or("ask");
    let title = if name.trim().is_empty() {
        format!("Subagent: {}", task.chars().take(48).collect::<String>())
    } else {
        name.trim().to_string()
    };
    let attempted_json = serde_json::to_string(&vec![binding.clone()])
        .map_err(|e| format!("attempted serialize: {e}"))?;

    let s = store()?;
    let conn = s.conn().map_err(|e| format!("conn lock: {e}"))?;
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;

    // Ensure parent exists (FK).
    let parent_exists: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM conversation WHERE id = ?1)",
            params![parent],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if !parent_exists {
        return Err(format!("parent conversation not found: {parent}"));
    }

    tx.execute(
        "INSERT INTO conversation (
            id, mode, project_id, title, provider_id, model_id,
            permission_profile_id, created_at, updated_at, parent_conversation_id
         ) VALUES (?1, 'agent', ?2, ?3, ?4, ?5, ?6, ?7, ?7, ?8)",
        params![
            child_id,
            project_id,
            title,
            binding.provider_id,
            binding.model_id,
            permission,
            now,
            parent,
        ],
    )
    .map_err(|e| format!("insert child conversation failed: {e}"))?;

    tx.execute(
        "INSERT INTO subagent_session (
            id, parent_conversation_id, child_conversation_id, parent_run_id, task_call_id,
            name, task, status, provider_id, key_id, model_id, attempted_bindings_json,
            last_activity_at, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'open', ?8, ?9, ?10, ?11, ?12, ?12, ?12)",
        params![
            session_id,
            parent,
            child_id,
            parent_run_id,
            task_call_id,
            name,
            task,
            binding.provider_id,
            binding.key_id,
            binding.model_id,
            attempted_json,
            now,
        ],
    )
    .map_err(|e| format!("insert subagent_session failed: {e}"))?;

    tx.commit().map_err(|e| e.to_string())?;

    // Initial task as user message on the child conversation.
    if !task.trim().is_empty() {
        let _ = crate::conversation_store::append_message_public(json!({
            "conversation_id": child_id,
            "role": "user",
            "content": task,
        }));
    }

    Ok((session_id, child_id))
}

pub fn insert_subagent_session(session: &SubagentSession) -> Result<(), String> {
    let attempted_json = serde_json::to_string(&session.attempted_bindings)
        .map_err(|e| format!("attempted serialize: {e}"))?;
    let s = store()?;
    let conn = s.conn().map_err(|e| format!("conn lock: {e}"))?;
    conn.execute(
        "INSERT INTO subagent_session (
            id, parent_conversation_id, child_conversation_id, parent_run_id, task_call_id,
            name, task, status, provider_id, key_id, model_id, attempted_bindings_json,
            last_activity_at, closed_at, error, created_at, updated_at
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
        params![
            session.id,
            session.parent_conversation_id,
            session.child_conversation_id,
            session.parent_run_id,
            session.task_call_id,
            session.name,
            session.task,
            session.status,
            session.provider_id,
            session.key_id,
            session.model_id,
            attempted_json,
            session.last_activity_at,
            session.closed_at,
            session.error,
            session.created_at,
            session.updated_at,
        ],
    )
    .map_err(|e| format!("insert_subagent_session failed: {e}"))?;
    Ok(())
}

pub fn update_subagent_session_status(
    id: &str,
    status: &str,
    error: Option<&str>,
) -> Result<(), String> {
    let now = chrono::Utc::now().to_rfc3339();
    let closed = matches!(
        status,
        "closed" | "failed" | "cancelled" | "interrupted" | "completed"
    );
    let s = store()?;
    let conn = s.conn().map_err(|e| format!("conn lock: {e}"))?;
    conn.execute(
        "UPDATE subagent_session
         SET status = ?1,
             error = COALESCE(?2, error),
             updated_at = ?3,
             last_activity_at = ?3,
             closed_at = CASE WHEN ?4 THEN ?3 ELSE closed_at END
         WHERE id = ?5",
        params![status, error, now, closed as i32, id],
    )
    .map_err(|e| format!("update_subagent_session_status failed: {e}"))?;
    Ok(())
}

pub fn update_session_binding(
    id: &str,
    binding: &RouteBinding,
    attempted: &[RouteBinding],
) -> Result<(), String> {
    let now = chrono::Utc::now().to_rfc3339();
    let attempted_json =
        serde_json::to_string(attempted).map_err(|e| format!("attempted serialize: {e}"))?;
    let s = store()?;
    let conn = s.conn().map_err(|e| format!("conn lock: {e}"))?;
    conn.execute(
        "UPDATE subagent_session
         SET provider_id = ?1, key_id = ?2, model_id = ?3,
             attempted_bindings_json = ?4, updated_at = ?5, last_activity_at = ?5
         WHERE id = ?6",
        params![
            binding.provider_id,
            binding.key_id,
            binding.model_id,
            attempted_json,
            now,
            id
        ],
    )
    .map_err(|e| format!("update_session_binding failed: {e}"))?;
    Ok(())
}

pub fn touch_subagent_session(id: &str) -> Result<(), String> {
    let now = chrono::Utc::now().to_rfc3339();
    let s = store()?;
    let conn = s.conn().map_err(|e| format!("conn lock: {e}"))?;
    let n = conn
        .execute(
            "UPDATE subagent_session SET last_activity_at = ?1, updated_at = ?1 WHERE id = ?2",
            params![now, id],
        )
        .map_err(|e| format!("touch_subagent_session failed: {e}"))?;
    if n == 0 {
        return Err(format!("subagent session not found: {id}"));
    }
    Ok(())
}

pub fn touch_by_child_conversation(child_conversation_id: &str) -> Result<(), String> {
    let now = chrono::Utc::now().to_rfc3339();
    let s = store()?;
    let conn = s.conn().map_err(|e| format!("conn lock: {e}"))?;
    conn.execute(
        "UPDATE subagent_session SET last_activity_at = ?1, updated_at = ?1
         WHERE child_conversation_id = ?2 AND status IN ('pending_assignment','open','queued','running','waiting','idle')",
        params![now, child_conversation_id],
    )
    .map_err(|e| format!("touch_by_child_conversation failed: {e}"))?;
    Ok(())
}

pub fn close_subagent_session(id: &str, status: &str, error: Option<&str>) -> Result<(), String> {
    let status = match status {
        "failed" => "failed",
        "cancelled" => "cancelled",
        "interrupted" => "interrupted",
        "completed" => "completed",
        _ => "closed",
    };
    update_subagent_session_status(id, status, error)
}

fn row_to_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<SubagentSession> {
    let attempted_raw: String = row.get(11)?;
    Ok(SubagentSession {
        id: row.get(0)?,
        parent_conversation_id: row.get(1)?,
        child_conversation_id: row.get(2)?,
        parent_run_id: row.get(3)?,
        task_call_id: row.get(4)?,
        name: row.get(5)?,
        task: row.get(6)?,
        status: row.get(7)?,
        provider_id: row.get(8)?,
        key_id: row.get(9)?,
        model_id: row.get(10)?,
        attempted_bindings: parse_bindings(&attempted_raw),
        last_activity_at: row.get(12)?,
        closed_at: row.get(13)?,
        error: row.get(14)?,
        created_at: row.get(15)?,
        updated_at: row.get(16)?,
    })
}

const SESSION_SELECT: &str = "SELECT id, parent_conversation_id, child_conversation_id, parent_run_id,
    task_call_id, name, task, status, provider_id, key_id, model_id, attempted_bindings_json,
    last_activity_at, closed_at, error, created_at, updated_at
 FROM subagent_session";

pub fn get_subagent_session(id: &str) -> Result<Option<SubagentSession>, String> {
    let s = store()?;
    let conn = s.conn().map_err(|e| format!("conn lock: {e}"))?;
    conn.query_row(
        &format!("{SESSION_SELECT} WHERE id = ?1"),
        params![id],
        row_to_session,
    )
    .optional()
    .map_err(|e| format!("get_subagent_session failed: {e}"))
}

pub fn list_subagent_sessions(
    parent_conversation_id: Option<&str>,
    include_closed: bool,
) -> Result<Vec<SubagentSession>, String> {
    let s = store()?;
    let conn = s.conn().map_err(|e| format!("conn lock: {e}"))?;
    let mut out = Vec::new();
    match (parent_conversation_id, include_closed) {
        (Some(pid), true) => {
            let mut stmt = conn
                .prepare(&format!(
                    "{SESSION_SELECT} WHERE parent_conversation_id = ?1 ORDER BY created_at DESC"
                ))
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map(params![pid], row_to_session)
                .map_err(|e| e.to_string())?;
            for r in rows {
                out.push(r.map_err(|e| e.to_string())?);
            }
        }
        (Some(pid), false) => {
            let mut stmt = conn
                .prepare(&format!(
                    "{SESSION_SELECT}
                     WHERE parent_conversation_id = ?1
                       AND status IN ('pending_assignment','open','queued','running','waiting','idle')
                     ORDER BY created_at DESC"
                ))
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map(params![pid], row_to_session)
                .map_err(|e| e.to_string())?;
            for r in rows {
                out.push(r.map_err(|e| e.to_string())?);
            }
        }
        (None, true) => {
            let mut stmt = conn
                .prepare(&format!("{SESSION_SELECT} ORDER BY created_at DESC"))
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([], row_to_session)
                .map_err(|e| e.to_string())?;
            for r in rows {
                out.push(r.map_err(|e| e.to_string())?);
            }
        }
        (None, false) => {
            let mut stmt = conn
                .prepare(&format!(
                    "{SESSION_SELECT}
                     WHERE status IN ('pending_assignment','open','queued','running','waiting','idle')
                     ORDER BY created_at DESC"
                ))
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([], row_to_session)
                .map_err(|e| e.to_string())?;
            for r in rows {
                out.push(r.map_err(|e| e.to_string())?);
            }
        }
    }
    Ok(out)
}

pub fn list_active_for_reaper() -> Result<Vec<SubagentSession>, String> {
    list_subagent_sessions(None, false)
}

/// Parent conversation heartbeat (independent of child `last_activity_at`).
///
/// Stored on `subagent_route_policy.last_parent_heartbeat_at`. If no policy row
/// exists yet, creates a placeholder row with empty bindings so the heartbeat
/// column can still be updated (bindings must be filled later by assignment).
pub fn touch_parent_heartbeat(parent_conversation_id: &str) -> Result<(), String> {
    let parent = parent_conversation_id.trim();
    if parent.is_empty() {
        return Err("conversation_id required for parent heartbeat".into());
    }
    let now = chrono::Utc::now().to_rfc3339();
    let s = store()?;
    let conn = s.conn().map_err(|e| format!("conn lock: {e}"))?;

    // Ensure parent conversation exists (FK on route policy).
    let parent_exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM conversation WHERE id = ?1)",
            params![parent],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if !parent_exists {
        return Err(format!("parent conversation not found: {parent}"));
    }

    let changed = conn
        .execute(
            "UPDATE subagent_route_policy
             SET last_parent_heartbeat_at = ?1, updated_at = ?1
             WHERE parent_conversation_id = ?2",
            params![now, parent],
        )
        .map_err(|e| format!("touch_parent_heartbeat update failed: {e}"))?;
    if changed == 0 {
        // No policy yet: insert a stub row so reaper/UI can still see heartbeat.
        // Empty bindings are rejected by upsert_route_policy; use a sentinel that
        // is never used for pick_binding (empty list is filtered by CHECK? none).
        // Store "[]" — pick_binding will fail until real assignment.
        conn.execute(
            "INSERT INTO subagent_route_policy (
                parent_conversation_id, mode, bindings_json,
                created_at, updated_at, last_parent_heartbeat_at
             ) VALUES (?1, 'default', '[]', ?2, ?2, ?2)
             ON CONFLICT(parent_conversation_id) DO UPDATE SET
               last_parent_heartbeat_at = excluded.last_parent_heartbeat_at,
               updated_at = excluded.updated_at",
            params![parent, now],
        )
        .map_err(|e| format!("touch_parent_heartbeat insert failed: {e}"))?;
    }
    Ok(())
}

/// True when parent heartbeat is fresher than `within_secs`.
pub fn parent_heartbeat_recent(parent_conversation_id: &str, within_secs: i64) -> bool {
    let parent = parent_conversation_id.trim();
    if parent.is_empty() {
        return false;
    }
    let Ok(s) = store() else {
        return false;
    };
    let Ok(conn) = s.conn() else {
        return false;
    };
    let ts: Option<String> = conn
        .query_row(
            "SELECT last_parent_heartbeat_at FROM subagent_route_policy
             WHERE parent_conversation_id = ?1",
            params![parent],
            |row| row.get(0),
        )
        .optional()
        .ok()
        .flatten();
    let Some(ts) = ts.filter(|s| !s.trim().is_empty()) else {
        return false;
    };
    let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(&ts) else {
        return false;
    };
    let age = chrono::Utc::now().signed_duration_since(parsed.with_timezone(&chrono::Utc));
    age.num_seconds() <= within_secs
}

/// Look up session by child conversation id (most recent open-ish row).
pub fn get_session_by_child_conversation(
    child_conversation_id: &str,
) -> Result<Option<SubagentSession>, String> {
    let child = child_conversation_id.trim();
    if child.is_empty() {
        return Ok(None);
    }
    let s = store()?;
    let conn = s.conn().map_err(|e| format!("conn lock: {e}"))?;
    conn.query_row(
        &format!("{SESSION_SELECT} WHERE child_conversation_id = ?1 ORDER BY created_at DESC LIMIT 1"),
        params![child],
        row_to_session,
    )
    .optional()
    .map_err(|e| format!("get_session_by_child_conversation failed: {e}"))
}

/// Classify provider/engine errors for failover eligibility.
pub fn is_failover_eligible_error(err: &str) -> bool {
    let lower = err.to_ascii_lowercase();
    if lower.contains("permission")
        || lower.contains("denied")
        || lower.contains("tool_not_allowlisted")
        || lower.contains("max steps")
        || lower.contains("max_steps")
        || lower.contains("doom loop")
    {
        return false;
    }
    lower.contains("401")
        || lower.contains("403")
        || lower.contains("429")
        || lower.contains("unauthorized")
        || lower.contains("forbidden")
        || lower.contains("rate limit")
        || lower.contains("too many requests")
        || lower.contains("network")
        || lower.contains("timeout")
        || lower.contains("timed out")
        || lower.contains("connection")
        || lower.contains("5xx")
        || lower.contains("502")
        || lower.contains("503")
        || lower.contains("504")
        || lower.contains("internal server")
        || lower.contains("provider error")
}

/// RPC: subagent.list / subagent.switchRoute / subagent.touch
pub async fn request(method: &str, params: Value) -> Result<Value, String> {
    match method {
        "subagent.list" => {
            let parent = params
                .get("conversation_id")
                .or_else(|| params.get("parent_conversation_id"))
                .and_then(Value::as_str);
            let include_closed = params
                .get("include_closed")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let sessions = list_subagent_sessions(parent, include_closed)?;
            let policy = match parent {
                Some(p) => get_route_policy(p)?,
                None => None,
            };
            Ok(json!({
                "sessions": sessions,
                "route_policy": policy,
            }))
        }
        "subagent.touch" => {
            let subagent_id = params
                .get("subagent_id")
                .or_else(|| params.get("id"))
                .or_else(|| params.get("session_id"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let child_conversation_id = params
                .get("child_conversation_id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let conversation_id = params
                .get("conversation_id")
                .or_else(|| params.get("parent_conversation_id"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();

            // Parent-only heartbeat: conversation_id without subagent/child id.
            if subagent_id.is_empty() && child_conversation_id.is_empty() {
                if conversation_id.is_empty() {
                    return Err(
                        "conversation_id required for parent heartbeat, or pass subagent_id/child_conversation_id"
                            .into(),
                    );
                }
                touch_parent_heartbeat(&conversation_id)?;
                return Ok(json!({
                    "ok": true,
                    "parent_heartbeat": true,
                    "conversation_id": conversation_id,
                }));
            }

            if !subagent_id.is_empty() {
                touch_subagent_session(&subagent_id)?;
                return Ok(json!({ "ok": true, "id": subagent_id }));
            }
            touch_by_child_conversation(&child_conversation_id)?;
            Ok(json!({ "ok": true, "child_conversation_id": child_conversation_id }))
        }
        "subagent.switchRoute" => {
            let parent = params
                .get("conversation_id")
                .or_else(|| params.get("parent_conversation_id"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            if parent.is_empty() {
                return Err("conversation_id required for subagent.switchRoute".into());
            }
            let mode = params
                .get("mode")
                .and_then(Value::as_str)
                .unwrap_or("default");
            let bindings: Vec<RouteBinding> = params
                .get("bindings")
                .cloned()
                .and_then(|v| serde_json::from_value(v).ok())
                .or_else(|| {
                    params
                        .get("pool")
                        .cloned()
                        .and_then(|v| serde_json::from_value(v).ok())
                })
                .unwrap_or_default();
            if bindings.is_empty() {
                return Err("bindings required for subagent.switchRoute".into());
            }
            // Validate bindings before persisting.
            for b in &bindings {
                crate::production::validate_route_binding(b)?;
            }
            let policy = upsert_route_policy(&parent, mode, &bindings)?;

            let mut restarted_run_id: Option<String> = None;
            if let Some(sid) = params
                .get("session_id")
                .or_else(|| params.get("subagent_id"))
                .and_then(Value::as_str)
            {
                let next = if let Some(explicit) = params.get("binding").cloned().and_then(|v| {
                    serde_json::from_value::<RouteBinding>(v).ok()
                }) {
                    crate::production::validate_route_binding(&explicit)?;
                    explicit
                } else {
                    pick_binding(&policy, &[])?
                };
                restarted_run_id =
                    crate::production::restart_subagent_with_binding(sid, &next).await?;
            }

            Ok(json!({
                "ok": true,
                "route_policy": policy,
                "restarted_run_id": restarted_run_id,
            }))
        }
        _ => Err(format!("unsupported subagent method: {method}")),
    }
}

/// Wake assignment waiters registered by ProductionRuntime (kind=subagent_assignment).
pub fn wake_assignment_waiter(interaction_id: &str, response: Value) -> bool {
    crate::production::wake_assignment_waiter(interaction_id, response)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_temp_db<F: FnOnce()>(f: F) {
        let _guard = crate::storage::DataStore::env_test_lock();
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join(format!("subagent-{}.db", Uuid::new_v4()));
        let art = dir.path().join("artifacts");
        crate::storage::set_test_db_override(Some(db.clone()), Some(art.clone()));
        let _warm = crate::storage::DataStore::new(&db, &art).expect("subagent temp db migrate");
        // Ensure parent conversation exists for FK tests.
        crate::conversation_store::ensure_conversation_stub(
            "parent-1",
            "openai",
            "gpt-4o",
            None,
            None,
        )
        .unwrap();
        f();
        crate::storage::set_test_db_override(None, None);
    }

    #[test]
    fn migration_010_tables_exist() {
        with_temp_db(|| {
            let s = store().unwrap();
            assert!(s.has_table("subagent_route_policy"));
            assert!(s.has_table("subagent_session"));
            let conn = s.conn().unwrap();
            let has_col: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM pragma_table_info('conversation')
                     WHERE name = 'parent_conversation_id'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(has_col, 1);
        });
    }

    #[test]
    fn route_policy_upsert_get_and_pick() {
        with_temp_db(|| {
            let b1 = RouteBinding {
                provider_id: "openai".into(),
                key_id: "k1".into(),
                model_id: "gpt-4o".into(),
            };
            let b2 = RouteBinding {
                provider_id: "anthropic".into(),
                key_id: "k2".into(),
                model_id: "claude".into(),
            };
            upsert_route_policy("parent-1", "default", &[b1.clone(), b2.clone()]).unwrap();
            let p = get_route_policy("parent-1").unwrap().unwrap();
            assert_eq!(p.bindings.len(), 2);
            let first = pick_binding(&p, &[]).unwrap();
            assert_eq!(first, b1);
            let second = pick_binding(&p, &[b1.clone()]).unwrap();
            assert_eq!(second, b2);
            assert!(pick_binding(&p, &[b1, b2]).is_err());
        });
    }

    #[test]
    fn create_hidden_child_and_list() {
        with_temp_db(|| {
            let binding = RouteBinding {
                provider_id: "openai".into(),
                key_id: "k1".into(),
                model_id: "gpt-4o".into(),
            };
            let (sid, child) = create_hidden_child_session(
                "parent-1",
                Some("run-1"),
                None,
                "worker",
                "do the thing",
                &binding,
                Some("ask"),
                None,
            )
            .unwrap();
            assert!(!sid.is_empty());
            assert!(!child.is_empty());
            let listed = list_subagent_sessions(Some("parent-1"), true).unwrap();
            assert_eq!(listed.len(), 1);
            assert_eq!(listed[0].child_conversation_id, child);
            assert_eq!(listed[0].task, "do the thing");
            touch_subagent_session(&sid).unwrap();
            close_subagent_session(&sid, "closed", None).unwrap();
            let open = list_subagent_sessions(Some("parent-1"), false).unwrap();
            assert!(open.is_empty());
        });
    }

    #[test]
    fn failover_error_classification() {
        assert!(is_failover_eligible_error("HTTP 401 unauthorized"));
        assert!(is_failover_eligible_error("429 rate limit"));
        assert!(is_failover_eligible_error("network timeout"));
        assert!(is_failover_eligible_error("502 bad gateway"));
        assert!(!is_failover_eligible_error("permission denied"));
        assert!(!is_failover_eligible_error("tool_not_allowlisted"));
        assert!(!is_failover_eligible_error("max steps exceeded"));
    }

    #[test]
    fn rejects_auto_key_id() {
        with_temp_db(|| {
            let b = RouteBinding {
                provider_id: "openai".into(),
                key_id: "auto".into(),
                model_id: "gpt-4o".into(),
            };
            assert!(upsert_route_policy("parent-1", "default", &[b]).is_err());
        });
    }

    #[test]
    fn three_tasks_share_one_route_policy_pool() {
        with_temp_db(|| {
            // Simulates batch assignment: one policy covers multiple task bindings.
            let bindings = vec![
                RouteBinding {
                    provider_id: "openai".into(),
                    key_id: "k1".into(),
                    model_id: "gpt-4o".into(),
                },
                RouteBinding {
                    provider_id: "anthropic".into(),
                    key_id: "k2".into(),
                    model_id: "claude".into(),
                },
                RouteBinding {
                    provider_id: "openai".into(),
                    key_id: "k3".into(),
                    model_id: "gpt-4o-mini".into(),
                },
            ];
            upsert_route_policy("parent-1", "default", &bindings).unwrap();
            let policy = get_route_policy("parent-1").unwrap().unwrap();
            let mut attempted = Vec::new();
            let mut assigned = Vec::new();
            for _ in 0..3 {
                let b = pick_binding(&policy, &attempted).unwrap();
                attempted.push(b.clone());
                assigned.push(b);
            }
            assert_eq!(assigned.len(), 3);
            assert_eq!(assigned[0].key_id, "k1");
            assert_eq!(assigned[1].key_id, "k2");
            assert_eq!(assigned[2].key_id, "k3");
            // Fourth would exhaust under exclusive attempt tracking.
            assert!(pick_binding(&policy, &attempted).is_err());
        });
    }

    #[test]
    fn migration_011_status_completed_and_parent_heartbeat() {
        with_temp_db(|| {
            let s = store().unwrap();
            let _conn = s.conn().unwrap();
            // completed is accepted by CHECK
            let binding = RouteBinding {
                provider_id: "openai".into(),
                key_id: "k1".into(),
                model_id: "gpt-4o".into(),
            };
            let (sid, _) = create_hidden_child_session(
                "parent-1",
                Some("run-1"),
                Some("call-1"),
                "worker",
                "do",
                &binding,
                Some("ask"),
                None,
            )
            .unwrap();
            update_subagent_session_status(&sid, "completed", None).unwrap();
            let sess = get_subagent_session(&sid).unwrap().unwrap();
            assert_eq!(sess.status, "completed");

            touch_parent_heartbeat("parent-1").unwrap();
            assert!(parent_heartbeat_recent("parent-1", 90));
            let before = get_subagent_session(&sid).unwrap().unwrap().last_activity_at;
            // Parent touch must not bump child activity.
            touch_parent_heartbeat("parent-1").unwrap();
            let after = get_subagent_session(&sid).unwrap().unwrap().last_activity_at;
            assert_eq!(before, after);
        });
    }

    #[tokio::test]
    async fn touch_parent_only_rpc_does_not_require_subagent_id() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("t.db");
        let art = dir.path().join("a");
        std::fs::create_dir_all(&art).unwrap();
        let _ = crate::storage::DataStore::new(&db, &art).unwrap();
        crate::storage::set_test_db_override(Some(db.clone()), Some(art.clone()));
        crate::conversation_store::ensure_conversation_stub(
            "parent-1",
            "openai",
            "gpt-4o",
            Some("ask"),
            None,
        )
        .unwrap();
        let out = request(
            "subagent.touch",
            json!({ "conversation_id": "parent-1" }),
        )
        .await
        .unwrap();
        assert_eq!(out["ok"], true);
        assert_eq!(out["parent_heartbeat"], true);
        crate::storage::set_test_db_override(None, None);
    }
}
