//! Durable slot + budget reservations for subagent sessions (T05) and the child
//! scope persisted at spawn (migration 029).
//!
//! Split from `subagent_store` (reservation/scope responsibility).

use super::*;

/// Child scope persisted on a subagent session (migration 029) so a route
/// restart restores the original project identity, permission ceiling, profile,
/// step budget, and tool allowlist instead of guessing defaults.
#[derive(Debug, Clone, Default)]
pub struct SubagentScope {
    pub project_path: Option<String>,
    pub project_id: Option<String>,
    pub project_identity_version: Option<i64>,
    pub permission_profile: Option<String>,
    pub agent_profile_id: Option<String>,
    pub max_steps: Option<i64>,
    pub tool_allowlist: Vec<String>,
}

/// Persist the child scope on an existing session after its initial spawn.
/// Called once at child creation; a route restart reads it back and fails
/// closed when the required scope fields are missing.
pub fn persist_subagent_scope(session_id: &str, scope: &SubagentScope) -> Result<(), String> {
    let s = store()?;
    let conn = s.conn().map_err(|e| format!("conn lock: {e}"))?;
    let allowlist_json = serde_json::to_string(&scope.tool_allowlist)
        .map_err(|e| format!("allowlist serialize: {e}"))?;
    conn.execute(
        "UPDATE subagent_session
         SET project_path = ?1, project_id = ?2, project_identity_version = ?3,
             permission_profile = ?4, agent_profile_id = ?5, max_steps = ?6,
             tool_allowlist_json = ?7, updated_at = ?8
         WHERE id = ?9",
        params![
            scope.project_path,
            scope.project_id,
            scope.project_identity_version,
            scope.permission_profile,
            scope.agent_profile_id,
            scope.max_steps,
            allowlist_json,
            chrono::Utc::now().to_rfc3339(),
            session_id,
        ],
    )
    .map_err(|e| format!("persist_subagent_scope failed: {e}"))?;
    Ok(())
}

/// Durable slot + budget reservation for one subagent session (T05).
///
/// Written once at spawn (saga phase 1), released exactly once by a terminal
/// child or a restart recovery. `scope_snapshot` is the full spawn-time scope
/// so a route restart can only tighten it, never widen it.
#[derive(Debug, Clone)]
pub struct SubagentReservation {
    pub session_id: String,
    pub parent_run_id: String,
    pub tree_root_run_id: String,
    pub depth: u32,
    pub max_tokens: Option<u64>,
    pub max_cost_usd: Option<f64>,
    pub failure_policy: String,
    pub max_retries: u32,
    pub scope_snapshot: serde_json::Value,
}

/// Reserve the concurrent slot + budget for a session. Idempotent: a slot that
/// is already active keeps its first `reserved_at` and is not double-counted.
pub fn reserve_subagent_slot(res: &SubagentReservation) -> Result<(), String> {
    let session_id = res.session_id.trim();
    if session_id.is_empty() {
        return Err("session_id required for reservation".into());
    }
    let policy = match res.failure_policy.trim() {
        "fail_fast" | "failfast" => "fail_fast",
        "require_all" | "requireall" => "require_all",
        "retry" | "retry_failed" => "retry",
        _ => "isolate",
    };
    let snapshot = serde_json::to_string(&res.scope_snapshot)
        .map_err(|e| format!("scope snapshot serialize: {e}"))?;
    let now = chrono::Utc::now().to_rfc3339();
    let s = store()?;
    let conn = s.conn().map_err(|e| format!("conn lock: {e}"))?;
    let changed = conn
        .execute(
            "UPDATE subagent_session
             SET reserved_at = COALESCE(reserved_at, ?1),
                 released_at = NULL,
                 reservation_released = 0,
                 tree_root_run_id = ?2,
                 depth = ?3,
                 max_tokens = ?4,
                 max_cost_usd = ?5,
                 failure_policy = ?6,
                 max_retries = ?7,
                 retry_count = 0,
                 scope_snapshot_json = ?8,
                 updated_at = ?1
             WHERE id = ?9
               AND (reservation_released = 1 OR reserved_at IS NULL)",
            params![
                now,
                res.tree_root_run_id,
                res.depth as i64,
                res.max_tokens.map(|v| v as i64),
                res.max_cost_usd,
                policy,
                res.max_retries as i64,
                snapshot,
                session_id,
            ],
        )
        .map_err(|e| format!("reserve_subagent_slot failed: {e}"))?;
    if changed == 0 {
        // Already reserved (or missing). Distinguish so a misspelled id fails
        // closed instead of silently accepting.
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM subagent_session WHERE id = ?1)",
                params![session_id],
                |row| row.get(0),
            )
            .map_err(|e| format!("reserve existence check: {e}"))?;
        if !exists {
            return Err(format!("subagent session not found: {session_id}"));
        }
    }
    Ok(())
}

/// Mark a session's slot released. Returns true when this call performed the
/// release (was active before); false when the slot was already released — the
/// exactly-once guarantee that makes restart recovery idempotent.
pub fn release_subagent_slot(session_id: &str, reason: Option<&str>) -> Result<bool, String> {
    let session_id = session_id.trim();
    if session_id.is_empty() {
        return Ok(false);
    }
    let now = chrono::Utc::now().to_rfc3339();
    let s = store()?;
    let conn = s.conn().map_err(|e| format!("conn lock: {e}"))?;
    let n = conn
        .execute(
            "UPDATE subagent_session
             SET reservation_released = 1,
                 released_at = COALESCE(released_at, ?1),
                 error = COALESCE(error, ?2),
                 updated_at = ?1
             WHERE id = ?3 AND reservation_released = 0",
            params![now, reason, session_id],
        )
        .map_err(|e| format!("release_subagent_slot failed: {e}"))?;
    Ok(n > 0)
}

/// Settle `tokens` against the session's durable budget. Persists the
/// increment and errors when the child budget is exceeded — the caller must
/// treat the child as failed (never Completed) and cancel it immediately.
/// Cost is persisted only when the provider reports a figure.
pub fn settle_subagent_usage(
    session_id: &str,
    tokens: u64,
    cost_usd: Option<f64>,
) -> Result<(), String> {
    let session_id = session_id.trim();
    if session_id.is_empty() {
        return Err("session_id required for usage settle".into());
    }
    let s = store()?;
    let conn = s.conn().map_err(|e| format!("conn lock: {e}"))?;
    let (used, max_tokens, used_cost, max_cost): (i64, Option<i64>, f64, Option<f64>) = conn
        .query_row(
            "SELECT tokens_used, max_tokens, cost_usd, max_cost_usd
             FROM subagent_session WHERE id = ?1",
            params![session_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, f64>(2)?,
                    row.get::<_, Option<f64>>(3)?,
                ))
            },
        )
        .map_err(|e| format!("settle read: {e}"))?;
    let new_used = (used.max(0) as u64).saturating_add(tokens);
    if let Some(max) = max_tokens {
        if new_used > max.max(0) as u64 {
            // Do not persist the over-budget amount: keep the ledger at the
            // last in-budget figure so a resume starts from a true state.
            return Err(format!(
                "subagent token budget exceeded ({new_used}/{})",
                max.max(0)
            ));
        }
    }
    let new_cost = (used_cost.max(0.0)) + cost_usd.unwrap_or(0.0).max(0.0);
    if let Some(max) = max_cost {
        if new_cost > max.max(0.0) {
            return Err(format!(
                "subagent cost budget exceeded ({new_cost:.4}/{max:.4})"
            ));
        }
    }
    conn.execute(
        "UPDATE subagent_session
         SET tokens_used = ?1, cost_usd = ?2, updated_at = ?3
         WHERE id = ?4",
        params![
            new_used as i64,
            new_cost,
            chrono::Utc::now().to_rfc3339(),
            session_id,
        ],
    )
    .map_err(|e| format!("settle_subagent_usage failed: {e}"))?;
    Ok(())
}

/// Sum of tokens used by every child ever spawned into a tree (for the tree
/// budget — cumulative, so a released child still counts toward the cap).
pub fn subagent_tree_tokens_used(tree_root_run_id: &str) -> Result<u64, String> {
    let s = store()?;
    let conn = s.conn().map_err(|e| format!("conn lock: {e}"))?;
    let sum: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(tokens_used), 0)
             FROM subagent_session
             WHERE tree_root_run_id = ?1",
            params![tree_root_run_id],
            |row| row.get(0),
        )
        .map_err(|e| format!("tree usage sum: {e}"))?;
    Ok(sum.max(0) as u64)
}

/// Increment the retry counter for a session; returns the new count.
pub fn bump_subagent_retry(session_id: &str) -> Result<u32, String> {
    let session_id = session_id.trim();
    if session_id.is_empty() {
        return Err("session_id required for retry bump".into());
    }
    let s = store()?;
    let conn = s.conn().map_err(|e| format!("conn lock: {e}"))?;
    conn.execute(
        "UPDATE subagent_session
         SET retry_count = retry_count + 1, updated_at = datetime('now')
         WHERE id = ?1",
        params![session_id],
    )
    .map_err(|e| format!("bump_subagent_retry failed: {e}"))?;
    let count: i64 = conn
        .query_row(
            "SELECT retry_count FROM subagent_session WHERE id = ?1",
            params![session_id],
            |row| row.get(0),
        )
        .map_err(|e| format!("retry count read: {e}"))?;
    Ok(count.max(0) as u32)
}

/// Most recent run on a session's hidden conversation — the live child run.
pub fn child_run_id_for_session(session_id: &str) -> Result<Option<String>, String> {
    let session_id = session_id.trim();
    if session_id.is_empty() {
        return Ok(None);
    }
    let s = store()?;
    let conn = s.conn().map_err(|e| format!("conn lock: {e}"))?;
    let child: Option<String> = conn
        .query_row(
            "SELECT child_conversation_id FROM subagent_session WHERE id = ?1",
            params![session_id],
            |row| row.get(0),
        )
        .map_err(|e| format!("session read: {e}"))?;
    let Some(child) = child else {
        return Ok(None);
    };
    conn.query_row(
        "SELECT id FROM run
         WHERE conversation_id = ?1
         ORDER BY created_at DESC, id DESC LIMIT 1",
        params![child],
        |row| row.get(0),
    )
    .optional()
    .map_err(|e| format!("child run lookup: {e}"))
}

/// Startup recovery: release every reservation whose child run is already
/// terminal (interrupted by the restart, or finished before it), and mark the
/// owning session interrupted. Returns the number of slots still occupied by
/// *active* children after recovery (normally 0 — the restart interrupted
/// them all).
pub fn recover_subagent_reservations() -> Result<usize, String> {
    let s = store()?;
    let conn = s.conn().map_err(|e| format!("conn lock: {e}"))?;
    recover_subagent_reservations_on(&conn)
}

/// Connection-scoped variant so `RunManager` startup recovery reuses the exact
/// DataStore it was given rather than re-resolving the env path.
pub fn recover_subagent_reservations_on(conn: &rusqlite::Connection) -> Result<usize, String> {
    let now = chrono::Utc::now().to_rfc3339();

    // Sessions with a reservation whose live child run is terminal (or gone)
    // must release their slot exactly once.
    let affected = conn
        .execute(
            "UPDATE subagent_session
             SET reservation_released = 1,
                 released_at = COALESCE(released_at, ?1),
                 updated_at = ?1,
                 status = CASE WHEN status IN
                     ('open','queued','running','waiting','pending_assignment')
                     THEN 'interrupted' ELSE status END
             WHERE id IN (
               SELECT s.id FROM subagent_session s
               WHERE s.reservation_released = 0
                 AND (
                   NOT EXISTS (
                     SELECT 1 FROM run r
                     WHERE r.conversation_id = s.child_conversation_id
                       AND r.status IN ('created','queued','preparing','running',
                                        'waiting_permission','waiting_subagent','cancelling')
                   )
                 )
             )",
            params![now],
        )
        .map_err(|e| format!("recover_subagent_reservations release: {e}"))?;

    // Count slots that legitimately remain occupied (an active child run with
    // an unreleased reservation) so the fresh in-memory ledger is seeded.
    let active: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM subagent_session s
             WHERE s.reservation_released = 0
               AND EXISTS (
                 SELECT 1 FROM run r
                 WHERE r.conversation_id = s.child_conversation_id
                   AND r.status IN ('created','queued','preparing','running',
                                    'waiting_permission','waiting_subagent','cancelling')
               )",
            [],
            |row| row.get(0),
        )
        .map_err(|e| format!("recover active reservation count: {e}"))?;
    let _ = affected;
    Ok(active.max(0) as usize)
}

/// Count of currently active (unreleased) reservations for a parent.
pub fn parent_active_reservations(parent_run_id: &str) -> Result<usize, String> {
    let s = store()?;
    let conn = s.conn().map_err(|e| format!("conn lock: {e}"))?;
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM subagent_session
             WHERE parent_run_id = ?1 AND reservation_released = 0",
            params![parent_run_id],
            |row| row.get(0),
        )
        .map_err(|e| format!("parent active reservation count: {e}"))?;
    Ok(n.max(0) as usize)
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

    fn temp_session(binding: &RouteBinding) -> (String, String) {
        create_hidden_child_session(
            "parent-1",
            Some("parent-run"),
            None,
            "worker",
            "do",
            binding,
            Some("ask"),
            None,
        )
        .unwrap()
    }

    fn snapshot(over: &serde_json::Value) -> SubagentReservation {
        SubagentReservation {
            session_id: String::new(),
            parent_run_id: "parent-run".into(),
            tree_root_run_id: "tree-root".into(),
            depth: 1,
            max_tokens: Some(1_000),
            max_cost_usd: None,
            failure_policy: "fail_fast".into(),
            max_retries: 2,
            scope_snapshot: over.clone(),
        }
    }

    #[test]
    fn subagent_scope_persists_and_round_trips() {
        with_temp_db(|| {
            let binding = RouteBinding {
                provider_id: "openai".into(),
                key_id: "key-1".into(),
                model_id: "gpt-4o".into(),
            };
            let (sid, _child) = create_hidden_child_session(
                "parent-1",
                Some("parent-run"),
                Some("task-call-1"),
                "sub",
                "do the thing",
                &binding,
                Some("readonly"),
                Some("proj-id-1"),
            )
            .unwrap();
            let scope = SubagentScope {
                project_path: Some("/tmp/project".into()),
                project_id: Some("proj-id-1".into()),
                project_identity_version: Some(1),
                permission_profile: Some("readonly".into()),
                agent_profile_id: Some("prof-1".into()),
                max_steps: Some(20),
                tool_allowlist: vec!["read_file".into(), "grep".into()],
            };
            persist_subagent_scope(&sid, &scope).unwrap();
            let loaded = get_subagent_session(&sid).unwrap().expect("session");
            assert_eq!(loaded.project_path.as_deref(), Some("/tmp/project"));
            assert_eq!(loaded.project_id.as_deref(), Some("proj-id-1"));
            assert_eq!(loaded.project_identity_version, Some(1));
            assert_eq!(loaded.permission_profile.as_deref(), Some("readonly"));
            assert_eq!(loaded.agent_profile_id.as_deref(), Some("prof-1"));
            assert_eq!(loaded.max_steps, Some(20));
            assert_eq!(
                loaded.tool_allowlist,
                vec!["read_file".to_string(), "grep".to_string()]
            );
        });
    }

    #[test]
    fn reservation_round_trip_and_exactly_once_release() {
        with_temp_db(|| {
            let binding = RouteBinding {
                provider_id: "openai".into(),
                key_id: "k1".into(),
                model_id: "gpt-4o".into(),
            };
            let (sid, _) = temp_session(&binding);
            let mut res = snapshot(&serde_json::json!({
                "project_id": "p1", "permission_profile": "readonly",
            }));
            res.session_id = sid.clone();
            reserve_subagent_slot(&res).unwrap();
            let loaded = get_subagent_session(&sid).unwrap().unwrap();
            assert_eq!(loaded.failure_policy, "fail_fast");
            assert_eq!(loaded.max_tokens, Some(1_000));
            assert_eq!(loaded.max_retries, 2);
            assert!(!loaded.reservation_released);
            assert!(loaded.reserved_at.is_some());
            assert_eq!(loaded.tree_root_run_id.as_deref(), Some("tree-root"));

            // Idempotent re-reserve keeps the same slot (no double count).
            reserve_subagent_slot(&res).unwrap();

            // Release is exactly-once: the second call reports no-op.
            assert!(release_subagent_slot(&sid, Some("done")).unwrap());
            assert!(!release_subagent_slot(&sid, Some("again")).unwrap());
            let released = get_subagent_session(&sid).unwrap().unwrap();
            assert!(released.reservation_released);
            assert!(released.released_at.is_some());
        });
    }

    #[test]
    fn settle_usage_enforces_budget_and_persists() {
        with_temp_db(|| {
            let binding = RouteBinding {
                provider_id: "openai".into(),
                key_id: "k1".into(),
                model_id: "gpt-4o".into(),
            };
            let (sid, _) = temp_session(&binding);
            let mut res = snapshot(&serde_json::json!({}));
            res.session_id = sid.clone();
            res.max_tokens = Some(100);
            reserve_subagent_slot(&res).unwrap();

            settle_subagent_usage(&sid, 60, Some(0.01)).unwrap();
            settle_subagent_usage(&sid, 40, None).unwrap();
            let loaded = get_subagent_session(&sid).unwrap().unwrap();
            assert_eq!(loaded.tokens_used, 100);
            assert!((loaded.cost_usd - 0.01).abs() < 1e-9);

            // Over budget: error, and the ledger stays at the last in-budget value.
            let err = settle_subagent_usage(&sid, 1, None).unwrap_err();
            assert!(err.contains("budget exceeded"), "{err}");
            let after = get_subagent_session(&sid).unwrap().unwrap();
            assert_eq!(
                after.tokens_used, 100,
                "over-budget amount must not persist"
            );
        });
    }

    #[test]
    fn retry_counter_bumps() {
        with_temp_db(|| {
            let binding = RouteBinding {
                provider_id: "openai".into(),
                key_id: "k1".into(),
                model_id: "gpt-4o".into(),
            };
            let (sid, _) = temp_session(&binding);
            let mut res = snapshot(&serde_json::json!({}));
            res.session_id = sid.clone();
            reserve_subagent_slot(&res).unwrap();
            assert_eq!(bump_subagent_retry(&sid).unwrap(), 1);
            assert_eq!(bump_subagent_retry(&sid).unwrap(), 2);
            let loaded = get_subagent_session(&sid).unwrap().unwrap();
            assert_eq!(loaded.retry_count, 2);
        });
    }

    #[test]
    fn tree_usage_sums_across_children() {
        with_temp_db(|| {
            let binding = RouteBinding {
                provider_id: "openai".into(),
                key_id: "k1".into(),
                model_id: "gpt-4o".into(),
            };
            let (sid_a, _) = temp_session(&binding);
            let (sid_b, _) = temp_session(&binding);
            let mut ra = snapshot(&serde_json::json!({}));
            ra.session_id = sid_a.clone();
            let mut rb = snapshot(&serde_json::json!({}));
            rb.session_id = sid_b.clone();
            reserve_subagent_slot(&ra).unwrap();
            reserve_subagent_slot(&rb).unwrap();
            settle_subagent_usage(&sid_a, 10, None).unwrap();
            settle_subagent_usage(&sid_b, 30, None).unwrap();
            assert_eq!(subagent_tree_tokens_used("tree-root").unwrap(), 40);
            // Tree budget is cumulative: a released child still counts toward it.
            release_subagent_slot(&sid_a, None).unwrap();
            assert_eq!(subagent_tree_tokens_used("tree-root").unwrap(), 40);
        });
    }

    #[test]
    fn recovery_releases_reservations_for_terminal_children() {
        with_temp_db(|| {
            let binding = RouteBinding {
                provider_id: "openai".into(),
                key_id: "k1".into(),
                model_id: "gpt-4o".into(),
            };
            let (sid_terminal, child_a) = temp_session(&binding);
            let (sid_active, child_b) = temp_session(&binding);
            let mut ra = snapshot(&serde_json::json!({}));
            ra.session_id = sid_terminal.clone();
            let mut rb = snapshot(&serde_json::json!({}));
            rb.session_id = sid_active.clone();
            reserve_subagent_slot(&ra).unwrap();
            reserve_subagent_slot(&rb).unwrap();

            let s = store().unwrap();
            let conn = s.conn().unwrap();
            // Create the child run rows: A already completed before restart,
            // B was still running.
            conn.execute(
                "INSERT INTO run (id, conversation_id, status, provider_id, model_id)
                 VALUES ('run-a', ?1, 'completed', 'openai', 'gpt-4o')",
                params![child_a],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO run (id, conversation_id, status, provider_id, model_id)
                 VALUES ('run-b', ?1, 'running', 'openai', 'gpt-4o')",
                params![child_b],
            )
            .unwrap();

            let active = recover_subagent_reservations().unwrap();
            assert_eq!(active, 1, "only the still-running child keeps a slot");
            let a = get_subagent_session(&sid_terminal).unwrap().unwrap();
            assert!(a.reservation_released, "terminal child slot released");
            let b = get_subagent_session(&sid_active).unwrap().unwrap();
            assert!(!b.reservation_released, "active child keeps its slot");
        });
    }

    #[test]
    fn restart_keeps_durable_budget_even_after_releasing_a_slot() {
        with_temp_db(|| {
            let binding = RouteBinding {
                provider_id: "openai".into(),
                key_id: "k1".into(),
                model_id: "gpt-4o".into(),
            };
            let (sid, child) = temp_session(&binding);
            let mut res = snapshot(&serde_json::json!({}));
            res.session_id = sid.clone();
            res.max_tokens = Some(1_000);
            reserve_subagent_slot(&res).unwrap();
            settle_subagent_usage(&sid, 250, None).unwrap();

            let s = store().unwrap();
            let conn = s.conn().unwrap();
            conn.execute(
                "UPDATE run SET status = 'interrupted' WHERE id = ?1",
                params![child],
            )
            .unwrap();

            // Daemon restart: the interrupted child releases its slot, but the
            // used budget must survive for tree accounting / resume.
            let active = recover_subagent_reservations().unwrap();
            assert_eq!(active, 0, "interrupted child frees its slot");
            let loaded = get_subagent_session(&sid).unwrap().unwrap();
            assert_eq!(loaded.tokens_used, 250, "budget persists across restart");
            assert_eq!(subagent_tree_tokens_used("tree-root").unwrap(), 250);
        });
    }
}
