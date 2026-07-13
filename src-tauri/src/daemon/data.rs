//! Data store — SQLite database access, migrations, and artifact storage.

use crate::Result;
use rusqlite::Connection;
use std::sync::Mutex;

/// Main data store for the daemon.
pub struct DataStore {
    conn: Mutex<Connection>,
    db_path: String,
}

impl DataStore {
    /// Open or create a database at the given path.
    pub fn new(db_path: &str) -> Result<Self> {
        let conn = Connection::open(db_path)
            .map_err(|e| crate::Error::Internal(format!("Failed to open database: {e}")))?;

        // Enable WAL mode and foreign keys
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA foreign_keys=ON;
             PRAGMA busy_timeout=5000;"
        ).map_err(|e| crate::Error::Internal(format!("Failed to set pragmas: {e}")))?;

        Ok(DataStore {
            conn: Mutex::new(conn),
            db_path: db_path.to_string(),
        })
    }

    /// Run all pending migrations.
    pub fn run_migrations(&self) -> Result<()> {
        // Step 1: Run schema migrations within a transaction
        {
            let mut conn = self.conn.lock().map_err(|e| crate::Error::Internal(e.to_string()))?;
            let tx = conn.transaction()
                .map_err(|e| crate::Error::Internal(format!("Migration transaction failed: {e}")))?;

            // Ensure schema version table exists
            tx.execute_batch(
                "CREATE TABLE IF NOT EXISTS _schema_version (
                    version INTEGER PRIMARY KEY,
                    applied_at TEXT NOT NULL DEFAULT (datetime('now'))
                );"
            ).map_err(|e| crate::Error::Internal(format!("Schema version table creation failed: {e}")))?;

            let current_version: i64 = tx
                .query_row("SELECT COALESCE(MAX(version), 0) FROM _schema_version", [], |row| row.get(0))
                .unwrap_or(0);

            // Apply migrations sequentially
            let migrations: Vec<(i64, &str)> = vec![
                (1, MIGRATION_001),
                (2, MIGRATION_002),
                (3, MIGRATION_003),
                (4, MIGRATION_004),
                (5, MIGRATION_005),
                (6, MIGRATION_006),
                (7, MIGRATION_007),
            ];

            for (version, sql) in migrations {
                if version > current_version {
                    tx.execute_batch(sql)
                        .map_err(|e| crate::Error::Internal(format!("Migration v{version} failed: {e}")))?;
                    tx.execute(
                        "INSERT INTO _schema_version (version) VALUES (?1)",
                        rusqlite::params![version],
                    ).map_err(|e| crate::Error::Internal(format!("Failed to record migration v{version}: {e}")))?;
                }
            }

            tx.commit()
                .map_err(|e| crate::Error::Internal(format!("Migration commit failed: {e}")))?;
        } // conn MutexGuard dropped here before legacy migration

        // Step 2: Run legacy data migration in a fresh transaction
        self.migrate_legacy_provider_keys()?;

        Ok(())
    }

    /// Migrate provider keys from old legacy tables (user_providers / provider_api_keys).
    /// Only runs if the legacy tables exist. Idempotent — INSERT OR IGNORE.
    fn migrate_legacy_provider_keys(&self) -> Result<()> {
        let mut conn = self.conn.lock().map_err(|e| crate::Error::Internal(e.to_string()))?;

        // Check if legacy tables exist
        let has_legacy_providers: bool = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='user_providers'")
            .and_then(|mut stmt| stmt.exists([]))
            .unwrap_or(false);

        let has_legacy_keys: bool = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='provider_api_keys'")
            .and_then(|mut stmt| stmt.exists([]))
            .unwrap_or(false);

        if !has_legacy_providers && !has_legacy_keys {
            return Ok(());
        }

        let tx = conn.transaction()
            .map_err(|e| crate::Error::Internal(format!("Legacy migration transaction failed: {e}")))?;

        // Migrate providers from user_providers
        if has_legacy_providers {
            tx.execute_batch(
                "INSERT OR IGNORE INTO assistant_provider_configs
                 (id, provider_type, display_name, api_base_url, website_url, default_model, health_status, created_at, updated_at)
                 SELECT
                     up.id,
                     CASE WHEN up.preset_name = 'openai' THEN 'openai'
                          WHEN up.preset_name = 'anthropic' THEN 'anthropic'
                          WHEN up.preset_name = 'gemini' THEN 'gemini'
                          WHEN up.preset_name = 'deepseek' THEN 'deepseek'
                          WHEN up.preset_name = 'ollama' THEN 'ollama'
                          ELSE 'openai_compatible' END,
                     up.name,
                     up.base_url,
                     up.website_url,
                     NULL,
                     'unknown',
                     up.created_at,
                     up.updated_at
                 FROM user_providers up
                 WHERE up.id NOT IN (SELECT id FROM assistant_provider_configs)"
            ).map_err(|e| crate::Error::Internal(format!("Legacy provider migration failed: {e}")))?;
        }

        // Migrate keys from provider_api_keys
        if has_legacy_keys {
            tx.execute_batch(
                "INSERT OR IGNORE INTO assistant_provider_keys
                 (id, provider_id, encrypted_key, masked_key, label, is_active, is_primary, test_status, created_at, updated_at)
                 SELECT
                     pak.id,
                     pak.provider_id,
                     pak.api_key_encrypted,
                     CASE WHEN LENGTH(pak.api_key_encrypted) > 8
                          THEN SUBSTR(pak.api_key_encrypted, 1, 4) || '...' || SUBSTR(pak.api_key_encrypted, -4)
                          ELSE '***' END,
                     pak.label,
                     1,
                     0,
                     'untested',
                     pak.created_at,
                     pak.created_at
                 FROM provider_api_keys pak
                 WHERE pak.id NOT IN (SELECT id FROM assistant_provider_keys)"
            ).map_err(|e| crate::Error::Internal(format!("Legacy key migration failed: {e}")))?;
        }

        tx.commit()
            .map_err(|e| crate::Error::Internal(format!("Legacy migration commit failed: {e}")))?;

        Ok(())
    }

    /// Get a reference to the underlying connection.
    pub fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().expect("DataStore connection lock poisoned")
    }

    /// Get the database file path.
    pub fn db_path(&self) -> &str {
        &self.db_path
    }
}

// Migration SQL definitions

/// v1: Core assistant tables
const MIGRATION_001: &str = "
CREATE TABLE IF NOT EXISTS assistant_conversations (
    id TEXT PRIMARY KEY,
    mode TEXT NOT NULL DEFAULT 'chat' CHECK(mode IN ('chat','agent')),
    project_id TEXT,
    title TEXT NOT NULL DEFAULT '',
    provider_id TEXT NOT NULL,
    model_id TEXT NOT NULL,
    permission_profile_id TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    archived_at TEXT
);

CREATE TABLE IF NOT EXISTS assistant_messages (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL REFERENCES assistant_conversations(id) ON DELETE CASCADE,
    parent_message_id TEXT,
    role TEXT NOT NULL CHECK(role IN ('system','user','assistant')),
    status TEXT NOT NULL DEFAULT 'complete' CHECK(status IN ('sending','streaming','complete','failed','interrupted')),
    input_tokens INTEGER DEFAULT 0,
    output_tokens INTEGER DEFAULT 0,
    reasoning_tokens INTEGER,
    cost_usd REAL,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_messages_conversation ON assistant_messages(conversation_id, created_at);
CREATE INDEX IF NOT EXISTS idx_messages_parent ON assistant_messages(parent_message_id);
";

/// v2: Message content blocks and runs
const MIGRATION_002: &str = "
CREATE TABLE IF NOT EXISTS assistant_message_blocks (
    id TEXT PRIMARY KEY,
    message_id TEXT NOT NULL REFERENCES assistant_messages(id) ON DELETE CASCADE,
    block_type TEXT NOT NULL,
    block_index INTEGER NOT NULL DEFAULT 0,
    content TEXT NOT NULL,
    metadata TEXT
);

CREATE TABLE IF NOT EXISTS assistant_runs (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL REFERENCES assistant_conversations(id) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'queued' CHECK(status IN ('queued','preparing','running','waiting_permission','cancelling','completed','failed','interrupted')),
    trigger_message_id TEXT,
    provider_id TEXT NOT NULL,
    model_id TEXT NOT NULL,
    runtime_id TEXT,
    permission_profile TEXT,
    max_steps INTEGER,
    max_duration_secs INTEGER,
    token_budget INTEGER,
    started_at TEXT,
    finished_at TEXT,
    error_code TEXT,
    step_count INTEGER DEFAULT 0,
    total_input_tokens INTEGER DEFAULT 0,
    total_output_tokens INTEGER DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_runs_conversation ON assistant_runs(conversation_id);
";

/// v3: Run events, tool calls, and permissions
const MIGRATION_003: &str = "
CREATE TABLE IF NOT EXISTS assistant_run_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id TEXT NOT NULL REFERENCES assistant_runs(id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL,
    timestamp TEXT NOT NULL,
    event_type TEXT NOT NULL,
    payload TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_run_events_sequence ON assistant_run_events(run_id, sequence);

CREATE TABLE IF NOT EXISTS assistant_tool_calls (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES assistant_runs(id) ON DELETE CASCADE,
    conversation_id TEXT NOT NULL,
    tool_name TEXT NOT NULL,
    tool_call_id TEXT NOT NULL,
    input TEXT,
    output TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    is_error INTEGER DEFAULT 0,
    duration_ms INTEGER,
    correlation_id TEXT,
    created_at TEXT NOT NULL,
    finished_at TEXT
);

CREATE TABLE IF NOT EXISTS assistant_permission_requests (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES assistant_runs(id) ON DELETE CASCADE,
    tool_call_id TEXT NOT NULL,
    tool_name TEXT NOT NULL,
    reason TEXT NOT NULL,
    input TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending','approved','rejected','expired')),
    scope TEXT,
    created_at TEXT NOT NULL,
    responded_at TEXT
);
";

/// v4: Artifacts, context snapshots, provider configs
const MIGRATION_004: &str = "
CREATE TABLE IF NOT EXISTS assistant_artifacts (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES assistant_runs(id) ON DELETE CASCADE,
    conversation_id TEXT NOT NULL,
    source_tool TEXT NOT NULL,
    path TEXT NOT NULL,
    sha256 TEXT NOT NULL,
    size INTEGER NOT NULL,
    mime_type TEXT NOT NULL,
    label TEXT,
    kind TEXT NOT NULL DEFAULT 'file',
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS assistant_context_snapshots (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES assistant_runs(id) ON DELETE CASCADE,
    before_tokens INTEGER NOT NULL,
    after_tokens INTEGER NOT NULL,
    summary TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS assistant_provider_configs (
    id TEXT PRIMARY KEY,
    provider_type TEXT NOT NULL,
    display_name TEXT NOT NULL,
    api_base_url TEXT NOT NULL,
    organization_id TEXT,
    project_id TEXT,
    proxy_url TEXT,
    timeout_secs INTEGER,
    default_model TEXT,
    health_status TEXT NOT NULL DEFAULT 'unknown',
    last_test_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS assistant_provider_keys (
    id TEXT PRIMARY KEY,
    provider_id TEXT NOT NULL REFERENCES assistant_provider_configs(id) ON DELETE CASCADE,
    encrypted_key TEXT NOT NULL,
    masked_key TEXT NOT NULL,
    label TEXT,
    is_active INTEGER NOT NULL DEFAULT 1,
    last_test_at TEXT,
    last_test_ok INTEGER,
    created_at TEXT NOT NULL
);
";

/// v5: Model cache, extensions, extension permissions
const MIGRATION_005: &str = "
CREATE TABLE IF NOT EXISTS assistant_model_cache (
    id TEXT PRIMARY KEY,
    provider_id TEXT NOT NULL,
    model_id TEXT NOT NULL,
    display_name TEXT,
    capabilities TEXT NOT NULL,
    context_window INTEGER NOT NULL,
    max_output INTEGER NOT NULL,
    source TEXT NOT NULL DEFAULT 'api_discovery',
    discovered_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS assistant_extensions (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    kind TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    description TEXT,
    manifest TEXT,
    health TEXT NOT NULL DEFAULT 'healthy',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS assistant_extension_permissions (
    id TEXT PRIMARY KEY,
    extension_id TEXT NOT NULL REFERENCES assistant_extensions(id) ON DELETE CASCADE,
    permission TEXT NOT NULL,
    granted INTEGER NOT NULL DEFAULT 0,
    granted_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_model_cache_provider ON assistant_model_cache(provider_id, model_id);
CREATE INDEX IF NOT EXISTS idx_extension_permissions_ext ON assistant_extension_permissions(extension_id);
";

/// v6: encrypted application settings used by env_manager and provider keys.
const MIGRATION_006: &str = "
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
";

/// v7: Unified provider key table — single source of truth for all provider/key data.
///
/// Schema only: adds columns, indexes. Data migration from legacy tables
/// is handled by `migrate_legacy_provider_keys()` called after all migrations.
const MIGRATION_007: &str = "
-- 1. Add website_url to configs if missing
ALTER TABLE assistant_provider_configs ADD COLUMN website_url TEXT NOT NULL DEFAULT '';

-- 2. Add new columns to keys table
ALTER TABLE assistant_provider_keys ADD COLUMN is_primary INTEGER NOT NULL DEFAULT 0;
ALTER TABLE assistant_provider_keys ADD COLUMN test_status TEXT NOT NULL DEFAULT 'untested'
    CHECK(test_status IN ('untested','valid','invalid','rate_limited','unavailable'));
ALTER TABLE assistant_provider_keys ADD COLUMN last_error_code TEXT;
ALTER TABLE assistant_provider_keys ADD COLUMN last_error_message TEXT;
ALTER TABLE assistant_provider_keys ADD COLUMN updated_at TEXT;

-- 3. Partial unique index: at most one primary key per provider
CREATE UNIQUE INDEX IF NOT EXISTS idx_provider_keys_unique_primary
    ON assistant_provider_keys(provider_id) WHERE is_primary = 1;

-- 4. Convert old last_test_ok to test_status
UPDATE assistant_provider_keys
SET test_status = CASE
    WHEN last_test_ok = 1 THEN 'valid'
    WHEN last_test_ok = 0 THEN 'invalid'
    ELSE 'untested'
    END
WHERE test_status = 'untested' AND last_test_ok IS NOT NULL;

-- 5. Set primary key: for each provider, pick earliest active key, or earliest key
UPDATE assistant_provider_keys
SET is_primary = 1
WHERE id IN (
    SELECT k.id FROM assistant_provider_keys k
    WHERE k.is_primary = 0
    AND k.id = (
        SELECT k2.id FROM assistant_provider_keys k2
        WHERE k2.provider_id = k.provider_id
        ORDER BY k2.is_active DESC, k2.created_at ASC
        LIMIT 1
    )
);
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_store_creation() {
        let store = DataStore::new(":memory:").unwrap();
        assert!(store.conn().is_autocommit());
    }

    #[test]
    fn test_migrations_run_successfully() {
        let store = DataStore::new(":memory:").unwrap();
        store.run_migrations().unwrap();

        // Verify tables exist
        let tables: Vec<String> = store
            .conn()
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(tables.contains(&"assistant_conversations".to_string()));
        assert!(tables.contains(&"assistant_messages".to_string()));
        assert!(tables.contains(&"assistant_message_blocks".to_string()));
        assert!(tables.contains(&"assistant_runs".to_string()));
        assert!(tables.contains(&"assistant_run_events".to_string()));
        assert!(tables.contains(&"assistant_tool_calls".to_string()));
        assert!(tables.contains(&"assistant_permission_requests".to_string()));
        assert!(tables.contains(&"assistant_artifacts".to_string()));
        assert!(tables.contains(&"assistant_context_snapshots".to_string()));
        assert!(tables.contains(&"assistant_provider_configs".to_string()));
        assert!(tables.contains(&"assistant_provider_keys".to_string()));
        assert!(tables.contains(&"assistant_model_cache".to_string()));
        assert!(tables.contains(&"assistant_extensions".to_string()));
        assert!(tables.contains(&"assistant_extension_permissions".to_string()));
    }

    #[test]
    fn test_migration_idempotency() {
        let store = DataStore::new(":memory:").unwrap();
        // Running migrations twice should be safe
        store.run_migrations().unwrap();
        store.run_migrations().unwrap();

        let version: i64 = store
            .conn()
            .query_row("SELECT COALESCE(MAX(version), 0) FROM _schema_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 7);
    }

    #[test]
    fn test_foreign_keys_enforced() {
        let store = DataStore::new(":memory:").unwrap();
        store.run_migrations().unwrap();

        // Try to insert a message with a non-existent conversation_id
        let result = store.conn().execute(
            "INSERT INTO assistant_messages (id, conversation_id, role, created_at) VALUES ('msg1', 'nonexistent', 'user', '2024-01-01T00:00:00Z')",
            [],
        );
        assert!(result.is_err());
    }
}
