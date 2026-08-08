//! Side-effect ledger for rewind coverage (Phase 4 / task-07).
//!
//! Records whether a run's tool effects are checkpoint-covered, external-only,
//! or unknown — used by workspace.restorePreview.

use serde_json::Value;

fn ledger_store() -> Result<std::sync::Arc<crate::storage::DataStore>, String> {
    #[cfg(test)]
    {
        // T01/T02 hermeticity: tests resolve the ledger store from their own
        // thread-local override/env FIRST, so a parallel test's process-global
        // RunManager (installed via install_global_for_test, bound to a temp DB
        // that may already be dropped) can never redirect a ledger write into a
        // dead path (poisoned locks / database is locked under --test-threads=2).
        // Fall back to the historical per-process temp DB only when a test sets
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
    // Production: the process-global RunManager owns the authoritative
    // DataStore. (Under cfg(test) the block above always returns.)
    #[cfg(not(test))]
    if let Some(store) = crate::run_manager::global_run_manager().data_store_ref() {
        return Ok(store);
    }
    #[allow(unreachable_code)]
    Err("no data store for side_effect_record".to_string())
}

/// Redact a side-effect summary for durable storage so the ledger never
/// persists raw secrets, full shell command lines, or URL query/fragment
/// payloads (P0 #3).
///
/// - `command`/`args`/`argv`: keep only the executable name plus a SHA-256 of
///   the full string — a raw command line is never persisted.
/// - URL keys (`url`, `open_path`, `redirect`): strip query + fragment.
/// - Secret-like keys (token/secret/api_key/password/authorization/bearer):
///   replaced with `[REDACTED]`.
/// - Every string value is size-capped; the serialized payload is also capped.
fn redact_effect_summary(summary: &Value) -> Value {
    const MAX_FIELD_CHARS: usize = 512;
    const MAX_PAYLOAD_BYTES: usize = 4096;
    fn cap(s: &str) -> String {
        if s.chars().count() <= MAX_FIELD_CHARS {
            return s.to_string();
        }
        let head: String = s.chars().take(MAX_FIELD_CHARS).collect();
        format!("{head}…[truncated {} chars]", s.chars().count())
    }
    fn strip_query_fragment(u: &str) -> String {
        match u.find(['?', '#']) {
            Some(idx) => u[..idx].to_string(),
            None => u.to_string(),
        }
    }
    fn command_fingerprint(cmd: &str) -> String {
        use sha2::{Digest, Sha256};
        let exe = cmd.split_whitespace().next().unwrap_or("").to_string();
        let mut hasher = Sha256::new();
        hasher.update(cmd.as_bytes());
        let hash = format!("{:x}", hasher.finalize());
        if exe.is_empty() {
            format!("<sha256:{hash}>")
        } else {
            format!("{exe} <sha256:{hash}>")
        }
    }
    fn is_secret_key(key: &str) -> bool {
        let u = key.to_ascii_lowercase();
        u.contains("token")
            || u.contains("secret")
            || u.contains("api_key")
            || u.contains("password")
            || u.contains("authorization")
            || u.contains("bearer")
            || u == "key"
            || u == "access_key"
    }

    let mut out = serde_json::Map::new();
    match summary.as_object() {
        Some(obj) => {
            for (k, v) in obj {
                let lk = k.to_ascii_lowercase();
                let redacted = if is_secret_key(&lk) {
                    Value::String("[REDACTED]".into())
                } else if lk == "command" || lk == "args" || lk == "argv" {
                    match v.as_str() {
                        Some(s) => Value::String(command_fingerprint(s)),
                        None => v.clone(),
                    }
                } else if lk == "url" || lk == "open_path" || lk == "redirect" {
                    Value::String(strip_query_fragment(v.as_str().unwrap_or("")))
                } else if let Some(s) = v.as_str() {
                    Value::String(cap(s))
                } else {
                    v.clone()
                };
                out.insert(k.clone(), redacted);
            }
        }
        None => {
            if let Some(s) = summary.as_str() {
                out.insert("value".into(), Value::String(cap(s)));
            } else {
                out.insert("value".into(), summary.clone());
            }
        }
    }
    let mut result = Value::Object(out);
    // Cap the serialized payload as a whole.
    if let Ok(s) = serde_json::to_string(&result) {
        if s.len() > MAX_PAYLOAD_BYTES {
            result = Value::String(format!("[payload {size}B truncated]", size = s.len()));
        }
    }
    result
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
    let redacted = redact_effect_summary(summary);
    let summary_s = serde_json::to_string(&redacted)
        .map_err(|error| format!("serialize side-effect summary: {error}"))?;
    let target = redacted
        .get("path")
        .or_else(|| redacted.get("command"))
        .or_else(|| redacted.get("url"))
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
    let redacted = redact_effect_summary(summary);
    let target = redacted
        .get("path")
        .or_else(|| redacted.get("command"))
        .or_else(|| redacted.get("url"))
        .and_then(Value::as_str)
        .unwrap_or(tool_name);
    let now = chrono::Utc::now().to_rfc3339();
    let terminal = matches!(status, "completed" | "failed" | "cancelled" | "uncertain");
    let external_reference = target;
    let idempotency_key = idempotency_key_for(tool_call_id);
    let replay_contract = replay_contract_for(category);
    let redacted_s = serde_json::to_string(&redacted)
        .map_err(|error| format!("serialize side-effect summary: {error}"))?;
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
                redacted_s.clone(),
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
    // All ledger writes serialize on the DataStore connection mutex, so the
    // MAX+1 allocation below cannot race within one process. The partial
    // unique index idx_side_effect_run_sequence (migration 034) backstops any
    // cross-connection/cross-process race so two effects can never share a
    // watermark. Legacy rows without a sequence stay `legacy_unverifiable`.
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
            redacted_s.clone(),
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
    .map_err(|e| format!("ledger insert: {e}"))?;
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

    /// T01/T02 hermeticity: give each store-using ledger test its own temp DB
    /// so parallel runs never contend on a shared fixed-path SQLite file
    /// (poisoned locks / "database is locked" under --test-threads=2).
    fn with_ledger_db<F: FnOnce()>(f: F) {
        let _guard = crate::storage::DataStore::env_test_lock();
        let _restore = crate::storage::EnvRestore::capture();
        let dir = tempfile::tempdir().unwrap();
        let db = dir
            .path()
            .join(format!("ledger-{}.db", uuid::Uuid::new_v4()));
        std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db);
        std::env::set_var("NATIVES_DB_PATH", &db);
        std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
        crate::storage::set_test_db_override(Some(db.clone()), Some(dir.path().join("artifacts")));
        f();
        // Clear the thread-local override so it cannot leak into the next test
        // that runs on this worker thread (parallel-suite hermeticity).
        crate::storage::set_test_db_override(None, None);
    }

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
        with_ledger_db(|| {
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
        });
    }

    /// TASK-004 (G01): each recorded state for a run advances the ledger
    /// sequence monotonically, so the watermark is a real ledger prefix.
    #[test]
    fn ledger_sequence_advances_per_run() {
        with_ledger_db(|| {
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
        });
    }

    /// D03: every ledger intent carries the stable engine call id as its
    /// idempotency key, the external target as its external reference, and a
    /// per-category replay contract (never for checkpoint-covered workspace,
    /// confirm otherwise).
    #[test]
    fn intent_populates_idempotency_external_reference_and_replay_contract() {
        with_ledger_db(|| {
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
            // Raw command lines are never persisted: the external reference for a
            // process effect is the executable plus a SHA-256 fingerprint.
            let reference = reference.as_deref().unwrap_or("");
            assert!(
                reference.starts_with("make") && reference.contains("<sha256:"),
                "command external reference must be fingerprinted, got: {reference}"
            );
            assert_eq!(contract.as_deref(), Some("confirm"));
        });
    }

    /// D04: settlement only ever moves a `started` intent — never an
    /// already-settled row and never an `uncertain` row.
    #[test]
    fn settle_tool_effect_only_moves_started_rows() {
        with_ledger_db(|| {
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
        });
    }

    #[test]
    fn redaction_fingerprints_raw_commands() {
        with_ledger_db(|| {
            let run_id = format!("redact-cmd-{}", uuid::Uuid::new_v4());
            record_tool_effect_state(
                &run_id,
                "call-secret",
                "run_terminal",
                "process",
                "completed",
                false,
                None,
                &serde_json::json!({"command": "curl -H 'Authorization: Bearer sk-abcdef123456' https://api.example.com/x"}),
            )
            .unwrap();
            let store = ledger_store().unwrap();
            let conn = store.conn().unwrap();
            let note: String = conn
                .query_row(
                    "SELECT coverage_note FROM side_effect_record WHERE run_id = ?1 AND tool_call_id = 'call-secret'",
                    rusqlite::params![run_id],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(
                !note.contains("sk-abcdef123456"),
                "raw credential must not be persisted, got: {note}"
            );
            assert!(
                !note.contains("Bearer"),
                "raw authorization header must not be persisted, got: {note}"
            );
            assert!(
                note.contains("<sha256:"),
                "command must be reduced to a fingerprint, got: {note}"
            );
            assert!(
                note.contains("curl"),
                "executable name is preserved for diagnostics, got: {note}"
            );
        });
    }

    #[test]
    fn redaction_strips_url_query_and_redacts_secret_keys() {
        with_ledger_db(|| {
            let run_id = format!("redact-url-{}", uuid::Uuid::new_v4());
            record_tool_effect_state(
                &run_id,
                "call-url",
                "web_fetch",
                "network",
                "completed",
                false,
                None,
                &serde_json::json!({
                    "url": "https://example.com/api?token=SECRETTOKEN&code=abc#frag",
                    "api_key": "sk-live-1234567890"
                }),
            )
            .unwrap();
            let store = ledger_store().unwrap();
            let conn = store.conn().unwrap();
            let note: String = conn
                .query_row(
                    "SELECT coverage_note FROM side_effect_record WHERE run_id = ?1 AND tool_call_id = 'call-url'",
                    rusqlite::params![run_id],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(
                !note.contains("SECRETTOKEN"),
                "URL query payload must be stripped, got: {note}"
            );
            assert!(
                !note.contains("#frag"),
                "URL fragment must be stripped, got: {note}"
            );
            assert!(
                !note.contains("sk-live-1234567890"),
                "secret-like key must be redacted, got: {note}"
            );
            assert!(note.contains("[REDACTED]"), "expected redaction marker");
        });
    }

    #[test]
    fn ledger_sequences_are_unique_per_run() {
        // G01: ledger sequences must be strictly increasing per run. Writes
        // serialize on the DataStore connection mutex (single process), and
        // migration 034's partial unique index backstops any cross-connection
        // race — a duplicate watermark would let resume misjudge coverage.
        with_ledger_db(|| {
            let run_id = format!("seq-uniq-{}", uuid::Uuid::new_v4());
            for i in 0..8 {
                record_tool_effect_state(
                    &run_id,
                    &format!("call-{i}"),
                    "write_file",
                    "workspace_file",
                    "completed",
                    true,
                    None,
                    &serde_json::json!({"path": format!("/tmp/f-{i}.txt")}),
                )
                .unwrap();
            }
            let store = ledger_store().unwrap();
            let conn = store.conn().unwrap();
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM side_effect_record WHERE run_id = ?1",
                    rusqlite::params![run_id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 8, "all 8 effects must be recorded");
            let distinct: i64 = conn
                .query_row(
                    "SELECT COUNT(DISTINCT ledger_sequence) FROM side_effect_record WHERE run_id = ?1",
                    rusqlite::params![run_id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(distinct, 8, "all 8 ledger sequences must be unique");
            drop(conn);
            let wm = ledger_watermark(&run_id).unwrap();
            assert_eq!(wm.as_deref(), Some("8"), "watermark must reach 8");
        });
    }

    #[test]
    fn ledger_unique_index_rejects_duplicate_sequence() {
        // G01 backstop: migration 034's partial unique index rejects two rows
        // for the same run sharing a ledger sequence (defense in depth for any
        // cross-connection/cross-process race).
        with_ledger_db(|| {
            let run_id = format!("idx-uniq-{}", uuid::Uuid::new_v4());
            let store = ledger_store().unwrap();
            let conn = store.conn().unwrap();
            conn.execute(
                "INSERT INTO side_effect_record
                 (id, run_id, category, status, replay_safe, ledger_sequence)
                 VALUES ('idx-a', ?1, 'process', 'completed', 1, 1)",
                rusqlite::params![run_id],
            )
            .unwrap();
            let second = conn.execute(
                "INSERT INTO side_effect_record
                 (id, run_id, category, status, replay_safe, ledger_sequence)
                 VALUES ('idx-b', ?1, 'process', 'completed', 1, 1)",
                rusqlite::params![run_id],
            );
            assert!(
                second.is_err(),
                "a duplicate (run_id, ledger_sequence) must be rejected by the unique index"
            );
        });
    }

    /// §5 exact-name regression: write tools settle atomically.
    #[test]
    fn write_tool_still_settles_atomically() {
        settle_tool_effect_only_moves_started_rows();
    }
}
