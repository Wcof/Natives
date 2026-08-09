//! Host legacy migration service — startup-only, one-way migration reader for
//! the historical `assistant_*` tables that predate the Agent Daemon authority.
//!
//! Authority: the Agent Daemon owns `assistant.db` schema migrations and the
//! canonical `conversation` / `message` / `run` / `run_event` / `prompt_queue`
//! tables. The Host no longer maintains an active `assistant_*` runtime schema:
//! the conversation / run / message / event / queue tables are Daemon authority
//! and are deliberately NOT created or written by the Host at normal runtime
//! (MIG-004 / DATA-002).
//!
//! What this service does at startup only (from `lib.rs` setup, never from a
//! request handler):
//! - Creates/upgrades the Host-owned tables that share the `assistant.db` file:
//!   the provider mirror (`assistant_provider_configs` / `assistant_provider_keys`
//!   / `assistant_model_cache`), `assistant_projects`, and `settings`.
//!   `src-tauri/src/commands/provider.rs` mirrors Settings (natives.db) rows into
//!   these tables and `provider.list` reads the model cache.
//! - Runs the one-way legacy conversions: old session-based messages are
//!   converted to the historical `assistant_*` conversation format so the
//!   Daemon's `host_authority_migration` (`src-agent-daemon/src/storage/`) can
//!   merge them into the canonical tables. Both steps are idempotent and never
//!   DROP or rebuild tables (R-D3).
//!
//! Normal runtime never reaches this service: business request paths (assistant
//! RPC handlers) read the Daemon or natives.db only.

use crate::Result;
use rusqlite::Connection;
use std::sync::{Mutex, MutexGuard};

/// Startup-only, one-way legacy migration service for `assistant.db`.
pub struct LegacyMigrationService {
    conn: Mutex<Connection>,
    db_path: String,
}

impl LegacyMigrationService {
    /// Open `assistant.db` and prepare it for a one-way legacy migration.
    pub fn open(db_path: &str) -> Result<Self> {
        let conn = Connection::open(db_path)
            .map_err(|e| crate::Error::Internal(format!("Failed to open database: {e}")))?;

        // Enable WAL mode and foreign keys
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA foreign_keys=ON;
             PRAGMA busy_timeout=5000;",
        )
        .map_err(|e| crate::Error::Internal(format!("Failed to set pragmas: {e}")))?;

        Ok(LegacyMigrationService {
            conn: Mutex::new(conn),
            db_path: db_path.to_string(),
        })
    }

    /// Run the startup one-way migration. Idempotent; safe to call repeatedly.
    ///
    /// Step 1 applies the schema for Host-owned tables only. Step 2 runs the
    /// one-way legacy conversions. No Daemon-authority table
    /// (conversation/run/message/event/queue) is created or written here.
    pub fn run(&self) -> Result<()> {
        self.run_schema_migrations()?;
        self.migrate_legacy_provider_keys()?;
        self.migrate_legacy_assistant_messages()?;
        // v9 is the historical marker for the legacy session migration; record
        // it after the conversion like the pre-D2-01 flow did.
        {
            let conn = self.conn();
            conn.execute(
                "INSERT OR IGNORE INTO _schema_version (version) VALUES (9)",
                [],
            )
            .map_err(|e| crate::Error::Internal(format!("Failed to record migration v9: {e}")))?;
        }
        Ok(())
    }

    /// Apply the schema for Host-owned tables (provider mirror / projects /
    /// settings) within a transaction. The Daemon-authority `assistant_*`
    /// runtime schema is intentionally absent.
    fn run_schema_migrations(&self) -> Result<()> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|e| crate::Error::Internal(e.to_string()))?;
        let tx = conn
            .transaction()
            .map_err(|e| crate::Error::Internal(format!("Migration transaction failed: {e}")))?;

        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS _schema_version (
                version INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL DEFAULT (datetime('now'))
            );",
        )
        .map_err(|e| {
            crate::Error::Internal(format!("Schema version table creation failed: {e}"))
        })?;

        let current_version: i64 = tx
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM _schema_version",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        // Only Host-owned tables. The legacy conversation/run/message/event/
        // queue schema is Daemon authority — on fresh installs it is never
        // created by the Host (the Daemon's host_authority_migration records
        // "no host tables" and skips); on upgraded installs the historical
        // tables persist as a read-only migration source.
        let migrations: Vec<(i64, &str)> = vec![
            (4, MIGRATION_004_PROVIDERS),
            (5, MIGRATION_005),
            (6, MIGRATION_006),
            (7, MIGRATION_007),
            (8, MIGRATION_008),
            (12, MIGRATION_012),
            (14, MIGRATION_014),
        ];

        for (version, sql) in migrations {
            if version > current_version {
                tx.execute_batch(sql).map_err(|e| {
                    crate::Error::Internal(format!("Migration v{version} failed: {e}"))
                })?;
                tx.execute(
                    "INSERT INTO _schema_version (version) VALUES (?1)",
                    rusqlite::params![version],
                )
                .map_err(|e| {
                    crate::Error::Internal(format!("Failed to record migration v{version}: {e}"))
                })?;
            }
        }

        tx.commit()
            .map_err(|e| crate::Error::Internal(format!("Migration commit failed: {e}")))?;
        Ok(())
    }

    /// Migrate provider keys from old legacy tables (user_providers / provider_api_keys).
    /// Only runs if the legacy tables exist. Idempotent — INSERT OR IGNORE.
    fn migrate_legacy_provider_keys(&self) -> Result<()> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|e| crate::Error::Internal(e.to_string()))?;

        let is_memory = self.db_path == ":memory:";
        let mut attached = false;

        if !is_memory {
            let path = std::path::Path::new(&self.db_path);
            if let Some(parent) = path.parent() {
                let natives_db_path = parent.join("natives.db");
                if natives_db_path.exists() {
                    let attach_sql = format!(
                        "ATTACH DATABASE '{}' AS natives_db",
                        natives_db_path.to_string_lossy().replace('\'', "''")
                    );
                    conn.execute(&attach_sql, []).map_err(|e| {
                        crate::Error::Internal(format!("Failed to attach natives.db: {e}"))
                    })?;
                    attached = true;
                }
            }
        }

        // Check if legacy tables exist
        let has_legacy_providers: bool = if attached {
            conn.prepare("SELECT name FROM natives_db.sqlite_master WHERE type='table' AND name='user_providers'")
                .and_then(|mut stmt| stmt.exists([]))
                .unwrap_or(false)
        } else {
            conn.prepare(
                "SELECT name FROM sqlite_master WHERE type='table' AND name='user_providers'",
            )
            .and_then(|mut stmt| stmt.exists([]))
            .unwrap_or(false)
        };

        let has_legacy_keys: bool = if attached {
            conn.prepare("SELECT name FROM natives_db.sqlite_master WHERE type='table' AND name='provider_api_keys'")
                .and_then(|mut stmt| stmt.exists([]))
                .unwrap_or(false)
        } else {
            conn.prepare(
                "SELECT name FROM sqlite_master WHERE type='table' AND name='provider_api_keys'",
            )
            .and_then(|mut stmt| stmt.exists([]))
            .unwrap_or(false)
        };

        if !has_legacy_providers && !has_legacy_keys {
            if attached {
                conn.execute("DETACH DATABASE natives_db", []).ok();
            }
            return Ok(());
        }

        let tx = conn.transaction().map_err(|e| {
            crate::Error::Internal(format!("Legacy migration transaction failed: {e}"))
        })?;

        // Migrate providers from user_providers
        if has_legacy_providers {
            let provider_sql = if attached {
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
                     up.default_model,
                     'unknown',
                     up.created_at,
                     up.updated_at
                 FROM natives_db.user_providers up
                 WHERE up.id NOT IN (SELECT id FROM assistant_provider_configs)"
            } else {
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
                     up.default_model,
                     'unknown',
                     up.created_at,
                     up.updated_at
                 FROM user_providers up
                 WHERE up.id NOT IN (SELECT id FROM assistant_provider_configs)"
            };
            tx.execute_batch(provider_sql).map_err(|e| {
                crate::Error::Internal(format!("Legacy provider migration failed: {e}"))
            })?;
        }

        // Migrate keys from provider_api_keys
        if has_legacy_keys {
            let keys_sql = if attached {
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
                     pak.is_active,
                     pak.is_primary,
                     pak.test_status,
                     pak.created_at,
                     pak.updated_at
                 FROM natives_db.provider_api_keys pak
                 WHERE pak.id NOT IN (SELECT id FROM assistant_provider_keys)"
            } else {
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
                     pak.is_active,
                     pak.is_primary,
                     pak.test_status,
                     pak.created_at,
                     pak.updated_at
                 FROM provider_api_keys pak
                 WHERE pak.id NOT IN (SELECT id FROM assistant_provider_keys)"
            };
            tx.execute_batch(keys_sql)
                .map_err(|e| crate::Error::Internal(format!("Legacy key migration failed: {e}")))?;
        }

        // Auto-seed assistant_model_cache with default models from user_providers
        if has_legacy_providers {
            let model_sql = if attached {
                "INSERT OR IGNORE INTO assistant_model_cache
                 (id, provider_id, model_id, display_name, capabilities, context_window, max_output, source, discovered_at)
                 SELECT
                     up.id || ':' || up.default_model,
                     up.id,
                     up.default_model,
                     up.default_model,
                     '{}',
                     0,
                     0,
                     'api_discovery',
                     up.created_at
                 FROM natives_db.user_providers up
                 WHERE up.default_model IS NOT NULL AND up.default_model != ''"
            } else {
                "INSERT OR IGNORE INTO assistant_model_cache
                 (id, provider_id, model_id, display_name, capabilities, context_window, max_output, source, discovered_at)
                 SELECT
                     up.id || ':' || up.default_model,
                     up.id,
                     up.default_model,
                     up.default_model,
                     '{}',
                     0,
                     0,
                     'api_discovery',
                     up.created_at
                 FROM user_providers up
                 WHERE up.default_model IS NOT NULL AND up.default_model != ''"
            };
            tx.execute_batch(model_sql).map_err(|e| {
                crate::Error::Internal(format!("Default model cache seeding failed: {e}"))
            })?;
        }

        tx.commit()
            .map_err(|e| crate::Error::Internal(format!("Legacy migration commit failed: {e}")))?;

        if attached {
            let _ = conn.execute("DETACH DATABASE natives_db", []);
        }

        Ok(())
    }

    /// Detect and migrate legacy assistant_messages table.
    /// If the old table has a `session_id` column (old schema), rename it
    /// to `legacy_assistant_messages` so the new schema tables can be used.
    /// Also migrates data from old `assistant_sessions` if it exists.
    /// This is idempotent: skips if already renamed or never existed.
    fn migrate_legacy_assistant_messages(&self) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| crate::Error::Internal(e.to_string()))?;

        let is_memory = self.db_path == ":memory:";
        let mut attached = false;

        if !is_memory {
            let path = std::path::Path::new(&self.db_path);
            if let Some(parent) = path.parent() {
                let natives_db_path = parent.join("natives.db");
                if natives_db_path.exists() {
                    let attach_sql = format!(
                        "ATTACH DATABASE '{}' AS natives_db",
                        natives_db_path.to_string_lossy().replace('\'', "''")
                    );
                    conn.execute(&attach_sql, []).map_err(|e| {
                        crate::Error::Internal(format!(
                            "Failed to attach natives.db for messages: {e}"
                        ))
                    })?;
                    attached = true;
                }
            }
        }

        // Check if legacy assistant_sessions exists in the right DB
        let has_sessions: bool = if attached {
            conn.prepare("SELECT name FROM natives_db.sqlite_master WHERE type='table' AND name='assistant_sessions'")
                .and_then(|mut stmt| stmt.exists([]))
                .unwrap_or(false)
        } else {
            conn.prepare(
                "SELECT name FROM sqlite_master WHERE type='table' AND name='assistant_sessions'",
            )
            .and_then(|mut stmt| stmt.exists([]))
            .unwrap_or(false)
        };

        if has_sessions {
            // The historical `assistant_conversations` table is the migration
            // source the Daemon's host_authority_migration reads. On upgraded
            // installs it already exists (the pre-D2-01 Host created it); when a
            // very old DB has only the session tables, create the conversation
            // shape so the conversion below has a target that is idempotent and
            // never DROPs anything (R-D3). Fresh installs skip this block.
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS assistant_conversations (
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
                );",
            )
            .map_err(|e| {
                crate::Error::Internal(format!(
                    "Failed to prepare assistant_conversations for legacy migration: {e}"
                ))
            })?;

            let insert_convs_sql = if attached {
                "INSERT OR IGNORE INTO assistant_conversations
                    (id, project_id, title, provider_id, model_id, created_at, updated_at)
                 SELECT
                     id, project_id,
                     COALESCE(title, ''),
                     COALESCE(provider_id, 'unknown'),
                     COALESCE(model_id, 'unknown'),
                     created_at, updated_at
                 FROM natives_db.assistant_sessions"
            } else {
                "INSERT OR IGNORE INTO assistant_conversations
                    (id, project_id, title, provider_id, model_id, created_at, updated_at)
                 SELECT
                     id, project_id,
                     COALESCE(title, ''),
                     COALESCE(provider_id, 'unknown'),
                     COALESCE(model_id, 'unknown'),
                     created_at, updated_at
                 FROM assistant_sessions"
            };
            conn.execute_batch(insert_convs_sql).map_err(|e| {
                crate::Error::Internal(format!("Legacy sessions migration failed: {e}"))
            })?;
        }

        // Check if assistant_messages has session_id column (old structure) in the right DB
        let has_legacy_messages_table: bool = if attached {
            conn.prepare("SELECT name FROM natives_db.sqlite_master WHERE type='table' AND name='assistant_messages'")
                .and_then(|mut stmt| stmt.exists([]))
                .unwrap_or(false)
        } else {
            conn.prepare(
                "SELECT name FROM sqlite_master WHERE type='table' AND name='assistant_messages'",
            )
            .and_then(|mut stmt| stmt.exists([]))
            .unwrap_or(false)
        };

        let has_session_id: bool = if has_legacy_messages_table {
            let pragma_sql = if attached {
                "PRAGMA natives_db.table_info(assistant_messages)"
            } else {
                "PRAGMA table_info(assistant_messages)"
            };
            conn.prepare(pragma_sql)
                .and_then(|mut stmt| {
                    let cols: Vec<String> = stmt
                        .query_map([], |row| row.get::<_, String>(1))
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?
                        .filter_map(|r| r.ok())
                        .collect();
                    Ok(cols.contains(&"session_id".to_string()))
                })
                .unwrap_or(false)
        } else {
            false
        };

        if !has_session_id {
            if attached {
                let _ = conn.execute("DETACH DATABASE natives_db", []);
            }
            return Ok(());
        }

        // This one-way legacy conversion never drops anything. The session-based
        // rows are preserved under `legacy_assistant_messages` (created only when
        // the name is free, never overwritten, backfilled only when empty), and
        // the session → conversation rename only happens when the target name is
        // free. A partial prior run that left an inconsistent state fails closed
        // instead of dropping data to make room.
        let legacy_exists: bool = conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE type='table' AND name='legacy_assistant_messages'",
            )
            .and_then(|mut stmt| stmt.exists([]))
            .unwrap_or(false);

        if attached {
            if legacy_exists {
                // Preserved copy already present (idempotent re-entry): backfill
                // only when it is empty so we never duplicate rows.
                let copied: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM legacy_assistant_messages",
                        [],
                        |row| row.get(0),
                    )
                    .unwrap_or(0);
                if copied == 0 {
                    conn.execute_batch(
                        "INSERT INTO legacy_assistant_messages
                         SELECT id, session_id, parent_message_id, role, content, status, created_at
                         FROM natives_db.assistant_messages;",
                    )
                    .map_err(|e| {
                        crate::Error::Internal(format!(
                            "Failed to backfill legacy assistant_messages: {e}"
                        ))
                    })?;
                }
            } else {
                conn.execute_batch(
                    "CREATE TABLE legacy_assistant_messages (
                        id TEXT PRIMARY KEY,
                        session_id TEXT,
                        parent_message_id TEXT,
                        role TEXT,
                        content TEXT,
                        status TEXT,
                        created_at TEXT
                    );
                    INSERT INTO legacy_assistant_messages
                    SELECT id, session_id, parent_message_id, role, content, status, created_at
                    FROM natives_db.assistant_messages;",
                )
                .map_err(|e| {
                    crate::Error::Internal(format!("Failed to copy legacy assistant_messages: {e}"))
                })?;
            }
        } else if legacy_exists {
            // The local session-based table still occupies `assistant_messages`
            // while a preserved copy already exists under
            // `legacy_assistant_messages`. Producing the new-schema table here
            // would require dropping one of them — refuse instead.
            return Err(crate::Error::Internal(
                "Legacy assistant_messages conversion is inconsistent: both \
                 `assistant_messages` (session-based) and \
                 `legacy_assistant_messages` exist; refusing to drop either"
                    .into(),
            ));
        } else {
            conn.execute_batch(
                "ALTER TABLE assistant_messages RENAME TO legacy_assistant_messages;",
            )
            .map_err(|e| crate::Error::Internal(format!("Legacy messages rename failed: {e}")))?;
        }

        // Recreate the new schema version of assistant_messages.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS assistant_messages (
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
            CREATE INDEX IF NOT EXISTS idx_messages_parent ON assistant_messages(parent_message_id);"
        ).map_err(|e| crate::Error::Internal(format!("Failed to recreate assistant_messages: {e}")))?;

        // If the local table is still session-based here (attached path where a
        // pre-existing local session table occupies the name), the CREATE above
        // was a no-op and any further write would target the wrong schema.
        let still_session: bool = conn
            .prepare("PRAGMA table_info(assistant_messages)")
            .and_then(|mut stmt| {
                let cols: Vec<String> = stmt
                    .query_map([], |row| row.get::<_, String>(1))
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?
                    .filter_map(|r| r.ok())
                    .collect();
                Ok(cols.contains(&"session_id".to_string()))
            })
            .unwrap_or(false);
        if still_session {
            return Err(crate::Error::Internal(
                "Legacy assistant_messages conversion cannot proceed: a local \
                 session-based `assistant_messages` table occupies the name and \
                 converting it would require dropping data (R-D3)"
                    .into(),
            ));
        }

        // Migrate legacy assistant messages to new schema. The session schema
        // defaulted `status` to '' which the conversation-schema CHECK rejects;
        // normalize invalid statuses to 'complete' instead of letting
        // INSERT OR IGNORE silently drop rows (data preservation).
        conn.execute_batch(
            "INSERT OR IGNORE INTO assistant_messages
                (id, conversation_id, role, status, created_at)
             SELECT
                 m.id, m.session_id,
                 COALESCE(m.role, 'user'),
                 CASE WHEN m.status IN ('sending','streaming','complete','failed','interrupted')
                      THEN m.status ELSE 'complete' END,
                 m.created_at
             FROM legacy_assistant_messages m
             JOIN assistant_conversations c ON c.id = m.session_id;",
        )
        .map_err(|e| {
            crate::Error::Internal(format!(
                "Failed to migrate legacy messages to assistant_messages: {e}"
            ))
        })?;

        // Migrate text content from legacy messages into new message blocks
        let _ = conn.execute_batch(
            "INSERT OR IGNORE INTO assistant_message_blocks
                (id, message_id, block_type, block_index, content, metadata)
             SELECT
                 hex(randomblob(16)),
                 m.id,
                 'text',
                 0,
                 m.content,
                 json_object('legacy', 1, 'role', m.role)
             FROM legacy_assistant_messages m
             JOIN assistant_messages am ON am.id = m.id
             WHERE m.content IS NOT NULL AND m.content != '';",
        );

        if attached {
            let _ = conn.execute("DETACH DATABASE natives_db", []);
        }

        Ok(())
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn
            .lock()
            .expect("LegacyMigrationService connection lock poisoned")
    }
}

// Migration SQL definitions — Host-owned tables only.

/// v4: Provider configs and keys (Host-owned mirror; the old `assistant_artifacts`
/// and `assistant_context_snapshots` tables were Daemon authority and are no
/// longer created by the Host).
const MIGRATION_004_PROVIDERS: &str = "
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

/// v5: Model cache (Host-owned mirror; the old `assistant_extensions` /
/// `assistant_extension_permissions` tables are unused and no longer created).
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

CREATE INDEX IF NOT EXISTS idx_model_cache_provider ON assistant_model_cache(provider_id, model_id);
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

/// v8: Legacy assistant session migration.
/// Creates assistant_projects table and migrates old data.
/// Table-level operations only — data migration is handled in Rust code
/// to safely check for source table existence.
const MIGRATION_008: &str = "
CREATE TABLE IF NOT EXISTS assistant_projects (
    id TEXT PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    label TEXT NOT NULL,
    created_at TEXT NOT NULL,
    last_opened_at TEXT NOT NULL
);
";

/// v12: Collapse duplicate model cache rows and enforce uniqueness per provider.
const MIGRATION_012: &str = "
DELETE FROM assistant_model_cache
WHERE rowid NOT IN (
    SELECT MIN(rowid)
    FROM assistant_model_cache
    GROUP BY provider_id, model_id
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_model_cache_provider_model
    ON assistant_model_cache(provider_id, model_id);
";

/// v14: Soft-delete support for assistant_projects (logical delete, keep sessions).
const MIGRATION_014: &str = "
ALTER TABLE assistant_projects ADD COLUMN deleted_at TEXT;
";

/// W1 (modular remediation): ensure the Host-owned provider mirror schema on
/// any connection. The mirror tables are Host authority and live in the Host's
/// own natives.db (via `db::ensure_host_owned_tables`); they are never created
/// inside the Daemon's assistant.db.
pub fn ensure_provider_mirror_schema(conn: &rusqlite::Connection) -> crate::Result<()> {
    conn.execute_batch(MIGRATION_004_PROVIDERS)
        .map_err(crate::Error::Database)?;
    conn.execute_batch(MIGRATION_005)
        .map_err(crate::Error::Database)?;
    conn.execute_batch(MIGRATION_007)
        .map_err(crate::Error::Database)?;
    conn.execute_batch(MIGRATION_008)
        .map_err(crate::Error::Database)?;
    conn.execute_batch(MIGRATION_012)
        .map_err(crate::Error::Database)?;
    conn.execute_batch(MIGRATION_014)
        .map_err(crate::Error::Database)?;
    Ok(())
}

#[cfg(test)]
#[path = "data_tests.rs"]
mod data_tests;
