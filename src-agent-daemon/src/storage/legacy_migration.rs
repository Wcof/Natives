//! Legacy assistant data migration.
//!
//! Migrates conversations from the old embedded assistant database format
//! to the new daemon-owned schema. The migration is restart-safe and
//! rolls back on any validation failure.

use crate::storage::DataStore;
use rusqlite::params;
use std::path::Path;

/// Result of a legacy migration.
#[derive(Debug)]
pub struct MigrationResult {
    pub conversations_migrated: u64,
    pub messages_migrated: u64,
    pub blocks_migrated: u64,
    pub errors: Vec<String>,
}

/// Migrate legacy assistant data from an old database file.
pub fn migrate_legacy_data(
    daemon_store: &DataStore,
    legacy_db_path: &Path,
) -> Result<MigrationResult, String> {
    // Open legacy database (read-only)
    let legacy_conn = rusqlite::Connection::open(legacy_db_path)
        .map_err(|e| format!("Failed to open legacy database: {e}"))?;

    // Check if legacy database has the expected schema
    let has_assistant_sessions: bool = legacy_conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='assistant_sessions'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if !has_assistant_sessions {
        return Err("Legacy database does not contain assistant_sessions table".to_string());
    }

    let daemon_conn = daemon_store.conn()?;
    let mut result = MigrationResult {
        conversations_migrated: 0,
        messages_migrated: 0,
        blocks_migrated: 0,
        errors: Vec::new(),
    };

    // Begin transaction on daemon database
    daemon_conn
        .execute_batch("BEGIN TRANSACTION")
        .map_err(|e| format!("Failed to begin transaction: {e}"))?;

    // Migrate conversations (sessions)
    {
        let mut stmt = legacy_conn
            .prepare("SELECT id, title, model, provider, created_at, updated_at FROM assistant_sessions")
            .map_err(|e| format!("Failed to prepare session query: {e}"))?;

        let rows = stmt
            .query_map([], |row| {
                let id: String = row.get(0)?;
                let title: String = row.get(1)?;
                let model: String = row.get(2)?;
                let provider: String = row.get(3)?;
                let created_at: String = row.get(4)?;
                let updated_at: String = row.get(5)?;
                Ok((id, title, model, provider, created_at, updated_at))
            })
            .map_err(|e| format!("Failed to query sessions: {e}"))?;

        for row in rows {
            let (id, title, model, provider, created_at, updated_at) = row
                .map_err(|e| format!("Failed to read session row: {e}"))?;

            // Insert into new conversation table
            if let Err(e) = daemon_conn.execute(
                "INSERT OR IGNORE INTO conversation (id, mode, title, provider_id, model_id, created_at, updated_at)
                 VALUES (?1, 'chat', ?2, ?3, ?4, ?5, ?6)",
                params![id, title, provider, model, created_at, updated_at],
            ) {
                result.errors.push(format!("Failed to migrate conversation {id}: {e}"));
            } else {
                result.conversations_migrated += 1;
            }
        }
    }

    // Migrate messages
    {
        let mut stmt = legacy_conn
            .prepare("SELECT id, session_id, role, content, token_count, created_at FROM assistant_messages")
            .map_err(|e| format!("Failed to prepare message query: {e}"))?;

        let rows = stmt
            .query_map([], |row| {
                let id: String = row.get(0)?;
                let session_id: String = row.get(1)?;
                let role: String = row.get(2)?;
                let content: String = row.get(3)?;
                let token_count: i64 = row.get(4)?;
                let created_at: String = row.get(5)?;
                Ok((id, session_id, role, content, token_count, created_at))
            })
            .map_err(|e| format!("Failed to query messages: {e}"))?;

        for row in rows {
            let (id, session_id, role, content, token_count, created_at) = row
                .map_err(|e| format!("Failed to read message row: {e}"))?;

            let status = if role == "assistant" { "complete" } else { "complete" };

            // Insert into new message table
            if let Err(e) = daemon_conn.execute(
                "INSERT OR IGNORE INTO message (id, conversation_id, role, status, input_tokens, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, session_id, role, status, token_count, created_at],
            ) {
                result.errors.push(format!("Failed to migrate message {id}: {e}"));
            } else {
                result.messages_migrated += 1;

                // Create a legacy content block for the message content
                let block_type = if role == "assistant" && content.contains("thinking") {
                    "legacy"
                } else {
                    "text"
                };

                let block_json = if role == "assistant" && content.contains("thinking") {
                    serde_json::json!({
                        "type": "legacy",
                        "raw": content,
                        "original_type": "thinking"
                    })
                } else {
                    serde_json::json!({
                        "type": "text",
                        "text": content
                    })
                };

                if let Err(e) = daemon_conn.execute(
                    "INSERT INTO message_block (message_id, sort_order, block_type, block_json)
                     VALUES (?1, 0, ?2, ?3)",
                    params![id, block_type, block_json.to_string()],
                ) {
                    result.errors.push(format!("Failed to migrate block for message {id}: {e}"));
                } else {
                    result.blocks_migrated += 1;
                }
            }
        }
    }

    // Commit or rollback
    if result.errors.is_empty() {
        daemon_conn
            .execute_batch("COMMIT")
            .map_err(|e| format!("Failed to commit migration: {e}"))?;
    } else {
        daemon_conn
            .execute_batch("ROLLBACK")
            .map_err(|e| format!("Failed to rollback migration: {e}"))?;
        return Err(format!(
            "Migration failed with {} errors. First error: {}",
            result.errors.len(),
            result.errors.first().unwrap()
        ));
    }

    Ok(result)
}

/// Create a backup of the legacy database before migration.
pub fn backup_legacy_db(legacy_db_path: &Path) -> Result<String, String> {
    let backup_path = format!(
        "{}.backup.{}",
        legacy_db_path.to_string_lossy(),
        chrono::Utc::now().format("%Y%m%d_%H%M%S")
    );
    std::fs::copy(legacy_db_path, &backup_path)
        .map_err(|e| format!("Failed to create backup: {e}"))?;
    Ok(backup_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn setup_test_stores() -> (DataStore, PathBuf) {
        let tmp = std::env::temp_dir();
        let daemon_db = tmp.join(format!("test_daemon_{}.db", uuid::Uuid::new_v4()));
        let art_dir = tmp.join(format!("test_art_{}", uuid::Uuid::new_v4()));
        let legacy_db = tmp.join(format!("test_legacy_{}.db", uuid::Uuid::new_v4()));

        let store = DataStore::new(&daemon_db, &art_dir).unwrap();

        // Create legacy database with old schema
        let legacy_conn = rusqlite::Connection::open(&legacy_db).unwrap();
        legacy_conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS assistant_sessions (
                id TEXT PRIMARY KEY, title TEXT, model TEXT, provider TEXT,
                created_at TEXT, updated_at TEXT
            );
            CREATE TABLE IF NOT EXISTS assistant_messages (
                id TEXT PRIMARY KEY, session_id TEXT, role TEXT, content TEXT,
                token_count INTEGER DEFAULT 0, created_at TEXT
            );"
        ).unwrap();
        legacy_conn.execute(
            "INSERT INTO assistant_sessions (id, title, model, provider, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params!["legacy-sess-1", "Legacy Chat", "gpt-4", "openai", "2026-01-01T00:00:00Z", "2026-01-01T00:00:00Z"],
        ).unwrap();

        (store, legacy_db)
    }

    #[test]
    fn test_backup_creation() {
        let tmp = std::env::temp_dir();
        let legacy_db = tmp.join(format!("test_backup_{}.db", uuid::Uuid::new_v4()));
        std::fs::write(&legacy_db, b"test content").unwrap();

        let backup_path = backup_legacy_db(&legacy_db).unwrap();
        assert!(std::path::Path::new(&backup_path).exists());

        let _ = std::fs::remove_file(&backup_path);
        let _ = std::fs::remove_file(&legacy_db);
    }

    #[test]
    fn test_migrate_legacy_conversations() {
        let (store, legacy_db) = setup_test_stores();
        let result = migrate_legacy_data(&store, &legacy_db).unwrap();
        assert_eq!(result.conversations_migrated, 1);
        assert_eq!(result.errors.len(), 0);

        let _ = std::fs::remove_file(&legacy_db);
    }

    #[test]
    fn test_migration_restart_safe() {
        let (store, legacy_db) = setup_test_stores();
        // First migration
        let result1 = migrate_legacy_data(&store, &legacy_db).unwrap();
        assert_eq!(result1.conversations_migrated, 1);

        // Second migration (restart-safe - uses INSERT OR IGNORE)
        let result2 = migrate_legacy_data(&store, &legacy_db).unwrap();
        assert_eq!(result2.conversations_migrated, 0, "No new conversations on restart");

        let _ = std::fs::remove_file(&legacy_db);
    }
}