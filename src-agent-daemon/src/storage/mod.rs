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

pub mod migrations;

use rusqlite::{params, Connection};
use std::path::PathBuf;
use std::sync::Mutex;

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
             PRAGMA busy_timeout=5000;",
        )
        .map_err(|e| format!("Failed to set pragmas: {e}"))?;

        let store = DataStore {
            conn: Mutex::new(conn),
            db_path: db_path.clone(),
            artifact_dir: artifact_dir.clone(),
        };

        // Run migrations
        store.run_migrations()?;

        Ok(store)
    }

    /// Run all pending migrations.
    fn run_migrations(&self) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| format!("Lock error: {e}"))?;

        let current_version: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM _schema_version",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        for (version, sql) in migrations::ALL {
            if *version > current_version {
                conn.execute_batch(sql)
                    .map_err(|e| format!("Migration {version} failed: {e}"))?;
                conn.execute(
                    "INSERT INTO _schema_version (version) VALUES (?1)",
                    params![version],
                )
                .map_err(|e| format!("Failed to record migration {version}: {e}"))?;
            }
        }

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
