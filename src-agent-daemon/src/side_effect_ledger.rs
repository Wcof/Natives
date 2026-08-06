//! Side-effect ledger for rewind coverage (Phase 4 / task-07).
//!
//! Records whether a run's tool effects are checkpoint-covered, external-only,
//! or unknown — used by workspace.restorePreview.

use serde_json::Value;

fn ledger_store() -> Result<std::sync::Arc<crate::storage::DataStore>, String> {
    if let Some(store) = crate::run_manager::global_run_manager().data_store_ref() {
        return Ok(store);
    }
    #[cfg(test)]
    {
        // T01 hermeticity: resolve the ledger store from the current test's
        // thread-local override/env first (each hermetic test gets its own
        // temp DB — no cross-test contention on a shared fixed path). Fall
        // back to the historical per-process temp DB only when a test sets
        // neither. Never default to ~/.natives.
        use std::collections::HashMap;
        use std::path::PathBuf;
        use std::sync::OnceLock;
        let (db, artifacts) = if let Some((db, art)) = crate::storage::test_db_override() {
            (db, art)
        } else {
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
            match db_path {
                Some(db) => {
                    let art = std::env::var("NATIVES_RUNTIME_DIR")
                        .map(PathBuf::from)
                        .unwrap_or_else(|_| db.with_extension("artifacts"));
                    (db, art)
                }
                None => {
                    // Historical fallback: one temp DB per test process.
                    static TEST_STORE: OnceLock<
                        std::sync::Mutex<
                            HashMap<PathBuf, std::sync::Arc<crate::storage::DataStore>>,
                        >,
                    > = OnceLock::new();
                    let db = std::env::temp_dir().join(format!(
                        "natives-side-effect-ledger-test-{}.db",
                        std::process::id()
                    ));
                    let art = db.with_extension("artifacts");
                    let cache = TEST_STORE.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
                    let mut map = cache.lock().unwrap_or_else(|e| e.into_inner());
                    return Ok(map
                        .entry(db.clone())
                        .or_insert_with(|| {
                            std::sync::Arc::new(
                                crate::storage::DataStore::new(&db, &art)
                                    .expect("test side-effect ledger store must migrate"),
                            )
                        })
                        .clone());
                }
            }
        };
        // Per-path cache so a test that resolves its own DB does not re-run
        // migrations on every ledger write.
        static PER_PATH: OnceLock<
            std::sync::Mutex<HashMap<PathBuf, std::sync::Arc<crate::storage::DataStore>>>,
        > = OnceLock::new();
        let cache = PER_PATH.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
        let mut map = cache.lock().unwrap_or_else(|e| e.into_inner());
        return Ok(map
            .entry(db.clone())
            .or_insert_with(|| {
                std::sync::Arc::new(
                    crate::storage::DataStore::new(&db, &artifacts)
                        .expect("test side-effect ledger store must migrate"),
                )
            })
            .clone());
    }
    #[allow(unreachable_code)]
    Err("no data store for side_effect_record".to_string())
}

/// Record a tool side-effect after execution.
pub fn record_tool_effect(
    run_id: &str,
    tool_name: &str,
    category: &str,
    summary: &Value,
    reversible: bool,
    checkpoint_id: Option<&str>,
) -> Result<(), String> {
    let store = ledger_store()?;
    let conn = store.conn()?;
    let id = uuid::Uuid::new_v4().to_string();
    let summary_s = serde_json::to_string(summary)
        .map_err(|error| format!("serialize side-effect summary: {error}"))?;
    let target = summary
        .get("path")
        .or_else(|| summary.get("command"))
        .or_else(|| summary.get("url"))
        .and_then(|v| v.as_str())
        .unwrap_or(tool_name);
    conn.execute(
        "INSERT INTO side_effect_record
         (id, run_id, tool_call_id, category, target_summary, reversible, checkpoint_id, coverage_note, created_at)
         VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, ?7, datetime('now'))",
        rusqlite::params![
            id,
            run_id,
            category,
            target,
            if reversible { 1 } else { 0 },
            checkpoint_id,
            summary_s,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Record the execution state with the call identity used by Core.  Unknown
/// completion is deliberately durable: resume code must block rather than
/// replaying a side effect it cannot prove safe.
#[allow(clippy::too_many_arguments)] // pre-existing: parameter list is fixed
pub fn record_tool_effect_state(
    run_id: &str,
    tool_call_id: &str,
    tool_name: &str,
    category: &str,
    status: &str,
    replay_safe: bool,
    turn_id: Option<&str>,
    summary: &Value,
) -> Result<(), String> {
    let store = ledger_store()?;
    let conn = store.conn()?;
    let target = summary
        .get("path")
        .or_else(|| summary.get("command"))
        .or_else(|| summary.get("url"))
        .and_then(Value::as_str)
        .unwrap_or(tool_name);
    let now = chrono::Utc::now().to_rfc3339();
    let terminal = matches!(status, "completed" | "failed" | "cancelled" | "uncertain");
    let external_reference = target;
    let idempotency_key = idempotency_key_for(tool_call_id);
    let replay_contract = replay_contract_for(category);
    let changed = conn
        .execute(
            "UPDATE side_effect_record
             SET category = ?1, target_summary = ?2, coverage_note = ?3,
                 turn_id = ?4, side_effect_class = ?5, status = ?6,
                 replay_safe = ?7, resource = ?8, idempotency_key = ?13,
                 external_reference = ?14, replay_contract = ?15,
                 completed_at = CASE WHEN ?9 THEN ?10 ELSE completed_at END
             WHERE run_id = ?11 AND tool_call_id = ?12
               AND (status NOT IN ('cancelled', 'uncertain') OR ?6 = 'uncertain')",
            rusqlite::params![
                category,
                target,
                serde_json::to_string(summary)
                    .map_err(|error| format!("serialize side-effect summary: {error}"))?,
                turn_id,
                category,
                status,
                if replay_safe { 1 } else { 0 },
                target,
                terminal,
                now,
                run_id,
                tool_call_id,
                idempotency_key,
                external_reference,
                replay_contract,
            ],
        )
        .map_err(|e| e.to_string())?;
    if changed > 0 {
        return Ok(());
    }
    conn.execute(
        "INSERT INTO side_effect_record
         (id, run_id, tool_call_id, category, target_summary, reversible, coverage_note,
          turn_id, side_effect_class, status, replay_safe, idempotency_key, external_reference,
          resource, started_at, completed_at, ledger_sequence, replay_contract)
         VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
                 CASE WHEN ?15 THEN ?14 ELSE NULL END,
                 (SELECT COALESCE(MAX(ledger_sequence),0)+1 FROM side_effect_record WHERE run_id = ?2),
                 ?16)",
        rusqlite::params![
            uuid::Uuid::new_v4().to_string(),
            run_id,
            tool_call_id,
            category,
            target,
            serde_json::to_string(summary)
                .map_err(|error| format!("serialize side-effect summary: {error}"))?,
            turn_id,
            category,
            status,
            if replay_safe { 1 } else { 0 },
            idempotency_key,
            external_reference,
            target,
            now,
            terminal,
            replay_contract,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Query the durable side-effect watermark for a run: the highest ledger
/// sequence recorded. This is what the checkpoint cursor must store — never
/// the run_event sequence (G01). A run with no recorded side effects has a
/// watermark of `"0"` (nothing happened, safe).
pub fn ledger_watermark(run_id: &str) -> Result<Option<String>, String> {
    let store = ledger_store()?;
    let conn = store.conn()?;
    let max: Option<i64> = conn
        .query_row(
            "SELECT MAX(ledger_sequence) FROM side_effect_record WHERE run_id = ?1",
            rusqlite::params![run_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    Ok(Some(max.unwrap_or(0).to_string()))
}

/// Aggregate coverage label for restore preview.
pub fn coverage_for_run(run_id: &str) -> Result<String, String> {
    let store = ledger_store()?;
    let conn = store.conn()?;
    let mut stmt = conn
        .prepare("SELECT category FROM side_effect_record WHERE run_id = ?1")
        .map_err(|e| e.to_string())?;
    let cats: Vec<String> = stmt
        .query_map(rusqlite::params![run_id], |r| r.get(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    if cats.is_empty() {
        return Ok("unknown".into());
    }
    let has_external = cats.iter().any(|c| {
        matches!(
            c.as_str(),
            "process" | "network" | "mcp" | "external" | "git" | "database"
        )
    });
    let has_workspace = cats.iter().any(|c| c == "workspace_file");
    if has_external && has_workspace {
        Ok("partial".into())
    } else if has_external {
        Ok("external_only".into())
    } else if has_workspace {
        Ok("checkpoint".into())
    } else {
        Ok("unknown".into())
    }
}

pub fn category_for_tool(tool_name: &str) -> &'static str {
    match tool_name {
        "write_file" | "apply_patch" | "edit_file" => "workspace_file",
        "run_terminal" | "bash" => "process",
        "web_fetch" | "fetch" => "network",
        name if name == "mcp_call" || name.starts_with("mcp__") => "mcp",
        _ => "external",
    }
}

/// Replay contract for a ledger row (D03).
///
/// - `never` — workspace files are covered by checkpoint before-images, so the
///   effect is never re-run; a restore replays the captured file instead.
/// - `confirm` — the external outcome (process/network/MCP) is unknown without
///   the original handler, so re-running requires explicit confirmation and is
///   never automatic.
/// - `legacy_unverifiable` — rows written before `ledger_sequence` existed
///   have no provable ledger prefix and must not be claimed safe (G01).
pub fn replay_contract_for(category: &str) -> &'static str {
    if category == "workspace_file" {
        "never"
    } else {
        "confirm"
    }
}

/// The stable idempotency key for an effect: the engine tool-call identity
/// that produced it. A retried invocation that reuses the same call id maps to
/// the same key, which is what lets the ledger recognise a duplicate instead
/// of treating it as a fresh side effect (D03).
pub fn idempotency_key_for(tool_call_id: &str) -> String {
    tool_call_id.to_string()
}

/// Settle a `started` intent to a terminal status. Guarded so it only ever
/// moves a `started` row — it never overwrites an already-settled or
/// `uncertain` row. Returns whether a row was actually settled. Callers run
/// this inside their own transaction so the settlement commits atomically
/// with the fact that triggered it (D04).
pub fn settle_tool_effect(
    conn: &rusqlite::Connection,
    run_id: &str,
    tool_call_id: &str,
    status: &str,
    replay_safe: bool,
) -> Result<bool, String> {
    let now = chrono::Utc::now().to_rfc3339();
    let changed = conn
        .execute(
            "UPDATE side_effect_record
             SET status = ?1, replay_safe = ?2, completed_at = ?3
             WHERE run_id = ?4 AND tool_call_id = ?5 AND status = 'started'",
            rusqlite::params![
                status,
                if replay_safe { 1 } else { 0 },
                now,
                run_id,
                tool_call_id
            ],
        )
        .map_err(|e| e.to_string())?;
    Ok(changed > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_mapping() {
        assert_eq!(category_for_tool("write_file"), "workspace_file");
        assert_eq!(category_for_tool("run_terminal"), "process");
        assert_eq!(category_for_tool("mcp_call"), "mcp");
    }

    /// TASK-004 (G01): the ledger watermark is the max ledger sequence for a
    /// run — never the run_event sequence. It is what the checkpoint cursor
    /// must store instead of the last event sequence.
    #[test]
    fn ledger_watermark_is_ledger_sequence() {
        let store = ledger_store().unwrap();
        let run_id = format!("wm-{}", uuid::Uuid::new_v4());
        let conn = store.conn().unwrap();
        for i in 1..=3 {
            conn.execute(
                "INSERT INTO side_effect_record
                 (id, run_id, category, status, replay_safe, ledger_sequence)
                 VALUES (?1, ?2, 'process', 'completed', 1, ?3)",
                rusqlite::params![format!("{run_id}-e{i}"), run_id, i],
            )
            .unwrap();
        }
        drop(conn);
        let wm = ledger_watermark(&run_id).unwrap();
        assert_eq!(
            wm.as_deref(),
            Some("3"),
            "watermark must be the max ledger sequence, got {wm:?}"
        );
    }

    /// TASK-004 (G01): each recorded state for a run advances the ledger
    /// sequence monotonically, so the watermark is a real ledger prefix.
    #[test]
    fn ledger_sequence_advances_per_run() {
        let run_id = format!("seq-{}", uuid::Uuid::new_v4());
        record_tool_effect_state(
            &run_id,
            "call-1",
            "write_file",
            "workspace_file",
            "completed",
            true,
            None,
            &serde_json::json!({"path": "a.txt"}),
        )
        .unwrap();
        record_tool_effect_state(
            &run_id,
            "call-2",
            "run_terminal",
            "process",
            "completed",
            true,
            None,
            &serde_json::json!({"command": "true"}),
        )
        .unwrap();
        let wm = ledger_watermark(&run_id).unwrap();
        assert_eq!(
            wm.as_deref(),
            Some("2"),
            "two effects must advance the ledger to sequence 2, got {wm:?}"
        );
    }

    /// D03: every ledger intent carries the stable engine call id as its
    /// idempotency key, the external target as its external reference, and a
    /// per-category replay contract (never for checkpoint-covered workspace,
    /// confirm otherwise).
    #[test]
    fn intent_populates_idempotency_external_reference_and_replay_contract() {
        let run_id = format!("d03-{}", uuid::Uuid::new_v4());
        record_tool_effect_state(
            &run_id,
            "call-1",
            "write_file",
            "workspace_file",
            "started",
            false,
            None,
            &serde_json::json!({"path": "/tmp/a.txt"}),
        )
        .unwrap();
        record_tool_effect_state(
            &run_id,
            "call-2",
            "run_terminal",
            "process",
            "started",
            false,
            None,
            &serde_json::json!({"command": "make build"}),
        )
        .unwrap();
        let store = ledger_store().unwrap();
        let conn = store.conn().unwrap();
        let (key, reference, contract): (Option<String>, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT idempotency_key, external_reference, replay_contract
                 FROM side_effect_record WHERE run_id = ?1 AND tool_call_id = 'call-1'",
                rusqlite::params![run_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            key.as_deref(),
            Some("call-1"),
            "workspace intent keeps the stable call id as its idempotency key"
        );
        assert_eq!(reference.as_deref(), Some("/tmp/a.txt"));
        assert_eq!(contract.as_deref(), Some("never"));
        let (key, reference, contract): (Option<String>, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT idempotency_key, external_reference, replay_contract
                 FROM side_effect_record WHERE run_id = ?1 AND tool_call_id = 'call-2'",
                rusqlite::params![run_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(key.as_deref(), Some("call-2"));
        assert_eq!(reference.as_deref(), Some("make build"));
        assert_eq!(contract.as_deref(), Some("confirm"));
    }

    /// D04: settlement only ever moves a `started` intent — never an
    /// already-settled row and never an `uncertain` row.
    #[test]
    fn settle_tool_effect_only_moves_started_rows() {
        let run_id = format!("d04-{}", uuid::Uuid::new_v4());
        record_tool_effect_state(
            &run_id,
            "call-a",
            "write_file",
            "workspace_file",
            "started",
            false,
            None,
            &serde_json::json!({"path": "a.txt"}),
        )
        .unwrap();
        record_tool_effect_state(
            &run_id,
            "call-b",
            "run_terminal",
            "process",
            "uncertain",
            false,
            None,
            &serde_json::json!({"command": "x"}),
        )
        .unwrap();
        let store = ledger_store().unwrap();
        let conn = store.conn().unwrap();
        // Started → settled.
        assert!(
            settle_tool_effect(&conn, &run_id, "call-a", "completed", true).unwrap(),
            "a started intent must settle to its terminal status"
        );
        // Already settled → no-op, never double-settled.
        assert!(
            !settle_tool_effect(&conn, &run_id, "call-a", "failed", false).unwrap(),
            "an already-settled row must not be re-settled"
        );
        // Uncertain → never overwritten by settlement.
        assert!(
            !settle_tool_effect(&conn, &run_id, "call-b", "completed", true).unwrap(),
            "settlement must not promote an uncertain effect to settled"
        );
        drop(conn);
        let wm = ledger_watermark(&run_id).unwrap();
        assert_eq!(wm.as_deref(), Some("2"));
    }
}
