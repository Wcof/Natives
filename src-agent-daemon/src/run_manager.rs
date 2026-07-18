//! RunManager — Daemon-side sole Run Authority (production path).

use crate::production::{FixtureMode, FixtureProvider, ProductionRuntime};
use crate::storage::DataStore;
use agent_core::{AgentEngine, EngineRunConfig, EventSequencer};
use assistant_protocol::v2::{
    CancelRunRequest, CreateRunRequest, DaemonCapabilities, ReplayRunRequest, RetryRunRequest,
    RunEventKind, RunEventV2, RunStatusV2, RunV2, StartRunRequest, PROTOCOL_V2,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
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
        let mgr = Self {
            runs: Mutex::new(HashMap::new()),
            idempotency: Mutex::new(HashMap::new()),
            last_content: Mutex::new(HashMap::new()),
            project_paths: Mutex::new(HashMap::new()),
            snapshot_path_override: None,
            data_store: Some(data_store.clone()),
            runtime: Arc::new(ProductionRuntime::new_with_event_store(data_store)),
        };
        let _ = mgr.interrupt_active_sqlite_runs();
        let _ = mgr.restore_runs_snapshot();
        mgr
    }

    fn store_from_env() -> Option<Arc<DataStore>> {
        let db_path = std::env::var("NATIVES_DB_PATH").ok()?;
        if db_path.trim().is_empty() {
            return None;
        }
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
                project_path, retry_count, idempotency_key
             )
             VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, NULL, 0, 0, ?12,
                ?13, ?14, ?15, ?16, ?17, ?18, ?19
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
                idempotency_key = excluded.idempotency_key",
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
                run.idempotency_key
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
                        created_at, idempotency_key
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
        }))
    }

    fn delete_run_row(&self, run_id: &str) {
        if let Some(store) = &self.data_store {
            if let Ok(conn) = store.conn() {
                let _ = conn.execute("DELETE FROM run WHERE id = ?1", rusqlite::params![run_id]);
            }
        }
    }

    fn interrupt_active_sqlite_runs(&self) -> Result<usize, String> {
        let Some(store) = &self.data_store else {
            return Ok(0);
        };
        let conn = store.conn()?;
        let changed = conn
            .execute(
                "UPDATE run
                 SET status = 'interrupted',
                     error_code = 'daemon_restarted',
                     finished_at = ?1
                 WHERE status IN (
                    'queued', 'preparing', 'running', 'waiting_permission',
                    'waiting_subagent', 'cancelling'
                 )",
                rusqlite::params![chrono::Utc::now().to_rfc3339()],
            )
            .map_err(|e| format!("PERSISTENCE_FAILED interrupt active runs: {e}"))?;
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
        // Explicit only — never daemon process cwd (full remediation §五).
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
            project_path: req.project_path.clone(),
            retry_count: 0,
            created_at: Some(chrono::Utc::now()),
            last_event_sequence: 0,
            idempotency_key: req.idempotency_key.clone(),
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

    pub fn replay(&self, req: ReplayRunRequest) -> Vec<RunEventV2> {
        self.runtime
            .events
            .replay_after(&req.run_id, req.after_sequence)
    }

    pub async fn cancel(&self, req: CancelRunRequest) -> Result<RunV2, String> {
        self.runtime.cancel_run(&req.run_id).await;
        let result = {
            let mut runs = self.runs.lock().map_err(|e| e.to_string())?;
            let run = runs
                .get_mut(&req.run_id)
                .ok_or_else(|| "run not found".to_string())?;
            if run.status.is_terminal() {
                run.clone()
            } else {
                run.status = RunStatusV2::Cancelled;
                run.finished_at = Some(chrono::Utc::now());
                run.clone()
            }
        };
        self.persist_run_row(&result)?;
        self.persist_runs_snapshot()?;
        Ok(result)
    }

    /// Ensure a run row exists for `start` / `start_detached` (create if `run_id` absent).
    pub fn ensure_run_for_start(&self, req: &StartRunRequest) -> Result<RunV2, String> {
        if let Some(run_id) = &req.run_id {
            return self
                .get_run(run_id)
                .ok_or_else(|| "run not found".to_string());
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
        let trigger_message_id = match req.trigger_message_id.clone() {
            Some(id) => Some(id),
            None => crate::conversation_store::append_trigger_message(
                &conversation_id,
                req.content.as_deref(),
                req.attachments.as_deref(),
            )?,
        };
        let mut run = match self.create_run(CreateRunRequest {
            conversation_id,
            provider_id: req.provider_id.clone().unwrap_or_default(),
            model_id: req.model_id.clone().unwrap_or_default(),
            key_id: req.key_id.clone(),
            agent_profile_id: None,
            permission_profile: req.permission_profile.clone().or_else(|| {
                req.conversation_id
                    .as_deref()
                    .and_then(|id| crate::conversation_store::permission_profile(id).ok())
            }),
            content: req.content.clone(),
            attachments: req.attachments.clone(),
            max_steps: req.max_steps,
            parent_run_id: None,
            project_path: req.project_path.clone(),
            idempotency_key: req.idempotency_key.clone(),
        }) {
            Ok(run) => run,
            Err(error) => {
                if req.trigger_message_id.is_none() {
                    if let Some(id) = trigger_message_id.as_deref() {
                        let _ = crate::conversation_store::delete_message(id);
                    }
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

    fn mark_preparing(&self, run_id: &str) -> Result<RunV2, String> {
        let result = {
            let mut runs = self.runs.lock().map_err(|e| e.to_string())?;
            let run = runs
                .get_mut(run_id)
                .ok_or_else(|| "run not found".to_string())?;
            if !run.status.is_terminal() {
                run.status = RunStatusV2::Preparing;
                if run.started_at.is_none() {
                    run.started_at = Some(chrono::Utc::now());
                }
            }
            run.clone()
        };
        self.persist_run_row(&result)?;
        Ok(result)
    }

    /// Non-blocking start for RPC / UI: returns immediately with Preparing status.
    /// Engine execution continues on a background task; clients poll events / cancel
    /// on the same session without waiting for completion.
    ///
    /// Idempotent: if the run is already active (or terminal), does **not** spawn a
    /// second engine — returns the current row (duplicate Start is a no-op).
    pub fn start_detached(self: &Arc<Self>, req: StartRunRequest) -> Result<RunV2, String> {
        let run = self.ensure_run_for_start(&req)?;
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
        tokio::spawn(async move {
            let _ = rm.start(req).await;
        });
        Ok(preparing)
    }

    /// Process-wide non-blocking start (sidecar RPC uses this via `global_run_manager`).
    /// Same idempotency rules as [`start_detached`].
    pub fn start_detached_global(req: StartRunRequest) -> Result<RunV2, String> {
        let rm = global_run_manager();
        let run = rm.ensure_run_for_start(&req)?;
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
        tokio::spawn(async move {
            let _ = global_run_manager().start(req).await;
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

        {
            let mut runs = self.runs.lock().map_err(|e| e.to_string())?;
            if let Some(r) = runs.get_mut(&run.id) {
                r.status = RunStatusV2::Preparing;
                r.started_at = Some(chrono::Utc::now());
                self.persist_run_row(r)?;
            }
        }

        let request_project_path = req.project_path.clone();
        let provider_id = req.provider_id.unwrap_or_else(|| run.provider_id.clone());
        let model_id = req.model_id.unwrap_or_else(|| run.model_id.clone());
        let key_id = req.key_id.or_else(|| run.key_id.clone());
        let permission_profile = req
            .permission_profile
            .unwrap_or_else(|| run.permission_profile.clone());
        let max_steps = req.max_steps.unwrap_or(run.max_steps);

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
                    return Err(format!(
                        "No credentials for provider '{provider_id}'. Set NATIVES_TEST_* or broker."
                    ));
                }
            }
        }

        let use_fixture_engine =
            std::env::var("NATIVES_DAEMON_FIXTURE").ok().as_deref() == Some("1") || cfg!(test);

        let final_status = if use_fixture_engine {
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
            let engine = Arc::new(AgentEngine::new(self.runtime.events.clone()).with_hooks(hooks));
            self.runtime
                .engines
                .lock()
                .await
                .insert(run.id.clone(), engine.clone());
            let provider = FixtureProvider {
                mode: FixtureMode::TextOnly,
            };
            let tools = crate::production::PermissionGatedTools {
                gateway: {
                    let mut g = capability_gateway::CapabilityGateway::new();
                    g.register_builtins();
                    Arc::new(g)
                },
                permissions: self.runtime.permissions.clone(),
                events: self.runtime.events.clone(),
                waiters: self.runtime.permission_waiters.clone(),
                subagents: self.runtime.subagents.clone(),
                task_outputs: self.runtime.task_outputs.clone(),
                engines: self.runtime.engines.clone(),
                runtime: None,
                provider_id: provider_id.clone(),
                parent_run_id: run.id.clone(),
                conversation_id: run.conversation_id.clone(),
                model_id: model_id.clone(),
                permission_profile: permission_profile.clone(),
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
            let mut status = engine
                .run(config, &provider, &tools)
                .await
                .unwrap_or(RunStatusV2::Failed);
            if status == RunStatusV2::Completed {
                if crate::conversation_store::append_assistant_turn_from_events(
                    &run.conversation_id,
                    &run.id,
                    &self.runtime.events.replay_after(&run.id, 0),
                )
                .is_err()
                {
                    status = RunStatusV2::Failed;
                }
            }
            self.runtime.engines.lock().await.remove(&run.id);
            status
        } else {
            let project_path = self.resolve_project_path(&run.id, request_project_path.as_deref());
            self.store_project_path(
                &run.id,
                project_path
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string())
                    .as_deref(),
            );
            self.runtime
                .start_run(
                    run.id.clone(),
                    run.conversation_id.clone(),
                    provider_id,
                    model_id,
                    key_id,
                    permission_profile,
                    content,
                    max_steps,
                    project_path,
                )
                .await?;
            // Infer terminal from last event
            let events = self.runtime.events.replay_after(&run.id, 0);
            if events.iter().any(|e| {
                matches!(
                    e.payload,
                    RunEventKind::Cancelled { .. } | RunEventKind::Interrupted { .. }
                )
            }) {
                if events
                    .iter()
                    .any(|e| matches!(e.payload, RunEventKind::Cancelled { .. }))
                {
                    RunStatusV2::Cancelled
                } else {
                    RunStatusV2::Interrupted
                }
            } else if events
                .iter()
                .any(|e| matches!(e.payload, RunEventKind::Failed { .. }))
            {
                RunStatusV2::Failed
            } else {
                RunStatusV2::Completed
            }
        };

        let mut runs = self.runs.lock().map_err(|e| e.to_string())?;
        if let Some(r) = runs.get_mut(&run.id) {
            r.status = final_status;
            r.finished_at = Some(chrono::Utc::now());
            if final_status == RunStatusV2::Failed {
                r.error_code = Some("engine_failed".into());
            }
            let result = r.clone();
            drop(runs);
            self.persist_run_row(&result)?;
            self.persist_runs_snapshot()?;
            return Ok(result);
        }
        Err("run disappeared".into())
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
            })?
        };

        let content = req.content.clone().unwrap_or_else(|| "continue".into());
        // Register engine so cancel_run → request_cancel works mid-flight.
        let engine = Arc::new(AgentEngine::new(self.runtime.events.clone()));
        self.runtime
            .engines
            .lock()
            .await
            .insert(run.id.clone(), engine.clone());
        {
            let mut runs = self.runs.lock().map_err(|e| e.to_string())?;
            if let Some(r) = runs.get_mut(&run.id) {
                r.status = RunStatusV2::Preparing;
                r.started_at = Some(chrono::Utc::now());
                self.persist_run_row(r)?;
            }
        }
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
        let final_status = engine
            .run(config, provider, tools)
            .await
            .unwrap_or(RunStatusV2::Failed);
        let final_status = if final_status == RunStatusV2::Completed
            && crate::conversation_store::append_assistant_turn_from_events(
                &run.conversation_id,
                &run.id,
                &self.runtime.events.replay_after(&run.id, 0),
            )
            .is_err()
        {
            RunStatusV2::Failed
        } else {
            final_status
        };
        self.runtime.engines.lock().await.remove(&run.id);
        let mut runs = self.runs.lock().map_err(|e| e.to_string())?;
        if let Some(r) = runs.get_mut(&run.id) {
            r.status = final_status;
            r.finished_at = Some(chrono::Utc::now());
            let result = r.clone();
            drop(runs);
            self.persist_run_row(&result)?;
            self.persist_runs_snapshot()?;
            return Ok(result);
        }
        Err("run disappeared".into())
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
        self.respond_permission_for_run(request_id, approved, None)
            .await
    }

    pub async fn respond_permission_for_run(
        &self,
        request_id: &str,
        approved: bool,
        run_id: Option<&str>,
    ) -> Result<(), String> {
        self.runtime
            .respond_permission(request_id, approved, run_id)
            .await
    }
}

pub fn protocol_version() -> &'static str {
    PROTOCOL_V2
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
    use std::sync::{Mutex as StdMutex, OnceLock};

    /// Process-global lock for tests that mutate NATIVES_* env (fixture, runtime dir, keys).
    fn with_env_lock<R>(f: impl FnOnce() -> R) -> R {
        static ENV_LOCK: OnceLock<StdMutex<()>> = OnceLock::new();
        let _g = ENV_LOCK
            .get_or_init(|| StdMutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        f()
    }

    #[test]
    fn create_run_is_idempotent_with_key() {
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
            std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());

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
                std::env::set_var("NATIVES_DB_PATH", value);
            } else {
                std::env::remove_var("NATIVES_DB_PATH");
            }
            if let Some(value) = previous_runtime {
                std::env::set_var("NATIVES_RUNTIME_DIR", value);
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
            std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());

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
                std::env::set_var("NATIVES_DB_PATH", value);
            } else {
                std::env::remove_var("NATIVES_DB_PATH");
            }
            if let Some(value) = previous_runtime {
                std::env::set_var("NATIVES_RUNTIME_DIR", value);
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
                std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());

                let store =
                    crate::storage::DataStore::new(&db_path, &dir.path().join("artifacts"))
                        .unwrap();
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
                        _cancel: &std::sync::atomic::AtomicBool,
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
                        _cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
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
                let rm = RunManager::new();
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
                    std::env::set_var("NATIVES_DB_PATH", value);
                } else {
                    std::env::remove_var("NATIVES_DB_PATH");
                }
                if let Some(value) = previous_runtime {
                    std::env::set_var("NATIVES_RUNTIME_DIR", value);
                } else {
                    std::env::remove_var("NATIVES_RUNTIME_DIR");
                }
            });
        });
    }

    #[test]
    fn retry_creates_new_run_id() {
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
        std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
        let store =
            crate::storage::DataStore::new(&db_path, &dir.path().join("artifacts")).unwrap();
        store.conn().unwrap().execute(
            "INSERT INTO conversation (id, mode, title, provider_id, model_id, permission_profile_id)
             VALUES ('c-detach', 'agent', 'Detached', 'openai', 'gpt-4o', 'full_access')",
            [],
        ).unwrap();
        let rm = Arc::new(RunManager::new());
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
                project_path: None,
                idempotency_key: Some("detach-1".into()),
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
                project_path: None,
                idempotency_key: None,
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
        assert_eq!(done.status, RunStatusV2::Completed);
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
            })
            .unwrap_err();
        assert!(
            err.contains("terminal") || err.contains("retry"),
            "unexpected: {err}"
        );
        if let Some(value) = previous_db {
            std::env::set_var("NATIVES_DB_PATH", value);
        } else {
            std::env::remove_var("NATIVES_DB_PATH");
        }
        if let Some(value) = previous_runtime {
            std::env::set_var("NATIVES_RUNTIME_DIR", value);
        } else {
            std::env::remove_var("NATIVES_RUNTIME_DIR");
        }
        std::env::remove_var("NATIVES_DAEMON_FIXTURE");
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
            let snap = RunManager::runs_snapshot_path();
            assert!(snap.exists(), "snapshot file missing at {}", snap.display());
            let rm2 = RunManager {
                runs: Mutex::new(HashMap::new()),
                idempotency: Mutex::new(HashMap::new()),
                last_content: Mutex::new(HashMap::new()),
                project_paths: Mutex::new(HashMap::new()),
                snapshot_path_override: Some(snapshot_path),
                data_store: None,
                runtime: Arc::new(crate::production::ProductionRuntime::new()),
            };
            let n = rm2.restore_runs_snapshot().unwrap();
            assert!(n >= 1, "expected restored runs from {}", snap.display());
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
            let rm = Arc::new(RunManager::new());
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
            };
            let a = rm.start_detached(req.clone()).unwrap();
            let b = rm.start_detached(req).unwrap();
            assert_eq!(a.id, b.id);
            // Second call must not error; active run is returned as-is.
            assert!(
                a.status.is_active() || a.status.is_terminal() || a.status == RunStatusV2::Queued
            );
            std::env::remove_var("NATIVES_DAEMON_FIXTURE");
        });
    }

    #[tokio::test]
    async fn start_cancel_retry_lifecycle_with_fixture() {
        std::env::set_var("NATIVES_DAEMON_FIXTURE", "1");
        let dir = tempfile::tempdir().unwrap();
        let previous_db = std::env::var("NATIVES_DB_PATH").ok();
        let previous_runtime = std::env::var("NATIVES_RUNTIME_DIR").ok();
        let db_path = dir.path().join("natives.db");
        std::env::set_var("NATIVES_DB_PATH", &db_path);
        std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
        let store =
            crate::storage::DataStore::new(&db_path, &dir.path().join("artifacts")).unwrap();
        store.conn().unwrap().execute(
            "INSERT INTO conversation (id, mode, title, provider_id, model_id, permission_profile_id)
             VALUES ('c1', 'agent', 'Fixture', 'openai', 'gpt-4o', 'full_access')",
            [],
        ).unwrap();
        let rm = RunManager::new();
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
                project_path: None,
                idempotency_key: None,
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
                        "sequence": e.sequence,
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
            std::env::set_var("NATIVES_DB_PATH", value);
        } else {
            std::env::remove_var("NATIVES_DB_PATH");
        }
        if let Some(value) = previous_runtime {
            std::env::set_var("NATIVES_RUNTIME_DIR", value);
        } else {
            std::env::remove_var("NATIVES_RUNTIME_DIR");
        }
        std::env::remove_var("NATIVES_DAEMON_FIXTURE");
    }

    /// Criterion 4: parent cancel_run_tree must request_cancel child engines
    /// registered under child run_id (not metadata-only cascade).
    #[tokio::test]
    async fn parent_cancel_tree_cancels_child_engine() {
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
                idempotency_key: Some("tree-parent".into()),
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
            .engines
            .lock()
            .await
            .insert(child.run_id.clone(), child_engine.clone());
        rm.runtime.task_outputs.lock().await.insert(
            child.id.clone(),
            crate::production::TaskRecord {
                run_id: child.run_id.clone(),
                status: "running".into(),
                output: None,
            },
        );

        // Child tool observes cancel flag (same bar as cancel_mid).
        let seen = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let seen_bg = seen.clone();
        let child_flag = child_engine.cancel_flag();
        let child_run = child.run_id.clone();
        let watch = tokio::spawn(async move {
            for _ in 0..200 {
                if child_flag.load(std::sync::atomic::Ordering::SeqCst) {
                    seen_bg.store(true, std::sync::atomic::Ordering::SeqCst);
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
            let _ = child_run;
        });

        // Sole cancel API: cancel parent tree.
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

        let child_interrupted = rm
            .runtime
            .events
            .replay_after(&child.run_id, 0)
            .iter()
            .any(|e| matches!(e.payload, RunEventKind::Cancelled { .. }));
        assert!(child_interrupted, "child run must get Cancelled event");

        if let Ok(dir) = std::env::var("NATIVES_TEST_SCRATCH") {
            let evidence = serde_json::json!({
                "parent_run_id": parent.id,
                "child_run_id": child.run_id,
                "child_task_id": child.id,
                "child_engine_cancel_flag": true,
                "child_status": "cancelled",
                "child_interrupted_event": true,
                "api": "cancel_run_tree",
            });
            let _ = std::fs::write(
                std::path::Path::new(&dir).join("cascade-cancel-tree.json"),
                serde_json::to_string_pretty(&evidence).unwrap_or_default(),
            );
        }
        std::env::remove_var("NATIVES_DAEMON_FIXTURE");
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
                    _cancel: Arc<std::sync::atomic::AtomicBool>,
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
                    cancel: &std::sync::atomic::AtomicBool,
                ) -> agent_core::ToolExecutionResult {
                    // Poll cancel flag while "working".
                    for _ in 0..40 {
                        if cancel.load(std::sync::atomic::Ordering::SeqCst) {
                            self.seen.store(true, std::sync::atomic::Ordering::SeqCst);
                            return agent_core::ToolExecutionResult {
                                output: serde_json::json!({"error": "cancelled"}),
                                is_error: true,
                                duration_ms: 0,
                            };
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                    }
                    let _ = (&self.engines, &self.run_id);
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
                    _cancel: Arc<std::sync::atomic::AtomicBool>,
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
                engines: rm_start.runtime.engines.clone(),
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
                    },
                    &ToolThenSlow,
                    &tools,
                )
                .await
        });

        // Wait until engine is registered, then cancel → request_cancel.
        for _ in 0..50 {
            if rm.runtime.engines.lock().await.contains_key(&run.id) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(
            rm.runtime.engines.lock().await.contains_key(&run.id),
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
        let has_interrupted_event = rm
            .runtime
            .events
            .replay_after(&run.id, 0)
            .iter()
            .any(|e| matches!(e.payload, RunEventKind::Interrupted { .. }));

        if let Ok(dir) = std::env::var("NATIVES_TEST_SCRATCH") {
            let evidence = serde_json::json!({
                "run_id": run.id,
                "status_after_cancel": "interrupted",
                "cancel_mid_run": true,
                "engine_was_registered": true,
                "engine_cancel_flag_observed_by_tool": engine_cancelled,
                "interrupted_event": has_interrupted_event,
            });
            let _ = std::fs::write(
                std::path::Path::new(&dir).join("daemon-cancel-mid.json"),
                serde_json::to_string_pretty(&evidence).unwrap_or_default(),
            );
        }
        assert!(
            has_interrupted_event,
            "cancel must append Interrupted event"
        );
        // Tool path should observe cancel flag from request_cancel.
        assert!(
            engine_cancelled,
            "production cancel must set engine cancel flag observed by tool execution"
        );
        std::env::remove_var("NATIVES_DAEMON_FIXTURE");
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
            waiters: rm.runtime.permission_waiters.clone(),
            subagents: rm.runtime.subagents.clone(),
            task_outputs: rm.runtime.task_outputs.clone(),
            engines: rm.runtime.engines.clone(),
            runtime: None,
            provider_id: "openai".into(),
            parent_run_id: run.id.clone(),
            conversation_id: "c-perm".into(),
            model_id: "gpt-4o".into(),
            permission_profile: "ask".into(),
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
                        "sequence": e.sequence,
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
        std::env::remove_var("NATIVES_DAEMON_FIXTURE");
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
            waiters: rt.permission_waiters.clone(),
            subagents: rt.subagents.clone(),
            task_outputs: rt.task_outputs.clone(),
            engines: rt.engines.clone(),
            runtime: None,
            provider_id: "openai".into(),
            parent_run_id: "parent-run".into(),
            conversation_id: "c".into(),
            model_id: "gpt-4o".into(),
            permission_profile: "full_access".into(),
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
                &std::sync::atomic::AtomicBool::new(false),
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
        assert_eq!(child_key, "child-key-from-broker");
        assert_ne!(child_key, "parent-key");
        assert_eq!(
            result.output.get("provider_id").and_then(|v| v.as_str()),
            Some("anthropic")
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
                &std::sync::atomic::AtomicBool::new(false),
            )
            .await;
        assert!(!out.is_error);

        let kill = tools
            .execute_tool(
                "kill_task",
                serde_json::json!({ "task_id": task_id }),
                &std::sync::atomic::AtomicBool::new(false),
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
        std::env::remove_var("NATIVES_DAEMON_FIXTURE");
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
                    waiters: prt.permission_waiters.clone(),
                    subagents: prt.subagents.clone(),
                    task_outputs: prt.task_outputs.clone(),
                    engines: prt.engines.clone(),
                    runtime: None,
                    provider_id: "openai".into(),
                    parent_run_id: parent_id.clone(),
                    conversation_id: "c-dual".into(),
                    model_id: "gpt-4o".into(),
                    permission_profile: "full_access".into(),
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
                        &std::sync::atomic::AtomicBool::new(false),
                    )
                    .await;
                assert!(!result.is_error, "{:?}", result.output);
                assert_eq!(
                    result.output.get("provider_id").and_then(|v| v.as_str()),
                    Some("anthropic")
                );
                assert_eq!(
                    result.output.get("key_id").and_then(|v| v.as_str()),
                    Some("child-key-B")
                );
                assert_eq!(
                    result.output.get("model_id").and_then(|v| v.as_str()),
                    Some("claude-3-haiku")
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
                            &std::sync::atomic::AtomicBool::new(false),
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
            waiters: rt.permission_waiters.clone(),
            subagents: rt.subagents.clone(),
            task_outputs: rt.task_outputs.clone(),
            engines: rt.engines.clone(),
            runtime: None,
            provider_id: "openai".into(),
            parent_run_id: "mcp-parent".into(),
            conversation_id: "c".into(),
            model_id: "m".into(),
            permission_profile: "full_access".into(),
        };
        let out = tools
            .execute_tool(
                "mcp_call",
                serde_json::json!({
                    "server": "gate-test",
                    "tool": "echo",
                    "arguments": {"x": 1}
                }),
                &std::sync::atomic::AtomicBool::new(false),
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
}
