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

pub mod host_authority_migration;
pub mod migrations;

use rusqlite::{params, Connection};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
#[cfg(test)]
use std::sync::MutexGuard;
#[cfg(test)]
use std::cell::Cell;

#[cfg(test)]
thread_local! {
    static ENV_LOCK_DEPTH: Cell<u32> = Cell::new(0);
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
        std::cell::RefCell::new(None);
    static TEST_ARTIFACT_OVERRIDE: std::cell::RefCell<Option<std::path::PathBuf>> =
        std::cell::RefCell::new(None);
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
    let art = TEST_ARTIFACT_OVERRIDE.with(|c| c.borrow().clone())
        .unwrap_or_else(|| db.parent().map(|p| p.join("artifacts")).unwrap_or_else(std::env::temp_dir));
    Some((db, art))
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

        // Enable WAL mode, foreign keys, and busy timeout
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA foreign_keys=ON;
             PRAGMA busy_timeout=30000;",
        )
        .map_err(|e| format!("Failed to set pragmas: {e}"))?;

        let store = DataStore {
            conn: Mutex::new(conn),
            db_path: db_path.clone(),
            artifact_dir: artifact_dir.clone(),
        };

        // Run migrations
        store.run_migrations()?;
        // Phase 0: merge Host assistant_* tables when present (idempotent).
        if !store.has_table("conversation") {
            // Fail closed: a half-migrated DB must not be returned to callers.
            return Err(format!(
                "conversation table missing after migrations at {}",
                db_path.display()
            ));
        }
        if let Err(e) = store.run_host_authority_migration() {
            eprintln!("[agent-daemon] host authority migration failed: {e}");
        }

        Ok(store)
    }

    /// Merge host `assistant_*` rows into canonical tables (best-effort).
    pub fn run_host_authority_migration(&self) -> Result<host_authority_migration::HostAuthorityMigrationResult, String> {
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

    /// Process-wide lock so concurrent DataStore::new calls cannot interleave
    /// schema migrations on different connections to the same file.
    fn migration_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    /// Process-wide lock for unit tests that mutate NATIVES_* env vars.
    /// Re-entrant on the same thread so store() can nest under with_temp_db.
    #[cfg(test)]
    pub fn env_test_lock() -> EnvTestGuard {
        EnvTestGuard::acquire()
    }

    /// Run all pending migrations.
    fn run_migrations(&self) -> Result<(), String> {
        let _migrate = Self::migration_lock();
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {e}"))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS _schema_version (
                version INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL DEFAULT (datetime('now'))
            );",
        )
        .map_err(|e| format!("Failed to ensure _schema_version: {e}"))?;

        let current_version: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM _schema_version",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        for (version, sql) in migrations::ALL {
            if *version <= current_version {
                continue;
            }
            // Avoid SAVEPOINT: some migration SQL toggles PRAGMA foreign_keys /
            // legacy_alter_table and interacts poorly with nested transactions.
            if let Err(e) = conn.execute_batch(sql) {
                let msg = e.to_string();
                // Tolerate additive column re-runs.
                if *version == 7 && msg.contains("duplicate column") {
                    let _ = conn.execute(
                        "INSERT OR IGNORE INTO _schema_version (version) VALUES (?1)",
                        params![version],
                    );
                    continue;
                }
                return Err(format!("Migration {version} failed: {e}"));
            }
            conn.execute(
                "INSERT OR IGNORE INTO _schema_version (version) VALUES (?1)",
                params![version],
            )
            .map_err(|e| format!("Failed to record migration {version}: {e}"))?;
        }

        Self::ensure_run_metadata_columns(&conn)?;
        Ok(())
    }

    fn ensure_run_metadata_columns(conn: &Connection) -> Result<(), String> {
        let alters = [
            "ALTER TABLE run ADD COLUMN parent_run_id TEXT",
            "ALTER TABLE run ADD COLUMN agent_profile_id TEXT",
            "ALTER TABLE run ADD COLUMN key_id TEXT",
            "ALTER TABLE run ADD COLUMN permission_profile TEXT NOT NULL DEFAULT 'ask'",
            "ALTER TABLE run ADD COLUMN project_path TEXT",
            "ALTER TABLE run ADD COLUMN retry_count INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE run ADD COLUMN idempotency_key TEXT",
        ];
        for sql in alters {
            if let Err(e) = conn.execute_batch(sql) {
                let msg = e.to_string();
                if !msg.contains("duplicate column") {
                    // Table may not exist yet on empty brand-new DB before mig1 — ignore
                    if msg.contains("no such table") {
                        continue;
                    }
                    return Err(format!("ensure run column failed: {e}"));
                }
            }
        }
        let _ = conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_run_idempotency_key ON run(idempotency_key);",
        );
        Ok(())
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
        let max_version: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM _schema_version",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(max_version > 0, "Migrations should have run");
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
