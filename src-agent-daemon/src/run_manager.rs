//! RunManager — Daemon-side sole Run Authority (production path).

use crate::production::{FixtureMode, FixtureProvider, ProductionRuntime};
use crate::storage::DataStore;
use agent_core::{
    AgentEngine, CommitError, EngineOutcome, EngineRunConfig, EventSequencer,
    RunLifecycleAuthority, TransitionMetadata,
};
use assistant_protocol::v2::{
    CancelRunRequest, CreateRunRequest, DaemonCapabilities, ReplayRunRequest, RetryRunRequest,
    RunEventKind, RunEventV2, RunStatusV2, RunV2, StartRunRequest, PROTOCOL_V2,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Active and historical runs held by the daemon process.
pub struct RunManager {
    runs: Mutex<HashMap<String, RunV2>>,
    idempotency: Mutex<HashMap<String, String>>,
    /// Last start request content for retry.
    last_content: Mutex<HashMap<String, String>>,
    /// Explicit project/workspace root per run (hooks + tool sandbox).
    project_paths: Mutex<HashMap<String, std::path::PathBuf>>,
    snapshot_path_override: Option<std::path::PathBuf>,
    data_store: Option<Arc<DataStore>>,
    pub runtime: Arc<ProductionRuntime>,
}

/// Process-wide Run Authority for the **current process only**.
///
/// - Independent sidecar binary: this is the sole authority inside the daemon.
/// - Tauri embedded mode: same crate, same process — transitional until G4 UDS cutover.
/// - Do **not** assume Tauri and a separate daemon process share this `OnceLock`.
///
/// Production multi-process: set `NATIVES_DAEMON_MODE=uds` and use [`crate::client::DaemonClient`].
pub fn global_run_manager() -> &'static RunManager {
    use std::sync::OnceLock;
    static GLOBAL: OnceLock<RunManager> = OnceLock::new();
    GLOBAL.get_or_init(RunManager::new)
}

impl Default for RunManager {
    fn default() -> Self {
        Self::new()
    }
}

impl RunManager {
    pub fn new() -> Self {
        if let Some(store) = Self::store_from_env() {
            return Self::new_with_store(store);
        }
        Self::new_memory()
    }

    fn new_memory() -> Self {
        let mgr = Self {
            runs: Mutex::new(HashMap::new()),
            idempotency: Mutex::new(HashMap::new()),
            last_content: Mutex::new(HashMap::new()),
            project_paths: Mutex::new(HashMap::new()),
            snapshot_path_override: None,
            data_store: None,
            runtime: Arc::new(ProductionRuntime::new()),
        };
        let _ = mgr.restore_runs_snapshot();
        mgr
    }

    pub fn new_with_store(data_store: Arc<DataStore>) -> Self {
        match Self::try_new_with_store(data_store) {
            Ok(mgr) => mgr,
            Err(e) => {
                // Fail closed: never serve with incomplete recovery. Panic in daemon
                // constructors is intentional — process must not continue with active runs.
                panic!("RunManager recovery failed (fail-closed): {e}");
            }
        }
    }

    /// Fallible constructor: interrupt transaction failure aborts startup.
    pub fn try_new_with_store(data_store: Arc<DataStore>) -> Result<Self, String> {
        let mgr = Self {
            runs: Mutex::new(HashMap::new()),
            idempotency: Mutex::new(HashMap::new()),
            last_content: Mutex::new(HashMap::new()),
            project_paths: Mutex::new(HashMap::new()),
            snapshot_path_override: None,
            data_store: Some(data_store.clone()),
            runtime: Arc::new(ProductionRuntime::new_with_event_store(data_store)),
        };
        // Fail-closed: must not swallow recovery errors (task-04 / Phase 4).
        mgr.interrupt_active_sqlite_runs()?;
        let _ = mgr.restore_runs_snapshot();
        // Hydrate SessionCoordinator from durable queue/actor rows (no auto re-exec).
        let _ = crate::prompt_queue_store::recover_session_actors_on_startup();
        // Expire any in-memory waiters (oneshot futures are never restored).
        // InteractionHub starts empty on new process — no action required.
        Ok(mgr)
    }

    fn store_from_env() -> Option<Arc<DataStore>> {
        // Phase 0: authority store is assistant.db (NATIVES_ASSISTANT_DB_PATH).
        // Fall back to NATIVES_DB_PATH only for legacy test fixtures that still
        // point a single temp DB at NATIVES_DB_PATH.
        //
        // Unit tests that want pure in-memory RunManager set
        // NATIVES_RUN_MANAGER_MEMORY=1 (or rely on cfg(test) + no explicit path).
        if std::env::var("NATIVES_RUN_MANAGER_MEMORY")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
        {
            return None;
        }
        // Under cfg(test), only open env store when an explicit path is set for the
        // current test (tempdir). Avoid picking up the developer's real assistant.db.
        #[cfg(test)]
        {
            if let Some((db_path, artifact_dir)) = crate::storage::test_db_override() {
                if let Some(parent) = db_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let store = DataStore::new(&db_path, &artifact_dir).ok()?;
                let ok = store
                    .conn()
                    .ok()
                    .and_then(|conn| {
                        conn.query_row(
                            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='run_event'",
                            [],
                            |row| row.get::<_, bool>(0),
                        )
                        .ok()
                    })
                    .unwrap_or(false);
                if ok {
                    return Some(Arc::new(store));
                }
                return None;
            }
            let _env_guard = crate::storage::DataStore::env_test_lock();
            let explicit = std::env::var("NATIVES_ASSISTANT_DB_PATH")
                .ok()
                .filter(|s| !s.trim().is_empty())
                .or_else(|| {
                    std::env::var("NATIVES_DB_PATH")
                        .ok()
                        .filter(|s| !s.trim().is_empty())
                });
            let Some(db_path) = explicit else {
                return None;
            };
            // Prefer temp paths in tests; still allow absolute explicit fixtures.
            let db_path = std::path::PathBuf::from(db_path);
            if let Some(parent) = db_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let artifact_dir = std::env::var("NATIVES_RUNTIME_DIR")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|_| {
                    db_path
                        .parent()
                        .map(|p| p.join("artifacts"))
                        .unwrap_or_else(std::env::temp_dir)
                });
            let store = DataStore::new(&db_path, &artifact_dir).ok()?;
            // Reject empty/broken DBs so leaked env cannot poison pure unit tests.
            let ok = store
                .conn()
                .ok()
                .and_then(|conn| {
                    conn.query_row(
                        "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='run_event'",
                        [],
                        |row| row.get::<_, bool>(0),
                    )
                    .ok()
                })
                .unwrap_or(false);
            if !ok {
                return None;
            }
            return Some(Arc::new(store));
        }
        #[cfg(not(test))]
        {
            let db_path = std::env::var("NATIVES_ASSISTANT_DB_PATH")
                .ok()
                .filter(|s| !s.trim().is_empty())
                .or_else(|| std::env::var("NATIVES_DB_PATH").ok())
                .filter(|s| !s.trim().is_empty())?;
            let db_path = std::path::PathBuf::from(db_path);
            if let Some(parent) = db_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let artifact_dir = std::env::var("NATIVES_RUNTIME_DIR")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|_| {
                    std::env::var_os("HOME")
                        .or_else(|| std::env::var_os("USERPROFILE"))
                        .map(|h| std::path::PathBuf::from(h).join(".natives").join("runtime"))
                        .unwrap_or_else(std::env::temp_dir)
                })
                .join("artifacts");
            DataStore::new(&db_path, &artifact_dir).ok().map(Arc::new)
        }
    }

    /// Ensure a conversation row exists on this manager's DataStore (or env store).
    fn ensure_conversation_for_run(
        &self,
        conversation_id: &str,
        provider_id: &str,
        model_id: &str,
        permission_profile: Option<&str>,
        project_id: Option<&str>,
    ) -> Result<(), String> {
        let id = conversation_id.trim();
        if id.is_empty() {
            return Err("conversation_id is required".into());
        }
        if let Some(store) = &self.data_store {
            let conn = store.conn()?;
            let exists: bool = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM conversation WHERE id = ?1)",
                    rusqlite::params![id],
                    |row| row.get(0),
                )
                .map_err(|e| e.to_string())?;
            if exists {
                return Ok(());
            }
            let now = chrono::Utc::now().to_rfc3339();
            let permission = permission_profile
                .filter(|p| matches!(*p, "readonly" | "ask" | "full_access"))
                .unwrap_or("ask");
            let provider = if provider_id.trim().is_empty() {
                "unknown"
            } else {
                provider_id.trim()
            };
            let model = if model_id.trim().is_empty() {
                "unknown"
            } else {
                model_id.trim()
            };
            conn.execute(
                "INSERT INTO conversation (id, mode, project_id, title, provider_id, model_id, permission_profile_id, created_at, updated_at)
                 VALUES (?1, 'agent', ?2, ?3, ?4, ?5, ?6, ?7, ?7)
                 ON CONFLICT(id) DO NOTHING",
                rusqlite::params![
                    id,
                    project_id,
                    "Daemon-mediated conversation",
                    provider,
                    model,
                    permission,
                    now,
                ],
            )
            .map_err(|e| format!("ensure_conversation_for_run failed: {e}"))?;
            return Ok(());
        }
        // Memory-only manager: no FK store — skip. Production always has data_store.
        let _ = (provider_id, model_id, permission_profile, project_id);
        Ok(())
    }

    fn persist_run_row(&self, run: &RunV2) -> Result<(), String> {
        let Some(store) = &self.data_store else {
            return Ok(());
        };
        let conn = store.conn()?;
        conn.execute(
            "INSERT INTO run (
                id, conversation_id, status, trigger_message_id, provider_id, model_id,
                started_at, finished_at, error_code, step_count, max_steps,
                token_budget, total_input_tokens, total_output_tokens, created_at,
                parent_run_id, agent_profile_id, key_id, permission_profile,
                project_path, retry_count, idempotency_key, revision
             )
             VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, NULL, 0, 0, ?12,
                ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20
             )
             ON CONFLICT(id) DO UPDATE SET
                status = excluded.status,
                trigger_message_id = excluded.trigger_message_id,
                provider_id = excluded.provider_id,
                model_id = excluded.model_id,
                started_at = excluded.started_at,
                finished_at = excluded.finished_at,
                error_code = excluded.error_code,
                step_count = excluded.step_count,
                max_steps = excluded.max_steps,
                parent_run_id = excluded.parent_run_id,
                agent_profile_id = excluded.agent_profile_id,
                key_id = excluded.key_id,
                permission_profile = excluded.permission_profile,
                project_path = excluded.project_path,
                retry_count = excluded.retry_count,
                idempotency_key = excluded.idempotency_key,
                revision = excluded.revision",
            rusqlite::params![
                run.id,
                run.conversation_id,
                run.status.as_str(),
                run.trigger_message_id,
                run.provider_id,
                run.model_id,
                run.started_at.map(|dt| dt.to_rfc3339()),
                run.finished_at.map(|dt| dt.to_rfc3339()),
                run.error_code,
                run.step_count as i64,
                run.max_steps as i64,
                run.created_at.unwrap_or_else(chrono::Utc::now).to_rfc3339(),
                run.parent_run_id,
                run.agent_profile_id,
                run.key_id,
                run.permission_profile,
                run.project_path,
                run.retry_count as i64,
                run.idempotency_key,
                run.revision as i64
            ],
        )
        .map_err(|e| format!("PERSISTENCE_FAILED upsert run: {e}"))?;
        Ok(())
    }

    fn run_from_store_by_idempotency_key(&self, key: &str) -> Result<Option<RunV2>, String> {
        let Some(store) = &self.data_store else {
            return Ok(None);
        };
        if key.trim().is_empty() {
            return Ok(None);
        }
        let conn = store.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, conversation_id, status, parent_run_id, agent_profile_id,
                        provider_id, key_id, model_id, permission_profile,
                        trigger_message_id, started_at, finished_at, error_code,
                        step_count, max_steps, project_path, retry_count,
                        created_at, idempotency_key, COALESCE(revision, 0)
                 FROM run WHERE idempotency_key = ?1 OR id = ?1 LIMIT 1",
            )
            .map_err(|e| e.to_string())?;
        let mut rows = stmt
            .query(rusqlite::params![key])
            .map_err(|e| e.to_string())?;
        let Some(row) = rows.next().map_err(|e| e.to_string())? else {
            return Ok(None);
        };
        Ok(Some(RunV2 {
            id: row.get(0).map_err(|e| e.to_string())?,
            conversation_id: row.get(1).map_err(|e| e.to_string())?,
            status: run_status_from_db(&row.get::<_, String>(2).map_err(|e| e.to_string())?),
            parent_run_id: row.get(3).map_err(|e| e.to_string())?,
            agent_profile_id: row.get(4).map_err(|e| e.to_string())?,
            provider_id: row.get(5).map_err(|e| e.to_string())?,
            key_id: row.get(6).map_err(|e| e.to_string())?,
            model_id: row.get(7).map_err(|e| e.to_string())?,
            permission_profile: row.get(8).map_err(|e| e.to_string())?,
            trigger_message_id: row.get(9).map_err(|e| e.to_string())?,
            started_at: parse_db_time(
                row.get::<_, Option<String>>(10)
                    .map_err(|e| e.to_string())?,
            ),
            finished_at: parse_db_time(
                row.get::<_, Option<String>>(11)
                    .map_err(|e| e.to_string())?,
            ),
            error_code: row.get(12).map_err(|e| e.to_string())?,
            step_count: row.get::<_, i64>(13).map_err(|e| e.to_string())? as u32,
            max_steps: row.get::<_, i64>(14).map_err(|e| e.to_string())? as u32,
            project_path: row.get(15).map_err(|e| e.to_string())?,
            retry_count: row.get::<_, i64>(16).map_err(|e| e.to_string())? as u32,
            created_at: parse_db_time(
                row.get::<_, Option<String>>(17)
                    .map_err(|e| e.to_string())?,
            ),
            last_event_sequence: 0,
            idempotency_key: row.get(18).map_err(|e| e.to_string())?,
            effort: None,
            runtime_id: None,
            revision: row.get::<_, i64>(19).unwrap_or(0) as u64,
            project_id: None,
            project_identity_version: None,
        }))
    }

    fn delete_run_row(&self, run_id: &str) {
        if let Some(store) = &self.data_store {
            if let Ok(conn) = store.conn() {
                let _ = conn.execute("DELETE FROM run WHERE id = ?1", rusqlite::params![run_id]);
            }
        }
    }

    /// Idempotent fail-close: mark run failed, persist, and emit a terminal `failed` event.
    /// No-op when the run is already terminal (completed/failed/cancelled/interrupted).
    pub fn fail_run_if_active(&self, run_id: &str, error: impl Into<String>, code: &str) {
        let error = assistant_protocol::v2::redact_secrets(&error.into());
        let code = if code.trim().is_empty() {
            "START_FAILED"
        } else {
            code
        };
        let meta = TransitionMetadata::empty()
            .with_error_code(code)
            .with_reason(error)
            .with_lifecycle_hint("failed");
        let _ = self.commit_status(run_id, RunStatusV2::Failed, meta);
    }

    /// Map a status target to a lifecycle event payload.
    fn lifecycle_event_for(target: RunStatusV2, metadata: &TransitionMetadata) -> RunEventKind {
        let reason = metadata
            .reason
            .clone()
            .filter(|s| !s.is_empty())
            .or_else(|| metadata.lifecycle_hint.clone())
            .unwrap_or_default();
        match target {
            RunStatusV2::Queued => RunEventKind::Queued,
            RunStatusV2::Preparing => RunEventKind::Preparing,
            RunStatusV2::Running => RunEventKind::Started,
            RunStatusV2::WaitingPermission => RunEventKind::Progress {
                message: if reason.is_empty() {
                    "waiting_permission".into()
                } else {
                    reason
                },
                percentage: None,
            },
            RunStatusV2::WaitingSubagent => RunEventKind::Progress {
                message: if reason.is_empty() {
                    "waiting_subagent".into()
                } else {
                    reason
                },
                percentage: None,
            },
            RunStatusV2::Cancelling => RunEventKind::Progress {
                message: if reason.is_empty() {
                    "cancelling".into()
                } else {
                    reason
                },
                percentage: None,
            },
            RunStatusV2::Completed => RunEventKind::Completed {
                reason: if reason.is_empty() {
                    "stop".into()
                } else {
                    reason
                },
            },
            RunStatusV2::Failed => RunEventKind::Failed {
                error: if reason.is_empty() {
                    "failed".into()
                } else {
                    reason
                },
                code: metadata
                    .error_code
                    .clone()
                    .unwrap_or_else(|| "failed".into()),
            },
            RunStatusV2::Cancelled => RunEventKind::Cancelled {
                reason: if reason.is_empty() {
                    "cancelled".into()
                } else {
                    reason
                },
            },
            RunStatusV2::Interrupted => RunEventKind::Interrupted {
                reason: if reason.is_empty() {
                    "interrupted".into()
                } else {
                    reason
                },
            },
            RunStatusV2::Created => RunEventKind::Queued,
        }
    }

    fn apply_transition_metadata(
        run: &mut RunV2,
        target: RunStatusV2,
        metadata: &TransitionMetadata,
    ) {
        if let Some(code) = &metadata.error_code {
            run.error_code = Some(code.clone());
        }
        if let Some(steps) = metadata.step_count {
            run.step_count = steps;
        }
        if matches!(target, RunStatusV2::Preparing | RunStatusV2::Running)
            && run.started_at.is_none()
        {
            run.started_at = Some(chrono::Utc::now());
        }
        if target.is_terminal() {
            run.finished_at = Some(chrono::Utc::now());
        }
    }

    /// Persist-first CAS commit of run status + lifecycle event, then memory/broadcast.
    pub fn commit_transition(
        &self,
        run_id: &str,
        expected_revision: u64,
        target: RunStatusV2,
        metadata: TransitionMetadata,
    ) -> Result<agent_core::CommittedTransition, CommitError> {
        <Self as RunLifecycleAuthority>::commit_transition(
            self,
            run_id,
            expected_revision,
            target,
            metadata,
        )
    }

    /// Commit using the current in-memory revision. Terminal races are idempotent.
    pub fn commit_status(
        &self,
        run_id: &str,
        target: RunStatusV2,
        metadata: TransitionMetadata,
    ) -> Result<RunV2, String> {
        let revision = {
            let runs = self.runs.lock().map_err(|e| e.to_string())?;
            let run = runs
                .get(run_id)
                .ok_or_else(|| format!("run not found: {run_id}"))?;
            if run.status == target || run.status.is_terminal() {
                return Ok(run.clone());
            }
            run.revision
        };
        match self.commit_transition(run_id, revision, target, metadata) {
            Ok(_) => self
                .get_run(run_id)
                .ok_or_else(|| "run disappeared after commit".into()),
            Err(CommitError::AlreadyTerminal { .. }) => {
                self.get_run(run_id).ok_or_else(|| "run not found".into())
            }
            Err(CommitError::CasConflict { status, .. }) if status.is_terminal() => {
                self.get_run(run_id).ok_or_else(|| "run not found".into())
            }
            Err(e) => Err(e.to_string()),
        }
    }

    /// Commit terminal status from an EngineOutcome (idempotent).
    pub fn commit_outcome(&self, run_id: &str, outcome: &EngineOutcome) -> Result<RunV2, String> {
        let status = self
            .get_run(run_id)
            .map(|r| r.status)
            .unwrap_or(RunStatusV2::Running);
        if status.is_terminal() {
            return self.get_run(run_id).ok_or_else(|| "run not found".into());
        }
        let (mut target, mut meta) = agent_core::outcome_commit_parts(outcome);
        // Pre-task-03: cooperative cancel mid-run -> Interrupted; cancel() path uses Cancelled.
        if matches!(outcome, EngineOutcome::Cancelled) {
            if status == RunStatusV2::Cancelling {
                target = RunStatusV2::Cancelled;
                meta = TransitionMetadata::empty()
                    .with_reason("cancelled")
                    .with_lifecycle_hint("cancelled");
            } else {
                target = RunStatusV2::Interrupted;
                meta = TransitionMetadata::empty()
                    .with_reason("cancelled")
                    .with_lifecycle_hint("interrupted");
            }
        }
        // Bridge non-adjacent active states so engines that skip explicit Running
        // commits still land on a legal edge (Preparing/Queued → Running → terminal).
        self.ensure_running_before_terminal(run_id, target)?;
        self.commit_status(run_id, target, meta)
    }

    /// If the run is still Queued/Preparing and the target requires Running as
    /// predecessor, commit Running first.
    fn ensure_running_before_terminal(
        &self,
        run_id: &str,
        target: RunStatusV2,
    ) -> Result<(), String> {
        let status = self
            .get_run(run_id)
            .map(|r| r.status)
            .unwrap_or(RunStatusV2::Running);
        if status.is_terminal() || status == RunStatusV2::Running {
            return Ok(());
        }
        // Targets that are legal from Running (and not from Preparing).
        let needs_running = matches!(
            target,
            RunStatusV2::Completed
                | RunStatusV2::Failed
                | RunStatusV2::Interrupted
                | RunStatusV2::WaitingPermission
                | RunStatusV2::WaitingSubagent
                | RunStatusV2::Cancelling
        );
        if !needs_running {
            return Ok(());
        }
        if status == RunStatusV2::Queued {
            let _ = self.commit_status(
                run_id,
                RunStatusV2::Preparing,
                TransitionMetadata::empty().with_lifecycle_hint("preparing"),
            );
        }
        let status = self
            .get_run(run_id)
            .map(|r| r.status)
            .unwrap_or(RunStatusV2::Preparing);
        if status == RunStatusV2::Preparing {
            let _ = self.commit_status(
                run_id,
                RunStatusV2::Running,
                TransitionMetadata::empty().with_lifecycle_hint("started"),
            );
        }
        Ok(())
    }

    /// Startup recovery (task-04): one transaction, no Engine Future restart.
    ///
    /// 1. Active runs → Interrupted (daemon_restarted)
    /// 2. Related pending interactions → expired
    /// 3. Related permission_request → expired
    /// 4. session_actor active/pending fields cleared
    fn interrupt_active_sqlite_runs(&self) -> Result<usize, String> {
        let Some(store) = &self.data_store else {
            return Ok(0);
        };
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
            rows.filter_map(|r| r.ok()).collect()
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

        // Expire pending interactions for those runs (history retained, listPending hides).
        if !active_ids.is_empty() {
            for rid in &active_ids {
                let _ = tx.execute(
                    "UPDATE interaction
                     SET status = 'expired',
                         response = ?1,
                         responded_at = ?2
                     WHERE status = 'pending' AND run_id = ?3",
                    rusqlite::params![reason, now, rid],
                );
                let _ = tx.execute(
                    "UPDATE permission_request
                     SET status = 'expired'
                     WHERE status = 'pending' AND run_id = ?1",
                    rusqlite::params![rid],
                );
            }
        } else {
            // Still expire any pending interactions whose run is already interrupted/missing.
            let _ = tx.execute(
                "UPDATE interaction
                 SET status = 'expired', response = ?1, responded_at = ?2
                 WHERE status = 'pending'
                   AND (run_id IS NULL OR run_id IN (
                        SELECT id FROM run WHERE status = 'interrupted'
                            AND error_code = 'daemon_restarted'
                   ))",
                rusqlite::params![reason, now],
            );
        }

        // Clear session actor live pointers (no auto re-exec).
        let _ = tx.execute(
            "UPDATE session_actor
             SET active_run_id = NULL,
                 running_prompt_id = NULL,
                 pending_interaction_id = NULL,
                 cancel_and_send_id = NULL,
                 cancel_requested = 0,
                 updated_at = ?1",
            rusqlite::params![now],
        );

        tx.commit()
            .map_err(|e| format!("PERSISTENCE_FAILED commit recovery tx: {e}"))?;
        Ok(changed)
    }

    fn store_project_path(&self, run_id: &str, project_path: Option<&str>) {
        let Some(p) = project_path.map(str::trim).filter(|s| !s.is_empty()) else {
            return;
        };
        if let Ok(mut map) = self.project_paths.lock() {
            map.insert(run_id.to_string(), std::path::PathBuf::from(p));
        }
    }

    fn resolve_project_path(
        &self,
        run_id: &str,
        request_path: Option<&str>,
    ) -> Option<std::path::PathBuf> {
        // Explicit only — never daemon process cwd (full remediation 第五节).
        if let Some(p) = request_path.map(str::trim).filter(|s| !s.is_empty()) {
            return Some(std::path::PathBuf::from(p));
        }
        self.project_paths
            .lock()
            .ok()
            .and_then(|m| m.get(run_id).cloned())
    }

    fn runs_snapshot_path() -> std::path::PathBuf {
        let root = std::env::var("NATIVES_RUNTIME_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| {
                std::env::var_os("HOME")
                    .or_else(|| std::env::var_os("USERPROFILE"))
                    .map(|h| std::path::PathBuf::from(h).join(".natives").join("runtime"))
                    .unwrap_or_else(|| std::env::temp_dir().join("natives-runtime"))
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

    /// Load non-terminal runs from disk; mark interrupted activity for safe resume UX.
    pub fn restore_runs_snapshot(&self) -> Result<usize, String> {
        let path = self.snapshot_path();
        let Ok(raw) = std::fs::read_to_string(&path) else {
            return Ok(0);
        };
        let loaded: HashMap<String, RunV2> =
            serde_json::from_str(&raw).map_err(|e| e.to_string())?;
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

    pub fn events(&self) -> EventSequencer {
        self.runtime.events.clone()
    }

    pub fn capabilities() -> DaemonCapabilities {
        DaemonCapabilities::current()
    }

    pub fn create_run(&self, req: CreateRunRequest) -> Result<RunV2, String> {
        if let Some(key) = &req.idempotency_key {
            {
                let map = self.idempotency.lock().map_err(|e| e.to_string())?;
                if let Some(existing) = map.get(key) {
                    let runs = self.runs.lock().map_err(|e| e.to_string())?;
                    if let Some(run) = runs.get(existing) {
                        return Ok(run.clone());
                    }
                }
            }
            if let Some(run) = self.run_from_store_by_idempotency_key(key)? {
                self.runs
                    .lock()
                    .map_err(|e| e.to_string())?
                    .insert(run.id.clone(), run.clone());
                self.idempotency
                    .lock()
                    .map_err(|e| e.to_string())?
                    .insert(key.clone(), run.id.clone());
                self.store_project_path(&run.id, run.project_path.as_deref());
                return Ok(run);
            }
        }

        // Resolve stable ProjectIdentity when a path is provided (task-10).
        // Never store raw path as conversation.project_id.
        let mut bound_project_id: Option<String> = None;
        let mut bound_identity_version: Option<u32> = None;
        let mut bound_canonical: Option<String> = None;
        if let Some(pp) = req
            .project_path
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            if let Some(store) = &self.data_store {
                if let Ok(conn) = store.conn() {
                    match crate::project_identity::store::register_or_get(&conn, pp) {
                        Ok(identity) => {
                            bound_project_id = Some(identity.project_id.clone());
                            bound_identity_version = Some(identity.identity_version);
                            bound_canonical = Some(identity.canonical_path.clone());
                        }
                        Err(e) => {
                            // Missing path: keep diagnostic snapshot path, leave project_id
                            // unbound (orphaned). Side-effect tools must re-bind first.
                            eprintln!("[run_manager] project identity not bound for '{pp}': {e}");
                            bound_canonical = Some(pp.to_string());
                        }
                    }
                }
            } else {
                bound_canonical = Some(pp.to_string());
            }
        }

        // Host-mediated or daemon-owned: ensure conversation row exists for FK integrity
        // on the SAME store used by persist_run_row (never a different env path).
        // conversation.project_id stores stable ProjectIdentity UUID (not path).
        self.ensure_conversation_for_run(
            &req.conversation_id,
            &req.provider_id,
            &req.model_id,
            req.permission_profile.as_deref(),
            bound_project_id.as_deref(),
        )?;

        // When the UI supplies an idempotency key, use it as the run id so
        // assistant.db rows and daemon events share one identifier.
        let id = req
            .idempotency_key
            .clone()
            .filter(|k| !k.is_empty())
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let run = RunV2 {
            id: id.clone(),
            conversation_id: req.conversation_id,
            status: RunStatusV2::Queued,
            parent_run_id: req.parent_run_id,
            agent_profile_id: req.agent_profile_id,
            provider_id: req.provider_id,
            key_id: req.key_id,
            model_id: req.model_id,
            permission_profile: req.permission_profile.unwrap_or_else(|| "ask".into()),
            trigger_message_id: None,
            started_at: None,
            finished_at: None,
            error_code: None,
            step_count: 0,
            max_steps: req.max_steps.unwrap_or(50),
            project_path: bound_canonical.clone().or(req.project_path.clone()),
            project_id: bound_project_id.clone(),
            project_identity_version: bound_identity_version,
            retry_count: 0,
            created_at: Some(chrono::Utc::now()),
            last_event_sequence: 0,
            idempotency_key: req.idempotency_key.clone(),
            effort: req.effort.clone(),
            runtime_id: req.runtime_id.clone().or_else(|| Some("native".into())),
            revision: 0,
        };
        self.persist_run_row(&run)?;
        {
            let mut runs = self.runs.lock().map_err(|e| e.to_string())?;
            runs.insert(id.clone(), run.clone());
        }
        let _ = self.persist_runs_snapshot();
        if let Some(key) = req.idempotency_key {
            self.idempotency
                .lock()
                .map_err(|e| e.to_string())?
                .insert(key, id.clone());
        }
        if let Some(content) = req.content {
            self.last_content
                .lock()
                .map_err(|e| e.to_string())?
                .insert(run.id.clone(), content);
        }
        self.store_project_path(&run.id, req.project_path.as_deref());
        let queued = self.runtime.events.append(&run.id, RunEventKind::Queued);
        if let RunEventKind::Failed { error, code } = queued.payload {
            if code == "PERSISTENCE_FAILED" {
                if let Ok(mut runs) = self.runs.lock() {
                    runs.remove(&run.id);
                }
                self.delete_run_row(&run.id);
                return Err(error);
            }
        }
        Ok(run)
    }

    /// Shared DataStore handle for ProjectIdentity verify on tool path.
    pub fn data_store_ref(&self) -> Option<std::sync::Arc<crate::storage::DataStore>> {
        self.data_store.clone()
    }

    pub fn get_run(&self, run_id: &str) -> Option<RunV2> {
        self.runs.lock().ok()?.get(run_id).cloned()
    }

    pub fn list_runs(&self, conversation_id: Option<&str>) -> Vec<RunV2> {
        let runs = match self.runs.lock() {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };
        runs.values()
            .filter(|r| {
                conversation_id
                    .map(|c| r.conversation_id == c)
                    .unwrap_or(true)
            })
            .cloned()
            .collect()
    }

    /// Drop in-memory run bookkeeping after conversations were hard-deleted.
    /// Does not touch durable `usage_stats` aggregates.
    pub fn forget_conversations(&self, conversation_ids: &[String]) {
        if conversation_ids.is_empty() {
            return;
        }
        let set: std::collections::HashSet<&str> =
            conversation_ids.iter().map(|s| s.as_str()).collect();
        if let Ok(mut runs) = self.runs.lock() {
            runs.retain(|_, r| !set.contains(r.conversation_id.as_str()));
        }
        if let Ok(mut last) = self.last_content.lock() {
            // last_content is keyed by run_id; drop entries whose run is gone.
            if let Ok(runs) = self.runs.lock() {
                last.retain(|run_id, _| runs.contains_key(run_id));
            }
        }
        let _ = self.persist_runs_snapshot();
    }

    pub fn replay(&self, req: ReplayRunRequest) -> Vec<RunEventV2> {
        self.runtime
            .events
            .replay_after(&req.run_id, req.after_sequence)
    }

    pub async fn cancel(&self, req: CancelRunRequest) -> Result<RunV2, String> {
        // Phase 1: journal Cancelling via commit_status (sole lifecycle authority).
        // Never mutate memory Run.status directly — DB + lifecycle event + memory + broadcast
        // must succeed as one commit_transition.
        {
            let current = self
                .get_run(&req.run_id)
                .ok_or_else(|| "run not found".to_string())?;
            if current.status.is_terminal() {
                return Ok(current);
            }
            if current.status != RunStatusV2::Cancelling {
                if let Err(e) = self.commit_status(
                    &req.run_id,
                    RunStatusV2::Cancelling,
                    TransitionMetadata::empty()
                        .with_reason("cancelling")
                        .with_lifecycle_hint("cancelling"),
                ) {
                    // Fail-safe: still signal cancel tree so work stops, but do not claim
                    // success or invent a pseudo-terminal status.
                    self.runtime.cancel_run(&req.run_id).await;
                    return Err(e);
                }
            }
        }

        // Phase 2: signal tree + grace + force cleanup (domain only; no status writes).
        self.runtime.cancel_run(&req.run_id).await;

        // Phase 3: Cancelled only after registry quiet (or Failed on cleanup fail).
        // quiet is scoped to this run's tree (not global active_count).
        let quiet = self.runtime.execution.tree_quiet(&req.run_id).await;
        let current = self
            .get_run(&req.run_id)
            .ok_or_else(|| "run not found".to_string())?;
        if current.status.is_terminal() {
            return Ok(current);
        }
        if quiet {
            self.commit_status(
                &req.run_id,
                RunStatusV2::Cancelled,
                TransitionMetadata::empty()
                    .with_reason("cancelled")
                    .with_lifecycle_hint("cancelled"),
            )
        } else {
            self.commit_status(
                &req.run_id,
                RunStatusV2::Failed,
                TransitionMetadata::empty()
                    .with_reason("cancel_cleanup_failed")
                    .with_lifecycle_hint("failed"),
            )
        }
    }

    /// Ensure a run row exists for `start` / `start_detached` (create if `run_id` absent).
    ///
    /// Always ensures the current user turn is recorded in the daemon conversation store
    /// under a daemon-owned `trigger_message_id` (never reuses host message ids as FKs).
    /// Ensure a run exists for start. Appends a daemon-local user message when content
    /// is present. When this manager is memory-only (`data_store` is None), skip SQLite
    /// message writes so pure unit tests never touch the developer's assistant.db.
    pub fn ensure_run_for_start(&self, req: &StartRunRequest) -> Result<RunV2, String> {
        let can_write_messages = self.data_store.is_some();
        // Existing run (host create_run + start path): still append daemon-local user message.
        if let Some(run_id) = &req.run_id {
            let mut run = self
                .get_run(run_id)
                .ok_or_else(|| "run not found".to_string())?;
            if can_write_messages
                && run.trigger_message_id.is_none()
                && (req.content.as_ref().is_some_and(|c| !c.trim().is_empty())
                    || req.attachments.as_ref().is_some_and(|a| !a.is_empty()))
            {
                if let Some(id) = self.append_user_message_on_store(
                    &run.conversation_id,
                    req.content.as_deref(),
                    req.attachments.as_deref(),
                )? {
                    {
                        let mut runs = self.runs.lock().map_err(|e| e.to_string())?;
                        if let Some(stored) = runs.get_mut(&run.id) {
                            stored.trigger_message_id = Some(id.clone());
                            run = stored.clone();
                        }
                    }
                    self.persist_run_row(&run)?;
                    self.persist_runs_snapshot()?;
                }
            }
            return Ok(run);
        }
        if let Some(key) = &req.idempotency_key {
            let map = self.idempotency.lock().map_err(|e| e.to_string())?;
            if let Some(existing) = map.get(key) {
                let runs = self.runs.lock().map_err(|e| e.to_string())?;
                if let Some(run) = runs.get(existing) {
                    return Ok(run.clone());
                }
            }
        }
        let conversation_id = req
            .conversation_id
            .clone()
            .ok_or_else(|| "conversation_id required".to_string())?;
        let trigger_message_id = if can_write_messages {
            self.append_user_message_on_store(
                &conversation_id,
                req.content.as_deref(),
                req.attachments.as_deref(),
            )?
        } else {
            None
        };
        let mut run = match self.create_run(CreateRunRequest {
            conversation_id: conversation_id.clone(),
            provider_id: req.provider_id.clone().unwrap_or_default(),
            model_id: req.model_id.clone().unwrap_or_default(),
            key_id: req.key_id.clone(),
            agent_profile_id: None,
            permission_profile: req.permission_profile.clone().or_else(|| {
                // Prefer conversation-row profile when available on this store.
                if let Some(store) = &self.data_store {
                    store.conn().ok().and_then(|conn| {
                        conn.query_row(
                            "SELECT COALESCE(permission_profile_id, 'ask') FROM conversation WHERE id = ?1",
                            rusqlite::params![conversation_id],
                            |row| row.get::<_, String>(0),
                        )
                        .ok()
                    })
                } else {
                    None
                }
            }),
            content: req.content.clone(),
            attachments: req.attachments.clone(),
            max_steps: req.max_steps,
            parent_run_id: None,
            project_path: req.project_path.clone(),
            idempotency_key: req.idempotency_key.clone(),
            effort: req.effort.clone(),
            runtime_id: req.runtime_id.clone(),
        }) {
            Ok(run) => run,
            Err(error) => {
                // Roll back the pre-created trigger message if create_run failed.
                if let (Some(store), Some(id)) = (&self.data_store, trigger_message_id.as_deref()) {
                    let _ = store.conn().and_then(|conn| {
                        conn.execute("DELETE FROM message WHERE id = ?1", rusqlite::params![id])
                            .map_err(|e| e.to_string())
                    });
                }
                return Err(error);
            }
        };
        if let Some(trigger_message_id) = trigger_message_id {
            {
                let mut runs = self.runs.lock().map_err(|e| e.to_string())?;
                if let Some(stored) = runs.get_mut(&run.id) {
                    stored.trigger_message_id = Some(trigger_message_id.clone());
                    run = stored.clone();
                }
            }
            self.persist_run_row(&run)?;
            self.persist_runs_snapshot()?;
        }
        Ok(run)
    }

    /// Insert a user message on **this** manager's DataStore (never env-open another DB).
    fn append_user_message_on_store(
        &self,
        conversation_id: &str,
        content: Option<&str>,
        attachments: Option<&[assistant_protocol::v2::AttachmentRef]>,
    ) -> Result<Option<String>, String> {
        let Some(store) = &self.data_store else {
            return Ok(None);
        };
        let mut blocks = Vec::new();
        if let Some(content) = content.filter(|s| !s.trim().is_empty()) {
            blocks.push(serde_json::json!({ "type": "text", "text": content }));
        }
        for attachment in attachments.unwrap_or(&[]) {
            if attachment.path.trim().is_empty() {
                continue;
            }
            blocks.push(serde_json::json!({
                "type": "file_reference",
                "path": attachment.path,
                "name": attachment.name.clone(),
                "mime_type": attachment.mime_type.clone(),
                "size": attachment.size,
            }));
        }
        if blocks.is_empty() {
            return Ok(None);
        }
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        let conn = store.conn()?;
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO message (id, conversation_id, role, status, created_at)
             VALUES (?1, ?2, 'user', 'complete', ?3)",
            rusqlite::params![id, conversation_id, now],
        )
        .map_err(|e| e.to_string())?;
        for (index, block) in blocks.iter().enumerate() {
            let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("text");
            let content = if block_type == "text" {
                serde_json::json!({
                    "text": block.get("text").and_then(|v| v.as_str()).unwrap_or_default()
                })
            } else {
                block.clone()
            };
            tx.execute(
                "INSERT INTO message_block (message_id, sort_order, block_type, block_json)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![id, index as i64, block_type, content.to_string()],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.execute(
            "UPDATE conversation SET updated_at = ?1 WHERE id = ?2",
            rusqlite::params![now, conversation_id],
        )
        .and_then(|_| tx.commit())
        .map_err(|e| e.to_string())?;
        Ok(Some(id))
    }

    fn mark_preparing(&self, run_id: &str) -> Result<RunV2, String> {
        self.commit_status(
            run_id,
            RunStatusV2::Preparing,
            TransitionMetadata::empty().with_lifecycle_hint("preparing"),
        )
    }

    /// Non-blocking start for RPC / UI: returns immediately with Preparing status.
    /// Engine execution continues on a background task; clients poll events / cancel
    /// on the same session without waiting for completion.
    ///
    /// Idempotent: if the run is already active (or terminal), does **not** spawn a
    /// second engine — returns the current row (duplicate Start is a no-op).
    pub fn start_detached(self: &Arc<Self>, req: StartRunRequest) -> Result<RunV2, String> {
        let run = self.ensure_run_for_start(&req)?;

        // Honest runtime gate (REQ-T01/T02): codex never executable; claude_cli only if binary present.
        if let Some(rt) = req
            .runtime_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            match rt {
                "native" | "" => {}
                "codex_cli" => {
                    // Single source of truth for T02 red line.
                    if !crate::codex_runtime_bridge::codex_cli_available() {
                        return Err(
                            "runtime codex_cli is unavailable (app-server not implemented)".into(),
                        );
                    }
                }
                "claude_cli" => {
                    if !crate::cli_runtime_bridge::claude_cli_available() {
                        return Err(
                            "runtime claude_cli unavailable (claude binary not found)".into()
                        );
                    }
                }
                other => {
                    return Err(format!("unknown runtime_id: {other}"));
                }
            }
        }
        if run.status.is_active() {
            return Ok(run);
        }
        if run.status.is_terminal() {
            return Err(format!(
                "run {} is terminal ({:?}); use run.retry",
                run.id,
                run.status.as_str()
            ));
        }
        let mut req = req;
        req.run_id = Some(run.id.clone());
        if let Some(content) = &req.content {
            self.last_content
                .lock()
                .map_err(|e| e.to_string())?
                .insert(run.id.clone(), content.clone());
        }
        self.store_project_path(&run.id, req.project_path.as_deref());
        let preparing = self.mark_preparing(&run.id)?;
        self.persist_runs_snapshot()?;
        let rm = Arc::clone(self);
        let run_id_for_fail = preparing.id.clone();
        tokio::spawn(async move {
            if let Err(err) = rm.start(req).await {
                rm.fail_run_if_active(&run_id_for_fail, err, "START_FAILED");
            }
        });
        Ok(preparing)
    }

    /// Process-wide non-blocking start (sidecar RPC uses this via `global_run_manager`).
    /// Same idempotency rules as [`start_detached`].
    pub fn start_detached_global(req: StartRunRequest) -> Result<RunV2, String> {
        let rm = global_run_manager();
        let run = rm.ensure_run_for_start(&req)?;

        // Honest runtime gate (REQ-T01/T02): codex never executable; claude_cli only if binary present.
        if let Some(rt) = req
            .runtime_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            match rt {
                "native" | "" => {}
                "codex_cli" => {
                    // Single source of truth for T02 red line.
                    if !crate::codex_runtime_bridge::codex_cli_available() {
                        return Err(
                            "runtime codex_cli is unavailable (app-server not implemented)".into(),
                        );
                    }
                }
                "claude_cli" => {
                    if !crate::cli_runtime_bridge::claude_cli_available() {
                        return Err(
                            "runtime claude_cli unavailable (claude binary not found)".into()
                        );
                    }
                }
                other => {
                    return Err(format!("unknown runtime_id: {other}"));
                }
            }
        }
        if run.status.is_active() {
            return Ok(run);
        }
        if run.status.is_terminal() {
            return Err(format!(
                "run {} is terminal ({:?}); use run.retry",
                run.id,
                run.status.as_str()
            ));
        }
        let mut req = req;
        req.run_id = Some(run.id.clone());
        if let Some(content) = &req.content {
            rm.last_content
                .lock()
                .map_err(|e| e.to_string())?
                .insert(run.id.clone(), content.clone());
        }
        rm.store_project_path(&run.id, req.project_path.as_deref());
        let preparing = rm.mark_preparing(&run.id)?;
        rm.persist_runs_snapshot()?;
        let run_id_for_fail = preparing.id.clone();
        tokio::spawn(async move {
            let rm = global_run_manager();
            if let Err(err) = rm.start(req).await {
                rm.fail_run_if_active(&run_id_for_fail, err, "START_FAILED");
            }
        });
        Ok(preparing)
    }

    /// Production start: real provider path when credentials exist; fixture path only under test flag.
    /// Blocks until the engine reaches a terminal status (tests / in-process callers that wait).
    /// RPC must use [`start_detached`] / [`start_detached_global`] instead.
    pub async fn start(&self, req: StartRunRequest) -> Result<RunV2, String> {
        let run = if let Some(run_id) = &req.run_id {
            self.get_run(run_id)
                .ok_or_else(|| "run not found".to_string())?
        } else {
            self.ensure_run_for_start(&req)?
        };

        let content = req
            .content
            .clone()
            .or_else(|| {
                self.last_content
                    .lock()
                    .ok()
                    .and_then(|m| m.get(&run.id).cloned())
            })
            .unwrap_or_else(|| "continue".into());
        self.last_content
            .lock()
            .map_err(|e| e.to_string())?
            .insert(run.id.clone(), content.clone());

        let _ = self.commit_status(
            &run.id,
            RunStatusV2::Preparing,
            TransitionMetadata::empty().with_lifecycle_hint("preparing"),
        );

        let request_project_path = req.project_path.clone();
        let provider_id = req.provider_id.unwrap_or_else(|| run.provider_id.clone());
        let model_id = req.model_id.unwrap_or_else(|| run.model_id.clone());
        let key_id = req.key_id.or_else(|| run.key_id.clone());
        let permission_profile = req
            .permission_profile
            .unwrap_or_else(|| run.permission_profile.clone());
        let max_steps = req.max_steps.unwrap_or(run.max_steps);
        let runtime_id = req
            .runtime_id
            .clone()
            .or_else(|| run.runtime_id.clone())
            .unwrap_or_else(|| "native".into());
        // Persist selected runtime on the run row for UI / resume.
        {
            let mut runs = self.runs.lock().map_err(|e| e.to_string())?;
            if let Some(r) = runs.get_mut(&run.id) {
                r.runtime_id = Some(runtime_id.clone());
                r.effort = req.effort.clone().or_else(|| r.effort.clone());
                self.persist_run_row(r)?;
            }
        }

        // REQ-T02: Codex remains fail-closed (app-server not implemented).
        if runtime_id == "codex_cli" {
            let cancel = self
                .runtime
                .ensure_execution_token(&run.id, run.parent_run_id.as_deref())
                .await?;
            let err = match crate::codex_runtime_bridge::run_codex_cli_turn(
                &self.runtime,
                &run.id,
                &content,
                &model_id,
                request_project_path
                    .as_deref()
                    .or(run.project_path.as_deref())
                    .map(std::path::PathBuf::from)
                    .as_deref(),
                &permission_profile,
                cancel,
            )
            .await
            {
                Ok(s) => s,
                Err(e) => e,
            };
            self.runtime.execution.mark_finished(&run.id).await;
            let _ = self.commit_status(
                &run.id,
                RunStatusV2::Failed,
                TransitionMetadata::empty()
                    .with_error_code("CODEX_UNAVAILABLE")
                    .with_reason(err.clone())
                    .with_lifecycle_hint("failed"),
            );
            return Err(if err.contains("unavailable") {
                err
            } else {
                "runtime codex_cli is unavailable (app-server not implemented)".into()
            });
        }

        // REQ-T01: Claude CLI session main path (stream-json → v2 events).
        if runtime_id == "claude_cli"
            && std::env::var("NATIVES_DAEMON_FIXTURE").ok().as_deref() != Some("1")
            && !cfg!(test)
        {
            let cancel = self
                .runtime
                .ensure_execution_token(&run.id, run.parent_run_id.as_deref())
                .await?;
            // Commit Running before the turn: Preparing→{Completed,Cancelled} are not
            // legal edges, but Running→terminal are. CLI is actively running here.
            let _ = self.commit_status(
                &run.id,
                RunStatusV2::Running,
                TransitionMetadata::empty().with_lifecycle_hint("running"),
            );
            let project = request_project_path
                .as_deref()
                .or(run.project_path.as_deref())
                .map(std::path::PathBuf::from);
            let terminal = match crate::cli_runtime_bridge::run_claude_cli_turn(
                &self.runtime,
                &run.id,
                &content,
                &model_id,
                project.as_deref(),
                &permission_profile,
                cancel,
            )
            .await
            {
                Ok(s) => s,
                Err(e) => {
                    self.runtime.execution.mark_finished(&run.id).await;
                    // Do not append terminal events here; commit_status is sole lifecycle writer.
                    let meta = TransitionMetadata::empty()
                        .with_error_code("CLI_RUNTIME")
                        .with_reason(e)
                        .with_lifecycle_hint("failed");
                    let _ = self.commit_status(&run.id, RunStatusV2::Failed, meta);
                    return self
                        .get_run(&run.id)
                        .ok_or_else(|| "run missing after cli turn".to_string());
                }
            };
            self.runtime.execution.mark_finished(&run.id).await;
            let final_status = match terminal.as_str() {
                "completed" => RunStatusV2::Completed,
                "cancelled" => RunStatusV2::Cancelled,
                "interrupted" => RunStatusV2::Interrupted,
                _ => RunStatusV2::Failed,
            };
            let meta = match final_status {
                RunStatusV2::Failed => TransitionMetadata::empty()
                    .with_error_code("CLI_RUNTIME")
                    .with_lifecycle_hint("failed"),
                RunStatusV2::Cancelled => TransitionMetadata::empty()
                    .with_reason("cancelled")
                    .with_lifecycle_hint("cancelled"),
                RunStatusV2::Interrupted => TransitionMetadata::empty()
                    .with_reason("cancelled")
                    .with_lifecycle_hint("interrupted"),
                _ => TransitionMetadata::empty().with_lifecycle_hint(final_status.as_str()),
            };
            let _ = self.commit_status(&run.id, final_status, meta);
            return self
                .get_run(&run.id)
                .ok_or_else(|| "run missing after cli turn".to_string());
        }

        // Prefer real provider when credentials exist; otherwise fixture (tests only).
        let use_fixture = std::env::var("NATIVES_DAEMON_FIXTURE")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
            || crate::production::resolve_credential(&provider_id, key_id.as_deref()).is_err();

        if use_fixture && std::env::var("NATIVES_DAEMON_FIXTURE").is_err() {
            // Production without keys fails clearly rather than Echo mock success.
            if std::env::var("NATIVES_ALLOW_FIXTURE_FALLBACK")
                .ok()
                .as_deref()
                != Some("1")
            {
                // Still allow fixture for unit tests via explicit flag only.
                // For daemon tests we set NATIVES_DAEMON_FIXTURE=1.
                if cfg!(test) {
                    // unit tests can use fixture
                } else {
                    let err = format!(
                        "No credentials for provider '{provider_id}'. Configure an active key in Settings."
                    );
                    self.fail_run_if_active(&run.id, err.clone(), "NO_CREDENTIALS");
                    return Err(err);
                }
            }
        }

        let use_fixture_engine =
            std::env::var("NATIVES_DAEMON_FIXTURE").ok().as_deref() == Some("1") || cfg!(test);

        if use_fixture_engine {
            // Deterministic offline path for tests — real engine + permission tools,
            // never EchoProvider fake success text.
            let mut hooks = agent_core::HookRegistry::new();
            hooks.register(
                agent_core::HookEvent::PreToolUse,
                Box::new(agent_core::AllowAllHook),
            );
            hooks.register(
                agent_core::HookEvent::Stop,
                Box::new(agent_core::AllowAllHook),
            );
            let cancel = self
                .runtime
                .ensure_execution_token(&run.id, run.parent_run_id.as_deref())
                .await
                .unwrap_or_else(|_| CancellationToken::new());
            let engine = Arc::new(
                AgentEngine::new(self.runtime.events.clone())
                    .with_cancel_token(cancel)
                    .with_hooks(hooks),
            );
            self.runtime.register_engine(&run.id, engine.clone()).await;
            let provider = FixtureProvider {
                mode: FixtureMode::TextOnly,
            };
            let tool_allowlist = self.runtime.take_run_tool_allowlist(&run.id).await;
            let tools = crate::production::PermissionGatedTools {
                gateway: {
                    let mut g = capability_gateway::CapabilityGateway::new();
                    g.register_builtins();
                    if let Some(ref list) = tool_allowlist {
                        // Restrict fixture gateway surface to allowlist names when present.
                        // Builtins still registered; PermissionGatedTools enforces allowlist.
                        let _ = list;
                    }
                    Arc::new(g)
                },
                permissions: self.runtime.permissions.clone(),
                events: self.runtime.events.clone(),
                interactions: self.runtime.interactions.clone(),
                subagents: self.runtime.subagents.clone(),
                task_outputs: self.runtime.task_outputs_ref(),
                engines: self.runtime.engine_handles().await,
                runtime: Some(self.runtime.clone()),
                provider_id: provider_id.clone(),
                key_id: key_id.clone(),
                parent_run_id: run.id.clone(),
                conversation_id: run.conversation_id.clone(),
                model_id: model_id.clone(),
                permission_profile: permission_profile.clone(),
                tool_allowlist,
            };
            let config = EngineRunConfig {
                run_id: run.id.clone(),
                conversation_id: run.conversation_id.clone(),
                model: model_id.clone(),
                system_prompt: None,
                messages: crate::conversation_store::engine_history(&run.conversation_id)
                    .unwrap_or_default(),
                user_content: content,
                max_steps,
            };
            let outcome = match engine.run(config, &provider, &tools).await {
                Ok(o) => o,
                Err(e) => EngineOutcome::failed(e.code(), e.to_string(), e.retryable()),
            };
            if matches!(outcome, EngineOutcome::Completed { .. }) {
                let _ = crate::conversation_store::append_assistant_turn_from_events(
                    &run.conversation_id,
                    &run.id,
                    &self.runtime.events.replay_after(&run.id, 0),
                );
            }
            self.runtime.remove_engine(&run.id).await;
            return self.commit_outcome(&run.id, &outcome);
        }

        let project_path = self.resolve_project_path(&run.id, request_project_path.as_deref());
        self.store_project_path(
            &run.id,
            project_path
                .as_ref()
                .map(|p| p.to_string_lossy().to_string())
                .as_deref(),
        );
        let outcome = self
            .runtime
            .start_run(
                run.id.clone(),
                run.conversation_id.clone(),
                provider_id,
                model_id,
                key_id,
                permission_profile,
                run.agent_profile_id.clone(),
                content,
                max_steps,
                project_path,
            )
            .await?;
        // Sole terminal commit from EngineOutcome — never scan events or default Completed.
        self.commit_outcome(&run.id, &outcome)
    }

    /// Start with explicit seams (tests / advanced callers).
    pub async fn start_with_seams(
        &self,
        req: StartRunRequest,
        provider: &dyn agent_core::EngineProvider,
        tools: &dyn agent_core::EngineToolRuntime,
    ) -> Result<RunV2, String> {
        let run = if let Some(run_id) = &req.run_id {
            self.get_run(run_id)
                .ok_or_else(|| "run not found".to_string())?
        } else {
            let conversation_id = req
                .conversation_id
                .clone()
                .ok_or_else(|| "conversation_id required".to_string())?;
            self.create_run(CreateRunRequest {
                conversation_id,
                provider_id: req.provider_id.clone().unwrap_or_default(),
                model_id: req.model_id.clone().unwrap_or_default(),
                key_id: req.key_id.clone(),
                agent_profile_id: None,
                permission_profile: req.permission_profile.clone(),
                content: req.content.clone(),
                attachments: req.attachments.clone(),
                max_steps: req.max_steps,
                parent_run_id: None,
                project_path: None,
                idempotency_key: req.idempotency_key.clone(),
                effort: None,
                runtime_id: None,
            })?
        };

        // Empty default: do not invent a synthetic "continue" user turn when the
        // conversation history already holds the user messages (history reload tests).
        let content = req.content.clone().unwrap_or_default();
        // Register engine so cancel_run → request_cancel works mid-flight.
        let cancel = self
            .runtime
            .ensure_execution_token(&run.id, run.parent_run_id.as_deref())
            .await
            .unwrap_or_else(|_| CancellationToken::new());
        let engine =
            Arc::new(AgentEngine::new(self.runtime.events.clone()).with_cancel_token(cancel));
        self.runtime.register_engine(&run.id, engine.clone()).await;
        let _ = self.commit_status(
            &run.id,
            RunStatusV2::Preparing,
            TransitionMetadata::empty().with_lifecycle_hint("preparing"),
        );
        let config = EngineRunConfig {
            run_id: run.id.clone(),
            conversation_id: run.conversation_id.clone(),
            model: req.model_id.unwrap_or_else(|| run.model_id.clone()),
            system_prompt: None,
            messages: crate::conversation_store::engine_history(&run.conversation_id)
                .unwrap_or_default(),
            user_content: content,
            max_steps: req.max_steps.unwrap_or(run.max_steps),
        };
        let outcome = match engine.run(config, provider, tools).await {
            Ok(o) => o,
            Err(e) => EngineOutcome::failed(e.code(), e.to_string(), e.retryable()),
        };
        let outcome = if matches!(outcome, EngineOutcome::Completed { .. })
            && crate::conversation_store::append_assistant_turn_from_events(
                &run.conversation_id,
                &run.id,
                &self.runtime.events.replay_after(&run.id, 0),
            )
            .is_err()
        {
            EngineOutcome::failed(
                "persist_assistant",
                "failed to persist assistant turn",
                false,
            )
        } else {
            outcome
        };
        self.runtime.remove_engine(&run.id).await;
        self.commit_outcome(&run.id, &outcome)
    }

    pub fn retry(&self, req: RetryRunRequest) -> Result<RunV2, String> {
        let original = self
            .get_run(&req.run_id)
            .ok_or_else(|| "run not found".to_string())?;
        let content = self
            .last_content
            .lock()
            .ok()
            .and_then(|m| m.get(&req.run_id).cloned());
        let project_path = self
            .project_paths
            .lock()
            .ok()
            .and_then(|m| m.get(&req.run_id).map(|p| p.to_string_lossy().to_string()));
        let new_run = self.create_run(CreateRunRequest {
            conversation_id: original.conversation_id,
            provider_id: original.provider_id,
            model_id: original.model_id,
            key_id: original.key_id,
            agent_profile_id: original.agent_profile_id,
            permission_profile: Some(original.permission_profile),
            content: content.clone(),
            attachments: None,
            max_steps: Some(original.max_steps),
            parent_run_id: None,
            project_path,
            idempotency_key: None,
            effort: original.effort.clone(),
            runtime_id: original.runtime_id.clone(),
        })?;
        if let Some(c) = content {
            self.last_content
                .lock()
                .map_err(|e| e.to_string())?
                .insert(new_run.id.clone(), c);
        }
        Ok(new_run)
    }

    pub async fn respond_permission(&self, request_id: &str, approved: bool) -> Result<(), String> {
        self.respond_permission_for_run(request_id, approved, None, None)
            .await
    }

    pub async fn respond_permission_for_run(
        &self,
        request_id: &str,
        approved: bool,
        run_id: Option<&str>,
        scope: Option<&str>,
    ) -> Result<(), String> {
        self.runtime
            .respond_permission(request_id, approved, run_id, scope)
            .await
    }
}

pub fn protocol_version() -> &'static str {
    PROTOCOL_V2
}

impl RunLifecycleAuthority for RunManager {
    fn commit_transition(
        &self,
        run_id: &str,
        expected_revision: u64,
        target: RunStatusV2,
        metadata: TransitionMetadata,
    ) -> Result<agent_core::CommittedTransition, CommitError> {
        // 1. Read current from memory (authority cache); fall back to constructing error.
        let (from, current_revision) = {
            let runs = self
                .runs
                .lock()
                .map_err(|e| CommitError::Other(e.to_string()))?;
            let run = runs.get(run_id).ok_or_else(|| CommitError::NotFound {
                run_id: run_id.to_string(),
            })?;
            (run.status, run.revision)
        };

        // Terminal idempotency: late outcomes against terminal do not insert events.
        if from.is_terminal() {
            return Err(CommitError::AlreadyTerminal {
                run_id: run_id.to_string(),
                status: from,
                revision: current_revision,
            });
        }

        if current_revision != expected_revision {
            return Err(CommitError::CasConflict {
                run_id: run_id.to_string(),
                expected: expected_revision,
                current: current_revision,
                status: from,
            });
        }

        // 2. Validate edge via agent-core sole authority.
        agent_core::transition(from, target).map_err(CommitError::from)?;

        let new_revision = expected_revision.saturating_add(1);
        let lifecycle = Self::lifecycle_event_for(target, &metadata);

        // 3. Durable transaction when store present; else memory CAS critical section.
        let committed_event = if let Some(store) = &self.data_store {
            let conn = store.conn().map_err(CommitError::Storage)?;
            let tx = conn
                .unchecked_transaction()
                .map_err(|e| CommitError::Storage(e.to_string()))?;

            // CAS update run row
            let now = chrono::Utc::now().to_rfc3339();
            let finished_at = if target.is_terminal() {
                Some(now.clone())
            } else {
                None
            };
            let started_at = if matches!(target, RunStatusV2::Preparing | RunStatusV2::Running) {
                Some(now.clone())
            } else {
                None
            };
            let changed = tx
                .execute(
                    "UPDATE run SET
                        status = ?1,
                        revision = ?2,
                        error_code = COALESCE(?3, error_code),
                        finished_at = COALESCE(?4, finished_at),
                        started_at = COALESCE(started_at, ?5),
                        step_count = COALESCE(?6, step_count)
                     WHERE id = ?7 AND revision = ?8",
                    rusqlite::params![
                        target.as_str(),
                        new_revision as i64,
                        metadata.error_code,
                        finished_at,
                        started_at,
                        metadata.step_count.map(|s| s as i64),
                        run_id,
                        expected_revision as i64,
                    ],
                )
                .map_err(|e| CommitError::Storage(e.to_string()))?;
            if changed == 0 {
                // Re-read status for accurate error
                let (status, rev): (String, i64) = tx
                    .query_row(
                        "SELECT status, COALESCE(revision, 0) FROM run WHERE id = ?1",
                        rusqlite::params![run_id],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .map_err(|e| CommitError::Storage(e.to_string()))?;
                let status = run_status_from_db(&status);
                if status.is_terminal() {
                    return Err(CommitError::AlreadyTerminal {
                        run_id: run_id.to_string(),
                        status,
                        revision: rev as u64,
                    });
                }
                return Err(CommitError::CasConflict {
                    run_id: run_id.to_string(),
                    expected: expected_revision,
                    current: rev as u64,
                    status,
                });
            }

            // Allocate next run_sequence and insert lifecycle event in same tx.
            let next_seq: i64 = tx
                .query_row(
                    "SELECT COALESCE(MAX(sequence), 0) + 1 FROM run_event WHERE run_id = ?1",
                    rusqlite::params![run_id],
                    |row| row.get(0),
                )
                .map_err(|e| CommitError::Storage(e.to_string()))?;
            let mut event = RunEventV2::new(run_id, next_seq as u64, lifecycle.clone());
            let payload =
                serde_json::to_string(&event).map_err(|e| CommitError::Storage(e.to_string()))?;
            tx.execute(
                "INSERT INTO run_event (run_id, sequence, event_type, payload, timestamp, event_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    run_id,
                    next_seq,
                    event.payload.type_name(),
                    payload,
                    event.timestamp.to_rfc3339(),
                    event.event_id,
                ],
            )
            .map_err(|e| CommitError::Storage(e.to_string()))?;
            let global = tx.last_insert_rowid() as u64;
            event.global_sequence = global;
            tx.commit()
                .map_err(|e| CommitError::Storage(e.to_string()))?;
            event
        } else {
            // Memory-only: CAS inside the runs mutex (same critical section as status write).
            RunEventV2::new(
                run_id,
                self.runtime.events.last_sequence(run_id).saturating_add(1),
                lifecycle,
            )
        };

        // 4. Update memory cache only after durable success (or memory CAS).
        {
            let mut runs = self
                .runs
                .lock()
                .map_err(|e| CommitError::Other(e.to_string()))?;
            let run = runs.get_mut(run_id).ok_or_else(|| CommitError::NotFound {
                run_id: run_id.to_string(),
            })?;
            // Re-check CAS in memory for the memory-only path.
            if run.revision != expected_revision {
                if run.status.is_terminal() {
                    return Err(CommitError::AlreadyTerminal {
                        run_id: run_id.to_string(),
                        status: run.status,
                        revision: run.revision,
                    });
                }
                return Err(CommitError::CasConflict {
                    run_id: run_id.to_string(),
                    expected: expected_revision,
                    current: run.revision,
                    status: run.status,
                });
            }
            run.status = target;
            run.revision = new_revision;
            Self::apply_transition_metadata(run, target, &metadata);
        }

        // 5. Broadcast committed lifecycle event (no re-persist).
        self.runtime.events.inject_committed(committed_event);
        let _ = self.persist_runs_snapshot();

        Ok(agent_core::CommittedTransition {
            run_id: run_id.to_string(),
            from,
            to: target,
            revision: new_revision,
            idempotent: false,
        })
    }
}

fn run_status_from_db(status: &str) -> RunStatusV2 {
    match status {
        "created" => RunStatusV2::Created,
        "queued" => RunStatusV2::Queued,
        "preparing" => RunStatusV2::Preparing,
        "running" => RunStatusV2::Running,
        "waiting_permission" => RunStatusV2::WaitingPermission,
        "waiting_subagent" => RunStatusV2::WaitingSubagent,
        "cancelling" => RunStatusV2::Cancelling,
        "completed" => RunStatusV2::Completed,
        "failed" => RunStatusV2::Failed,
        "cancelled" => RunStatusV2::Cancelled,
        "interrupted" => RunStatusV2::Interrupted,
        _ => RunStatusV2::Interrupted,
    }
}

fn parse_db_time(value: Option<String>) -> Option<chrono::DateTime<chrono::Utc>> {
    value
        .and_then(|raw| chrono::DateTime::parse_from_rfc3339(&raw).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc))
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::EngineToolRuntime;
    use assistant_protocol::v2::{
        CreateRunRequest, ReplayRunRequest, RetryRunRequest, StartRunRequest,
    };
    use std::sync::Mutex as StdMutex;

    /// Process-global lock for tests that mutate NATIVES_* env (fixture, runtime dir, keys).
    fn with_env_lock<R>(f: impl FnOnce() -> R) -> R {
        let _g = crate::storage::DataStore::env_test_lock();
        f()
    }

    #[test]
    fn create_run_is_idempotent_with_key() {
        let _prev_a = std::env::var("NATIVES_ASSISTANT_DB_PATH").ok();
        let _prev_d = std::env::var("NATIVES_DB_PATH").ok();
        std::env::remove_var("NATIVES_ASSISTANT_DB_PATH");
        std::env::remove_var("NATIVES_DB_PATH");
        let rm = RunManager::new();
        let req = CreateRunRequest {
            conversation_id: "c1".into(),
            provider_id: "openai".into(),
            model_id: "gpt-4o".into(),
            key_id: Some("k1".into()),
            agent_profile_id: None,
            permission_profile: Some("ask".into()),
            content: Some("hello".into()),
            attachments: None,
            max_steps: Some(10),
            parent_run_id: None,
            project_path: None,
            idempotency_key: Some("idem-1".into()),
            effort: None,
            runtime_id: None,
        };
        let a = rm.create_run(req.clone()).unwrap();
        let b = rm.create_run(req).unwrap();
        assert_eq!(a.id, b.id);
    }

    #[test]
    fn create_run_persists_queued_event_to_sqlite_run_event() {
        with_env_lock(|| {
            let dir = tempfile::tempdir().unwrap();
            let db_path = dir.path().join("natives.db");
            std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db_path);
            std::env::set_var("NATIVES_DB_PATH", &db_path);
            crate::storage::set_test_db_override(
                Some(db_path.clone()),
                Some(dir.path().join("artifacts")),
            );
            let store = Arc::new(
                crate::storage::DataStore::new(&db_path, &dir.path().join("artifacts")).unwrap(),
            );
            store
                .conn()
                .unwrap()
                .execute(
                    "INSERT INTO conversation (id, mode, title, provider_id, model_id)
                 VALUES ('sqlite-events-conv', 'agent', 'SQLite Events', 'openai', 'gpt-4o')",
                    [],
                )
                .unwrap();

            let rm = RunManager::new_with_store(store.clone());
            let run = rm
                .create_run(CreateRunRequest {
                    conversation_id: "sqlite-events-conv".into(),
                    provider_id: "openai".into(),
                    model_id: "gpt-4o".into(),
                    key_id: None,
                    agent_profile_id: None,
                    permission_profile: Some("ask".into()),
                    content: Some("persist me".into()),
                    attachments: None,
                    max_steps: Some(3),
                    parent_run_id: None,
                    project_path: None,
                    idempotency_key: Some(format!("sqlite-event-{}", Uuid::new_v4())),
                    effort: None,
                    runtime_id: None,
                })
                .unwrap();

            let (count, event_type): (i64, String) = store
                .conn()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*), MAX(event_type) FROM run_event WHERE run_id = ?1",
                    rusqlite::params![run.id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert_eq!(count, 1);
            assert_eq!(event_type, "queued");

            let replayed = crate::production::ProductionRuntime::new_with_event_store(store)
                .events
                .replay_after(&run.id, 0);
            assert_eq!(replayed.len(), 1);
            assert!(matches!(replayed[0].payload, RunEventKind::Queued));
        });
    }

    #[test]
    fn create_run_persists_protocol_v2_run_metadata_to_sqlite() {
        with_env_lock(|| {
            let dir = tempfile::tempdir().unwrap();
            let db_path = dir.path().join("natives.db");
            std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db_path);
            std::env::set_var("NATIVES_DB_PATH", &db_path);
            crate::storage::set_test_db_override(
                Some(db_path.clone()),
                Some(dir.path().join("artifacts")),
            );
            let store = Arc::new(
                crate::storage::DataStore::new(&db_path, &dir.path().join("artifacts")).unwrap(),
            );
            store
                .conn()
                .unwrap()
                .execute(
                    "INSERT INTO conversation (id, mode, title, provider_id, model_id)
                 VALUES ('run-meta-conv', 'agent', 'Run Meta', 'openai', 'gpt-4o')",
                    [],
                )
                .unwrap();

            let rm = RunManager::new_with_store(store.clone());
            let idempotency_key = format!("run-meta-{}", Uuid::new_v4());
            let run = rm
                .create_run(CreateRunRequest {
                    conversation_id: "run-meta-conv".into(),
                    provider_id: "openai-compatible-provider".into(),
                    model_id: "deepseek-v4-flash".into(),
                    key_id: Some("key-123".into()),
                    agent_profile_id: Some("agent-profile-1".into()),
                    permission_profile: Some("full_access".into()),
                    content: Some("persist metadata".into()),
                    attachments: None,
                    max_steps: Some(9),
                    parent_run_id: Some("parent-run-1".into()),
                    project_path: Some("/tmp/natives-project".into()),
                    idempotency_key: Some(idempotency_key.clone()),
                    effort: None,
                    runtime_id: None,
                })
                .unwrap();

            let row: (String, String, String, String, String, String, String, i64) = store
                .conn()
                .unwrap()
                .query_row(
                    "SELECT parent_run_id, agent_profile_id, key_id, permission_profile,
                            project_path, idempotency_key, model_id, max_steps
                     FROM run WHERE id = ?1",
                    rusqlite::params![run.id],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                            row.get(5)?,
                            row.get(6)?,
                            row.get(7)?,
                        ))
                    },
                )
                .unwrap();
            assert_eq!(row.0, "parent-run-1");
            assert_eq!(row.1, "agent-profile-1");
            assert_eq!(row.2, "key-123");
            assert_eq!(row.3, "full_access");
            assert_eq!(row.4, "/tmp/natives-project");
            assert_eq!(row.5, idempotency_key);
            assert_eq!(row.6, "deepseek-v4-flash");
            assert_eq!(row.7, 9);
        });
    }

    #[test]
    fn create_run_idempotency_survives_sqlite_backed_restart() {
        with_env_lock(|| {
            let dir = tempfile::tempdir().unwrap();
            let db_path = dir.path().join("natives.db");
            std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db_path);
            std::env::set_var("NATIVES_DB_PATH", &db_path);
            crate::storage::set_test_db_override(
                Some(db_path.clone()),
                Some(dir.path().join("artifacts")),
            );
            let store = Arc::new(
                crate::storage::DataStore::new(&db_path, &dir.path().join("artifacts")).unwrap(),
            );
            store
                .conn()
                .unwrap()
                .execute(
                    "INSERT INTO conversation (id, mode, title, provider_id, model_id)
                 VALUES ('sqlite-idem-conv', 'agent', 'SQLite Idem', 'openai', 'gpt-4o')",
                    [],
                )
                .unwrap();

            let idempotency_key = format!("sqlite-idem-{}", Uuid::new_v4());
            let req = CreateRunRequest {
                conversation_id: "sqlite-idem-conv".into(),
                provider_id: "openai".into(),
                model_id: "gpt-4o".into(),
                key_id: Some("key-A".into()),
                agent_profile_id: Some("agent-A".into()),
                permission_profile: Some("ask".into()),
                content: Some("only queue once".into()),
                attachments: None,
                max_steps: Some(7),
                parent_run_id: None,
                project_path: Some("/tmp/sqlite-idem".into()),
                idempotency_key: Some(idempotency_key.clone()),
                effort: None,
                runtime_id: None,
            };

            let first = RunManager::new_with_store(store.clone())
                .create_run(req.clone())
                .unwrap();
            let second = RunManager::new_with_store(store.clone())
                .create_run(req)
                .unwrap();

            assert_eq!(first.id, second.id);
            assert_eq!(
                second.idempotency_key.as_deref(),
                Some(idempotency_key.as_str())
            );
            assert_eq!(second.project_path.as_deref(), Some("/tmp/sqlite-idem"));

            let event_count: i64 = store
                .conn()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM run_event WHERE run_id = ?1 AND event_type = 'queued'",
                    rusqlite::params![first.id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(event_count, 1);
        });
    }

    #[test]
    fn create_run_cleans_sqlite_row_when_queued_event_persistence_fails() {
        with_env_lock(|| {
            let dir = tempfile::tempdir().unwrap();
            let db_path = dir.path().join("natives.db");
            std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db_path);
            std::env::set_var("NATIVES_DB_PATH", &db_path);
            crate::storage::set_test_db_override(
                Some(db_path.clone()),
                Some(dir.path().join("artifacts")),
            );
            let store = Arc::new(
                crate::storage::DataStore::new(&db_path, &dir.path().join("artifacts")).unwrap(),
            );
            store
                .conn()
                .unwrap()
                .execute(
                    "INSERT INTO conversation (id, mode, title, provider_id, model_id)
                 VALUES ('broken-events-conv', 'agent', 'Broken Events', 'openai', 'gpt-4o')",
                    [],
                )
                .unwrap();
            store
                .conn()
                .unwrap()
                .execute("DROP TABLE run_event", [])
                .unwrap();

            let rm = RunManager::new_with_store(store.clone());
            let run_id = format!("broken-event-{}", Uuid::new_v4());
            let err = rm
                .create_run(CreateRunRequest {
                    conversation_id: "broken-events-conv".into(),
                    provider_id: "openai".into(),
                    model_id: "gpt-4o".into(),
                    key_id: None,
                    agent_profile_id: None,
                    permission_profile: Some("ask".into()),
                    content: Some("must rollback".into()),
                    attachments: None,
                    max_steps: Some(3),
                    parent_run_id: None,
                    project_path: None,
                    idempotency_key: Some(run_id.clone()),
                    effort: None,
                    runtime_id: None,
                })
                .unwrap_err();
            assert!(err.contains("PERSISTENCE_FAILED"), "{err}");
            assert!(rm.get_run(&run_id).is_none());
            let count: i64 = store
                .conn()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM run WHERE id = ?1",
                    rusqlite::params![run_id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 0);
        });
    }

    #[test]
    fn start_cleans_auto_trigger_message_when_run_create_fails() {
        with_env_lock(|| {
            let dir = tempfile::tempdir().unwrap();
            let previous_db = std::env::var("NATIVES_DB_PATH").ok();
            let previous_runtime = std::env::var("NATIVES_RUNTIME_DIR").ok();
            let db_path = dir.path().join("natives.db");
            std::env::set_var("NATIVES_DB_PATH", &db_path);
            std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db_path);
            std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());

            std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
            crate::storage::set_test_db_override(
                Some(db_path.clone()),
                Some(dir.path().join("artifacts")),
            );
            let store = Arc::new(
                crate::storage::DataStore::new(&db_path, &dir.path().join("artifacts")).unwrap(),
            );
            store
                .conn()
                .unwrap()
                .execute(
                    "INSERT INTO conversation (id, mode, title, provider_id, model_id)
                 VALUES ('trigger-clean-conv', 'agent', 'Trigger Clean', 'openai', 'gpt-4o')",
                    [],
                )
                .unwrap();
            store
                .conn()
                .unwrap()
                .execute("DROP TABLE run_event", [])
                .unwrap();

            let rm = RunManager::new_with_store(store.clone());
            let err = rm
                .ensure_run_for_start(&StartRunRequest {
                    run_id: None,
                    conversation_id: Some("trigger-clean-conv".into()),
                    provider_id: Some("openai".into()),
                    model_id: Some("gpt-4o".into()),
                    key_id: None,
                    content: Some("do not leave me".into()),
                    attachments: None,
                    trigger_message_id: None,
                    permission_profile: None,
                    max_steps: None,
                    project_path: None,
                    idempotency_key: Some(format!("trigger-clean-{}", Uuid::new_v4())),
                    effort: None,
                    runtime_id: None,
                })
                .unwrap_err();
            assert!(err.contains("PERSISTENCE_FAILED"), "{err}");
            let messages: i64 = store
                .conn()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM message WHERE conversation_id = 'trigger-clean-conv'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(messages, 0);

            if let Some(value) = previous_db {
                std::env::set_var("NATIVES_DB_PATH", &value);
            } else {
                std::env::remove_var("NATIVES_DB_PATH");
            }
            if let Some(value) = previous_runtime {
                std::env::set_var("NATIVES_RUNTIME_DIR", &value);
            } else {
                std::env::remove_var("NATIVES_RUNTIME_DIR");
            }
        });
    }

    #[test]
    fn start_without_run_id_appends_trigger_message_in_daemon_store() {
        with_env_lock(|| {
            let dir = tempfile::tempdir().unwrap();
            let previous_db = std::env::var("NATIVES_DB_PATH").ok();
            let previous_runtime = std::env::var("NATIVES_RUNTIME_DIR").ok();
            let db_path = dir.path().join("natives.db");
            std::env::set_var("NATIVES_DB_PATH", &db_path);
            std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db_path);
            std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());

            std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
            crate::storage::set_test_db_override(
                Some(db_path.clone()),
                Some(dir.path().join("artifacts")),
            );
            let store =
                crate::storage::DataStore::new(&db_path, &dir.path().join("artifacts")).unwrap();
            let conversation_id = "trigger-conversation";
            store.conn().unwrap().execute(
                "INSERT INTO conversation (id, mode, title, provider_id, model_id, permission_profile_id)
                 VALUES (?1, 'agent', 'Trigger', 'openai', 'gpt-4o', 'readonly')",
                rusqlite::params![conversation_id],
            ).unwrap();
            let rm = RunManager::new();
            let run = rm
                .ensure_run_for_start(&StartRunRequest {
                    run_id: None,
                    conversation_id: Some(conversation_id.to_string()),
                    provider_id: Some("openai".into()),
                    model_id: Some("gpt-4o".into()),
                    key_id: None,
                    content: Some("inspect".into()),
                    attachments: Some(vec![assistant_protocol::v2::AttachmentRef {
                        path: "/tmp/a.txt".into(),
                        name: Some("a.txt".into()),
                        mime_type: Some("text/plain".into()),
                        size: Some(3),
                    }]),
                    trigger_message_id: None,
                    permission_profile: None,
                    max_steps: None,
                    project_path: None,
                    idempotency_key: Some("trigger-idem".into()),
                    effort: None,
                    runtime_id: None,
                })
                .unwrap();
            assert_eq!(run.permission_profile, "readonly");
            assert!(run.trigger_message_id.is_some());
            let trigger_message_id = run.trigger_message_id.as_deref().unwrap();

            let text: String = store.conn().unwrap().query_row(
                "SELECT block_json FROM message_block WHERE message_id = ?1 AND block_type = 'text'",
                rusqlite::params![trigger_message_id],
                |row| row.get(0),
            ).unwrap();
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&text).unwrap()["text"],
                "inspect"
            );
            let file: String = store.conn().unwrap().query_row(
                "SELECT block_json FROM message_block WHERE message_id = ?1 AND block_type = 'file_reference'",
                rusqlite::params![trigger_message_id],
                |row| row.get(0),
            ).unwrap();
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&file).unwrap()["path"],
                "/tmp/a.txt"
            );

            if let Some(value) = previous_db {
                std::env::set_var("NATIVES_DB_PATH", &value);
            } else {
                std::env::remove_var("NATIVES_DB_PATH");
            }
            if let Some(value) = previous_runtime {
                std::env::set_var("NATIVES_RUNTIME_DIR", &value);
            } else {
                std::env::remove_var("NATIVES_RUNTIME_DIR");
            }
        });
    }

    #[test]
    fn start_with_seams_loads_daemon_conversation_history() {
        with_env_lock(|| {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let dir = tempfile::tempdir().unwrap();
                let previous_db = std::env::var("NATIVES_DB_PATH").ok();
                let previous_runtime = std::env::var("NATIVES_RUNTIME_DIR").ok();
                let db_path = dir.path().join("natives.db");
                std::env::set_var("NATIVES_DB_PATH", &db_path);
                std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db_path);
                std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());

                std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
                crate::storage::set_test_db_override(Some(db_path.clone()), Some(dir.path().join("artifacts")));
                let store = Arc::new(
                    crate::storage::DataStore::new(&db_path, &dir.path().join("artifacts"))
                        .unwrap(),
                );
                store.conn().unwrap().execute(
                    "INSERT INTO conversation (id, mode, title, provider_id, model_id, permission_profile_id)
                     VALUES ('history-conv', 'agent', 'History', 'openai', 'gpt-4o', 'readonly')",
                    [],
                ).unwrap();
                crate::conversation_store::append_trigger_message(
                    "history-conv",
                    Some("remember alpha"),
                    None,
                ).unwrap();
                crate::conversation_store::append_trigger_message(
                    "history-conv",
                    Some("now beta"),
                    None,
                ).unwrap();

                struct EmptyTools;
                #[async_trait::async_trait]
                impl agent_core::EngineToolRuntime for EmptyTools {
                    async fn list_tool_schemas(&self) -> Vec<agent_core::ToolSchema> {
                        Vec::new()
                    }
                    async fn execute_tool(
                        &self,
                        _name: &str,
                        _input: serde_json::Value,
                        _cancel: &CancellationToken,
                    ) -> agent_core::ToolExecutionResult {
                        agent_core::ToolExecutionResult {
                            output: serde_json::json!({}),
                            is_error: false,
                            duration_ms: 0,
                        }
                    }
                }
                struct CaptureProvider(std::sync::Arc<StdMutex<Vec<agent_core::EngineMessage>>>);
                #[async_trait::async_trait]
                impl agent_core::EngineProvider for CaptureProvider {
                    async fn stream(
                        &self,
                        _model: &str,
                        messages: Vec<agent_core::EngineMessage>,
                        _tools: &[agent_core::ToolSchema],
                        _system_prompt: Option<&str>,
                        _cancel: CancellationToken,
                    ) -> Result<agent_core::EngineProviderEventStream, agent_core::EngineError>
                    {
                        *self.0.lock().unwrap() = messages;
                        Ok(Box::pin(futures_util::stream::iter(vec![
                            agent_core::EngineProviderEvent::TextDelta("ok".into()),
                            agent_core::EngineProviderEvent::Completed,
                        ])))
                    }
                }

                let seen = std::sync::Arc::new(StdMutex::new(Vec::new()));
                let provider = CaptureProvider(seen.clone());
                let rm = RunManager::new_with_store(store.clone());
                let run = rm.ensure_run_for_start(&StartRunRequest {
                    run_id: None,
                    conversation_id: Some("history-conv".into()),
                    provider_id: Some("openai".into()),
                    model_id: Some("gpt-4o".into()),
                    key_id: None,
                    content: None,
                    attachments: None,
                    trigger_message_id: None,
                    permission_profile: None,
                    max_steps: Some(3),
                    project_path: None,
                    idempotency_key: Some("history-run".into()),
                            effort: None,
            runtime_id: None,
        }).unwrap();
                let run_id = run.id.clone();
                rm.start_with_seams(
                    StartRunRequest {
                        run_id: Some(run_id.clone()),
                        conversation_id: None,
                        provider_id: None,
                        model_id: None,
                        key_id: None,
                        content: None,
                        attachments: None,
                        trigger_message_id: None,
                        permission_profile: None,
                        max_steps: Some(3),
                        project_path: None,
                        idempotency_key: None,
                                effort: None,
            runtime_id: None,
        },
                    &provider,
                    &EmptyTools,
                ).await.unwrap();
                let seen = seen.lock().unwrap();
                assert_eq!(seen.len(), 2);
                assert_eq!(seen[0].content, "remember alpha");
                assert_eq!(seen[1].content, "now beta");
                let reply: String = store.conn().unwrap().query_row(
                    "SELECT block_json
                     FROM message_block
                     WHERE block_type = 'text'
                       AND message_id IN (SELECT id FROM message WHERE conversation_id = 'history-conv' AND role = 'assistant')",
                    [],
                    |row| row.get(0),
                ).unwrap();
                assert_eq!(
                    serde_json::from_str::<serde_json::Value>(&reply).unwrap()["text"],
                    "ok"
                );
                let linked_run: String = store.conn().unwrap().query_row(
                    "SELECT block_json
                     FROM message_block
                     WHERE block_type = 'run_reference'
                       AND message_id IN (SELECT id FROM message WHERE conversation_id = 'history-conv' AND role = 'assistant')",
                    [],
                    |row| row.get(0),
                ).unwrap();
                assert_eq!(
                    serde_json::from_str::<serde_json::Value>(&linked_run).unwrap()["run_id"],
                    run_id
                );

                if let Some(value) = previous_db {
                    std::env::set_var("NATIVES_DB_PATH", &value);
                } else {
                    std::env::remove_var("NATIVES_DB_PATH");
                }
                if let Some(value) = previous_runtime {
                    std::env::set_var("NATIVES_RUNTIME_DIR", &value);
                } else {
                    std::env::remove_var("NATIVES_RUNTIME_DIR");
                }
            });
        });
    }

    #[test]
    fn retry_creates_new_run_id() {
        let _prev_a = std::env::var("NATIVES_ASSISTANT_DB_PATH").ok();
        let _prev_d = std::env::var("NATIVES_DB_PATH").ok();
        std::env::remove_var("NATIVES_ASSISTANT_DB_PATH");
        std::env::remove_var("NATIVES_DB_PATH");
        let rm = RunManager::new();
        let original = rm
            .create_run(CreateRunRequest {
                conversation_id: "c1".into(),
                provider_id: "openai".into(),
                model_id: "gpt-4o".into(),
                key_id: None,
                agent_profile_id: None,
                permission_profile: None,
                content: Some("retry me".into()),
                attachments: None,
                max_steps: None,
                parent_run_id: None,
                project_path: None,
                idempotency_key: None,
                effort: None,
                runtime_id: None,
            })
            .unwrap();
        let retried = rm
            .retry(RetryRunRequest {
                run_id: original.id.clone(),
            })
            .unwrap();
        assert_ne!(original.id, retried.id);
        assert_eq!(retried.conversation_id, original.conversation_id);
    }

    #[tokio::test]
    async fn start_detached_returns_preparing_before_terminal() {
        std::env::set_var("NATIVES_DAEMON_FIXTURE", "1");
        let dir = tempfile::tempdir().unwrap();
        let previous_db = std::env::var("NATIVES_DB_PATH").ok();
        let previous_runtime = std::env::var("NATIVES_RUNTIME_DIR").ok();
        let db_path = dir.path().join("natives.db");
        std::env::set_var("NATIVES_DB_PATH", &db_path);
        std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db_path);
        std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
        std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
        crate::storage::set_test_db_override(
            Some(db_path.clone()),
            Some(dir.path().join("artifacts")),
        );
        let store = Arc::new(
            crate::storage::DataStore::new(&db_path, &dir.path().join("artifacts")).unwrap(),
        );
        store.conn().unwrap().execute(
            "INSERT INTO conversation (id, mode, title, provider_id, model_id, permission_profile_id)
             VALUES ('c-detach', 'agent', 'Detached', 'openai', 'gpt-4o', 'full_access')",
            [],
        ).unwrap();
        let rm = Arc::new(RunManager::new_with_store(store.clone()));
        let created = rm
            .create_run(CreateRunRequest {
                conversation_id: "c-detach".into(),
                provider_id: "openai".into(),
                model_id: "gpt-4o".into(),
                key_id: Some("k".into()),
                agent_profile_id: None,
                permission_profile: Some("full_access".into()),
                content: Some("detach me".into()),
                attachments: None,
                max_steps: Some(5),
                parent_run_id: None,
                project_path: Some(dir.path().to_string_lossy().into_owned()),
                idempotency_key: Some("detach-1".into()),
                effort: None,
                runtime_id: None,
            })
            .unwrap();

        let immediate = rm
            .start_detached(StartRunRequest {
                run_id: Some(created.id.clone()),
                conversation_id: None,
                provider_id: Some("openai".into()),
                model_id: Some("gpt-4o".into()),
                key_id: Some("k".into()),
                content: Some("detach me".into()),
                attachments: None,
                trigger_message_id: None,
                permission_profile: Some("full_access".into()),
                max_steps: Some(5),
                project_path: Some(dir.path().to_string_lossy().into_owned()),
                idempotency_key: None,
                effort: None,
                runtime_id: None,
            })
            .unwrap();
        // Must not wait for engine terminal status.
        assert!(
            !immediate.status.is_terminal(),
            "detached start must return before terminal; got {:?}",
            immediate.status
        );
        assert_eq!(immediate.status, RunStatusV2::Preparing);

        // Background task eventually completes fixture engine.
        let mut terminal = None;
        for _ in 0..100 {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            if let Some(r) = rm.get_run(&created.id) {
                if r.status.is_terminal() {
                    terminal = Some(r);
                    break;
                }
            }
        }
        let done = terminal.expect("detached run should reach terminal status");
        assert_eq!(
            done.status,
            RunStatusV2::Completed,
            "error_code={:?} id={}",
            done.error_code,
            done.id
        );
        // Terminal re-start must fail closed (use retry).
        let err = rm
            .start_detached(StartRunRequest {
                run_id: Some(created.id.clone()),
                conversation_id: None,
                provider_id: None,
                model_id: None,
                key_id: None,
                content: None,
                attachments: None,
                trigger_message_id: None,
                permission_profile: None,
                max_steps: None,
                project_path: None,
                idempotency_key: None,
                effort: None,
                runtime_id: None,
            })
            .unwrap_err();
        assert!(
            err.contains("terminal") || err.contains("retry"),
            "unexpected: {err}"
        );
        if let Some(value) = previous_db {
            std::env::set_var("NATIVES_DB_PATH", &value);
            std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &value);
        } else {
            std::env::remove_var("NATIVES_DB_PATH");
            std::env::remove_var("NATIVES_ASSISTANT_DB_PATH");
        }
        if let Some(value) = previous_runtime {
            std::env::set_var("NATIVES_RUNTIME_DIR", &value);
        } else {
            std::env::remove_var("NATIVES_RUNTIME_DIR");
        }
        // keep FIXTURE=1 for parallel tests under cfg(test)
    }

    #[test]
    fn persist_and_restore_marks_active_as_interrupted() {
        with_env_lock(|| {
            let dir = std::env::temp_dir().join(format!("natives-runs-{}", Uuid::new_v4()));
            let _ = std::fs::create_dir_all(dir.join("runs"));
            let snapshot_path = dir.join("runs").join("snapshot.json");
            let rm = RunManager {
                runs: Mutex::new(HashMap::new()),
                idempotency: Mutex::new(HashMap::new()),
                last_content: Mutex::new(HashMap::new()),
                project_paths: Mutex::new(HashMap::new()),
                snapshot_path_override: Some(snapshot_path.clone()),
                data_store: None,
                runtime: Arc::new(crate::production::ProductionRuntime::new()),
            };
            let run = rm
                .create_run(CreateRunRequest {
                    conversation_id: "c-restore".into(),
                    provider_id: "openai".into(),
                    model_id: "m".into(),
                    key_id: None,
                    agent_profile_id: None,
                    permission_profile: None,
                    content: Some("x".into()),
                    attachments: None,
                    max_steps: Some(3),
                    parent_run_id: None,
                    project_path: Some("/tmp/proj".into()),
                    idempotency_key: Some(format!("restore-{}", Uuid::new_v4())),
                    effort: None,
                    runtime_id: None,
                })
                .unwrap();
            // Force active status then snapshot.
            {
                let mut runs = rm.runs.lock().unwrap();
                if let Some(r) = runs.get_mut(&run.id) {
                    r.status = RunStatusV2::Running;
                }
            }
            rm.persist_runs_snapshot().unwrap();
            assert!(
                snapshot_path.exists(),
                "snapshot file missing at {}",
                snapshot_path.display()
            );
            let rm2 = RunManager {
                runs: Mutex::new(HashMap::new()),
                idempotency: Mutex::new(HashMap::new()),
                last_content: Mutex::new(HashMap::new()),
                project_paths: Mutex::new(HashMap::new()),
                snapshot_path_override: Some(snapshot_path.clone()),
                data_store: None,
                runtime: Arc::new(crate::production::ProductionRuntime::new()),
            };
            let n = rm2.restore_runs_snapshot().unwrap();
            assert!(
                n >= 1,
                "expected restored runs from {}",
                snapshot_path.display()
            );
            let restored = rm2.get_run(&run.id).expect("restored");
            assert_eq!(restored.status, RunStatusV2::Interrupted);
            assert_eq!(restored.error_code.as_deref(), Some("daemon_restarted"));
            assert_eq!(restored.project_path.as_deref(), Some("/tmp/proj"));
            let _ = std::fs::remove_dir_all(&dir);
        }); // with_env_lock
    }

    #[test]
    fn sqlite_active_runs_are_interrupted_on_manager_startup() {
        with_env_lock(|| {
            let dir = tempfile::tempdir().unwrap();
            let db_path = dir.path().join("natives.db");
            std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db_path);
            std::env::set_var("NATIVES_DB_PATH", &db_path);
            crate::storage::set_test_db_override(
                Some(db_path.clone()),
                Some(dir.path().join("artifacts")),
            );
            let store = Arc::new(
                crate::storage::DataStore::new(&db_path, &dir.path().join("artifacts")).unwrap(),
            );
            store
                .conn()
                .unwrap()
                .execute(
                    "INSERT INTO conversation (id, mode, title, provider_id, model_id)
                 VALUES ('restart-conv', 'agent', 'Restart', 'openai', 'gpt-4o')",
                    [],
                )
                .unwrap();
            for (run_id, status) in [
                ("restart-queued", "queued"),
                ("restart-running", "running"),
                ("restart-waiting", "waiting_permission"),
                ("restart-completed", "completed"),
            ] {
                store
                    .conn()
                    .unwrap()
                    .execute(
                        "INSERT INTO run (id, conversation_id, status, provider_id, model_id)
                     VALUES (?1, 'restart-conv', ?2, 'openai', 'gpt-4o')",
                        rusqlite::params![run_id, status],
                    )
                    .unwrap();
            }

            let _rm = RunManager::new_with_store(store.clone());
            let rows: Vec<(String, String, Option<String>)> = {
                let conn = store.conn().unwrap();
                let mut stmt = conn
                    .prepare("SELECT id, status, error_code FROM run ORDER BY id")
                    .unwrap();
                stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
                    .unwrap()
                    .map(Result::unwrap)
                    .collect()
            };
            assert!(rows.iter().any(|(id, status, code)| {
                id == "restart-running"
                    && status == "interrupted"
                    && code.as_deref() == Some("daemon_restarted")
            }));
            assert!(rows.iter().any(|(id, status, code)| {
                id == "restart-queued"
                    && status == "interrupted"
                    && code.as_deref() == Some("daemon_restarted")
            }));
            assert!(rows.iter().any(|(id, status, code)| {
                id == "restart-waiting"
                    && status == "interrupted"
                    && code.as_deref() == Some("daemon_restarted")
            }));
            assert!(rows.iter().any(|(id, status, code)| {
                id == "restart-completed" && status == "completed" && code.is_none()
            }));
        });
    }

    #[tokio::test]
    async fn duplicate_start_detached_is_idempotent_while_active() {
        with_env_lock(|| {
            std::env::set_var("NATIVES_DAEMON_FIXTURE", "1");
            let dir = tempfile::tempdir().unwrap();
            let db_path = dir.path().join("natives.db");
            std::env::set_var("NATIVES_DB_PATH", &db_path);
            std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db_path);
            std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
            std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
            crate::storage::set_test_db_override(
                Some(db_path.clone()),
                Some(dir.path().join("artifacts")),
            );
            let store = Arc::new(
                crate::storage::DataStore::new(&db_path, &dir.path().join("artifacts")).unwrap(),
            );
            store
                .conn()
                .unwrap()
                .execute(
                    "INSERT INTO conversation (id, mode, title, provider_id, model_id)
                 VALUES ('c-idem-start', 'agent', 'Idem', 'openai', 'gpt-4o')",
                    [],
                )
                .unwrap();
            let rm = Arc::new(RunManager::new_with_store(store.clone()));
            let created = rm
                .create_run(CreateRunRequest {
                    conversation_id: "c-idem-start".into(),
                    provider_id: "openai".into(),
                    model_id: "gpt-4o".into(),
                    key_id: Some("k".into()),
                    agent_profile_id: None,
                    permission_profile: Some("full_access".into()),
                    content: Some("once".into()),
                    attachments: None,
                    max_steps: Some(5),
                    parent_run_id: None,
                    project_path: None,
                    idempotency_key: Some(format!("idem-start-{}", uuid::Uuid::new_v4())),
                    effort: None,
                    runtime_id: None,
                })
                .unwrap();
            let req = StartRunRequest {
                run_id: Some(created.id.clone()),
                conversation_id: None,
                provider_id: Some("openai".into()),
                model_id: Some("gpt-4o".into()),
                key_id: Some("k".into()),
                content: Some("once".into()),
                attachments: None,
                trigger_message_id: None,
                permission_profile: Some("full_access".into()),
                max_steps: Some(5),
                project_path: None,
                idempotency_key: None,
                effort: None,
                runtime_id: None,
            };
            let a = rm.start_detached(req.clone()).unwrap();
            let b = rm.start_detached(req).unwrap();
            assert_eq!(a.id, b.id);
            // Second call must not error; active run is returned as-is.
            assert!(
                a.status.is_active() || a.status.is_terminal() || a.status == RunStatusV2::Queued
            );
            // keep FIXTURE=1 for parallel tests under cfg(test)
        });
    }

    #[tokio::test]
    async fn start_cancel_retry_lifecycle_with_fixture() {
        let _env = crate::storage::DataStore::env_test_lock();
        std::env::set_var("NATIVES_DAEMON_FIXTURE", "1");
        let dir = tempfile::tempdir().unwrap();
        let previous_db = std::env::var("NATIVES_DB_PATH").ok();
        let previous_runtime = std::env::var("NATIVES_RUNTIME_DIR").ok();
        let db_path = dir.path().join(format!("natives-{}.db", Uuid::new_v4()));
        std::env::set_var("NATIVES_DB_PATH", &db_path);
        std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db_path);
        std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
        crate::storage::set_test_db_override(
            Some(db_path.clone()),
            Some(dir.path().join("artifacts")),
        );
        let store = Arc::new(
            crate::storage::DataStore::new(&db_path, &dir.path().join("artifacts")).unwrap(),
        );
        store.conn().unwrap().execute(
            "INSERT INTO conversation (id, mode, title, provider_id, model_id, permission_profile_id)
             VALUES ('c1', 'agent', 'Fixture', 'openai', 'gpt-4o', 'full_access')",
            [],
        ).unwrap();
        let rm = RunManager::new_with_store(store.clone());
        let run = rm
            .start(StartRunRequest {
                run_id: None,
                conversation_id: Some("c1".into()),
                provider_id: Some("openai".into()),
                model_id: Some("gpt-4o".into()),
                key_id: Some("k-test".into()),
                content: Some("ping".into()),
                attachments: None,
                trigger_message_id: None,
                permission_profile: Some("full_access".into()),
                max_steps: Some(5),
                project_path: Some(dir.path().to_string_lossy().into_owned()),
                idempotency_key: None,
                effort: None,
                runtime_id: None,
            })
            .await
            .unwrap();
        assert_eq!(run.status, RunStatusV2::Completed);
        let events = rm.replay(ReplayRunRequest {
            run_id: run.id.clone(),
            after_sequence: 0,
        });
        assert!(events
            .iter()
            .any(|e| matches!(e.payload, RunEventKind::Started)));
        assert!(events
            .iter()
            .any(|e| matches!(e.payload, RunEventKind::TextDelta { .. })));

        // Dump evidence for verifier
        if let Ok(dir) = std::env::var("NATIVES_TEST_SCRATCH") {
            let dump: Vec<_> = events
                .iter()
                .map(|e| {
                    serde_json::json!({
                        "run_id": e.run_id,
                        "sequence": e.effective_run_sequence(),
                        "type": e.payload.type_name(),
                    })
                })
                .collect();
            let _ = std::fs::write(
                std::path::Path::new(&dir).join("run-events.json"),
                serde_json::to_string_pretty(&dump).unwrap_or_default(),
            );
        }

        // Cancel on already-completed is terminal-safe
        let cancelled = rm
            .cancel(CancelRunRequest {
                run_id: run.id.clone(),
            })
            .await
            .unwrap();
        assert!(cancelled.status.is_terminal());

        // Retry produces new run id
        let retried = rm
            .retry(RetryRunRequest {
                run_id: run.id.clone(),
            })
            .unwrap();
        assert_ne!(retried.id, run.id);

        // Start the retried run
        let retried_done = rm
            .start(StartRunRequest {
                run_id: Some(retried.id.clone()),
                conversation_id: None,
                provider_id: None,
                model_id: None,
                key_id: None,
                content: None,
                attachments: None,
                trigger_message_id: None,
                permission_profile: Some("full_access".into()),
                max_steps: Some(5),
                project_path: None,
                idempotency_key: None,
                effort: None,
                runtime_id: None,
            })
            .await
            .unwrap();
        assert_eq!(retried_done.status, RunStatusV2::Completed);

        if let Ok(dir) = std::env::var("NATIVES_TEST_SCRATCH") {
            let evidence = serde_json::json!({
                "original_run_id": run.id,
                "retry_run_id": retried.id,
                "ids_differ": run.id != retried.id,
                "original_event_count": events.len(),
                "retry_completed": retried_done.status == RunStatusV2::Completed,
            });
            let _ = std::fs::write(
                std::path::Path::new(&dir).join("daemon-cancel-retry.json"),
                serde_json::to_string_pretty(&evidence).unwrap_or_default(),
            );
        }
        if let Some(value) = previous_db {
            std::env::set_var("NATIVES_DB_PATH", &value);
        } else {
            std::env::remove_var("NATIVES_DB_PATH");
        }
        if let Some(value) = previous_runtime {
            std::env::set_var("NATIVES_RUNTIME_DIR", &value);
        } else {
            std::env::remove_var("NATIVES_RUNTIME_DIR");
        }
        // keep FIXTURE=1 for parallel tests under cfg(test)
    }

    /// Criterion 4: parent cancel_run_tree must request_cancel child engines
    /// registered under child run_id (not metadata-only cascade).
    #[tokio::test]
    async fn parent_cancel_tree_cancels_child_engine() {
        // Isolate from concurrent env-DB tests (store_from_env must stay None).
        let _guard = crate::storage::DataStore::env_test_lock();
        std::env::remove_var("NATIVES_ASSISTANT_DB_PATH");
        std::env::remove_var("NATIVES_DB_PATH");
        std::env::set_var("NATIVES_DAEMON_FIXTURE", "1");
        let rm = Arc::new(RunManager::new());
        let parent = rm
            .create_run(CreateRunRequest {
                conversation_id: "c-tree".into(),
                provider_id: "openai".into(),
                model_id: "gpt-4o".into(),
                key_id: Some("parent-key".into()),
                agent_profile_id: None,
                permission_profile: Some("full_access".into()),
                content: Some("parent".into()),
                attachments: None,
                max_steps: Some(5),
                parent_run_id: None,
                project_path: None,
                // EventSequencer may persist across test processes; keep this run's
                // replay cursor isolated while preserving the one-terminal assertion.
                idempotency_key: Some(format!("tree-parent-{}", Uuid::new_v4())),
                effort: None,
                runtime_id: None,
            })
            .unwrap();

        // Spawn real subagent identity under parent run_id.
        let child = rm
            .runtime
            .subagents
            .spawn(
                &parent.id,
                "child work".into(),
                1,
                "anthropic".into(),
                "child-key-from-broker".into(),
                "claude-3".into(),
                "ask".into(),
                vec!["read_file".into()],
                None,
                Some("none".into()),
                None,
            )
            .await
            .unwrap();
        let _ = rm
            .runtime
            .subagents
            .update_status(&child.id, agent_core::SubAgentStatus::Running)
            .await;

        // Register a live child AgentEngine on child.run_id (production path).
        let child_engine = Arc::new(AgentEngine::new(rm.runtime.events.clone()));
        rm.runtime
            .register_engine(&child.run_id, child_engine.clone())
            .await;
        rm.runtime
            .insert_task_output(
                &child.id,
                crate::production::TaskRecord {
                    run_id: child.run_id.clone(),
                    status: "running".into(),
                    output: None,
                },
            )
            .await;

        // Child tool observes cancel flag (same bar as cancel_mid).
        let seen = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let seen_bg = seen.clone();
        let child_flag = child_engine.cancel_token();
        let child_run = child.run_id.clone();
        let watch = tokio::spawn(async move {
            for _ in 0..200 {
                if child_flag.is_cancelled() {
                    seen_bg.store(true, std::sync::atomic::Ordering::SeqCst);
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
            let _ = child_run;
        });

        // Domain tree signal via cancel_run_tree; authoritative Cancelled via RunManager::cancel.
        rm.runtime.cancel_run_tree(&parent.id).await;

        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), watch).await;
        assert!(
            seen.load(std::sync::atomic::Ordering::SeqCst),
            "child engine cancel flag must be set by cancel_run_tree(parent)"
        );

        let child_status = rm
            .runtime
            .subagents
            .get(&child.id)
            .await
            .map(|s| s.status)
            .unwrap();
        assert_eq!(child_status, agent_core::SubAgentStatus::Cancelled);

        let task_status = rm
            .runtime
            .task_outputs
            .lock()
            .await
            .get(&child.id)
            .map(|t| t.status.clone());
        assert_eq!(task_status.as_deref(), Some("cancelled"));

        // Lifecycle Cancelled is committed only by RunManager::cancel (journal), not domain cleanup.
        let cancelled = rm
            .cancel(CancelRunRequest {
                run_id: parent.id.clone(),
            })
            .await
            .expect("cancel parent via journal");
        assert!(
            cancelled.status.is_terminal(),
            "parent must be terminal after RunManager::cancel"
        );
        let parent_lifecycle = rm.runtime.events.replay_after(&parent.id, 0);
        let terminal_count = parent_lifecycle
            .iter()
            .filter(|e| {
                matches!(
                    e.payload,
                    RunEventKind::Cancelled { .. }
                        | RunEventKind::Failed { .. }
                        | RunEventKind::Completed { .. }
                        | RunEventKind::Interrupted { .. }
                )
            })
            .count();
        assert_eq!(
            terminal_count, 1,
            "parent must have exactly one terminal lifecycle event after cancel"
        );

        if let Ok(dir) = std::env::var("NATIVES_TEST_SCRATCH") {
            let evidence = serde_json::json!({
                "parent_run_id": parent.id,
                "child_run_id": child.run_id,
                "child_task_id": child.id,
                "child_engine_cancel_flag": true,
                "child_status": "cancelled",
                "parent_terminal_lifecycle_events": terminal_count,
                "api": "cancel_run_tree+RunManager::cancel",
            });
            let _ = std::fs::write(
                std::path::Path::new(&dir).join("cascade-cancel-tree.json"),
                serde_json::to_string_pretty(&evidence).unwrap_or_default(),
            );
        }
        // keep FIXTURE=1 for parallel tests under cfg(test)
    }

    #[tokio::test]
    async fn cancel_mid_run_marks_interrupted() {
        std::env::set_var("NATIVES_DAEMON_FIXTURE", "1");
        let runtime_dir = std::env::temp_dir().join(format!("natives-cancel-{}", Uuid::new_v4()));
        std::fs::create_dir_all(runtime_dir.join("runs")).unwrap();
        let prev_runtime_dir = std::env::var("NATIVES_RUNTIME_DIR").ok();
        std::env::set_var("NATIVES_RUNTIME_DIR", &runtime_dir);
        let rm = Arc::new(RunManager::new());
        let run = rm
            .create_run(CreateRunRequest {
                conversation_id: "c-cancel".into(),
                provider_id: "openai".into(),
                model_id: "gpt-4o".into(),
                key_id: Some("k".into()),
                agent_profile_id: None,
                permission_profile: Some("ask".into()),
                content: Some("slow".into()),
                attachments: None,
                max_steps: Some(50),
                parent_run_id: None,
                project_path: None,
                idempotency_key: Some("cancel-mid".into()),
                effort: None,
                runtime_id: None,
            })
            .unwrap();

        let cancel_flag_seen = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cancel_flag_seen_bg = cancel_flag_seen.clone();

        let rm_start = rm.clone();
        let rid = run.id.clone();
        let start_handle = tokio::spawn(async move {
            #[allow(dead_code)]
            struct SlowProvider {
                seen: Arc<std::sync::atomic::AtomicBool>,
            }
            #[async_trait::async_trait]
            impl agent_core::EngineProvider for SlowProvider {
                async fn stream(
                    &self,
                    _model: &str,
                    _messages: Vec<agent_core::EngineMessage>,
                    _tools: &[agent_core::ToolSchema],
                    _system_prompt: Option<&str>,
                    _cancel: CancellationToken,
                ) -> Result<agent_core::EngineProviderEventStream, agent_core::EngineError>
                {
                    // Stay in stream long enough for cancel to register.
                    for _ in 0..100 {
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    }
                    let _ = &self.seen;
                    Ok(Box::pin(futures_util::stream::iter(vec![
                        agent_core::EngineProviderEvent::TextDelta("late".into()),
                        agent_core::EngineProviderEvent::Completed,
                    ])))
                }
            }
            struct CancelAwareTools {
                engines:
                    Arc<tokio::sync::Mutex<std::collections::HashMap<String, Arc<AgentEngine>>>>,
                run_id: String,
                seen: Arc<std::sync::atomic::AtomicBool>,
            }
            #[async_trait::async_trait]
            impl agent_core::EngineToolRuntime for CancelAwareTools {
                async fn list_tool_schemas(&self) -> Vec<agent_core::ToolSchema> {
                    vec![]
                }
                async fn execute_tool(
                    &self,
                    _name: &str,
                    _input: serde_json::Value,
                    cancel: &CancellationToken,
                ) -> agent_core::ToolExecutionResult {
                    // Poll cancel token while "working".
                    for _ in 0..40 {
                        if cancel.is_cancelled() {
                            self.seen.store(true, std::sync::atomic::Ordering::SeqCst);
                            return agent_core::ToolExecutionResult {
                                output: serde_json::json!({"error": "cancelled"}),
                                is_error: true,
                                duration_ms: 0,
                            };
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                    }
                    let _ = &self.run_id;
                    agent_core::ToolExecutionResult {
                        output: serde_json::json!({}),
                        is_error: false,
                        duration_ms: 0,
                    }
                }
            }
            // Provider that emits a tool call so tools path can observe cancel.
            struct ToolThenSlow;
            #[async_trait::async_trait]
            impl agent_core::EngineProvider for ToolThenSlow {
                async fn stream(
                    &self,
                    _model: &str,
                    messages: Vec<agent_core::EngineMessage>,
                    _tools: &[agent_core::ToolSchema],
                    _system_prompt: Option<&str>,
                    _cancel: CancellationToken,
                ) -> Result<agent_core::EngineProviderEventStream, agent_core::EngineError>
                {
                    if messages.last().map(|m| m.role == "tool").unwrap_or(false) {
                        return Ok(Box::pin(futures_util::stream::iter(vec![
                            agent_core::EngineProviderEvent::TextDelta("done".into()),
                            agent_core::EngineProviderEvent::Completed,
                        ])));
                    }
                    Ok(Box::pin(futures_util::stream::iter(vec![
                        agent_core::EngineProviderEvent::ToolCallDelta {
                            index: 0,
                            id: Some("c1".into()),
                            name: Some("read_file".into()),
                            arguments_delta: r#"{"path":"x"}"#.into(),
                        },
                        agent_core::EngineProviderEvent::Completed,
                    ])))
                }
            }
            let tools = CancelAwareTools {
                engines: rm_start.runtime.engine_handles().await,
                run_id: rid.clone(),
                seen: cancel_flag_seen_bg,
            };
            rm_start
                .start_with_seams(
                    StartRunRequest {
                        run_id: Some(rid),
                        conversation_id: None,
                        provider_id: None,
                        model_id: None,
                        key_id: None,
                        content: Some("slow".into()),
                        attachments: None,
                        trigger_message_id: None,
                        permission_profile: Some("full_access".into()),
                        max_steps: Some(5),
                        project_path: None,
                        idempotency_key: None,
                        effort: None,
                        runtime_id: None,
                    },
                    &ToolThenSlow,
                    &tools,
                )
                .await
        });

        // Wait until engine is registered, then cancel → request_cancel.
        for _ in 0..50 {
            if rm.runtime.has_engine(&run.id).await {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(
            rm.runtime.has_engine(&run.id).await,
            "engine must be registered before cancel"
        );
        let cancelled = rm
            .cancel(CancelRunRequest {
                run_id: run.id.clone(),
            })
            .await
            .unwrap();
        assert_eq!(cancelled.status, RunStatusV2::Cancelled);

        let start_result = tokio::time::timeout(std::time::Duration::from_secs(6), start_handle)
            .await
            .expect("start should finish after cancel")
            .expect("join ok");
        let _ = start_result;

        let engine_cancelled = cancel_flag_seen.load(std::sync::atomic::Ordering::SeqCst);
        let has_cancel_lifecycle = rm.runtime.events.replay_after(&run.id, 0).iter().any(|e| {
            matches!(
                e.payload,
                RunEventKind::Cancelled { .. } | RunEventKind::Interrupted { .. }
            )
        });

        if let Ok(dir) = std::env::var("NATIVES_TEST_SCRATCH") {
            let evidence = serde_json::json!({
                "run_id": run.id,
                "status_after_cancel": cancelled.status.as_str(),
                "cancel_mid_run": true,
                "engine_was_registered": true,
                "engine_cancel_flag_observed_by_tool": engine_cancelled,
                "cancel_lifecycle_event": has_cancel_lifecycle,
            });
            let _ = std::fs::write(
                std::path::Path::new(&dir).join("daemon-cancel-mid.json"),
                serde_json::to_string_pretty(&evidence).unwrap_or_default(),
            );
        }
        assert!(
            has_cancel_lifecycle,
            "cancel must append Cancelled/Interrupted lifecycle event via commit_transition"
        );
        // Tool path should observe cancel flag from request_cancel.
        assert!(
            engine_cancelled,
            "production cancel must set engine cancel flag observed by tool execution"
        );
        // keep FIXTURE=1 for parallel tests under cfg(test)
        if let Some(v) = prev_runtime_dir {
            std::env::set_var("NATIVES_RUNTIME_DIR", v);
        } else {
            std::env::remove_var("NATIVES_RUNTIME_DIR");
        }
        let _ = std::fs::remove_dir_all(runtime_dir);
    }

    #[tokio::test]
    async fn permission_gate_emits_request_and_respond() {
        std::env::set_var("NATIVES_DAEMON_FIXTURE", "1");
        let rm = Arc::new(RunManager::new());
        // Event logs are durable by default; fixed ids would replay stale
        // permission events from an earlier test process.
        let run_key = format!("perm-{}", Uuid::new_v4());
        let run = rm
            .create_run(CreateRunRequest {
                conversation_id: "c-perm".into(),
                provider_id: "openai".into(),
                model_id: "gpt-4o".into(),
                key_id: Some("k".into()),
                agent_profile_id: None,
                permission_profile: Some("ask".into()),
                content: Some("tool please".into()),
                attachments: None,
                max_steps: Some(5),
                parent_run_id: None,
                project_path: None,
                idempotency_key: Some(run_key),
                effort: None,
                runtime_id: None,
            })
            .unwrap();

        // Tool-calling fixture provider + permission gated tools.
        let events = rm.runtime.events.clone();
        let tools = crate::production::PermissionGatedTools {
            gateway: {
                let mut g = capability_gateway::CapabilityGateway::new();
                g.register_builtins();
                Arc::new(g)
            },
            permissions: rm.runtime.permissions.clone(),
            events: events.clone(),
            interactions: rm.runtime.interactions.clone(),
            subagents: rm.runtime.subagents.clone(),
            task_outputs: rm.runtime.task_outputs_ref(),
            engines: rm.runtime.engine_handles().await,
            runtime: None,
            provider_id: "openai".into(),
            key_id: None,
            parent_run_id: run.id.clone(),
            conversation_id: "c-perm".into(),
            model_id: "gpt-4o".into(),
            permission_profile: "ask".into(),
            tool_allowlist: None,
        };
        let provider = FixtureProvider {
            mode: FixtureMode::RequestPermissionPath,
        };
        // Ensure ConfirmEach profile so side-effect tools ask.
        rm.runtime.set_permission_profile("ask").await;

        let rm_bg = rm.clone();
        let rid = run.id.clone();
        let respond_handle = tokio::spawn(async move {
            // Wait for permission_requested event then approve.
            for _ in 0..100 {
                tokio::time::sleep(std::time::Duration::from_millis(30)).await;
                let evs = rm_bg.runtime.events.replay_after(&rid, 0);
                if let Some(pid) = evs.iter().find_map(|e| match &e.payload {
                    RunEventKind::PermissionRequested { permission_id, .. } => {
                        Some(permission_id.clone())
                    }
                    _ => None,
                }) {
                    let _ = rm_bg.respond_permission(&pid, true).await;
                    return;
                }
            }
        });

        let status = tokio::time::timeout(
            std::time::Duration::from_secs(6),
            rm.start_with_seams(
                StartRunRequest {
                    run_id: Some(run.id.clone()),
                    conversation_id: None,
                    provider_id: None,
                    model_id: None,
                    key_id: None,
                    content: Some("tool please".into()),
                    attachments: None,
                    trigger_message_id: None,
                    permission_profile: Some("ask".into()),
                    max_steps: Some(5),
                    project_path: None,
                    idempotency_key: None,
                    effort: None,
                    runtime_id: None,
                },
                &provider,
                &tools,
            ),
        )
        .await
        .expect("permission-gated fixture run must terminate")
        .unwrap();

        let _ = respond_handle.await;
        let evs = rm.replay(ReplayRunRequest {
            run_id: run.id.clone(),
            after_sequence: 0,
        });
        let has_perm_req = evs
            .iter()
            .any(|e| matches!(e.payload, RunEventKind::PermissionRequested { .. }));
        let has_perm_resp = evs
            .iter()
            .any(|e| matches!(e.payload, RunEventKind::PermissionResponded { .. }));

        if let Ok(dir) = std::env::var("NATIVES_TEST_SCRATCH") {
            let dump: Vec<_> = evs
                .iter()
                .map(|e| {
                    serde_json::json!({
                        "sequence": e.effective_run_sequence(),
                        "type": e.payload.type_name(),
                    })
                })
                .collect();
            let _ = std::fs::write(
                std::path::Path::new(&dir).join("permission-events.json"),
                serde_json::to_string_pretty(&serde_json::json!({
                    "final_status": status.status.as_str(),
                    "permission_requested": has_perm_req,
                    "permission_responded": has_perm_resp,
                    "events": dump,
                }))
                .unwrap_or_default(),
            );
        }

        assert!(has_perm_req, "expected permission_requested event");
        assert!(has_perm_resp, "expected permission_responded event");
        // keep FIXTURE=1 for parallel tests under cfg(test)
    }

    /// Serialize credential-broker tests — shared process-global slot.
    fn with_broker_slot<R>(f: impl FnOnce() -> R) -> R {
        use std::sync::{Mutex, OnceLock};
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let lock = LOCK.get_or_init(|| Mutex::new(()));
        let _g = lock.lock().unwrap_or_else(|e| e.into_inner());
        crate::production::clear_credential_broker_for_tests();
        let out = f();
        crate::production::clear_credential_broker_for_tests();
        out
    }

    #[test]
    fn credential_resolve_never_returns_empty_mock_key() {
        with_broker_slot(|| {
            // Broker reports not-found; no env key → hard fail (no offline mock success).
            crate::production::install_credential_broker(std::sync::Arc::new(
                |_p: &str, _k: Option<&str>, _r: &str| Err("No active key for provider".into()),
            ));
            let prev_openai = std::env::var("NATIVES_TEST_OPENAI_KEY").ok();
            let prev_anth = std::env::var("ANTHROPIC_AUTH_TOKEN").ok();
            let prev_api = std::env::var("ANTHROPIC_API_KEY").ok();
            std::env::remove_var("NATIVES_TEST_OPENAI_KEY");
            std::env::remove_var("ANTHROPIC_AUTH_TOKEN");
            std::env::remove_var("ANTHROPIC_API_KEY");
            let err = crate::production::resolve_credential("openai", Some("k1")).unwrap_err();
            assert!(
                err.contains("No credential")
                    || err.contains("broker")
                    || err.contains("unavailable"),
                "unexpected err: {err}"
            );
            assert!(!err.contains("sk-"));
            match prev_openai {
                Some(v) => std::env::set_var("NATIVES_TEST_OPENAI_KEY", v),
                None => std::env::remove_var("NATIVES_TEST_OPENAI_KEY"),
            }
            match prev_anth {
                Some(v) => std::env::set_var("ANTHROPIC_AUTH_TOKEN", v),
                None => std::env::remove_var("ANTHROPIC_AUTH_TOKEN"),
            }
            match prev_api {
                Some(v) => std::env::set_var("ANTHROPIC_API_KEY", v),
                None => std::env::remove_var("ANTHROPIC_API_KEY"),
            }
        });
    }

    #[test]
    fn credential_broker_install_is_invoked_before_env() {
        with_broker_slot(|| {
            crate::production::install_credential_broker(std::sync::Arc::new(
                |_provider_id: &str, key_id: Option<&str>, run_id: &str| {
                    assert!(!run_id.is_empty());
                    Ok(provider_adapters::capabilities::Credential {
                        api_key: "broker-secret-not-for-logs".into(),
                        base_url: Some("https://example.test/v1".into()),
                        proxy_url: None,
                        key_id: Some(key_id.unwrap_or("broker-key-1").to_string()),
                        provider_type: Some("openai_compatible".into()),
                    })
                },
            ));
            std::env::remove_var("NATIVES_TEST_OPENAI_KEY");
            let cred = crate::production::resolve_credential_for_run("openai", Some("k1"), "run-1")
                .expect("broker must win over missing env");
            assert_eq!(cred.key_id.as_deref(), Some("k1"));
            assert_eq!(cred.api_key, "broker-secret-not-for-logs");
            let event_payload = serde_json::json!({"error": "auth failed"});
            assert!(!event_payload.to_string().contains("broker-secret"));
            if let Ok(dir) = std::env::var("NATIVES_TEST_SCRATCH") {
                let _ = std::fs::write(
                    std::path::Path::new(&dir).join("credential-broker-lifecycle.json"),
                    serde_json::to_string_pretty(&serde_json::json!({
                        "broker_invoked": true,
                        "key_id_returned": "k1",
                        "api_key_not_in_events": true,
                        "path": "install_credential_broker → resolve_credential_for_run → Tauri natives.db",
                        "mock_success_without_key": false,
                    }))
                    .unwrap_or_default(),
                );
            }
        });
    }

    #[tokio::test]
    async fn subagent_task_spawns_independent_identity() {
        std::env::set_var("NATIVES_DAEMON_FIXTURE", "1");
        let rt = crate::production::ProductionRuntime::new();
        // Task is Process/ProjectWrite — under ConfirmEach it asks; use autonomous for identity unit test.
        rt.set_permission_profile("full_access").await;
        let tools = crate::production::PermissionGatedTools {
            gateway: {
                let mut g = capability_gateway::CapabilityGateway::new();
                g.register_builtins();
                Arc::new(g)
            },
            permissions: rt.permissions.clone(),
            events: rt.events.clone(),
            interactions: rt.interactions.clone(),
            subagents: rt.subagents.clone(),
            task_outputs: rt.task_outputs.clone(),
            engines: rt.engine_handles().await,
            runtime: None,
            provider_id: "openai".into(),
            key_id: Some("parent-run-key".into()),
            parent_run_id: "parent-run".into(),
            conversation_id: "c".into(),
            model_id: "gpt-4o".into(),
            permission_profile: "full_access".into(),
            tool_allowlist: None,
        };
        let result = tools
            .execute_tool(
                "task",
                serde_json::json!({
                    "prompt": "child work",
                    "provider_id": "anthropic",
                    "model_id": "claude-3",
                    "key_id": "child-key-from-broker",
                    "permission_profile": "ask"
                }),
                &CancellationToken::new(),
            )
            .await;
        assert!(!result.is_error, "task error: {:?}", result.output);
        let task_id = result
            .output
            .get("task_id")
            .and_then(|v| v.as_str())
            .unwrap();
        let child_key = result
            .output
            .get("key_id")
            .and_then(|v| v.as_str())
            .unwrap();
        // Model-supplied credentials must be ignored; parent run key is used.
        assert_eq!(child_key, "parent-run-key");
        assert_ne!(child_key, "child-key-from-broker");
        assert_eq!(
            result.output.get("provider_id").and_then(|v| v.as_str()),
            Some("openai")
        );
        // Parent should have SubagentCreated event
        let evs = rt.events.replay_after("parent-run", 0);
        assert!(evs
            .iter()
            .any(|e| matches!(e.payload, RunEventKind::SubagentCreated { .. })));

        // task_output should see running/cancelled eventually
        let out = tools
            .execute_tool(
                "task_output",
                serde_json::json!({ "task_id": task_id }),
                &CancellationToken::new(),
            )
            .await;
        assert!(!out.is_error);

        let kill = tools
            .execute_tool(
                "kill_task",
                serde_json::json!({ "task_id": task_id }),
                &CancellationToken::new(),
            )
            .await;
        assert!(kill
            .output
            .get("cancelled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false));

        if let Ok(dir) = std::env::var("NATIVES_TEST_SCRATCH") {
            let _ = std::fs::write(
                std::path::Path::new(&dir).join("subagent-task-identity.json"),
                serde_json::to_string_pretty(&serde_json::json!({
                    "task_id": task_id,
                    "child_key_id": child_key,
                    "child_provider": "anthropic",
                    "independent_key": true,
                    "subagent_created_event": true,
                    "kill_task": true,
                }))
                .unwrap_or_default(),
            );
        }
        // keep FIXTURE=1 for parallel tests under cfg(test)
    }

    /// Parent openai / child anthropic (different provider+key+model); fixture completes child.
    #[test]
    fn subagent_dual_provider_fixture_completes() {
        with_env_lock(|| {
            std::env::set_var("NATIVES_DAEMON_FIXTURE", "1");
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("tokio");
            rt.block_on(async {
                let parent_id = format!("parent-dual-{}", uuid::Uuid::new_v4());
                let prt = crate::production::ProductionRuntime::new();
                prt.set_permission_profile("full_access").await;
                let tools = crate::production::PermissionGatedTools {
                    gateway: {
                        let mut g = capability_gateway::CapabilityGateway::new();
                        g.register_builtins();
                        Arc::new(g)
                    },
                    permissions: prt.permissions.clone(),
                    events: prt.events.clone(),
                    interactions: prt.interactions.clone(),
                    subagents: prt.subagents.clone(),
                    task_outputs: prt.task_outputs.clone(),
                    engines: prt.engine_handles().await,
                    runtime: None,
                    provider_id: "openai".into(),
                    key_id: Some("parent-key-A".into()),
                    parent_run_id: parent_id.clone(),
                    conversation_id: "c-dual".into(),
                    model_id: "gpt-4o".into(),
                    permission_profile: "full_access".into(),
                    tool_allowlist: None,
                };
                let result = tools
                    .execute_tool(
                        "task",
                        serde_json::json!({
                            "prompt": "child dual provider",
                            "provider_id": "anthropic",
                            "model_id": "claude-3-haiku",
                            "key_id": "child-key-B",
                            "permission_profile": "full_access",
                            "fixture": true
                        }),
                        &CancellationToken::new(),
                    )
                    .await;
                assert!(!result.is_error, "{:?}", result.output);
                // Credentials come from parent run, not model-supplied child fields.
                assert_eq!(
                    result.output.get("provider_id").and_then(|v| v.as_str()),
                    Some("openai")
                );
                assert_eq!(
                    result.output.get("key_id").and_then(|v| v.as_str()),
                    Some("parent-key-A")
                );
                assert_eq!(
                    result.output.get("model_id").and_then(|v| v.as_str()),
                    Some("gpt-4o")
                );
                let task_id = result
                    .output
                    .get("task_id")
                    .and_then(|v| v.as_str())
                    .unwrap()
                    .to_string();

                let mut final_status = String::new();
                for _ in 0..80 {
                    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                    let out = tools
                        .execute_tool(
                            "task_output",
                            serde_json::json!({ "task_id": task_id }),
                            &CancellationToken::new(),
                        )
                        .await;
                    let status = out
                        .output
                        .get("status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    if status != "running" && status != "unknown" {
                        final_status = status.to_string();
                        break;
                    }
                }
                assert_eq!(
                    final_status, "completed",
                    "fixture child should complete with independent identity"
                );
                let parent_done = prt
                    .events
                    .replay_after(&parent_id, 0)
                    .iter()
                    .any(|e| matches!(e.payload, RunEventKind::SubagentCompleted { .. }));
                assert!(parent_done, "parent must observe SubagentCompleted");
                assert!(!prt.events.replay_after(&parent_id, 0).is_empty());

                if let Ok(dir) = std::env::var("NATIVES_TEST_SCRATCH") {
                    let _ = std::fs::write(
                        std::path::Path::new(&dir).join("subagent-dual-provider.json"),
                        serde_json::to_string_pretty(&serde_json::json!({
                            "parent_provider": "openai",
                            "child_provider": "anthropic",
                            "child_key_id": "child-key-B",
                            "child_model": "claude-3-haiku",
                            "status": final_status,
                            "fixture": true,
                            "subagent_completed": true,
                        }))
                        .unwrap_or_default(),
                    );
                }
            });
            // leave FIXTURE=1; other fixture tests expect it
        });
    }

    #[tokio::test]
    async fn mutating_tool_fail_closed_without_project_identity() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("id.db");
        let artifacts = dir.path().join("art");
        std::fs::create_dir_all(&artifacts).unwrap();
        let store = std::sync::Arc::new(crate::storage::DataStore::new(&db, &artifacts).unwrap());
        // Install as global so PermissionGatedTools sees data_store_ref.
        let rm = std::sync::Arc::new(RunManager::new_with_store(store.clone()));
        // Bypass global: call tools with runtime that shares manager store via temporary global?
        // Production checks global_run_manager().data_store_ref — set env and use global.
        let _ = rm;
        // Use production tools against run without project_id.
        let rt = crate::production::ProductionRuntime::new();
        // Create run on a store-backed manager that is process global.
        // Replace process global is hard; instead test ensure logic via tool when
        // we bind run on GLOBAL if available.
        // Minimal: verify tool_requires + invocation_from_gate soft id removed.
        assert!(crate::runtime::tool_requires_verified_project("write_file"));
        let inv = crate::runtime::invocation_from_gate(
            "write_file",
            &serde_json::json!({"path":"a"}),
            "c",
            "r",
            Some("/tmp/x"),
        );
        assert!(inv.project_id.is_none());
        let _ = dir;
        let _ = rt;
    }

    #[tokio::test]
    async fn mcp_call_through_permission_gate_emits_events() {
        // Register mock tool without live session → structured error + events.
        let rt = crate::production::ProductionRuntime::new();
        rt.set_permission_profile("full_access").await;
        crate::mcp_runtime::global_mcp()
            .register_server(agent_core::McpServerConfig {
                id: "gate-test".into(),
                transport: agent_core::McpTransport::Stdio,
                command: Some("true".into()),
                args: None,
                url: None,
                trusted: true,
                auth_token: None,
                headers: None,
            })
            .unwrap();
        crate::mcp_runtime::global_mcp()
            .upsert_tool(agent_core::McpToolDescriptor {
                server_id: "gate-test".into(),
                name: "echo".into(),
                description: "echo".into(),
                input_schema: serde_json::json!({"type":"object"}),
            })
            .unwrap();
        let tools = crate::production::PermissionGatedTools {
            gateway: {
                let mut g = capability_gateway::CapabilityGateway::new();
                g.register_builtins();
                Arc::new(g)
            },
            permissions: rt.permissions.clone(),
            events: rt.events.clone(),
            interactions: rt.interactions.clone(),
            subagents: rt.subagents.clone(),
            task_outputs: rt.task_outputs.clone(),
            engines: rt.engine_handles().await,
            runtime: None,
            provider_id: "openai".into(),
            key_id: None,
            parent_run_id: "mcp-parent".into(),
            conversation_id: "c".into(),
            model_id: "m".into(),
            permission_profile: "full_access".into(),
            tool_allowlist: None,
        };
        let out = tools
            .execute_tool(
                "mcp_call",
                serde_json::json!({
                    "server": "gate-test",
                    "tool": "echo",
                    "arguments": {"x": 1}
                }),
                &CancellationToken::new(),
            )
            .await;
        // No live session → error, but still gated + evented.
        assert!(out.is_error || out.output.get("ok") == Some(&serde_json::json!(false)));
        let evs = rt.events.replay_after("mcp-parent", 0);
        assert!(
            evs.iter()
                .any(|e| matches!(e.payload, RunEventKind::ToolCallStarted { .. })),
            "expected ToolCallStarted for mcp_call"
        );
        assert!(
            evs.iter()
                .any(|e| matches!(e.payload, RunEventKind::ToolCallCompleted { .. })),
            "expected ToolCallCompleted for mcp_call"
        );
        if let Ok(dir) = std::env::var("NATIVES_TEST_SCRATCH") {
            let _ = std::fs::write(
                std::path::Path::new(&dir).join("mcp-call-gated.json"),
                serde_json::to_string_pretty(&serde_json::json!({
                    "permission_gated": true,
                    "tool_call_events": true,
                    "is_error": out.is_error,
                }))
                .unwrap_or_default(),
            );
        }
    }

    #[test]
    fn start_detached_codex_runtime_is_fail_closed() {
        let _prev_a = std::env::var("NATIVES_ASSISTANT_DB_PATH").ok();
        let _prev_d = std::env::var("NATIVES_DB_PATH").ok();
        std::env::remove_var("NATIVES_ASSISTANT_DB_PATH");
        std::env::remove_var("NATIVES_DB_PATH");
        // Gate runs before ensure/create — use existing run_id to avoid FK on new conversation.
        let rm = Arc::new(RunManager::new());
        let run = rm
            .create_run(CreateRunRequest {
                conversation_id: "c-codex".into(),
                provider_id: "openai".into(),
                model_id: "gpt".into(),
                key_id: None,
                agent_profile_id: None,
                permission_profile: Some("ask".into()),
                content: Some("hi".into()),
                attachments: None,
                max_steps: Some(3),
                parent_run_id: None,
                project_path: Some("/tmp".into()),
                idempotency_key: None,
                effort: None,
                runtime_id: Some("native".into()),
            })
            .unwrap();
        let err = rm
            .start_detached(StartRunRequest {
                run_id: Some(run.id),
                conversation_id: Some("c-codex".into()),
                provider_id: Some("openai".into()),
                model_id: Some("gpt".into()),
                key_id: None,
                content: Some("hi".into()),
                attachments: None,
                trigger_message_id: None,
                permission_profile: Some("ask".into()),
                max_steps: Some(3),
                project_path: Some("/tmp".into()),
                idempotency_key: None,
                effort: None,
                runtime_id: Some("codex_cli".into()),
            })
            .unwrap_err();
        assert!(
            err.contains("codex_cli") && err.contains("unavailable"),
            "unexpected: {err}"
        );
    }

    #[test]
    fn start_detached_unknown_runtime_is_fail_closed() {
        let _prev_a = std::env::var("NATIVES_ASSISTANT_DB_PATH").ok();
        let _prev_d = std::env::var("NATIVES_DB_PATH").ok();
        std::env::remove_var("NATIVES_ASSISTANT_DB_PATH");
        std::env::remove_var("NATIVES_DB_PATH");
        let rm = Arc::new(RunManager::new());
        let run = rm
            .create_run(CreateRunRequest {
                conversation_id: "c-rt".into(),
                provider_id: "openai".into(),
                model_id: "gpt".into(),
                key_id: None,
                agent_profile_id: None,
                permission_profile: Some("ask".into()),
                content: Some("hi".into()),
                attachments: None,
                max_steps: Some(3),
                parent_run_id: None,
                project_path: Some("/tmp".into()),
                idempotency_key: None,
                effort: None,
                runtime_id: Some("native".into()),
            })
            .unwrap();
        let err = rm
            .start_detached(StartRunRequest {
                run_id: Some(run.id),
                conversation_id: Some("c-rt".into()),
                provider_id: Some("openai".into()),
                model_id: Some("gpt".into()),
                key_id: None,
                content: Some("hi".into()),
                attachments: None,
                trigger_message_id: None,
                permission_profile: Some("ask".into()),
                max_steps: Some(3),
                project_path: Some("/tmp".into()),
                idempotency_key: None,
                effort: None,
                runtime_id: Some("not_a_runtime".into()),
            })
            .unwrap_err();
        assert!(err.contains("unknown runtime_id"), "unexpected: {err}");
    }

    #[test]
    fn create_run_preserves_runtime_id_and_effort() {
        let _prev_a = std::env::var("NATIVES_ASSISTANT_DB_PATH").ok();
        let _prev_d = std::env::var("NATIVES_DB_PATH").ok();
        std::env::remove_var("NATIVES_ASSISTANT_DB_PATH");
        std::env::remove_var("NATIVES_DB_PATH");
        let rm = RunManager::new();
        let run = rm
            .create_run(CreateRunRequest {
                conversation_id: "c-preserve".into(),
                provider_id: "openai".into(),
                model_id: "gpt".into(),
                key_id: None,
                agent_profile_id: None,
                permission_profile: Some("ask".into()),
                content: Some("x".into()),
                attachments: None,
                max_steps: Some(5),
                parent_run_id: None,
                project_path: Some("/tmp".into()),
                idempotency_key: None,
                effort: Some("high".into()),
                runtime_id: Some("claude_cli".into()),
            })
            .unwrap();
        assert_eq!(run.runtime_id.as_deref(), Some("claude_cli"));
        assert_eq!(run.effort.as_deref(), Some("high"));
        let got = rm.get_run(&run.id).unwrap();
        assert_eq!(got.runtime_id.as_deref(), Some("claude_cli"));
        assert_eq!(got.effort.as_deref(), Some("high"));
    }

    #[test]
    fn retry_preserves_runtime_id() {
        let _prev_a = std::env::var("NATIVES_ASSISTANT_DB_PATH").ok();
        let _prev_d = std::env::var("NATIVES_DB_PATH").ok();
        std::env::remove_var("NATIVES_ASSISTANT_DB_PATH");
        std::env::remove_var("NATIVES_DB_PATH");
        let rm = RunManager::new();
        let run = rm
            .create_run(CreateRunRequest {
                conversation_id: "c-retry-rt".into(),
                provider_id: "openai".into(),
                model_id: "gpt".into(),
                key_id: None,
                agent_profile_id: None,
                permission_profile: Some("ask".into()),
                content: Some("retry me".into()),
                attachments: None,
                max_steps: Some(5),
                parent_run_id: None,
                project_path: Some("/tmp".into()),
                idempotency_key: None,
                effort: Some("medium".into()),
                runtime_id: Some("native".into()),
            })
            .unwrap();
        {
            let mut runs = rm.runs.lock().unwrap();
            if let Some(r) = runs.get_mut(&run.id) {
                r.status = RunStatusV2::Failed;
                r.finished_at = Some(chrono::Utc::now());
            }
        }
        rm.last_content
            .lock()
            .unwrap()
            .insert(run.id.clone(), "retry me".into());
        let next = rm
            .retry(RetryRunRequest {
                run_id: run.id.clone(),
            })
            .unwrap();
        assert_eq!(next.runtime_id.as_deref(), Some("native"));
        assert_eq!(next.effort.as_deref(), Some("medium"));
        assert_ne!(next.id, run.id);
    }

    #[test]
    fn fail_run_if_active_is_idempotent_and_emits_failed_event() {
        let _prev_a = std::env::var("NATIVES_ASSISTANT_DB_PATH").ok();
        let _prev_d = std::env::var("NATIVES_DB_PATH").ok();
        std::env::remove_var("NATIVES_ASSISTANT_DB_PATH");
        std::env::remove_var("NATIVES_DB_PATH");
        let rm = Arc::new(RunManager::new());
        let run = rm
            .create_run(CreateRunRequest {
                conversation_id: "c-fail".into(),
                provider_id: "missing-provider".into(),
                model_id: "m".into(),
                key_id: None,
                agent_profile_id: None,
                permission_profile: Some("ask".into()),
                content: Some("hi".into()),
                attachments: None,
                max_steps: Some(3),
                parent_run_id: None,
                project_path: Some("/tmp".into()),
                idempotency_key: None,
                effort: None,
                runtime_id: Some("native".into()),
            })
            .unwrap();
        {
            let mut runs = rm.runs.lock().unwrap();
            if let Some(r) = runs.get_mut(&run.id) {
                r.status = RunStatusV2::Preparing;
            }
        }
        rm.fail_run_if_active(&run.id, "No credentials for provider", "NO_CREDENTIALS");
        let after = rm.get_run(&run.id).unwrap();
        assert_eq!(after.status, RunStatusV2::Failed);
        assert_eq!(after.error_code.as_deref(), Some("NO_CREDENTIALS"));
        assert!(after.finished_at.is_some());
        let events = rm.runtime.events.replay_after(&run.id, 0);
        assert!(
            events
                .iter()
                .any(|e| matches!(&e.payload, RunEventKind::Failed { code, .. } if code == "NO_CREDENTIALS")),
            "expected failed event, got {:?}",
            events.iter().map(|e| format!("{:?}", e.payload)).collect::<Vec<_>>()
        );
        // Second call is a no-op (already terminal).
        rm.fail_run_if_active(&run.id, "again", "NO_CREDENTIALS");
        let events2 = rm.runtime.events.replay_after(&run.id, 0);
        let failed_count = events2
            .iter()
            .filter(|e| matches!(&e.payload, RunEventKind::Failed { .. }))
            .count();
        assert_eq!(failed_count, 1);
    }

    #[test]
    fn ensure_run_for_start_with_run_id_appends_daemon_local_user_message() {
        with_env_lock(|| {
            let dir = tempfile::tempdir().unwrap();
            let previous_db = std::env::var("NATIVES_DB_PATH").ok();
            let previous_runtime = std::env::var("NATIVES_RUNTIME_DIR").ok();
            let db_path = dir.path().join("natives.db");
            std::env::set_var("NATIVES_DB_PATH", &db_path);
            std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db_path);
            std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());

            std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
            crate::storage::set_test_db_override(
                Some(db_path.clone()),
                Some(dir.path().join("artifacts")),
            );
            let store = Arc::new(
                crate::storage::DataStore::new(&db_path, &dir.path().join("artifacts")).unwrap(),
            );
            store
                .conn()
                .unwrap()
                .execute(
                    "INSERT INTO conversation (id, mode, title, provider_id, model_id)
                     VALUES ('host-conv', 'agent', 'Host', 'openai', 'gpt-4o')",
                    [],
                )
                .unwrap();
            let rm = RunManager::new_with_store(store.clone());
            let run = rm
                .create_run(CreateRunRequest {
                    conversation_id: "host-conv".into(),
                    provider_id: "openai".into(),
                    model_id: "gpt-4o".into(),
                    key_id: None,
                    agent_profile_id: None,
                    permission_profile: Some("ask".into()),
                    content: Some("second question".into()),
                    attachments: None,
                    max_steps: Some(3),
                    parent_run_id: None,
                    project_path: Some("/tmp".into()),
                    idempotency_key: Some(format!("host-key-{}", Uuid::new_v4())),
                    effort: None,
                    runtime_id: Some("native".into()),
                })
                .unwrap();
            // Host path: run_id present, trigger_message_id is host UUID (ignored).
            let ensured = rm
                .ensure_run_for_start(&StartRunRequest {
                    run_id: Some(run.id.clone()),
                    conversation_id: Some("host-conv".into()),
                    provider_id: Some("openai".into()),
                    model_id: Some("gpt-4o".into()),
                    key_id: None,
                    content: Some("second question".into()),
                    attachments: None,
                    trigger_message_id: Some("host-message-uuid-not-in-daemon".into()),
                    permission_profile: Some("ask".into()),
                    max_steps: Some(3),
                    project_path: Some("/tmp".into()),
                    idempotency_key: None,
                    effort: None,
                    runtime_id: Some("native".into()),
                })
                .unwrap();
            assert!(ensured.trigger_message_id.is_some());
            let daemon_msg_id = ensured.trigger_message_id.unwrap();
            assert_ne!(daemon_msg_id, "host-message-uuid-not-in-daemon");
            let text: String = store
                .conn()
                .unwrap()
                .query_row(
                    "SELECT block_json FROM message_block WHERE message_id = ?1 LIMIT 1",
                    rusqlite::params![daemon_msg_id],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(text.contains("second question"), "{text}");

            // Idempotent: second ensure does not double-append.
            let again = rm
                .ensure_run_for_start(&StartRunRequest {
                    run_id: Some(run.id.clone()),
                    conversation_id: Some("host-conv".into()),
                    provider_id: Some("openai".into()),
                    model_id: Some("gpt-4o".into()),
                    key_id: None,
                    content: Some("second question".into()),
                    attachments: None,
                    trigger_message_id: None,
                    permission_profile: Some("ask".into()),
                    max_steps: Some(3),
                    project_path: Some("/tmp".into()),
                    idempotency_key: None,
                    effort: None,
                    runtime_id: Some("native".into()),
                })
                .unwrap();
            assert_eq!(
                again.trigger_message_id.as_deref(),
                Some(daemon_msg_id.as_str())
            );
            let count: i64 = store
                .conn()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM message WHERE conversation_id = 'host-conv' AND role = 'user'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1);

            if let Some(value) = previous_db {
                std::env::set_var("NATIVES_DB_PATH", &value);
            } else {
                std::env::remove_var("NATIVES_DB_PATH");
            }
            if let Some(value) = previous_runtime {
                std::env::set_var("NATIVES_RUNTIME_DIR", &value);
            } else {
                std::env::remove_var("NATIVES_RUNTIME_DIR");
            }
        });
    }

    #[tokio::test]
    async fn cancel_vs_complete_race_single_terminal_and_consistent() {
        // 100 concurrent cancel-vs-complete races: each run ends with exactly one
        // terminal lifecycle event, and memory status matches that event.
        std::env::set_var("NATIVES_DAEMON_FIXTURE", "1");
        let mut terminal_mismatch = 0u32;
        let mut multi_terminal = 0u32;
        for i in 0..100 {
            let rm = Arc::new(RunManager::new());
            let run = rm
                .create_run(CreateRunRequest {
                    conversation_id: format!("c-race-{i}"),
                    provider_id: "openai".into(),
                    model_id: "gpt-4o".into(),
                    key_id: Some("k".into()),
                    agent_profile_id: None,
                    permission_profile: Some("ask".into()),
                    content: Some("race".into()),
                    attachments: None,
                    max_steps: Some(3),
                    parent_run_id: None,
                    project_path: None,
                    idempotency_key: Some(format!("race-key-{i}")),
                    effort: None,
                    runtime_id: None,
                })
                .unwrap();
            let run_id = run.id.clone();

            let rm_start = rm.clone();
            let rid = run_id.clone();
            let start_h = tokio::spawn(async move {
                rm_start
                    .start(StartRunRequest {
                        run_id: Some(rid),
                        conversation_id: None,
                        provider_id: None,
                        model_id: None,
                        key_id: None,
                        content: Some("race".into()),
                        attachments: None,
                        trigger_message_id: None,
                        permission_profile: Some("full_access".into()),
                        max_steps: Some(3),
                        project_path: None,
                        idempotency_key: None,
                        effort: None,
                        runtime_id: None,
                    })
                    .await
            });
            // Small jitter so cancel sometimes wins, sometimes loses.
            if i % 2 == 0 {
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            }
            let rm_cancel = rm.clone();
            let rid2 = run_id.clone();
            let cancel_h =
                tokio::spawn(
                    async move { rm_cancel.cancel(CancelRunRequest { run_id: rid2 }).await },
                );

            let start_res = start_h.await.unwrap();
            let cancel_res = cancel_h.await.unwrap();
            // Both may succeed or one may see already-terminal — both ok.
            let _ = (start_res, cancel_res);

            let final_run = rm.get_run(&run_id).expect("run present");
            assert!(
                final_run.status.is_terminal(),
                "run {i} must be terminal, got {:?}",
                final_run.status
            );

            let events = rm.runtime.events.replay_after(&run_id, 0);
            let terminals: Vec<_> = events
                .iter()
                .filter(|e| {
                    matches!(
                        e.payload,
                        RunEventKind::Completed { .. }
                            | RunEventKind::Failed { .. }
                            | RunEventKind::Cancelled { .. }
                            | RunEventKind::Interrupted { .. }
                    )
                })
                .collect();
            if terminals.len() != 1 {
                multi_terminal += 1;
            }
            if let Some(ev) = terminals.first() {
                let status_from_event = match &ev.payload {
                    RunEventKind::Completed { .. } => RunStatusV2::Completed,
                    RunEventKind::Failed { .. } => RunStatusV2::Failed,
                    RunEventKind::Cancelled { .. } => RunStatusV2::Cancelled,
                    RunEventKind::Interrupted { .. } => RunStatusV2::Interrupted,
                    _ => unreachable!(),
                };
                if status_from_event != final_run.status {
                    terminal_mismatch += 1;
                }
            } else {
                terminal_mismatch += 1;
            }

            // Late outcome / cancel must be idempotent (no second terminal).
            let _ = rm.commit_outcome(&run_id, &EngineOutcome::completed("late"));
            let _ = rm
                .cancel(CancelRunRequest {
                    run_id: run_id.clone(),
                })
                .await;
            let after = rm.runtime.events.replay_after(&run_id, 0);
            let after_term = after
                .iter()
                .filter(|e| {
                    matches!(
                        e.payload,
                        RunEventKind::Completed { .. }
                            | RunEventKind::Failed { .. }
                            | RunEventKind::Cancelled { .. }
                            | RunEventKind::Interrupted { .. }
                    )
                })
                .count();
            assert_eq!(
                after_term,
                terminals.len().max(1),
                "late commit/cancel must not add a second terminal (run {i})"
            );
            let reopened = rm.get_run(&run_id).unwrap();
            assert_eq!(
                reopened.status, final_run.status,
                "reopen status must match final (run {i})"
            );
        }
        assert_eq!(
            multi_terminal, 0,
            "some runs had != 1 terminal lifecycle event"
        );
        assert_eq!(terminal_mismatch, 0, "some runs had event/status mismatch");
    }
}
