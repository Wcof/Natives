//! Structured Tool Grant matching (task-09).
//!
//! Callers use [`ToolPolicyState::check`] / [`ToolPolicyState::remember`] only —
//! they never read/write the grant Vec/DB directly.
//!
//! Agent C relocates this into ProductionRuntime thin facade (task-01).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Bump when matching rules change; old grants auto-fail match.
pub const TOOL_GRANT_POLICY_VERSION: u32 = 1;

/// Decision returned by [`ToolPolicyState::check`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrantDecision {
    Allowed { grant_id: String },
    NeedsApproval,
    Denied { reason: String },
}

/// Structured invocation used for grant matching.
#[derive(Debug, Clone)]
pub struct ToolInvocation {
    pub tool_name: String,
    pub permission_class: String,
    pub conversation_id: String,
    pub run_id: String,
    pub session_id: Option<String>,
    pub project_id: Option<String>,
    pub project_identity_version: Option<String>,
    pub project_fingerprint: Option<String>,
    pub input: Value,
    /// Canonical project root for path tools.
    pub project_root: Option<String>,
}

/// Durable structured grant (policy_version >= 1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuredToolGrant {
    pub id: String,
    pub project_id: Option<String>,
    pub project_identity_version: Option<String>,
    pub project_fingerprint: Option<String>,
    pub tool_name: String,
    pub permission_class: String,
    pub path_scope_json: Value,
    pub argument_constraint_json: Value,
    pub conversation_id: Option<String>,
    pub run_id: Option<String>,
    pub session_id: Option<String>,
    /// "this_run" | "session" | "project"
    pub scope: String,
    pub expires_at: Option<String>,
    pub policy_version: u32,
    pub created_by: Option<String>,
    pub created_at: String,
    pub revoked_at: Option<String>,
    pub constraint_summary: String,
}

/// In-memory + optional SQLite-backed policy state.
pub struct ToolPolicyState {
    grants: Arc<Mutex<Vec<StructuredToolGrant>>>,
}

impl Default for ToolPolicyState {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolPolicyState {
    pub fn new() -> Self {
        Self {
            grants: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub async fn check(&self, inv: &ToolInvocation) -> GrantDecision {
        let now = chrono::Utc::now();
        let grants = self.grants.lock().await;
        for g in grants.iter() {
            if grant_matches(g, inv, now) {
                return GrantDecision::Allowed {
                    grant_id: g.id.clone(),
                };
            }
        }
        drop(grants);
        // DB fallback
        if let Some(id) = load_matching_grant_db(inv, now) {
            return GrantDecision::Allowed { grant_id: id };
        }
        GrantDecision::NeedsApproval
    }

    /// Remember a durable grant after successful approval path.
    ///
    /// DB is written first; on DB failure the grant is **not** kept for reuse
    /// (caller may still execute the one-shot approval).
    pub async fn remember(
        &self,
        inv: &ToolInvocation,
        scope: &str,
        created_by: Option<&str>,
    ) -> Result<Option<StructuredToolGrant>, String> {
        let scope = normalize_scope(scope);
        if scope == "once" {
            return Ok(None);
        }
        let constraint = build_constraint(inv);
        let grant = StructuredToolGrant {
            id: uuid::Uuid::new_v4().to_string(),
            project_id: inv.project_id.clone(),
            project_identity_version: inv.project_identity_version.clone(),
            project_fingerprint: inv.project_fingerprint.clone(),
            tool_name: inv.tool_name.clone(),
            permission_class: inv.permission_class.clone(),
            path_scope_json: constraint.path_scope,
            argument_constraint_json: constraint.argument,
            conversation_id: Some(inv.conversation_id.clone()),
            run_id: if scope == "this_run" {
                Some(inv.run_id.clone())
            } else {
                None
            },
            session_id: if scope == "session" {
                inv.session_id.clone()
            } else {
                None
            },
            scope: scope.clone(),
            expires_at: default_expiry(&scope),
            policy_version: TOOL_GRANT_POLICY_VERSION,
            created_by: created_by.map(|s| s.to_string()),
            created_at: chrono::Utc::now().to_rfc3339(),
            revoked_at: None,
            constraint_summary: constraint.summary,
        };

        // Durable first.
        if let Err(e) = persist_grant_db(&grant) {
            // Do not leave reusable in-memory grant; one-shot approval still valid.
            eprintln!("[tool_policy] grant persist failed (not remembered): {e}");
            return Err(e);
        }
        self.grants.lock().await.push(grant.clone());
        Ok(Some(grant))
    }

    pub async fn revoke(&self, grant_id: &str) {
        let mut grants = self.grants.lock().await;
        if let Some(g) = grants.iter_mut().find(|g| g.id == grant_id) {
            g.revoked_at = Some(chrono::Utc::now().to_rfc3339());
        }
        let _ = revoke_grant_db(grant_id);
    }
}

struct BuiltConstraint {
    path_scope: Value,
    argument: Value,
    summary: String,
}

fn build_constraint(inv: &ToolInvocation) -> BuiltConstraint {
    match inv.tool_name.as_str() {
        "run_terminal" => {
            let cmd = inv
                .input
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let cwd = inv
                .input
                .get("cwd")
                .and_then(|v| v.as_str())
                .unwrap_or(".")
                .to_string();
            let arg = serde_json::json!({
                "kind": "terminal_exact",
                "command": normalize_command(&cmd),
                "cwd": normalize_cwd(&cwd),
            });
            BuiltConstraint {
                path_scope: Value::Null,
                summary: format!("terminal cmd={} cwd={}", redact_preview(&cmd), cwd),
                argument: arg,
            }
        }
        "write_file" | "read_file" | "apply_patch" | "edit_file" | "delete_file" => {
            let path = inv
                .input
                .get("path")
                .or_else(|| inv.input.get("file_path"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let rel = canonical_rel_path(inv.project_root.as_deref(), &path);
            let op = match inv.tool_name.as_str() {
                "read_file" => "read",
                "delete_file" => "delete",
                _ => "write",
            };
            BuiltConstraint {
                path_scope: serde_json::json!({
                    "kind": "path_exact",
                    "path": rel,
                    "operation": op,
                }),
                argument: Value::Null,
                summary: format!("file {op} path={rel}"),
            }
        }
        name if name.starts_with("mcp__") || name == "mcp_call" => {
            let server = inv
                .input
                .get("server_id")
                .or_else(|| inv.input.get("server"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let tool = inv
                .input
                .get("tool")
                .or_else(|| inv.input.get("tool_name"))
                .and_then(|v| v.as_str())
                .unwrap_or(name)
                .to_string();
            let schema_ver = inv
                .input
                .get("schema_version")
                .cloned()
                .unwrap_or(Value::Null);
            let args_hash = canonical_json_hash(
                inv.input
                    .get("arguments")
                    .or_else(|| inv.input.get("input"))
                    .unwrap_or(&Value::Null),
            );
            BuiltConstraint {
                path_scope: Value::Null,
                argument: serde_json::json!({
                    "kind": "mcp_exact",
                    "server_id": server,
                    "tool": tool,
                    "schema_version": schema_ver,
                    "args_hash": args_hash,
                }),
                summary: format!("mcp server={server} tool={tool}"),
            }
        }
        "web_fetch" | "fetch" | "http_request" => {
            let url = inv
                .input
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            BuiltConstraint {
                path_scope: Value::Null,
                argument: serde_json::json!({
                    "kind": "network_scope",
                    "url": url,
                }),
                summary: format!("network url={}", redact_preview(&url)),
            }
        }
        _ => {
            let hash = canonical_json_hash(&inv.input);
            BuiltConstraint {
                path_scope: Value::Null,
                argument: serde_json::json!({
                    "kind": "input_hash",
                    "hash": hash,
                }),
                summary: format!("generic hash={}", &hash[..8.min(hash.len())]),
            }
        }
    }
}

fn grant_matches(
    g: &StructuredToolGrant,
    inv: &ToolInvocation,
    now: chrono::DateTime<chrono::Utc>,
) -> bool {
    // Legacy / wrong policy version never match.
    if g.policy_version == 0 || g.policy_version != TOOL_GRANT_POLICY_VERSION {
        return false;
    }
    if g.revoked_at.is_some() {
        return false;
    }
    if let Some(ref exp) = g.expires_at {
        if let Ok(t) = chrono::DateTime::parse_from_rfc3339(exp) {
            if t.with_timezone(&chrono::Utc) <= now {
                return false;
            }
        }
    }
    if g.tool_name != inv.tool_name {
        return false;
    }
    if !g.permission_class.is_empty()
        && g.permission_class != "unknown"
        && g.permission_class != inv.permission_class
    {
        return false;
    }
    // Project identity: if grant binds project, invocation must match.
    if let Some(ref pid) = g.project_id {
        if inv.project_id.as_deref() != Some(pid.as_str()) {
            return false;
        }
    }
    if let Some(ref fp) = g.project_fingerprint {
        if inv.project_fingerprint.as_deref() != Some(fp.as_str()) {
            return false;
        }
    }
    if let Some(ref ver) = g.project_identity_version {
        if inv.project_identity_version.as_deref() != Some(ver.as_str()) {
            return false;
        }
    }
    match g.scope.as_str() {
        "this_run" => {
            if g.run_id.as_deref() != Some(inv.run_id.as_str()) {
                return false;
            }
        }
        "session" => match (&g.session_id, &inv.session_id) {
            (Some(gs), Some(is)) if gs == is => {}
            _ => return false,
        },
        "project" => {}
        _ => return false,
    }
    // Structured constraints
    let built = build_constraint(inv);
    if g.path_scope_json != Value::Null && g.path_scope_json != built.path_scope {
        return false;
    }
    if g.argument_constraint_json != Value::Null && g.argument_constraint_json != built.argument {
        return false;
    }
    true
}

fn normalize_scope(scope: &str) -> String {
    match scope.trim().to_ascii_lowercase().as_str() {
        "once" | "this_time" => "once".into(),
        "run" | "this_run" => "this_run".into(),
        "session" => "session".into(),
        "project" | "always" | "forever" => "project".into(),
        _ => "once".into(),
    }
}

fn default_expiry(scope: &str) -> Option<String> {
    let dur = match scope {
        "this_run" => chrono::Duration::hours(24),
        "session" => chrono::Duration::days(7),
        "project" => chrono::Duration::days(90),
        _ => return None,
    };
    Some((chrono::Utc::now() + dur).to_rfc3339())
}

fn normalize_command(cmd: &str) -> String {
    cmd.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalize_cwd(cwd: &str) -> String {
    let t = cwd.trim().trim_end_matches('/');
    if t.is_empty() {
        ".".into()
    } else {
        t.to_string()
    }
}

fn canonical_rel_path(project_root: Option<&str>, path: &str) -> String {
    let p = path.trim();
    if p.is_empty() {
        return String::new();
    }
    if let Some(root) = project_root {
        let root = root.trim_end_matches('/');
        if let Ok(abs) = std::fs::canonicalize(p) {
            if let Ok(root_abs) = std::fs::canonicalize(root) {
                if let Ok(rel) = abs.strip_prefix(&root_abs) {
                    return rel.to_string_lossy().replace('\\', "/");
                }
            }
        }
        if p.starts_with(root) {
            return p[root.len()..].trim_start_matches('/').replace('\\', "/");
        }
    }
    p.trim_start_matches("./").replace('\\', "/")
}

fn canonical_json_hash(v: &Value) -> String {
    let s = canonical_json(v);
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    format!("{:x}", h.finalize())
}

fn canonical_json(v: &Value) -> String {
    match v {
        Value::Object(map) => {
            let mut keys: Vec<_> = map.keys().cloned().collect();
            keys.sort();
            let parts: Vec<String> = keys
                .into_iter()
                .map(|k| format!("\"{}\":{}", k, canonical_json(&map[&k])))
                .collect();
            format!("{{{}}}", parts.join(","))
        }
        Value::Array(arr) => {
            let parts: Vec<String> = arr.iter().map(canonical_json).collect();
            format!("[{}]", parts.join(","))
        }
        other => other.to_string(),
    }
}

fn redact_preview(s: &str) -> String {
    let t = s.trim();
    if t.len() <= 48 {
        t.to_string()
    } else {
        format!("{}…", &t[..45])
    }
}

fn persist_grant_db(grant: &StructuredToolGrant) -> Result<(), String> {
    let db_path = std::env::var("NATIVES_ASSISTANT_DB_PATH")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            std::env::var("NATIVES_DB_PATH")
                .ok()
                .filter(|s| !s.trim().is_empty())
        });
    let Some(db_path) = db_path else {
        // No DB configured — memory-only is acceptable for unit tests.
        return Ok(());
    };
    let path = std::path::PathBuf::from(&db_path);
    if !path.exists() {
        return Ok(());
    }
    let art = path
        .parent()
        .map(|p| p.join("artifacts"))
        .unwrap_or_else(std::env::temp_dir);
    let store = crate::storage::DataStore::new(&path, &art)?;
    let conn = store.conn()?;
    // Ensure v2 table exists even if migration not yet applied on this file.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS tool_grant_v2 (
            id TEXT PRIMARY KEY,
            project_id TEXT,
            project_identity_version TEXT,
            project_fingerprint TEXT,
            tool_name TEXT NOT NULL,
            permission_class TEXT NOT NULL DEFAULT 'unknown',
            path_scope_json TEXT NOT NULL DEFAULT 'null',
            argument_constraint_json TEXT NOT NULL DEFAULT 'null',
            conversation_id TEXT,
            run_id TEXT,
            session_id TEXT,
            scope TEXT NOT NULL DEFAULT 'once',
            expires_at TEXT,
            policy_version INTEGER NOT NULL DEFAULT 1,
            created_by TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            revoked_at TEXT,
            constraint_summary TEXT
        );",
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT OR REPLACE INTO tool_grant_v2 (
            id, project_id, project_identity_version, project_fingerprint,
            tool_name, permission_class, path_scope_json, argument_constraint_json,
            conversation_id, run_id, session_id, scope, expires_at, policy_version,
            created_by, created_at, revoked_at, constraint_summary
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
        rusqlite::params![
            grant.id,
            grant.project_id,
            grant.project_identity_version,
            grant.project_fingerprint,
            grant.tool_name,
            grant.permission_class,
            grant.path_scope_json.to_string(),
            grant.argument_constraint_json.to_string(),
            grant.conversation_id,
            grant.run_id,
            grant.session_id,
            grant.scope,
            grant.expires_at,
            grant.policy_version as i64,
            grant.created_by,
            grant.created_at,
            grant.revoked_at,
            grant.constraint_summary,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn load_matching_grant_db(
    inv: &ToolInvocation,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<String> {
    let db_path = std::env::var("NATIVES_ASSISTANT_DB_PATH")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            std::env::var("NATIVES_DB_PATH")
                .ok()
                .filter(|s| !s.trim().is_empty())
        })?;
    let path = std::path::PathBuf::from(db_path);
    if !path.exists() {
        return None;
    }
    let art = path
        .parent()
        .map(|p| p.join("artifacts"))
        .unwrap_or_else(std::env::temp_dir);
    let store = crate::storage::DataStore::new(&path, &art).ok()?;
    let conn = store.conn().ok()?;
    let mut stmt = conn
        .prepare(
            "SELECT id, project_id, project_identity_version, project_fingerprint,
                    tool_name, permission_class, path_scope_json, argument_constraint_json,
                    conversation_id, run_id, session_id, scope, expires_at, policy_version,
                    created_by, created_at, revoked_at, constraint_summary
             FROM tool_grant_v2
             WHERE tool_name = ?1 AND policy_version = ?2 AND revoked_at IS NULL",
        )
        .ok()?;
    let rows = stmt
        .query_map(
            rusqlite::params![inv.tool_name, TOOL_GRANT_POLICY_VERSION as i64],
            |row| {
                Ok(StructuredToolGrant {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    project_identity_version: row.get(2)?,
                    project_fingerprint: row.get(3)?,
                    tool_name: row.get(4)?,
                    permission_class: row.get(5)?,
                    path_scope_json: serde_json::from_str(&row.get::<_, String>(6)?)
                        .unwrap_or(Value::Null),
                    argument_constraint_json: serde_json::from_str(&row.get::<_, String>(7)?)
                        .unwrap_or(Value::Null),
                    conversation_id: row.get(8)?,
                    run_id: row.get(9)?,
                    session_id: row.get(10)?,
                    scope: row.get(11)?,
                    expires_at: row.get(12)?,
                    policy_version: row.get::<_, i64>(13)? as u32,
                    created_by: row.get(14)?,
                    created_at: row.get(15)?,
                    revoked_at: row.get(16)?,
                    constraint_summary: row.get::<_, Option<String>>(17)?.unwrap_or_default(),
                })
            },
        )
        .ok()?;
    for row in rows.flatten() {
        if grant_matches(&row, inv, now) {
            return Some(row.id);
        }
    }
    None
}

fn revoke_grant_db(grant_id: &str) -> Result<(), String> {
    let db_path = std::env::var("NATIVES_ASSISTANT_DB_PATH")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            std::env::var("NATIVES_DB_PATH")
                .ok()
                .filter(|s| !s.trim().is_empty())
        });
    let Some(db_path) = db_path else {
        return Ok(());
    };
    let path = std::path::PathBuf::from(db_path);
    if !path.exists() {
        return Ok(());
    }
    let art = path
        .parent()
        .map(|p| p.join("artifacts"))
        .unwrap_or_else(std::env::temp_dir);
    let store = crate::storage::DataStore::new(&path, &art)?;
    let conn = store.conn()?;
    conn.execute(
        "UPDATE tool_grant_v2 SET revoked_at = datetime('now') WHERE id = ?1",
        rusqlite::params![grant_id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Infer a stable permission_class string for grant binding.
pub fn permission_class_for_tool(tool_name: &str) -> String {
    match tool_name {
        "read_file" | "list_dir" | "search_files" | "grep" | "glob" => "project_read".into(),
        "write_file" | "apply_patch" | "edit_file" => "project_write".into(),
        "run_terminal" | "bash" => "destructive_command".into(),
        "web_fetch" | "fetch" | "mcp_call" => "external_write".into(),
        "task" | "kill_task" => "project_write".into(),
        "task_output" | "memory_search" | "memory_get" | "skill" | "todo_write"
        | "notification" => "always_allowed".into(),
        name if name.starts_with("mcp__") => "external_write".into(),
        _ => "unknown".into(),
    }
}

/// Build invocation with **no** project identity binding (readonly-only / tests).
/// Prefer [`invocation_from_verified_identity`] for mutating tools.
pub fn invocation_from_gate(
    tool_name: &str,
    input: &Value,
    conversation_id: &str,
    run_id: &str,
    project_root: Option<&str>,
) -> ToolInvocation {
    ToolInvocation {
        tool_name: tool_name.to_string(),
        permission_class: permission_class_for_tool(tool_name),
        conversation_id: conversation_id.to_string(),
        run_id: run_id.to_string(),
        // A conversation is the Native session boundary. Keep it available so
        // session-scoped grants can match across runs in the same conversation.
        session_id: Some(conversation_id.to_string()),
        // Soft path-as-id removed: project_id must come from verified ProjectIdentity.
        project_id: None,
        project_identity_version: None,
        project_fingerprint: None,
        input: input.clone(),
        project_root: project_root.map(|s| s.to_string()),
    }
}

/// Build invocation from a verified ProjectIdentity.
/// Callers cannot omit fingerprint/version — they are taken from identity only.
pub fn invocation_from_verified_identity(
    tool_name: &str,
    input: &Value,
    conversation_id: &str,
    run_id: &str,
    session_id: Option<&str>,
    identity: &crate::project_identity::ProjectIdentity,
) -> ToolInvocation {
    ToolInvocation {
        tool_name: tool_name.to_string(),
        permission_class: permission_class_for_tool(tool_name),
        conversation_id: conversation_id.to_string(),
        run_id: run_id.to_string(),
        session_id: session_id.map(|s| s.to_string()),
        project_id: Some(identity.project_id.clone()),
        project_identity_version: Some(identity.identity_version.to_string()),
        project_fingerprint: Some(identity.filesystem_fingerprint.clone()),
        input: input.clone(),
        project_root: Some(identity.canonical_path.clone()),
    }
}

/// True when the tool may proceed without a bound ProjectIdentity.
///
/// The creative draft tools are here despite being mutating: a draft is not a
/// project. They address their target by a validated `draftId` and can only ever
/// touch `~/.natives/drafts/<draftId>/`, so a project binding would add a
/// requirement the creator workbench cannot satisfy — it has no project to bind —
/// while protecting nothing that the id validation does not already cover.
pub fn tool_allows_unbound_project(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "read_file"
            | "list_dir"
            | "grep"
            | "glob"
            | "task_output"
            | "task"
            | "kill_task"
            | "write_draft_module"
            | "read_draft_module"
            | "rollback_draft_revision"
            | "lint_draft_module"
    )
}

/// Side-effect / mutating tools require a verified project binding.
pub fn tool_requires_verified_project(tool_name: &str) -> bool {
    !tool_allows_unbound_project(tool_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inv_terminal(cmd: &str, cwd: &str, project: &str) -> ToolInvocation {
        ToolInvocation {
            tool_name: "run_terminal".into(),
            permission_class: "destructive_command".into(),
            conversation_id: "c1".into(),
            run_id: "r1".into(),
            session_id: Some("s1".into()),
            project_id: Some(project.into()),
            project_identity_version: None,
            project_fingerprint: Some("fp1".into()),
            input: serde_json::json!({"command": cmd, "cwd": cwd}),
            project_root: Some(project.into()),
        }
    }

    #[tokio::test]
    async fn terminal_exact_command_and_cwd() {
        let pol = ToolPolicyState::new();
        let inv = inv_terminal("npm run dev", ".", "/proj/a");
        pol.remember(&inv, "project", None).await.unwrap();
        assert!(matches!(
            pol.check(&inv).await,
            GrantDecision::Allowed { .. }
        ));
        // Different command
        let bad = inv_terminal("rm -rf /", ".", "/proj/a");
        assert_eq!(pol.check(&bad).await, GrantDecision::NeedsApproval);
        // Different cwd
        let bad_cwd = inv_terminal("npm run dev", "other", "/proj/a");
        assert_eq!(pol.check(&bad_cwd).await, GrantDecision::NeedsApproval);
        // Different project
        let bad_proj = inv_terminal("npm run dev", ".", "/proj/b");
        assert_eq!(pol.check(&bad_proj).await, GrantDecision::NeedsApproval);
    }

    #[tokio::test]
    async fn this_run_does_not_cross_run() {
        let pol = ToolPolicyState::new();
        let mut inv = inv_terminal("cargo test", ".", "/p");
        pol.remember(&inv, "this_run", None).await.unwrap();
        assert!(matches!(
            pol.check(&inv).await,
            GrantDecision::Allowed { .. }
        ));
        inv.run_id = "r2".into();
        assert_eq!(pol.check(&inv).await, GrantDecision::NeedsApproval);
    }

    #[tokio::test]
    async fn session_scope_matches_runs_in_same_conversation_only() {
        let pol = ToolPolicyState::new();
        let inv = inv_terminal("cargo test", ".", "/p");
        pol.remember(&inv, "session", None).await.unwrap();
        let mut next_run = inv.clone();
        next_run.run_id = "r2".into();
        assert!(matches!(pol.check(&next_run).await, GrantDecision::Allowed { .. }));
        next_run.conversation_id = "c2".into();
        next_run.session_id = Some("s2".into());
        assert_eq!(pol.check(&next_run).await, GrantDecision::NeedsApproval);
    }

    #[tokio::test]
    async fn once_is_not_remembered() {
        let pol = ToolPolicyState::new();
        let inv = inv_terminal("ls", ".", "/p");
        let g = pol.remember(&inv, "once", None).await.unwrap();
        assert!(g.is_none());
        assert_eq!(pol.check(&inv).await, GrantDecision::NeedsApproval);
    }

    #[tokio::test]
    async fn legacy_policy_version_zero_ignored() {
        let pol = ToolPolicyState::new();
        let inv = inv_terminal("echo hi", ".", "/p");
        // Inject legacy-shaped grant manually
        pol.grants.lock().await.push(StructuredToolGrant {
            id: "legacy".into(),
            project_id: Some("/p".into()),
            project_identity_version: None,
            project_fingerprint: None,
            tool_name: "run_terminal".into(),
            permission_class: "destructive_command".into(),
            path_scope_json: Value::Null,
            argument_constraint_json: Value::Null,
            conversation_id: Some("c1".into()),
            run_id: None,
            session_id: None,
            scope: "project".into(),
            expires_at: None,
            policy_version: 0,
            created_by: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            revoked_at: None,
            constraint_summary: "legacy".into(),
        });
        assert_eq!(pol.check(&inv).await, GrantDecision::NeedsApproval);
    }

    #[tokio::test]
    async fn file_path_exact_match() {
        let pol = ToolPolicyState::new();
        let inv = ToolInvocation {
            tool_name: "write_file".into(),
            permission_class: "project_write".into(),
            conversation_id: "c1".into(),
            run_id: "r1".into(),
            session_id: None,
            project_id: Some("/proj".into()),
            project_identity_version: None,
            project_fingerprint: None,
            input: serde_json::json!({"path": "src/main.rs", "content": "x"}),
            project_root: Some("/proj".into()),
        };
        pol.remember(&inv, "project", None).await.unwrap();
        assert!(matches!(
            pol.check(&inv).await,
            GrantDecision::Allowed { .. }
        ));
        let other = ToolInvocation {
            input: serde_json::json!({"path": "src/other.rs", "content": "x"}),
            ..inv.clone()
        };
        assert_eq!(pol.check(&other).await, GrantDecision::NeedsApproval);
    }

    #[tokio::test]
    async fn revoked_grant_does_not_match() {
        let pol = ToolPolicyState::new();
        let inv = inv_terminal("pwd", ".", "/p");
        let g = pol.remember(&inv, "project", None).await.unwrap().unwrap();
        pol.revoke(&g.id).await;
        assert_eq!(pol.check(&inv).await, GrantDecision::NeedsApproval);
    }

    #[test]
    fn invocation_from_verified_identity_binds_all_fields() {
        let id = crate::project_identity::ProjectIdentity {
            project_id: "pid-1".into(),
            canonical_path: "/tmp/proj".into(),
            filesystem_fingerprint: "fp-abc".into(),
            identity_version: 3,
            verified_at: 0,
        };
        let inv = invocation_from_verified_identity(
            "write_file",
            &serde_json::json!({"path": "a.rs"}),
            "c1",
            "r1",
            Some("sess"),
            &id,
        );
        assert_eq!(inv.project_id.as_deref(), Some("pid-1"));
        assert_eq!(inv.project_identity_version.as_deref(), Some("3"));
        assert_eq!(inv.project_fingerprint.as_deref(), Some("fp-abc"));
        assert_eq!(inv.project_root.as_deref(), Some("/tmp/proj"));
        assert_eq!(inv.session_id.as_deref(), Some("sess"));
        assert!(tool_requires_verified_project("write_file"));
        assert!(tool_requires_verified_project("run_terminal"));
        assert!(tool_requires_verified_project("mcp_call"));
        assert!(!tool_requires_verified_project("read_file"));
    }

    #[test]
    fn permission_classes_match_gateway_tool_categories() {
        assert_eq!(permission_class_for_tool("mcp_call"), "external_write");
        assert_eq!(permission_class_for_tool("task"), "project_write");
        assert_eq!(permission_class_for_tool("task_output"), "always_allowed");
        assert_eq!(permission_class_for_tool("search_files"), "project_read");
    }

    #[test]
    fn invocation_from_gate_does_not_use_path_as_project_id() {
        let inv = invocation_from_gate(
            "write_file",
            &serde_json::json!({}),
            "c",
            "r",
            Some("/some/path"),
        );
        assert!(inv.project_id.is_none());
        assert!(inv.project_fingerprint.is_none());
        assert_eq!(inv.project_root.as_deref(), Some("/some/path"));
    }
}
