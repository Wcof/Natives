//! # Daemon Storage — SQLite database, migrations, and artifact store.
//!
//! ## Schema
//!
//! The daemon manages the following logical table families:
//!
//! - conversation, message, message_block
//! - run, run_event, tool_call
//! - permission_request
//! - artifact
//! - context_snapshot
//! - provider, provider_key, model_cache
//! - extension, extension_permission
//!
//! All tables use explicit foreign keys with CASCADE behavior.
//! SQLite is configured with WAL mode and foreign keys enabled.

pub mod actor;
pub mod host_authority_migration;
pub mod migrations;

use rusqlite::{params, Connection};
#[cfg(test)]
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

#[cfg(test)]
thread_local! {
    static ENV_LOCK_DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// Re-entrant env lock for tests (same thread may nest).
#[cfg(test)]
pub struct EnvTestGuard {
    // None when this acquisition was nested (outer guard still holds mutex).
    #[allow(dead_code)]
    inner: Option<MutexGuard<'static, ()>>,
}

#[cfg(test)]
impl EnvTestGuard {
    pub fn acquire() -> Self {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let nested = ENV_LOCK_DEPTH.with(|d| {
            let n = d.get();
            d.set(n + 1);
            n > 0
        });
        if nested {
            Self { inner: None }
        } else {
            let g = LOCK
                .get_or_init(|| Mutex::new(()))
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            Self { inner: Some(g) }
        }
    }
}

#[cfg(test)]
impl Drop for EnvTestGuard {
    fn drop(&mut self) {
        ENV_LOCK_DEPTH.with(|d| {
            let n = d.get().saturating_sub(1);
            d.set(n);
        });
        // inner MutexGuard drops here if present
    }
}

#[cfg(test)]
thread_local! {
    /// Per-test DB path override — avoids process-global env races under multi-thread tests.
    static TEST_DB_OVERRIDE: std::cell::RefCell<Option<std::path::PathBuf>> =
        const { std::cell::RefCell::new(None) };
    static TEST_ARTIFACT_OVERRIDE: std::cell::RefCell<Option<std::path::PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

/// Install a thread-local DB path for the current test thread (cfg(test) only).
#[cfg(test)]
pub fn set_test_db_override(db: Option<std::path::PathBuf>, artifacts: Option<std::path::PathBuf>) {
    TEST_DB_OVERRIDE.with(|c| *c.borrow_mut() = db);
    TEST_ARTIFACT_OVERRIDE.with(|c| *c.borrow_mut() = artifacts);
}

#[cfg(test)]
pub fn test_db_override() -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let db = TEST_DB_OVERRIDE.with(|c| c.borrow().clone())?;
    let art = TEST_ARTIFACT_OVERRIDE
        .with(|c| c.borrow().clone())
        .unwrap_or_else(|| {
            db.parent()
                .map(|p| p.join("artifacts"))
                .unwrap_or_else(std::env::temp_dir)
        });
    Some((db, art))
}

/// Test-only RAII guard: captures the `NATIVES_*` env vars and restores them on
/// drop so one test's fixture can never leak a deleted temp path into a later
/// test (T01 hermeticity). Use together with [`DataStore::env_test_lock`].
#[cfg(test)]
pub struct EnvRestore {
    db: Option<String>,
    asst: Option<String>,
    rt: Option<String>,
    fixture: Option<String>,
    event_dir: Option<String>,
    event_disable: Option<String>,
    run_mgr_memory: Option<String>,
}

#[cfg(test)]
impl EnvRestore {
    pub fn capture() -> Self {
        Self {
            db: std::env::var("NATIVES_DB_PATH").ok(),
            asst: std::env::var("NATIVES_ASSISTANT_DB_PATH").ok(),
            rt: std::env::var("NATIVES_RUNTIME_DIR").ok(),
            fixture: std::env::var("NATIVES_DAEMON_FIXTURE").ok(),
            event_dir: std::env::var("NATIVES_EVENT_LOG_DIR").ok(),
            event_disable: std::env::var("NATIVES_EVENT_LOG_DISABLE").ok(),
            run_mgr_memory: std::env::var("NATIVES_RUN_MANAGER_MEMORY").ok(),
        }
    }

    pub fn restore(&self) {
        for (name, value) in [
            ("NATIVES_DB_PATH", self.db.as_deref()),
            ("NATIVES_ASSISTANT_DB_PATH", self.asst.as_deref()),
            ("NATIVES_RUNTIME_DIR", self.rt.as_deref()),
            ("NATIVES_DAEMON_FIXTURE", self.fixture.as_deref()),
            ("NATIVES_EVENT_LOG_DIR", self.event_dir.as_deref()),
            ("NATIVES_EVENT_LOG_DISABLE", self.event_disable.as_deref()),
            ("NATIVES_RUN_MANAGER_MEMORY", self.run_mgr_memory.as_deref()),
        ] {
            match value {
                Some(v) => std::env::set_var(name, v),
                None => std::env::remove_var(name),
            }
        }
    }
}

#[cfg(test)]
impl Drop for EnvRestore {
    fn drop(&mut self) {
        self.restore();
    }
}

/// Cross-process, cross-instance exclusive lock for schema migrations
/// (DATA-002).
///
/// Host and Daemon open the same `assistant.db`, and two daemon instances can
/// race on the same file. A process-wide `Mutex` cannot serialize those, so the
/// lock is a filesystem directory next to the DB file: `mkdir` is atomic across
/// processes, and a live PID marker lets a new process detect and break a stale
/// lock left by a crash. Acquisition is bounded by a timeout with an explicit
/// error; two processes can never migrate simultaneously.
#[derive(Debug)]
struct MigrationFileLock {
    lock_dir: PathBuf,
}

/// A lock directory with no readable owner PID is considered stale only after
/// this age; a dead owner PID is always stale regardless of age.
const MIGRATION_LOCK_STALENESS_MS: u64 = 120_000;

impl MigrationFileLock {
    fn path_for(db_path: &Path) -> PathBuf {
        let mut os = db_path.as_os_str().to_os_string();
        os.push(".migration.lock");
        PathBuf::from(os)
    }

    /// Acquire the lock with a bounded wait. `NATIVES_MIGRATION_LOCK_TIMEOUT_MS`
    /// (test knob) overrides the production 30s timeout.
    fn acquire(db_path: &Path) -> Result<Self, String> {
        let lock_dir = Self::path_for(db_path);
        if let Some(parent) = lock_dir.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                format!(
                    "Failed to prepare directory for migration lock {}: {e}",
                    lock_dir.display()
                )
            })?;
        }
        let timeout_ms = std::env::var("NATIVES_MIGRATION_LOCK_TIMEOUT_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(30_000);
        let started = std::time::Instant::now();
        loop {
            match std::fs::create_dir(&lock_dir) {
                Ok(()) => {
                    // Marker with the owning PID; used by other processes to
                    // decide whether the lock is live or stale.
                    let _ =
                        std::fs::write(lock_dir.join("owner.pid"), std::process::id().to_string());
                    return Ok(MigrationFileLock { lock_dir });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    if Self::break_stale(&lock_dir) {
                        continue;
                    }
                    if started.elapsed().as_millis() >= u128::from(timeout_ms) {
                        return Err(format!(
                            "Migration lock is held by another process: {} (timed out after \
                             {} ms). Wait for the other instance to finish migrating, or remove \
                             the lock directory if it was left behind by a crashed process.",
                            lock_dir.display(),
                            timeout_ms
                        ));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    // Parent vanished (e.g. a tempdir torn down mid-flight in
                    // tests). Recreate it and retry rather than fail spuriously.
                    if let Some(parent) = lock_dir.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    if started.elapsed().as_millis() >= u128::from(timeout_ms) {
                        return Err(format!(
                            "Failed to acquire migration lock at {}: {e}",
                            lock_dir.display()
                        ));
                    }
                }
                Err(e) => {
                    return Err(format!(
                        "Failed to acquire migration lock at {}: {e}",
                        lock_dir.display()
                    ));
                }
            }
        }
    }

    /// Reclaim a stale lock directory. Returns true when the directory was
    /// removed. A live owning PID is never considered stale (PID liveness is
    /// authoritative on unix); a dead PID is always stale; a directory without
    /// a readable owner PID falls back to mtime age.
    fn break_stale(lock_dir: &Path) -> bool {
        let pid = std::fs::read_to_string(lock_dir.join("owner.pid"))
            .ok()
            .and_then(|s| s.trim().parse::<i32>().ok());
        match pid {
            Some(pid) => {
                if process_alive(pid) {
                    return false;
                }
                std::fs::remove_dir_all(lock_dir).is_ok()
            }
            None => {
                let age_ms = std::fs::metadata(lock_dir)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.elapsed().ok())
                    .map(|d| d.as_millis());
                if let Some(age_ms) = age_ms {
                    if age_ms < u128::from(MIGRATION_LOCK_STALENESS_MS) {
                        return false;
                    }
                }
                std::fs::remove_dir_all(lock_dir).is_ok()
            }
        }
    }
}

#[cfg(unix)]
fn process_alive(pid: i32) -> bool {
    // SAFETY: `kill(pid, 0)` signals nothing; it only probes existence.
    // Returns 0 when the process exists, -1 with ESRCH when it does not.
    unsafe { libc::kill(pid, 0) == 0 }
}

#[cfg(not(unix))]
fn process_alive(_pid: i32) -> bool {
    // Non-unix (Windows): no portable PID liveness probe without a dependency.
    // Staleness falls back to the lock directory mtime only.
    false
}

impl Drop for MigrationFileLock {
    fn drop(&mut self) {
        // Only remove the directory if we still own it — a stale-lock reclaimer
        // may have replaced us after we were considered dead.
        let owner = std::fs::read_to_string(self.lock_dir.join("owner.pid")).ok();
        if owner.as_deref() == Some(&std::process::id().to_string()) {
            let _ = std::fs::remove_file(self.lock_dir.join("owner.pid"));
            let _ = std::fs::remove_dir(&self.lock_dir);
        }
    }
}

/// What `run_migrations` holds for the duration of the schema migration.
///
/// The variant payload is deliberately never read — it exists for its RAII
/// Drop side effect (releasing the file lock / process mutex), so dead_code is
/// expected.
#[allow(dead_code)]
enum MigrationGuard {
    /// Directory lock next to the DB file — cross-process visibility.
    File(MigrationFileLock),
    /// Process-wide mutex — used only for `:memory:` databases, which cannot
    /// be shared across processes, so the mutex is sufficient there.
    Process(MutexGuard<'static, ()>),
}

/// The data store — manages SQLite connection and artifact storage.
pub struct DataStore {
    conn: Mutex<Connection>,
    /// Original database path (retained for diagnostics / reopen).
    #[allow(dead_code)]
    db_path: PathBuf,
    artifact_dir: PathBuf,
}

impl DataStore {
    /// Open or create a database at the given path.
    pub fn new(db_path: &PathBuf, artifact_dir: &PathBuf) -> Result<Self, String> {
        std::fs::create_dir_all(artifact_dir)
            .map_err(|e| format!("Failed to create artifact dir: {e}"))?;

        let conn =
            Connection::open(db_path).map_err(|e| format!("Failed to open database: {e}"))?;

        // Test-only knob: a short busy timeout lets hermetic tests surface
        // SQLITE_BUSY quickly (a held write lock from a second connection)
        // instead of parking on the production 30s timeout. Absent in
        // production, the default is unchanged.
        let busy_timeout_ms = std::env::var("NATIVES_TEST_BUSY_TIMEOUT_MS")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(30_000);

        // Enable WAL mode, foreign keys, and busy timeout
        conn.execute_batch(&format!(
            "PRAGMA journal_mode=WAL;
             PRAGMA foreign_keys=ON;
             PRAGMA busy_timeout={busy_timeout_ms};"
        ))
        .map_err(|e| format!("Failed to set pragmas: {e}"))?;

        let store = DataStore {
            conn: Mutex::new(conn),
            db_path: db_path.clone(),
            artifact_dir: artifact_dir.clone(),
        };

        // Run migrations
        store.run_migrations()?;

        // Fail closed: Host and Daemon share assistant.db but Host owns
        // `_schema_version` with higher version numbers for `assistant_*`
        // tables. Daemon migrations must use `_daemon_schema_version` so they
        // are not skipped on Host-first DBs. If the canonical table is still
        // missing after migrations, refuse to open rather than return a half
        // store that will fail on every conversation RPC.
        if !store.has_table("conversation") {
            return Err(format!(
                "conversation table missing after migrations at {}",
                db_path.display()
            ));
        }

        // Phase 0: merge Host assistant_* tables when present (idempotent).
        if let Err(e) = store.run_host_authority_migration() {
            eprintln!("[agent-daemon] host authority migration failed: {e}");
        }

        Ok(store)
    }

    /// Merge host `assistant_*` rows into canonical tables (best-effort).
    pub fn run_host_authority_migration(
        &self,
    ) -> Result<host_authority_migration::HostAuthorityMigrationResult, String> {
        let conn = self.conn()?;
        host_authority_migration::migrate_host_authority(&conn)
    }

    pub fn db_path(&self) -> &PathBuf {
        &self.db_path
    }

    pub fn has_table(&self, name: &str) -> bool {
        let Ok(conn) = self.conn() else {
            return false;
        };
        conn.query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name=?1",
            params![name],
            |row| row.get(0),
        )
        .unwrap_or(false)
    }

    /// Acquire a cross-process, cross-instance exclusive lock for the schema
    /// migration (DATA-002). `:memory:` databases cannot be shared across
    /// processes, so for them a process-wide mutex is sufficient; file-backed
    /// databases use the directory lock (see [`MigrationFileLock`]).
    fn acquire_migration_lock(db_path: &Path) -> Result<MigrationGuard, String> {
        if db_path == Path::new(":memory:") {
            static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
            let guard = LOCK
                .get_or_init(|| Mutex::new(()))
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            return Ok(MigrationGuard::Process(guard));
        }
        MigrationFileLock::acquire(db_path).map(MigrationGuard::File)
    }

    /// Process-wide lock for unit tests that mutate NATIVES_* env vars.
    /// Re-entrant on the same thread so store() can nest under with_temp_db.
    #[cfg(test)]
    pub fn env_test_lock() -> EnvTestGuard {
        // T01 hermeticity: never let agent-core's EventSequencer fall through
        // to ~/.natives/events in tests. When no event log is configured for
        // the current test, force memory-only events so a test never reads or
        // writes the developer's home directory. Tests that need durable
        // events set NATIVES_EVENT_LOG_DIR or NATIVES_RUNTIME_DIR first.
        if std::env::var("NATIVES_EVENT_LOG_DIR").is_err()
            && std::env::var("NATIVES_RUNTIME_DIR").is_err()
            && std::env::var("NATIVES_EVENT_LOG_DISABLE").is_err()
        {
            std::env::set_var("NATIVES_EVENT_LOG_DISABLE", "1");
        }
        EnvTestGuard::acquire()
    }

    /// Run all pending daemon migrations under a cross-process exclusive lock.
    ///
    /// Host (`src-tauri/src/daemon/data.rs`) and Daemon both open the same
    /// `assistant.db` file. Host already owns `_schema_version` for its
    /// `assistant_*` migrations (currently up to v14). Sharing that table
    /// caused Daemon migrations 1–9 to be skipped on Host-first DBs, leaving
    /// no canonical `conversation` / `message` / `run` tables.
    ///
    /// Daemon therefore tracks progress in `_daemon_schema_version`. On first
    /// open of a legacy Host DB we bootstrap that table from the presence of
    /// the canonical tables (not from Host's version numbers).
    /// Run pending migrations via the versioned, reentrant runner
    /// (`migrations::run_pending`): per-version ledger membership, checksum
    /// drift fail-closed, postcondition-driven adoption for partial/crash
    /// states, and a final `foreign_key_check` gate. The whole run holds a
    /// cross-process file lock (DATA-002), so a concurrent Host or daemon
    /// instance cannot interleave schema migrations.
    fn run_migrations(&self) -> Result<(), String> {
        let _migrate = Self::acquire_migration_lock(&self.db_path)?;
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {e}"))?;
        migrations::run_pending(&conn)
    }

    /// Get a connection for direct queries.
    pub fn conn(&self) -> Result<std::sync::MutexGuard<'_, Connection>, String> {
        self.conn.lock().map_err(|e| format!("Lock error: {e}"))
    }

    /// Get the artifact storage directory.
    pub fn artifact_dir(&self) -> &PathBuf {
        &self.artifact_dir
    }

    /// Store artifact content by hash, return the file path.
    pub fn store_artifact_content(&self, sha256: &str, content: &[u8]) -> Result<PathBuf, String> {
        let path = self.artifact_dir.join(sha256);
        std::fs::write(&path, content).map_err(|e| format!("Failed to write artifact: {e}"))?;
        Ok(path)
    }

    /// Read artifact content by hash.
    pub fn read_artifact_content(&self, sha256: &str) -> Result<Vec<u8>, String> {
        let path = self.artifact_dir.join(sha256);
        std::fs::read(&path).map_err(|e| format!("Failed to read artifact: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_test_store() -> DataStore {
        let tmp = std::env::temp_dir();
        let db_path = tmp.join(format!("test_daemon_{}.db", uuid::Uuid::new_v4()));
        let art_dir = tmp.join(format!("test_artifacts_{}", uuid::Uuid::new_v4()));
        DataStore::new(&db_path, &art_dir).unwrap()
    }

    fn temp_db_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("{name}_{}.db", uuid::Uuid::new_v4()))
    }

    /// DATA-002 regression: the migration lock is cross-instance. A second
    /// acquire while the first is held must fail with a bounded timeout and an
    /// explicit error, exactly what a concurrent Host/Daemon (or two daemon
    /// instances) would hit — a process-internal Mutex cannot provide this.
    #[test]
    fn migration_lock_is_exclusive_across_instances() {
        let _env = EnvTestGuard::acquire();
        let _restore = EnvRestore::capture();
        std::env::set_var("NATIVES_MIGRATION_LOCK_TIMEOUT_MS", "200");

        let db = temp_db_path("lock-excl");
        let first = MigrationFileLock::acquire(&db).expect("first acquire");
        let err = MigrationFileLock::acquire(&db).expect_err("second acquire must fail");
        assert!(
            err.contains("Migration lock is held by another process"),
            "unexpected lock error: {err}"
        );
        assert!(
            err.contains("timed out after"),
            "error must report the bounded timeout: {err}"
        );
        drop(first);

        // After release the lock is re-acquirable.
        let second = MigrationFileLock::acquire(&db).expect("re-acquire after release");
        drop(second);
        let lock_dir = MigrationFileLock::path_for(&db);
        assert!(
            !lock_dir.exists(),
            "lock directory must be released on drop"
        );
    }

    /// A lock directory left behind by a dead process (crash) must be reclaimed
    /// automatically instead of blocking startup forever.
    #[test]
    fn stale_migration_lock_is_reclaimed() {
        let _env = EnvTestGuard::acquire();
        let _restore = EnvRestore::capture();
        std::env::set_var("NATIVES_MIGRATION_LOCK_TIMEOUT_MS", "500");

        let db = temp_db_path("lock-stale");
        let lock_dir = MigrationFileLock::path_for(&db);
        std::fs::create_dir_all(&lock_dir).unwrap();
        // An impossible PID is dead on any real system (pid_max << 99999999).
        std::fs::write(lock_dir.join("owner.pid"), "99999999").unwrap();

        let guard = MigrationFileLock::acquire(&db).expect("stale lock must be reclaimed");
        drop(guard);
        assert!(!lock_dir.exists(), "lock directory must be removed on drop");
    }

    /// A lock directory owned by a live process must NOT be reclaimed — only
    /// the bounded timeout applies.
    #[test]
    fn live_lock_is_never_reclaimed() {
        let _env = EnvTestGuard::acquire();
        let _restore = EnvRestore::capture();
        std::env::set_var("NATIVES_MIGRATION_LOCK_TIMEOUT_MS", "200");

        let db = temp_db_path("lock-live");
        let lock_dir = MigrationFileLock::path_for(&db);
        std::fs::create_dir_all(&lock_dir).unwrap();
        // This test process is alive: the lock is genuinely held.
        std::fs::write(lock_dir.join("owner.pid"), std::process::id().to_string()).unwrap();

        let err = MigrationFileLock::acquire(&db).expect_err("live lock must not be reclaimed");
        assert!(
            err.contains("Migration lock is held by another process"),
            "{err}"
        );
        // Cleanup: this test owns the dir, remove it.
        let _ = std::fs::remove_dir_all(&lock_dir);
    }

    /// DATA-002 end-to-end: `DataStore::new` fails closed when another instance
    /// holds the migration lock, and succeeds after it is released.
    #[test]
    fn datastore_open_fails_closed_while_migration_lock_held() {
        let _env = EnvTestGuard::acquire();
        let _restore = EnvRestore::capture();
        std::env::set_var("NATIVES_MIGRATION_LOCK_TIMEOUT_MS", "200");

        let db = temp_db_path("lock-ds");
        let art = std::env::temp_dir().join(format!("lock-ds-art_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&art).unwrap();

        let _held = MigrationFileLock::acquire(&db).expect("hold lock");
        let err = match DataStore::new(&db, &art) {
            Ok(_) => panic!("DataStore::new must fail while the migration lock is held"),
            Err(e) => e,
        };
        assert!(
            err.contains("Migration lock is held by another process"),
            "unexpected error: {err}"
        );
        drop(_held);

        let store = DataStore::new(&db, &art).expect("open after lock released");
        assert!(store.has_table("conversation"));
        let _ = std::fs::remove_dir_all(&art);
    }

    #[test]
    fn test_store_initializes_with_wal() {
        let store = setup_test_store();
        let conn = store.conn().unwrap();
        let journal_mode: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(journal_mode.to_lowercase(), "wal");
    }

    #[test]
    fn test_foreign_keys_enabled() {
        let store = setup_test_store();
        let conn = store.conn().unwrap();
        let fk_enabled: i32 = conn
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .unwrap();
        assert_eq!(fk_enabled, 1);
    }

    #[test]
    fn test_artifact_store_and_read() {
        let store = setup_test_store();
        let content = b"Hello, artifact world!";
        let hash = "abc123hash";
        let path = store.store_artifact_content(hash, content).unwrap();
        assert!(path.exists());
        let read_content = store.read_artifact_content(hash).unwrap();
        assert_eq!(read_content, content);
    }

    #[test]
    fn test_migrations_run_sequentially() {
        let store = setup_test_store();
        let conn = store.conn().unwrap();
        let applied: i64 = conn
            .query_row("SELECT COUNT(*) FROM _daemon_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert!(applied > 0, "Migrations should have run");
    }

    /// Host-first DBs already have `_schema_version` at v14 for `assistant_*`
    /// tables. Daemon must still create the unprefixed canonical tables.
    #[test]
    fn test_host_schema_version_does_not_skip_daemon_migrations() {
        let tmp = std::env::temp_dir();
        let db_path = tmp.join(format!("test_host_first_{}.db", uuid::Uuid::new_v4()));
        let art_dir = tmp.join(format!("test_host_first_art_{}", uuid::Uuid::new_v4()));

        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "PRAGMA journal_mode=WAL;
                 PRAGMA foreign_keys=ON;
                 CREATE TABLE _schema_version (
                    version INTEGER PRIMARY KEY,
                    applied_at TEXT NOT NULL DEFAULT (datetime('now'))
                 );
                 INSERT INTO _schema_version (version) VALUES (1),(2),(3),(4),(5),(6),(7),(8),(9),(11),(12),(13),(14);
                 CREATE TABLE assistant_conversations (
                    id TEXT PRIMARY KEY,
                    mode TEXT NOT NULL DEFAULT 'chat',
                    project_id TEXT,
                    title TEXT NOT NULL DEFAULT '',
                    provider_id TEXT NOT NULL,
                    model_id TEXT NOT NULL,
                    permission_profile_id TEXT,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    archived_at TEXT
                 );
                 INSERT INTO assistant_conversations
                   (id, mode, title, provider_id, model_id, created_at, updated_at)
                 VALUES ('host-c1', 'agent', 'From Host', 'openai', 'gpt-4o',
                         '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');",
            )
            .unwrap();
        }

        let store = DataStore::new(&db_path, &art_dir).expect("open host-first DB");
        {
            let conn = store.conn().unwrap();
            let has_conversation: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='conversation'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(has_conversation, 1, "canonical conversation must exist");

            let has_message: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='message'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(has_message, 1, "canonical message must exist");

            let daemon_version: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM _daemon_migrations WHERE id >= 10",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(
                daemon_version >= 1,
                "daemon migrations should record in _daemon_migrations"
            );

            // Host row should be merged into canonical conversation.
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM conversation WHERE id='host-c1'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "host conversation should be merged");

            // Host version table must remain intact for Host migrations.
            let host_version: i64 = conn
                .query_row(
                    "SELECT COALESCE(MAX(version), 0) FROM _schema_version",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(host_version, 14);
        }

        let _ = std::fs::remove_file(&db_path);
        let _ = std::fs::remove_dir_all(&art_dir);
    }

    /// Optional recovery check against a real Host-first `assistant.db` backup.
    /// Set `NATIVES_TEST_ASSISTANT_DB` to a writable copy of the file.
    #[test]
    fn test_open_real_host_first_backup_if_present() {
        let Ok(path) = std::env::var("NATIVES_TEST_ASSISTANT_DB") else {
            return;
        };
        let db = std::path::PathBuf::from(path);
        if !db.exists() {
            return;
        }
        let art = db
            .parent()
            .map(|p| p.join("artifacts_test"))
            .unwrap_or_else(|| std::env::temp_dir().join("artifacts_test"));
        let _ = std::fs::create_dir_all(&art);
        let store = DataStore::new(&db, &art).expect("open host-first backup");
        assert!(store.has_table("conversation"));
        assert!(store.has_table("message"));
        assert!(store.has_table("run"));
        assert!(store.has_table("run_event"));
        assert!(store.has_table("_daemon_migrations"));
        let conn = store.conn().unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM conversation", [], |r| r.get(0))
            .unwrap();
        let host_n: i64 = conn
            .query_row("SELECT COUNT(*) FROM assistant_conversations", [], |r| {
                r.get(0)
            })
            .unwrap_or(0);
        assert!(
            n >= host_n,
            "expected host conversations merged into canonical table (canonical={n}, host={host_n})"
        );
    }

    #[test]
    fn test_tables_exist_after_migration() {
        let store = setup_test_store();
        let conn = store.conn().unwrap();
        let required_tables = [
            "conversation",
            "message",
            "message_block",
            "run",
            "run_event",
            "tool_call",
            "permission_request",
            "artifact",
            "context_snapshot",
            "provider",
            "provider_key",
            "model_cache",
            "extension",
            "extension_permission",
            "subagent_route_policy",
            "subagent_session",
        ];
        for table in &required_tables {
            let count: i32 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    params![table],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "Table '{}' should exist after migration", table);
        }
    }

    #[test]
    fn test_run_status_check_matches_protocol_v2_statuses() {
        let store = setup_test_store();
        let conn = store.conn().unwrap();
        conn.execute(
            "INSERT INTO conversation (id, mode, title, provider_id, model_id)
             VALUES ('status-conv', 'agent', 'Status', 'openai', 'gpt-4o')",
            [],
        )
        .unwrap();
        for status in [
            "created",
            "queued",
            "preparing",
            "running",
            "waiting_permission",
            "waiting_subagent",
            "cancelling",
            "completed",
            "failed",
            "cancelled",
            "interrupted",
        ] {
            conn.execute(
                "INSERT INTO run (id, conversation_id, status, provider_id, model_id)
                 VALUES (?1, 'status-conv', ?2, 'openai', 'gpt-4o')",
                params![format!("run-{status}"), status],
            )
            .unwrap_or_else(|e| panic!("status {status} rejected by run CHECK: {e}"));
        }
    }
}
