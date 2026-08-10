//! RunManager type, constructors, process global, and the test suite.

use crate::production::ProductionRuntime;
use crate::storage::DataStore;
use agent_core::EventSequencer;
use assistant_protocol::v2::{DaemonCapabilities, RunV2, PROTOCOL_V2};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[cfg(test)]
use crate::production::{FixtureMode, FixtureProvider};
#[cfg(test)]
use agent_core::{AgentEngine, EngineOutcome, TransitionMetadata};
#[cfg(test)]
use assistant_protocol::v2::{
    CancelRunRequest, ResumeDecision, ResumeRunRequest, RunEventKind, RunStatusV2,
};
#[cfg(test)]
use tokio_util::sync::CancellationToken;
#[cfg(test)]
use uuid::Uuid;

/// Active and historical runs held by the daemon process.
/// Releases a run's MCP server references on every exit path of `start()`
/// (success, resolve failure after acquire, panic unwind).
pub(crate) struct McpRunRefGuard {
    pub(crate) run_id: String,
}

impl Drop for McpRunRefGuard {
    fn drop(&mut self) {
        crate::mcp_runtime::global_mcp().release_run(&self.run_id);
    }
}
pub struct RunManager {
    pub(crate) runs: Mutex<HashMap<String, RunV2>>,
    pub(crate) idempotency: Mutex<HashMap<String, String>>,
    /// Last start request content for retry.
    pub(crate) last_content: Mutex<HashMap<String, String>>,
    /// Explicit project/workspace root per run (hooks + tool sandbox).
    pub(crate) project_paths: Mutex<HashMap<String, std::path::PathBuf>>,
    pub(crate) snapshot_path_override: Option<std::path::PathBuf>,
    pub(crate) data_store: Option<Arc<DataStore>>,
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
    #[cfg(not(test))]
    {
        use std::sync::OnceLock;
        static GLOBAL: OnceLock<RunManager> = OnceLock::new();
        GLOBAL.get_or_init(RunManager::new)
    }
    #[cfg(test)]
    {
        *test_global_lock().lock().unwrap_or_else(|e| e.into_inner())
    }
}
/// Test-only process-global RunManager storage. Unlike the production
/// `OnceLock`, a test can replace the global via [`install_global_for_test`]
/// so a hermetic test deterministically binds the global to its own temp
/// database (or to a memory-only manager) before touching it. The lazy fallback
/// keeps the historical behavior for tests that never install.
#[cfg(test)]
fn test_global_lock() -> &'static std::sync::Mutex<&'static RunManager> {
    use std::sync::{Mutex, OnceLock};
    static TEST_GLOBAL: OnceLock<Mutex<&'static RunManager>> = OnceLock::new();
    TEST_GLOBAL.get_or_init(|| Mutex::new(Box::leak(Box::new(RunManager::new()))))
}
/// Test-only: replace the process-global RunManager returned by
/// [`global_run_manager`]. Call while holding [`DataStore::env_test_lock`] so
/// the installation is deterministic under `--test-threads=2`. Returns the
/// installed manager.
#[cfg(test)]
pub fn install_global_for_test(mgr: RunManager) -> &'static RunManager {
    use std::sync::Mutex;
    let m: &'static RunManager = Box::leak(Box::new(mgr));
    let lock: &'static Mutex<&'static RunManager> = test_global_lock();
    *lock.lock().unwrap_or_else(|e| e.into_inner()) = m;
    m
}
/// Test-only: install a memory-only global RunManager (no data store, no
/// ~/.natives snapshot/event defaults). Used by fixtures that exercise
/// tool-gate behavior without a durable store.
#[cfg(test)]
pub fn install_memory_global_for_test() -> &'static RunManager {
    install_global_for_test(RunManager::new_memory())
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
        // T05: after the restart interrupts active child runs, release every
        // subagent slot whose child is now terminal (interrupted) so the
        // in-memory ledger is not over-subscribed. Durable budgets stay on the
        // session rows and are not reset by recovery.
        if let Some(store) = &mgr.data_store {
            let conn = store.conn().map_err(|e| format!("conn lock: {e}"))?;
            crate::subagent_store::recover_subagent_reservations_on(&conn)?;
        }
        mgr.restore_runs_snapshot()?;
        // Hydrate SessionCoordinator from durable queue/actor rows (no auto re-exec).
        crate::prompt_queue_store::recover_session_actors_on_startup()?;
        // Idempotent crash-gap repair: project context_snapshot rows for runs
        // that committed a ContextSnapshotCommitted event but never reached the
        // run-end projection. Best-effort — the loaders already fail closed on
        // a missing snapshot, so a repair failure must not block daemon start.
        if let Err(error) = crate::conversation_store::backfill_context_snapshots() {
            eprintln!("[run_manager] context snapshot backfill failed: {error}");
        }
        // B03: idempotent conversation projection recovery — any committed
        // turn whose events are not yet covered by the projector watermark is
        // re-projected at startup. Best-effort like the snapshot backfill: a
        // corrupt event quarantines its run, the daemon still starts and the
        // remaining runs recover.
        if let Err(error) = crate::conversation_projector::recover_projections() {
            eprintln!("[run_manager] conversation projection recovery failed: {error}");
        }
        // Expire any in-memory waiters (oneshot futures are never restored).
        // InteractionHub starts empty on new process — no action required.
        Ok(mgr)
    }
    #[allow(clippy::needless_return)] // cfg(test)/cfg(not(test)) branches make the tail ambiguous
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
            // Tests must opt into a real store via set_test_db_override. Reading
            // process-global NATIVES_* env vars here is a parallel-test race
            // (one test's fixture leaks into another), so under cfg(test) the
            // default is always in-memory.
            return None;
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
            // T103 (P0-026): a production assistant.db open/migration failure
            // must fail closed — never silently degrade to memory-only. The
            // in-memory fallback is only for tests / no configured store.
            match DataStore::new(&db_path, &artifact_dir) {
                Ok(store) => Some(Arc::new(store)),
                Err(e) => {
                    panic!(
                        "RunManager fail-closed: assistant.db open/migration failed at {}: {e}",
                        db_path.display()
                    )
                }
            }
        }
    }
    pub fn events(&self) -> EventSequencer {
        self.runtime.events.clone()
    }
    pub fn capabilities() -> DaemonCapabilities {
        let mut caps = DaemonCapabilities::current();
        // TASK-013: the Claude CLI bridge is NOT a native daemon authority —
        // runs it executes are controlled by the CLI, outside the daemon's
        // execution/side-effect ledger. Mark it explicitly (Undetermined) so a
        // caller can never mistake the CLI for a native runtime.
        if crate::cli_runtime_bridge::claude_cli_available() {
            caps.runtimes.push(assistant_protocol::v2::RuntimeCapability {
                id: "cli".into(),
                display_name: "Claude CLI (bridge)".into(),
                status: assistant_protocol::v2::RuntimeAvailability::Undetermined,
                reason: Some(
                    "CLI executes externally; not a native daemon authority over runs, side effects, or ledger"
                        .into(),
                ),
                methods: Vec::new(),
            });
        }
        caps
    }
}
pub fn protocol_version() -> &'static str {
    PROTOCOL_V2
}

#[cfg(test)]
#[path = "manager_tests.rs"]
mod tests;
