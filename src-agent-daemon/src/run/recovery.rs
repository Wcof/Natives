//! Startup interruption and snapshot restore.

use super::lifecycle::{parse_db_time, run_status_from_db};
use super::manager::RunManager;
use assistant_protocol::v2::{RunEventKind, RunEventV2, RunStatusV2, RunV2};
use std::collections::HashMap;

impl RunManager {
    /// Startup recovery (task-04): one transaction, no Engine Future restart.
    ///
    /// 1. Active runs → Interrupted (daemon_restarted)
    /// 2. Related pending interactions → expired
    /// 3. Related permission_request → expired
    /// 4. session_actor active/pending fields cleared
    pub(crate) fn interrupt_active_sqlite_runs(&self) -> Result<usize, String> {
        let Some(store) = &self.data_store else {
            return Ok(0);
        };
        let (changed, interrupted_events) = {
            let conn = store.conn()?;
            let tx = conn
                .unchecked_transaction()
                .map_err(|e| format!("PERSISTENCE_FAILED begin recovery tx: {e}"))?;
            let now = chrono::Utc::now().to_rfc3339();
            let reason = r#"{"reason":"daemon_restarted"}"#;

            // Collect active run ids first (for interaction/permission expiry filters).
            let active_ids: Vec<String> = {
                let mut stmt = tx
                    .prepare(
                        "SELECT id FROM run WHERE status IN (
                        'created', 'queued', 'preparing', 'running', 'waiting_permission',
                        'waiting_subagent', 'cancelling'
                     )",
                    )
                    .map_err(|e| e.to_string())?;
                let rows = stmt
                    .query_map([], |row| row.get::<_, String>(0))
                    .map_err(|e| e.to_string())?;
                rows.collect::<Result<Vec<_>, _>>()
                    .map_err(|e| format!("PERSISTENCE_FAILED read active runs: {e}"))?
            };

            let changed = tx
                .execute(
                    "UPDATE run
                 SET status = 'interrupted',
                     error_code = 'daemon_restarted',
                     finished_at = ?1,
                     revision = COALESCE(revision, 0) + 1
                 WHERE status IN (
                    'created', 'queued', 'preparing', 'running', 'waiting_permission',
                    'waiting_subagent', 'cancelling'
                 )",
                    rusqlite::params![now],
                )
                .map_err(|e| format!("PERSISTENCE_FAILED interrupt active runs: {e}"))?;

            let mut interrupted_events = Vec::with_capacity(active_ids.len());
            for run_id in &active_ids {
                let sequence: i64 = tx
                    .query_row(
                        "SELECT COALESCE(MAX(sequence), 0) + 1 FROM run_event WHERE run_id = ?1",
                        rusqlite::params![run_id],
                        |row| row.get(0),
                    )
                    .map_err(|e| format!("PERSISTENCE_FAILED allocate recovery event: {e}"))?;
                let mut event = RunEventV2::new(
                    run_id,
                    sequence as u64,
                    RunEventKind::Interrupted {
                        reason: "daemon_restarted".into(),
                    },
                );
                let payload = serde_json::to_string(&event)
                    .map_err(|e| format!("PERSISTENCE_FAILED serialize recovery event: {e}"))?;
                tx.execute(
                    "INSERT INTO run_event (run_id, sequence, event_type, payload, timestamp, event_id)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    rusqlite::params![
                        run_id,
                        sequence,
                        event.payload.type_name(),
                        payload,
                        event.timestamp.to_rfc3339(),
                        event.event_id,
                    ],
                )
                .map_err(|e| format!("PERSISTENCE_FAILED append recovery event: {e}"))?;
                event.global_sequence = tx.last_insert_rowid() as u64;
                interrupted_events.push(event);
            }

            // Expire pending interactions for those runs (history retained, listPending hides).
            if !active_ids.is_empty() {
                for rid in &active_ids {
                    tx.execute(
                        "UPDATE interaction
                     SET status = 'expired',
                         response = ?1,
                         responded_at = ?2
                     WHERE status = 'pending' AND run_id = ?3",
                        rusqlite::params![reason, now, rid],
                    )
                    .map_err(|e| format!("PERSISTENCE_FAILED expire interaction: {e}"))?;
                    tx.execute(
                        "UPDATE permission_request
                     SET status = 'expired'
                     WHERE status = 'pending' AND run_id = ?1",
                        rusqlite::params![rid],
                    )
                    .map_err(|e| format!("PERSISTENCE_FAILED expire permission request: {e}"))?;
                }
            } else {
                // Still expire any pending interactions whose run is already interrupted/missing.
                tx.execute(
                    "UPDATE interaction
                 SET status = 'expired', response = ?1, responded_at = ?2
                 WHERE status = 'pending'
                   AND (run_id IS NULL OR run_id IN (
                   SELECT id FROM run WHERE status = 'interrupted'
                            AND error_code = 'daemon_restarted'
                   ))",
                    rusqlite::params![reason, now],
                )
                .map_err(|e| format!("PERSISTENCE_FAILED expire stale interactions: {e}"))?;
            }

            // Clear session actor live pointers (no auto re-exec).
            tx.execute(
                "UPDATE session_actor
             SET active_run_id = NULL,
                 running_prompt_id = NULL,
                 pending_interaction_id = NULL,
                 cancel_and_send_id = NULL,
                 cancel_requested = 0,
                 updated_at = ?1",
                rusqlite::params![now],
            )
            .map_err(|e| format!("PERSISTENCE_FAILED clear session actors: {e}"))?;

            if changed != interrupted_events.len() {
                return Err("PERSISTENCE_FAILED recovery run/event mismatch".into());
            }
            tx.commit()
                .map_err(|e| format!("PERSISTENCE_FAILED commit recovery tx: {e}"))?;
            (changed, interrupted_events)
        };
        for event in interrupted_events {
            self.runtime.events.inject_committed(event);
        }
        Ok(changed)
    }
    fn runs_snapshot_path() -> std::path::PathBuf {
        let root = std::env::var("NATIVES_RUNTIME_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| {
                #[cfg(test)]
                {
                    // T01 hermeticity: never default the runs snapshot to
                    // ~/.natives in tests. A per-process temp dir keeps tests
                    // from reading or writing the developer's home directory.
                    std::env::temp_dir().join(format!("natives-runtime-{}", std::process::id()))
                }
                #[cfg(not(test))]
                {
                    std::env::var_os("HOME")
                        .or_else(|| std::env::var_os("USERPROFILE"))
                        .map(|h| std::path::PathBuf::from(h).join(".natives").join("runtime"))
                        .unwrap_or_else(|| std::env::temp_dir().join("natives-runtime"))
                }
            });
        root.join("runs").join("snapshot.json")
    }
    fn snapshot_path(&self) -> std::path::PathBuf {
        self.snapshot_path_override
            .clone()
            .unwrap_or_else(Self::runs_snapshot_path)
    }
    /// Persist run rows for restart recovery (best-effort).
    pub fn persist_runs_snapshot(&self) -> Result<(), String> {
        let path = self.snapshot_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let runs = self.runs.lock().map_err(|e| e.to_string())?;
        let raw = serde_json::to_string_pretty(&*runs).map_err(|e| e.to_string())?;
        std::fs::write(path, raw).map_err(|e| e.to_string())
    }
    /// Bind the workspace checkpoint to the run before execution begins.
    pub fn set_run_checkpoint_id(&self, run_id: &str, checkpoint_id: &str) -> Result<(), String> {
        let mut runs = self.runs.lock().map_err(|e| e.to_string())?;
        let run = runs
            .get_mut(run_id)
            .ok_or_else(|| format!("run not found: {run_id}"))?;
        run.checkpoint_id = Some(checkpoint_id.to_string());
        let snapshot = run.clone();
        drop(runs);
        self.persist_run_row(&snapshot)?;
        self.persist_runs_snapshot()
    }
    /// Hydrate the in-memory run map from SQLite (the single persistence
    /// authority). `snapshot.json` is NOT a truth source (P0-027): durable runs
    /// live in the `run` table; a missing/corrupt snapshot must never lose a
    /// durable run or block daemon startup.
    pub fn restore_runs_snapshot(&self) -> Result<usize, String> {
        if self.data_store.is_some() {
            return self.hydrate_runs_from_store();
        }
        // Memory-only mode (tests / no configured store): a snapshot file is an
        // optional, disposable cache. A missing or corrupt snapshot is NOT an
        // error here — it simply means nothing to restore.
        let path = self.snapshot_path();
        let Ok(raw) = std::fs::read_to_string(&path) else {
            return Ok(0);
        };
        let Ok(loaded) = serde_json::from_str::<HashMap<String, RunV2>>(&raw) else {
            return Ok(0);
        };
        let mut count = 0;
        let mut runs = self.runs.lock().map_err(|e| e.to_string())?;
        for (id, mut run) in loaded {
            if run.status.is_active() || run.status == RunStatusV2::Queued {
                // Safe recovery: surface as interrupted so UI can retry (no silent re-exec).
                run.status = RunStatusV2::Interrupted;
                run.error_code = Some("daemon_restarted".into());
                run.finished_at = Some(chrono::Utc::now());
            }
            if let Some(pp) = run.project_path.clone() {
                if let Ok(mut map) = self.project_paths.lock() {
                    map.insert(id.clone(), std::path::PathBuf::from(pp));
                }
            }
            runs.insert(id, run);
            count += 1;
        }
        Ok(count)
    }
    /// Restore durable runs from SQLite — the canonical restart source.
    /// Active/queued rows are surfaced as `interrupted` (safe resume, no
    /// silent re-exec); the same semantics the old snapshot restore applied,
    /// but reading from the authoritative `run` table instead of a JSON cache.
    fn hydrate_runs_from_store(&self) -> Result<usize, String> {
        let Some(store) = &self.data_store else {
            return Ok(0);
        };
        let conn = store.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, conversation_id, status, parent_run_id, agent_profile_id,
                        provider_id, key_id, model_id, permission_profile,
                        trigger_message_id, started_at, finished_at, error_code,
                        step_count, max_steps, project_path, retry_count,
                        created_at, idempotency_key, COALESCE(revision, 0),
                        retry_of_run_id, retry_of_turn_id, continued_from_run_id,
                        branch_id, branch_parent_message_id, checkpoint_id, resume_of_run_id,
                        capability_snapshot_json, project_id, project_identity_version,
                        effort, runtime_id
                 FROM run ORDER BY created_at",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok(RunV2 {
                    capability_snapshot: row
                        .get::<_, Option<String>>(27)
                        .ok()
                        .flatten()
                        .and_then(|s| serde_json::from_str(&s).ok()),
                    id: row.get(0)?,
                    conversation_id: row.get(1)?,
                    status: run_status_from_db(&row.get::<_, String>(2)?),
                    parent_run_id: row.get(3)?,
                    agent_profile_id: row.get(4)?,
                    provider_id: row.get(5)?,
                    key_id: row.get(6)?,
                    model_id: row.get(7)?,
                    permission_profile: row.get(8)?,
                    trigger_message_id: row.get(9)?,
                    started_at: parse_db_time(row.get::<_, Option<String>>(10)?),
                    finished_at: parse_db_time(row.get::<_, Option<String>>(11)?),
                    error_code: row.get(12)?,
                    step_count: row.get::<_, i64>(13)? as u32,
                    max_steps: row.get::<_, i64>(14)? as u32,
                    project_path: row.get(15)?,
                    retry_count: row.get::<_, i64>(16)? as u32,
                    created_at: parse_db_time(row.get::<_, Option<String>>(17)?),
                    last_event_sequence: 0,
                    idempotency_key: row.get(18)?,
                    effort: row.get(30)?,
                    runtime_id: row.get(31)?,
                    revision: row.get::<_, i64>(19).unwrap_or(0) as u64,
                    project_id: row.get(28)?,
                    project_identity_version: row.get(29)?,
                    retry_of_run_id: row.get(20)?,
                    retry_of_turn_id: row.get(21)?,
                    continued_from_run_id: row.get(22)?,
                    branch_id: row.get(23)?,
                    branch_parent_message_id: row.get(24)?,
                    checkpoint_id: row.get(25)?,
                    resume_of_run_id: row.get(26)?,
                })
            })
            .map_err(|e| e.to_string())?;

        let mut count = 0;
        let mut runs = self.runs.lock().map_err(|e| e.to_string())?;
        for row in rows {
            let mut run = row.map_err(|e| e.to_string())?;
            if run.status.is_active() || run.status == RunStatusV2::Queued {
                // Safe recovery: surface as interrupted so UI can retry (no silent re-exec).
                run.status = RunStatusV2::Interrupted;
                run.error_code = Some("daemon_restarted".into());
                run.finished_at = Some(chrono::Utc::now());
            }
            if let Some(pp) = run.project_path.clone() {
                if let Ok(mut map) = self.project_paths.lock() {
                    map.insert(run.id.clone(), std::path::PathBuf::from(pp));
                }
            }
            runs.insert(run.id.clone(), run);
            count += 1;
        }
        Ok(count)
    }
}
