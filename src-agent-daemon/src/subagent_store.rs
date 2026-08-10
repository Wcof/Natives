//! Subagent route policy + session registry (migration 010).
//!
//! Stores only provider/key/model **IDs** — never plaintext credentials.
//!
//! Aggregate module: route policy lives in [`subagent_route`], the durable
//! pending Child Directive in [`subagent_directive`], and slot/budget
//! reservations in [`subagent_reservation`]. All their public items are
//! re-exported here so `crate::subagent_store::*` keeps working unchanged.
//!
//! W2/P0-02: the split files live as siblings of this file (Rust's default
//! `mod x;` resolution looks in `subagent_store/x.rs`), so each declaration
//! carries an explicit `#[path]` — the same documented pattern used by
//! `run/manager_tests.rs` for its test splits.
#[path = "subagent_directive.rs"]
mod subagent_directive;
#[path = "subagent_reservation.rs"]
mod subagent_reservation;
#[path = "subagent_route.rs"]
mod subagent_route;

pub use subagent_directive::*;
pub use subagent_reservation::*;
pub use subagent_route::*;

use crate::storage::DataStore;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

fn store() -> Result<DataStore, String> {
    // W2: single Daemon DataStore open path (assistant.db authority + test hook).
    crate::storage::open_daemon_store()
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
    /// Child scope persisted at spawn (migration 029) so a route restart can
    /// restore it exactly instead of guessing. `None` marks sessions created
    /// before the columns existed; a route restart on those must fail closed.
    #[serde(default)]
    pub project_path: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub project_identity_version: Option<i64>,
    #[serde(default)]
    pub permission_profile: Option<String>,
    #[serde(default)]
    pub agent_profile_id: Option<String>,
    #[serde(default)]
    pub max_steps: Option<i64>,
    #[serde(default)]
    pub tool_allowlist: Vec<String>,
    // T05 budget / reservation ledger (migration 036). `None`/zero marks
    // sessions created before the columns existed; those fail closed.
    #[serde(default)]
    pub tokens_used: u64,
    #[serde(default)]
    pub cost_usd: f64,
    #[serde(default)]
    pub max_tokens: Option<u64>,
    #[serde(default)]
    pub max_cost_usd: Option<f64>,
    #[serde(default)]
    pub failure_policy: String,
    #[serde(default)]
    pub max_retries: u32,
    #[serde(default)]
    pub retry_count: u32,
    #[serde(default)]
    pub reservation_released: bool,
    #[serde(default)]
    pub reserved_at: Option<String>,
    #[serde(default)]
    pub released_at: Option<String>,
    #[serde(default)]
    pub tree_root_run_id: Option<String>,
    #[serde(default)]
    pub depth: u32,
    #[serde(default)]
    pub scope_snapshot_json: String,
}

fn derive_subagent_name(name: &str, task: &str) -> String {
    let explicit = name.trim();
    if !explicit.is_empty() {
        return explicit.to_string();
    }
    task.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(48)
        .collect::<String>()
        .trim()
        .to_string()
}

/// Create a hidden child conversation under parent + subagent_session row.
/// Returns (session_id, child_conversation_id).
#[allow(clippy::too_many_arguments)] // pre-existing: parameter list is fixed
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
    let session_name = derive_subagent_name(name, task);
    let title = if session_name.is_empty() {
        "Subagent".to_string()
    } else {
        session_name.clone()
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
            last_activity_at, created_at, updated_at,
            project_id, permission_profile
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'open', ?8, ?9, ?10, ?11, ?12, ?12, ?12,
                   ?13, ?14)",
        params![
            session_id,
            parent,
            child_id,
            parent_run_id,
            task_call_id,
            session_name,
            task,
            binding.provider_id,
            binding.key_id,
            binding.model_id,
            attempted_json,
            now,
            project_id,
            permission_profile,
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
            last_activity_at, closed_at, error, created_at, updated_at,
            project_path, project_id, project_identity_version, permission_profile,
            agent_profile_id, max_steps, tool_allowlist_json,
            tokens_used, cost_usd, max_tokens, max_cost_usd, failure_policy,
            max_retries, retry_count, reservation_released, reserved_at, released_at,
            tree_root_run_id, depth, scope_snapshot_json
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,
                   ?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28,?29,?30,?31,?32,?33,?34,
                   ?35,?36,?37)",
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
            session.project_path,
            session.project_id,
            session.project_identity_version,
            session.permission_profile,
            session.agent_profile_id,
            session.max_steps,
            serde_json::to_string(&session.tool_allowlist).unwrap_or_else(|_| "[]".into()),
            session.tokens_used as i64,
            session.cost_usd,
            session.max_tokens.map(|v| v as i64),
            session.max_cost_usd,
            session.failure_policy,
            session.max_retries as i64,
            session.retry_count as i64,
            session.reservation_released as i64,
            session.reserved_at,
            session.released_at,
            session.tree_root_run_id,
            session.depth as i64,
            session.scope_snapshot_json,
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
        attempted_bindings: subagent_route::parse_bindings(&attempted_raw),
        last_activity_at: row.get(12)?,
        closed_at: row.get(13)?,
        error: row.get(14)?,
        created_at: row.get(15)?,
        updated_at: row.get(16)?,
        project_path: row.get(17)?,
        project_id: row.get(18)?,
        project_identity_version: row.get(19)?,
        permission_profile: row.get(20)?,
        agent_profile_id: row.get(21)?,
        max_steps: row.get(22)?,
        tool_allowlist: row
            .get::<_, String>(23)
            .ok()
            .and_then(|raw| serde_json::from_str::<Vec<String>>(&raw).ok())
            .unwrap_or_default(),
        tokens_used: row.get::<_, i64>(24).unwrap_or(0).max(0) as u64,
        cost_usd: row.get::<_, f64>(25).unwrap_or(0.0).max(0.0),
        max_tokens: row.get::<_, Option<i64>>(26)?.map(|v| v.max(0) as u64),
        max_cost_usd: row.get::<_, Option<f64>>(27)?,
        failure_policy: row
            .get::<_, String>(28)
            .unwrap_or_else(|_| "isolate".into()),
        max_retries: row.get::<_, i64>(29).unwrap_or(0).max(0) as u32,
        retry_count: row.get::<_, i64>(30).unwrap_or(0).max(0) as u32,
        reservation_released: row.get::<_, i64>(31).unwrap_or(0) != 0,
        reserved_at: row.get(32)?,
        released_at: row.get(33)?,
        tree_root_run_id: row.get(34)?,
        depth: row.get::<_, i64>(35).unwrap_or(0).max(0) as u32,
        scope_snapshot_json: row.get::<_, String>(36).unwrap_or_else(|_| "{}".into()),
    })
}

const SESSION_SELECT: &str =
    "SELECT id, parent_conversation_id, child_conversation_id, parent_run_id,
    task_call_id, name, task, status, provider_id, key_id, model_id, attempted_bindings_json,
    last_activity_at, closed_at, error, created_at, updated_at,
    project_path, project_id, project_identity_version, permission_profile, agent_profile_id,
    max_steps, tool_allowlist_json,
    tokens_used, cost_usd, max_tokens, max_cost_usd, failure_policy,
    max_retries, retry_count, reservation_released, reserved_at, released_at,
    tree_root_run_id, depth, scope_snapshot_json
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
        &format!(
            "{SESSION_SELECT} WHERE child_conversation_id = ?1 ORDER BY created_at DESC LIMIT 1"
        ),
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
            // NE-P0-08 / 19.3-⑤: field-level visibility — the parent-authored
            // pending directive text never leaves the protected store. The RPC
            // surface receives digest-only redaction (same persona identifiable
            // by digest, text unrecoverable).
            let sessions: Vec<Value> = sessions.iter().map(redact_session_for_export).collect();
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
                let next = if let Some(explicit) = params
                    .get("binding")
                    .cloned()
                    .and_then(|v| serde_json::from_value::<RouteBinding>(v).ok())
                {
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
            "parent-1", "openai", "gpt-4o", None, None,
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
    fn create_hidden_child_session_persists_permission_and_project_id() {
        with_temp_db(|| {
            let binding = RouteBinding {
                provider_id: "openai".into(),
                key_id: "key-1".into(),
                model_id: "gpt-4o".into(),
            };
            let (sid, _child) = create_hidden_child_session(
                "parent-1",
                None,
                None,
                "sub",
                "task",
                &binding,
                Some("ask"),
                Some("proj-path-as-id"),
            )
            .unwrap();
            let loaded = get_subagent_session(&sid).unwrap().expect("session");
            assert_eq!(loaded.permission_profile.as_deref(), Some("ask"));
            assert_eq!(loaded.project_id.as_deref(), Some("proj-path-as-id"));
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
            assert_eq!(listed[0].name, "worker");
            assert_eq!(listed[0].task, "do the thing");
            touch_subagent_session(&sid).unwrap();
            close_subagent_session(&sid, "closed", None).unwrap();
            let open = list_subagent_sessions(Some("parent-1"), false).unwrap();
            assert!(open.is_empty());
        });
    }

    #[test]
    fn child_session_name_falls_back_to_prompt() {
        with_temp_db(|| {
            let binding = RouteBinding {
                provider_id: "openai".into(),
                key_id: "k1".into(),
                model_id: "gpt-4o".into(),
            };
            let (sid, _) = create_hidden_child_session(
                "parent-1",
                Some("run-1"),
                None,
                "",
                "  investigate   renderer sidebar leak  ",
                &binding,
                Some("ask"),
                None,
            )
            .unwrap();
            let sess = get_subagent_session(&sid).unwrap().unwrap();
            assert_eq!(sess.name, "investigate renderer sidebar leak");
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
            let before = get_subagent_session(&sid)
                .unwrap()
                .unwrap()
                .last_activity_at;
            // Parent touch must not bump child activity.
            touch_parent_heartbeat("parent-1").unwrap();
            let after = get_subagent_session(&sid)
                .unwrap()
                .unwrap()
                .last_activity_at;
            assert_eq!(before, after);
        });
    }

    /// TASK-009 (N05/E04): a child scope binds the parent's REAL project id +
    /// version — never the project path masquerading as an id — and a closed
    /// session leaves no active orphan.
    #[test]
    fn subagent_lifecycle_binds_real_identity_and_closes_without_orphan() {
        with_temp_db(|| {
            let binding = RouteBinding {
                provider_id: "openai".into(),
                key_id: "key-1".into(),
                model_id: "gpt-4o".into(),
            };
            let (sid, _child) = create_hidden_child_session(
                "parent-1",
                None,
                None,
                "sub",
                "task",
                &binding,
                Some("ask"),
                Some("real-project-uuid"),
            )
            .unwrap();
            persist_subagent_scope(
                &sid,
                &SubagentScope {
                    project_path: Some("/tmp/proj".into()),
                    project_id: Some("real-project-uuid".into()),
                    project_identity_version: Some(7),
                    permission_profile: Some("ask".into()),
                    agent_profile_id: None,
                    max_steps: Some(30),
                    tool_allowlist: vec![],
                },
            )
            .unwrap();
            let loaded = get_subagent_session(&sid).unwrap().expect("session");
            assert_eq!(loaded.project_id.as_deref(), Some("real-project-uuid"));
            assert_ne!(
                loaded.project_id.as_deref(),
                loaded.project_path.as_deref(),
                "the project path must never be stored as project_id"
            );
            assert_eq!(loaded.project_identity_version, Some(7));

            // E04: closing the session settles it — no active orphan remains.
            close_subagent_session(&sid, "completed", None).unwrap();
            let closed = get_subagent_session(&sid).unwrap().expect("session");
            assert_eq!(closed.status, "completed");
            assert!(
                closed.closed_at.is_some(),
                "closed session records its close"
            );
        });
    }

    // ── NE-P0-08 / §19.1: durable Child Directive persistence ──

    /// §19.1: the pending directive is persisted into the protected pending
    /// execution plan (reservation scope snapshot) BEFORE the child run is
    /// created. A daemon crash between reservation and child start does not
    /// lose the persona — the durable copy survives.
    #[test]
    fn pending_directive_persists_before_child_run_creation() {
        with_temp_db(|| {
            let binding = RouteBinding {
                provider_id: "openai".into(),
                key_id: "k1".into(),
                model_id: "gpt-4o".into(),
            };
            let (sid, _) = create_hidden_child_session(
                "parent-1",
                Some("parent-run"),
                None,
                "worker",
                "do",
                &binding,
                Some("ask"),
                None,
            )
            .unwrap();
            let text = "You are a crash-safe persona.";
            let digest = directive_sha256_hex(text);
            // Embed the directive in the reservation scope snapshot and
            // persist — this happens BEFORE the child run is created.
            let scope_snapshot =
                with_pending_directive(json!({ "project_id": "p1" }), Some((text, &digest)));
            let mut res = SubagentReservation {
                session_id: sid.clone(),
                parent_run_id: "parent-run".into(),
                tree_root_run_id: "tree-root".into(),
                depth: 1,
                max_tokens: Some(1_000),
                max_cost_usd: None,
                failure_policy: "fail_fast".into(),
                max_retries: 2,
                scope_snapshot,
            };
            reserve_subagent_slot(&res).unwrap();
            // The durable directive is readable from the session — the
            // child run does NOT need to exist for the persona to survive.
            let pd = pending_directive_for_session(&sid)
                .unwrap()
                .expect("durable directive survives before child run creation");
            assert_eq!(pd.text, text, "same text survives crash");
            assert_eq!(pd.digest, digest, "same digest survives crash");
        });
    }

    /// §19.5: crash recovery restores the EXACT same persona. The durable
    /// directive (text + digest) is byte-identical across reads — the
    /// loader is deterministic, not re-rolled or truncated.
    #[test]
    fn crash_recovery_restores_exact_same_persona() {
        with_temp_db(|| {
            let binding = RouteBinding {
                provider_id: "openai".into(),
                key_id: "k1".into(),
                model_id: "gpt-4o".into(),
            };
            let (sid, _) = create_hidden_child_session(
                "parent-1",
                Some("parent-run"),
                None,
                "worker",
                "do",
                &binding,
                Some("ask"),
                None,
            )
            .unwrap();
            let text = "You are a terse Rust reviewer. Check safety and logic.";
            let digest = directive_sha256_hex(text);
            let scope_snapshot = with_pending_directive(json!({}), Some((text, &digest)));
            let mut res = SubagentReservation {
                session_id: sid.clone(),
                parent_run_id: "parent-run".into(),
                tree_root_run_id: "tree-root".into(),
                depth: 1,
                max_tokens: Some(1_000),
                max_cost_usd: None,
                failure_policy: "fail_fast".into(),
                max_retries: 2,
                scope_snapshot,
            };
            reserve_subagent_slot(&res).unwrap();
            // Simulate crash: read the directive multiple times — each read
            // returns the exact same text + digest (deterministic recovery).
            let first = pending_directive_for_session(&sid).unwrap().unwrap();
            let second = pending_directive_for_session(&sid).unwrap().unwrap();
            assert_eq!(first.text, second.text, "text is deterministic");
            assert_eq!(first.digest, second.digest, "digest is deterministic");
            assert_eq!(first.text, text, "recovered text matches original");
            assert_eq!(first.digest, digest, "recovered digest matches original");
            // The digest verifies the text (same SHA-256).
            assert_eq!(
                directive_sha256_hex(&first.text),
                first.digest,
                "digest verifies the text"
            );
        });
    }

    /// §19.1: persona digest consistency — same text always produces the
    /// same digest, so retry/restart/continue can verify they restore the
    /// exact same persona by comparing digests.
    #[test]
    fn persona_digest_consistency_same_text_same_digest() {
        with_temp_db(|| {
            let text_a = "You are a reviewer.";
            let text_b = "You are a reviewer.";
            let text_c = "You are a different reviewer.";
            let digest_a = directive_sha256_hex(text_a);
            let digest_b = directive_sha256_hex(text_b);
            let digest_c = directive_sha256_hex(text_c);
            assert_eq!(digest_a, digest_b, "same text => same digest");
            assert_ne!(digest_a, digest_c, "different text => different digest");
            // SHA-256 hex is 64 chars.
            assert_eq!(digest_a.len(), 64);
        });
    }

    /// §19.1: the durable directive survives session status changes —
    /// closing and re-reading the session does not lose the persona text.
    /// (The scope snapshot is a durable column, not an in-memory field.)
    #[test]
    fn durable_directive_survives_session_status_change() {
        with_temp_db(|| {
            let binding = RouteBinding {
                provider_id: "openai".into(),
                key_id: "k1".into(),
                model_id: "gpt-4o".into(),
            };
            let (sid, _) = create_hidden_child_session(
                "parent-1",
                Some("parent-run"),
                None,
                "worker",
                "do",
                &binding,
                Some("ask"),
                None,
            )
            .unwrap();
            let text = "Persistent persona across status changes.";
            let digest = directive_sha256_hex(text);
            let scope_snapshot = with_pending_directive(json!({}), Some((text, &digest)));
            let mut res = SubagentReservation {
                session_id: sid.clone(),
                parent_run_id: "parent-run".into(),
                tree_root_run_id: "tree-root".into(),
                depth: 1,
                max_tokens: Some(1_000),
                max_cost_usd: None,
                failure_policy: "fail_fast".into(),
                max_retries: 2,
                scope_snapshot,
            };
            reserve_subagent_slot(&res).unwrap();
            // Change status to running, then completed — the directive
            // text is still readable (durable column, not in-memory).
            update_subagent_session_status(&sid, "running", None).unwrap();
            update_subagent_session_status(&sid, "completed", None).unwrap();
            let pd = pending_directive_for_session(&sid)
                .unwrap()
                .expect("directive survives status changes");
            assert_eq!(pd.text, text);
            assert_eq!(pd.digest, digest);
        });
    }

    /// §19.3/§19.5: redact_session_for_export hides the directive text and
    /// the scope snapshot from the RPC surface — only the digest marker
    /// is visible. This is field-level visibility for logs/exports.
    #[test]
    fn redact_session_export_for_subagent_list_hides_directive() {
        with_temp_db(|| {
            let binding = RouteBinding {
                provider_id: "openai".into(),
                key_id: "k1".into(),
                model_id: "gpt-4o".into(),
            };
            let (sid, _) = create_hidden_child_session(
                "parent-1",
                Some("parent-run"),
                None,
                "worker",
                "do",
                &binding,
                Some("ask"),
                None,
            )
            .unwrap();
            let secret_text = "secret persona that must never appear in exports";
            let digest = directive_sha256_hex(secret_text);
            let scope_snapshot = with_pending_directive(json!({}), Some((secret_text, &digest)));
            let mut res = SubagentReservation {
                session_id: sid.clone(),
                parent_run_id: "parent-run".into(),
                tree_root_run_id: "tree-root".into(),
                depth: 1,
                max_tokens: Some(1_000),
                max_cost_usd: None,
                failure_policy: "fail_fast".into(),
                max_retries: 2,
                scope_snapshot,
            };
            reserve_subagent_slot(&res).unwrap();
            let sess = get_subagent_session(&sid).unwrap().unwrap();
            let export = redact_session_for_export(&sess);
            let serialized = export.to_string();
            // The directive text never appears in the export.
            assert!(
                !serialized.contains(secret_text),
                "directive text must not leak through subagent.list export"
            );
            // The digest marker IS visible for field-level verification.
            assert!(
                serialized.contains(&digest),
                "digest stays visible for verification"
            );
            // The scope_snapshot_json field is redacted (no raw text).
            let snapshot_json = export["scope_snapshot_json"].as_str().unwrap_or("");
            assert!(
                !snapshot_json.contains(secret_text),
                "scope_snapshot_json must not carry raw directive text"
            );
        });
    }

    /// §19.1: the in-memory directive (run_agent_directives) is the
    /// transient copy; the durable copy is the scope snapshot. A session
    /// created WITHOUT a persisted directive returns None — callers fail
    /// closed rather than inventing a persona (legacy session).
    #[test]
    fn session_without_persisted_directive_returns_none() {
        with_temp_db(|| {
            let binding = RouteBinding {
                provider_id: "openai".into(),
                key_id: "k1".into(),
                model_id: "gpt-4o".into(),
            };
            let (sid, _) = create_hidden_child_session(
                "parent-1",
                None,
                None,
                "worker",
                "do",
                &binding,
                Some("ask"),
                None,
            )
            .unwrap();
            // No reservation with directive → None (legacy session).
            let pd = pending_directive_for_session(&sid).unwrap();
            assert!(
                pd.is_none(),
                "legacy session without directive returns None"
            );
        });
    }
}
