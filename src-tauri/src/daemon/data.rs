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
             PRAGMA busy_timeout=5000;",
        )
        .map_err(|e| crate::Error::Internal(format!("Failed to set pragmas: {e}")))?;

        let store = DataStore {
            conn: Mutex::new(conn),
            db_path: db_path.to_string(),
        };

        // Run schema migrations automatically
        store.run_migrations()?;

        Ok(store)
    }

    /// Run all pending migrations.
    pub fn run_migrations(&self) -> Result<()> {
        // Step 0: Pre-migration — ensure assistant_messages has all V1 columns
        // if the table already exists from old db.rs init_assistant_db
        {
            let conn = self.conn();
            let has_table: bool = conn
                .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='assistant_messages'")
                .and_then(|mut stmt| stmt.exists([]))
                .unwrap_or(false);
            if has_table {
                let existing_cols: Vec<String> = conn
                    .prepare("PRAGMA table_info(assistant_messages)")
                    .and_then(|mut stmt| {
                        let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
                        let cols: Vec<String> = rows.filter_map(|r| r.ok()).collect();
                        Ok(cols)
                    })
                    .unwrap_or_default();
                let v1_cols: Vec<(&str, &str)> = vec![
                    (
                        "conversation_id",
                        "TEXT REFERENCES assistant_conversations(id) ON DELETE CASCADE",
                    ),
                    ("parent_message_id", "TEXT"),
                    ("role", "TEXT NOT NULL DEFAULT 'user'"),
                    ("status", "TEXT NOT NULL DEFAULT 'complete'"),
                    ("input_tokens", "INTEGER DEFAULT 0"),
                    ("output_tokens", "INTEGER DEFAULT 0"),
                    ("reasoning_tokens", "INTEGER"),
                    ("cost_usd", "REAL"),
                ];
                for (col, def) in v1_cols {
                    if !existing_cols.contains(&col.to_string()) {
                        let sql =
                            format!("ALTER TABLE assistant_messages ADD COLUMN {} {}", col, def);
                        let _ = conn.execute_batch(&sql);
                    }
                }
            }

            let has_runs: bool = conn
                .prepare(
                    "SELECT name FROM sqlite_master WHERE type='table' AND name='assistant_runs'",
                )
                .and_then(|mut stmt| stmt.exists([]))
                .unwrap_or(false);
            if has_runs {
                for (column, definition) in [
                    (
                        "parent_run_id",
                        "TEXT REFERENCES assistant_runs(id) ON DELETE CASCADE",
                    ),
                    ("subagent_definition_id", "TEXT"),
                    ("effort", "TEXT"),
                ] {
                    let exists = conn
                        .prepare("PRAGMA table_info(assistant_runs)")
                        .and_then(|mut stmt| {
                            let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
                            Ok(rows.filter_map(|row| row.ok()).any(|name| name == column))
                        })
                        .unwrap_or(false);
                    if !exists {
                        let _ = conn.execute(
                            &format!("ALTER TABLE assistant_runs ADD COLUMN {column} {definition}"),
                            [],
                        );
                    }
                }
                let _ = conn.execute(
                    "CREATE INDEX IF NOT EXISTS idx_runs_parent ON assistant_runs(parent_run_id)",
                    [],
                );
            }
        }

        // Step 1: Run schema migrations within a transaction
        {
            let mut conn = self
                .conn
                .lock()
                .map_err(|e| crate::Error::Internal(e.to_string()))?;
            let tx = conn.transaction().map_err(|e| {
                crate::Error::Internal(format!("Migration transaction failed: {e}"))
            })?;

            // Ensure schema version table exists
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

            // Apply migrations sequentially. P0-011: v10 (the explicit
            // user-cancelled terminal state) was defined but missing from this
            // list, so old v8/v9 DBs permanently skipped it. v10 is a table
            // rebuild and runs OUTSIDE the list via `run_v10_rebuild_if_needed`
            // (postcondition-driven, see below). v13 is likewise a rebuild that
            // must run outside the transaction (P0-025): PRAGMA foreign_keys=OFF
            // is a no-op inside a transaction, and the old in-tx rebuild
            // silently cascade-deleted messages/runs.
            let migrations: Vec<(i64, &str)> = vec![
                (1, MIGRATION_001),
                (2, MIGRATION_002),
                (3, MIGRATION_003),
                (4, MIGRATION_004),
                (5, MIGRATION_005),
                (6, MIGRATION_006),
                (7, MIGRATION_007),
                (8, MIGRATION_008),
                (11, MIGRATION_011),
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
                        crate::Error::Internal(format!(
                            "Failed to record migration v{version}: {e}"
                        ))
                    })?;
                }
            }

            tx.commit()
                .map_err(|e| crate::Error::Internal(format!("Migration commit failed: {e}")))?;
        } // conn MutexGuard dropped here before legacy migration

        // P0-011: v10 rebuild (add `cancelled` terminal state) — only for old
        // DBs whose `assistant_runs` CHECK still lacks `cancelled`. Fresh DBs
        // (MIGRATION_001 already includes it) skip the rebuild.
        self.run_v10_rebuild_if_needed()?;

        // P0-025: v13 rebuild must run OUTSIDE any transaction with FK off
        // (SQLite ignores PRAGMA foreign_keys inside transactions). The old
        // in-tx rebuild silently cascade-deleted messages/runs on
        // `DROP TABLE assistant_conversations` because the FK cascade was
        // still active. Rebuild protocol: FK off → create-copy-swap → FK on →
        // foreign_key_check.
        self.run_v13_rebuild()?;

        // Step 2: Run legacy data migration in a fresh transaction
        self.migrate_legacy_provider_keys()?;
        self.migrate_legacy_assistant_messages()?;
        self.repair_message_blocks_foreign_key()?;
        self.recover_stale_runs()?;
        {
            let conn = self.conn();
            conn.execute(
                "INSERT OR IGNORE INTO _schema_version (version) VALUES (9)",
                [],
            )
            .map_err(|e| crate::Error::Internal(format!("Failed to record migration v9: {e}")))?;
        }
        self.cleanup_orphaned_rows()?;

        Ok(())
    }

    /// P0-011: run the v10 rebuild (add explicit `cancelled` terminal state)
    /// only when the database actually needs it.
    ///
    /// Fresh DBs get `cancelled` from MIGRATION_001's CHECK and must skip the
    /// rebuild; old v8/v9 DBs whose `assistant_runs` CHECK still lacks
    /// `cancelled` get the rebuild. Postcondition-driven so the migration can
    /// never be "defined but skipped forever" nor re-run destructively.
    fn run_v10_rebuild_if_needed(&self) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| crate::Error::Internal(e.to_string()))?;

        // Already recorded: nothing to do.
        let applied: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM _schema_version WHERE version = 10",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        if applied > 0 {
            return Ok(());
        }

        // Postcondition: does `assistant_runs` already include `cancelled` in
        // its status CHECK? If yes, v10's postcondition is already satisfied —
        // record it and skip (fresh DBs).
        let already_has_cancelled: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master
                 WHERE type='table' AND name='assistant_runs' AND sql LIKE '%cancelled%'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(false);
        if already_has_cancelled {
            conn.execute(
                "INSERT OR IGNORE INTO _schema_version (version) VALUES (10)",
                [],
            )
            .map_err(|e| {
                crate::Error::Internal(format!("Failed to record migration v10: {e}"))
            })?;
            return Ok(());
        }

        // Old DB: rebuild outside any transaction with FK off (same protocol as
        // v13 — the rebuild DROPs the legacy table and must not cascade).
        conn.execute_batch("PRAGMA foreign_keys=OFF;")
            .map_err(|e| crate::Error::Internal(format!("v10 FK off failed: {e}")))?;
        let rebuild = conn
            .execute_batch(MIGRATION_010)
            .map_err(|e| crate::Error::Internal(format!("Migration v10 failed: {e}")));
        match rebuild {
            Ok(()) => {
                conn.execute_batch("PRAGMA foreign_keys=ON;")
                    .map_err(|e| crate::Error::Internal(format!("v10 FK on failed: {e}")))?;
                conn.execute(
                    "INSERT OR IGNORE INTO _schema_version (version) VALUES (10)",
                    [],
                )
                .map_err(|e| {
                    crate::Error::Internal(format!("Failed to record migration v10: {e}"))
                })?;
                Ok(())
            }
            Err(e) => {
                conn.execute_batch("PRAGMA foreign_keys=ON;").ok();
                Err(e)
            }
        }
    }

    /// P0-025: run the v13 conversation rebuild OUTSIDE any transaction.
    ///
    /// SQLite ignores `PRAGMA foreign_keys=OFF` inside a transaction. The old
    /// code ran v13 inside `run_migrations`' transaction, so the FK cascade
    /// stayed active and `DROP TABLE assistant_conversations` silently
    /// cascade-deleted every `assistant_messages` / `assistant_runs` row that
    /// referenced it. Correct protocol: FK off → create-copy-swap → FK on →
    /// `foreign_key_check` → record v13.
    fn run_v13_rebuild(&self) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| crate::Error::Internal(e.to_string()))?;

        // Skip if v13 already recorded.
        let applied: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM _schema_version WHERE version = 13",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        if applied > 0 {
            return Ok(());
        }

        // FK off must be set outside any transaction.
        conn.execute_batch("PRAGMA foreign_keys=OFF;")
            .map_err(|e| crate::Error::Internal(format!("v13 FK off failed: {e}")))?;

        let rebuild = conn.execute_batch(MIGRATION_013).map_err(|e| {
            crate::Error::Internal(format!("Migration v13 failed: {e}"))
        });

        match rebuild {
            Ok(()) => {
                conn.execute_batch("PRAGMA foreign_keys=ON;")
                    .map_err(|e| crate::Error::Internal(format!("v13 FK on failed: {e}")))?;
                // Fail closed on any FK violation the rebuild left behind.
                let violations: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM pragma_foreign_key_check",
                        [],
                        |row| row.get(0),
                    )
                    .map_err(|e| {
                        crate::Error::Internal(format!("v13 foreign_key_check failed: {e}"))
                    })?;
                if violations > 0 {
                    return Err(crate::Error::Internal(format!(
                        "Migration v13 left {violations} foreign key violations"
                    )));
                }
                conn.execute(
                    "INSERT OR IGNORE INTO _schema_version (version) VALUES (13)",
                    [],
                )
                .map_err(|e| {
                    crate::Error::Internal(format!("Failed to record migration v13: {e}"))
                })?;
                Ok(())
            }
            Err(e) => {
                conn.execute_batch("PRAGMA foreign_keys=ON;").ok();
                Err(e)
            }
        }
    }

    /// Migrate provider keys from old legacy tables (user_providers / provider_api_keys).
    /// Only runs if the legacy tables exist. Idempotent — INSERT OR IGNORE.
    pub fn migrate_legacy_provider_keys(&self) -> Result<()> {
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

        // Drop local legacy_assistant_messages if it exists, so we can copy fresh from natives_db
        conn.execute_batch("DROP TABLE IF EXISTS legacy_assistant_messages;")
            .ok();

        // Create legacy_assistant_messages locally and load from natives_db
        if attached {
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
        } else {
            conn.execute_batch(
                "ALTER TABLE assistant_messages RENAME TO legacy_assistant_messages;",
            )
            .map_err(|e| crate::Error::Internal(format!("Legacy messages rename failed: {e}")))?;
        }

        // Recreate the new schema version of assistant_messages immediately
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

        // Migrate legacy assistant messages to new schema
        conn.execute_batch(
            "INSERT OR IGNORE INTO assistant_messages
                (id, conversation_id, role, status, created_at)
             SELECT
                 m.id, m.session_id,
                 COALESCE(m.role, 'user'),
                 COALESCE(m.status, 'complete'),
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

    /// Repair databases where SQLite rewrote message_blocks' FK to the
    /// temporary legacy table while assistant_messages was being renamed.
    fn repair_message_blocks_foreign_key(&self) -> Result<()> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|e| crate::Error::Internal(e.to_string()))?;
        let target: Option<String> = conn
            .prepare("PRAGMA foreign_key_list(assistant_message_blocks)")
            .and_then(|mut stmt| {
                let mut rows = stmt.query([])?;
                rows.next()?.map(|row| row.get(2)).transpose()
            })
            .map_err(|e| {
                crate::Error::Internal(format!("Failed to inspect message block foreign key: {e}"))
            })?;

        if target.as_deref() == Some("assistant_messages") {
            return Ok(());
        }

        let tx = conn
            .transaction()
            .map_err(|e| crate::Error::Internal(format!("Message block migration failed: {e}")))?;
        tx.execute_batch(
            "CREATE TABLE assistant_message_blocks_v9 (
                id TEXT PRIMARY KEY,
                message_id TEXT NOT NULL REFERENCES assistant_messages(id) ON DELETE CASCADE,
                block_type TEXT NOT NULL,
                block_index INTEGER NOT NULL DEFAULT 0,
                content TEXT NOT NULL,
                metadata TEXT
            );
            INSERT OR IGNORE INTO assistant_message_blocks_v9
                (id, message_id, block_type, block_index, content, metadata)
            SELECT b.id, b.message_id, b.block_type, b.block_index, b.content, b.metadata
            FROM assistant_message_blocks b
            JOIN assistant_messages m ON m.id = b.message_id;
            DROP TABLE assistant_message_blocks;
            ALTER TABLE assistant_message_blocks_v9 RENAME TO assistant_message_blocks;",
        )
        .map_err(|e| {
            crate::Error::Internal(format!("Message block foreign key repair failed: {e}"))
        })?;
        tx.commit().map_err(|e| {
            crate::Error::Internal(format!("Message block migration commit failed: {e}"))
        })?;
        Ok(())
    }

    /// Finish runs left active by an interrupted app process and preserve any
    /// streamed text/reasoning that was already recorded in run events.
    fn recover_stale_runs(&self) -> Result<()> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|e| crate::Error::Internal(e.to_string()))?;
        let stale: Vec<(String, String)> = {
            let mut stmt = conn.prepare(
                "SELECT id, conversation_id FROM assistant_runs WHERE status IN ('queued','preparing','running','waiting_permission','cancelling')"
            ).map_err(|e| crate::Error::Internal(format!("Failed to inspect stale runs: {e}")))?;
            let rows = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .map_err(|e| crate::Error::Internal(format!("Failed to read stale runs: {e}")))?;
            let mut result = Vec::new();
            for row in rows.flatten() {
                result.push(row);
            }
            result
        };

        for (run_id, conversation_id) in stale {
            let events: Vec<(String, String)> = {
                let mut stmt = conn.prepare(
                    "SELECT event_type, payload FROM assistant_run_events WHERE run_id = ?1 ORDER BY sequence ASC"
                ).map_err(|e| crate::Error::Internal(format!("Failed to inspect stale run events: {e}")))?;
                let rows = stmt
                    .query_map(rusqlite::params![run_id], |row| {
                        Ok((row.get(0)?, row.get(1)?))
                    })
                    .map_err(|e| {
                        crate::Error::Internal(format!("Failed to read stale run events: {e}"))
                    })?;
                let mut result = Vec::new();
                for row in rows.flatten() {
                    result.push(row);
                }
                result
            };
            let mut text = String::new();
            let mut reasoning = String::new();
            for (event_type, payload) in &events {
                let value: serde_json::Value = serde_json::from_str(payload).unwrap_or_default();
                match event_type.as_str() {
                    "assistant_delta" | "text_delta" => text.push_str(
                        value
                            .get("text")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or_default(),
                    ),
                    "reasoning_delta" => reasoning.push_str(
                        value
                            .get("text")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or_default(),
                    ),
                    _ => {}
                }
            }

            let tx = conn
                .transaction()
                .map_err(|e| crate::Error::Internal(format!("Stale run recovery failed: {e}")))?;
            let now = chrono::Utc::now().to_rfc3339();
            if !text.is_empty() || !reasoning.is_empty() {
                let message_id = uuid::Uuid::new_v4().to_string();
                tx.execute(
                    "INSERT INTO assistant_messages (id, conversation_id, role, status, created_at) VALUES (?1, ?2, 'assistant', 'interrupted', ?3)",
                    rusqlite::params![message_id, conversation_id, now],
                ).map_err(|e| crate::Error::Internal(format!("Failed to recover assistant message: {e}")))?;
                let mut index = 0_i64;
                if !reasoning.is_empty() {
                    tx.execute(
                        "INSERT INTO assistant_message_blocks (id, message_id, block_type, block_index, content) VALUES (?1, ?2, 'reasoning', ?3, ?4)",
                        rusqlite::params![uuid::Uuid::new_v4().to_string(), message_id, index, serde_json::json!({"reasoning": reasoning}).to_string()],
                    ).map_err(|e| crate::Error::Internal(format!("Failed to recover reasoning block: {e}")))?;
                    index += 1;
                }
                if !text.is_empty() {
                    tx.execute(
                        "INSERT INTO assistant_message_blocks (id, message_id, block_type, block_index, content) VALUES (?1, ?2, 'text', ?3, ?4)",
                        rusqlite::params![uuid::Uuid::new_v4().to_string(), message_id, index, text],
                    ).map_err(|e| crate::Error::Internal(format!("Failed to recover text block: {e}")))?;
                }
            }
            tx.execute("UPDATE assistant_permission_requests SET status = 'rejected', responded_at = ?1 WHERE run_id = ?2 AND status = 'pending'", rusqlite::params![now, run_id])
                .map_err(|e| crate::Error::Internal(format!("Failed to close stale permission request: {e}")))?;
            tx.execute("UPDATE assistant_runs SET status = 'interrupted', error_code = COALESCE(error_code, 'app_restarted'), finished_at = ?1 WHERE id = ?2", rusqlite::params![now, run_id])
                .map_err(|e| crate::Error::Internal(format!("Failed to recover stale run: {e}")))?;
            let sequence: i64 = tx.query_row("SELECT COALESCE(MAX(sequence), 0) + 1 FROM assistant_run_events WHERE run_id = ?1", rusqlite::params![run_id], |row| row.get(0))
                .unwrap_or(1);
            tx.execute("INSERT INTO assistant_run_events (run_id, sequence, timestamp, event_type, payload) VALUES (?1, ?2, ?3, 'interrupted', ?4)", rusqlite::params![run_id, sequence, now, serde_json::json!({"reason":"app_restarted"}).to_string()])
                .map_err(|e| crate::Error::Internal(format!("Failed to record stale run recovery: {e}")))?;
            tx.commit().map_err(|e| {
                crate::Error::Internal(format!("Stale run recovery commit failed: {e}"))
            })?;
        }
        Ok(())
    }

    /// Clean up any orphaned rows that violate foreign key constraints to keep database integrity.
    fn cleanup_orphaned_rows(&self) -> Result<()> {
        let conn = self.conn();
        conn.execute_batch(
            "DELETE FROM assistant_messages WHERE conversation_id NOT IN (SELECT id FROM assistant_conversations);
             DELETE FROM assistant_message_blocks WHERE message_id NOT IN (SELECT id FROM assistant_messages);
             DELETE FROM assistant_runs WHERE conversation_id NOT IN (SELECT id FROM assistant_conversations);
             DELETE FROM assistant_run_events WHERE run_id NOT IN (SELECT id FROM assistant_runs);
             DELETE FROM assistant_tool_calls WHERE run_id NOT IN (SELECT id FROM assistant_runs);
             DELETE FROM assistant_permission_requests WHERE run_id NOT IN (SELECT id FROM assistant_runs);
             DELETE FROM assistant_artifacts WHERE run_id NOT IN (SELECT id FROM assistant_runs);
             DELETE FROM assistant_context_snapshots WHERE run_id NOT IN (SELECT id FROM assistant_runs);"
        ).map_err(|e| crate::Error::Internal(format!("Failed to clean up orphaned database rows: {e}")))?;
        Ok(())
    }

    /// Get a reference to the underlying connection.
    pub fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn
            .lock()
            .expect("DataStore connection lock poisoned")
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
    parent_run_id TEXT REFERENCES assistant_runs(id) ON DELETE CASCADE,
    subagent_definition_id TEXT,
    status TEXT NOT NULL DEFAULT 'queued' CHECK(status IN ('queued','preparing','running','waiting_permission','cancelling','completed','failed','cancelled','interrupted')),
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

/// v10: Add the explicit user-cancelled terminal state to existing databases.
#[allow(dead_code)]
const MIGRATION_010: &str = "
ALTER TABLE assistant_runs RENAME TO assistant_runs_legacy;
CREATE TABLE assistant_runs (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL REFERENCES assistant_conversations(id) ON DELETE CASCADE,
    parent_run_id TEXT REFERENCES assistant_runs(id) ON DELETE CASCADE,
    subagent_definition_id TEXT,
    status TEXT NOT NULL DEFAULT 'queued' CHECK(status IN ('queued','preparing','running','waiting_permission','cancelling','completed','failed','cancelled','interrupted')),
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
INSERT INTO assistant_runs SELECT * FROM assistant_runs_legacy;
DROP TABLE assistant_runs_legacy;
CREATE INDEX IF NOT EXISTS idx_runs_conversation ON assistant_runs(conversation_id);
";

/// v11: Host-owned prompt queue.
const MIGRATION_011: &str = "
CREATE TABLE IF NOT EXISTS assistant_prompt_queue (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL REFERENCES assistant_conversations(id) ON DELETE CASCADE,
    content TEXT NOT NULL,
    source TEXT NOT NULL DEFAULT 'user',
    attachments TEXT,
    position INTEGER NOT NULL DEFAULT 0,
    client_temp_id TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_prompt_queue_conversation ON assistant_prompt_queue(conversation_id, position);
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

/// v13: Allow conversation mode = goal (long-running task chrome).
/// SQLite cannot ALTER CHECK constraints; rebuild the table.
const MIGRATION_013: &str = "
PRAGMA foreign_keys=OFF;
CREATE TABLE assistant_conversations_v13 (
    id TEXT PRIMARY KEY,
    mode TEXT NOT NULL DEFAULT 'chat' CHECK(mode IN ('chat','agent','goal')),
    project_id TEXT,
    title TEXT NOT NULL DEFAULT '',
    provider_id TEXT NOT NULL,
    model_id TEXT NOT NULL,
    permission_profile_id TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    archived_at TEXT
);
INSERT INTO assistant_conversations_v13 (
    id, mode, project_id, title, provider_id, model_id,
    permission_profile_id, created_at, updated_at, archived_at
)
SELECT
    id,
    CASE WHEN mode IN ('chat','agent','goal') THEN mode ELSE 'agent' END,
    project_id, title, provider_id, model_id,
    permission_profile_id, created_at, updated_at, archived_at
FROM assistant_conversations;
DROP TABLE assistant_conversations;
ALTER TABLE assistant_conversations_v13 RENAME TO assistant_conversations;
CREATE INDEX IF NOT EXISTS idx_conversations_project ON assistant_conversations(project_id);
CREATE INDEX IF NOT EXISTS idx_conversations_updated ON assistant_conversations(updated_at);
PRAGMA foreign_keys=ON;
";

/// v14: Soft-delete support for assistant_projects (logical delete, keep sessions).
const MIGRATION_014: &str = "
ALTER TABLE assistant_projects ADD COLUMN deleted_at TEXT;
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
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM _schema_version",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, 14);
    }

    #[test]
    fn stale_run_recovery_preserves_text_delta() {
        let store = DataStore::new(":memory:").unwrap();
        store.run_migrations().unwrap();
        let conn = store.conn();
        conn.execute(
            "INSERT INTO assistant_conversations (id, title, provider_id, model_id, created_at, updated_at) VALUES ('c1', 'C', 'p1', 'm1', 'now', 'now')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO assistant_runs (id, conversation_id, status, provider_id, model_id, started_at) VALUES ('r1', 'c1', 'running', 'p1', 'm1', 'now')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO assistant_run_events (run_id, sequence, timestamp, event_type, payload) VALUES ('r1', 1, 'now', 'text_delta', '{\"text\":\"partial answer\"}')",
            [],
        ).unwrap();
        drop(conn);

        store.recover_stale_runs().unwrap();

        let content: String = store.conn().query_row(
            "SELECT content FROM assistant_message_blocks WHERE message_id IN (SELECT id FROM assistant_messages WHERE conversation_id = 'c1' AND role = 'assistant')",
            [],
            |row| row.get(0),
        ).unwrap();
        assert!(content.contains("partial answer"));
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

    #[test]
    fn test_repairs_legacy_message_block_foreign_key() {
        let store = DataStore::new(":memory:").unwrap();
        {
            let conn = store.conn();
            conn.execute_batch(
                "ALTER TABLE assistant_messages RENAME TO legacy_assistant_messages;
                 CREATE TABLE assistant_messages (
                    id TEXT PRIMARY KEY,
                    conversation_id TEXT NOT NULL REFERENCES assistant_conversations(id) ON DELETE CASCADE,
                    parent_message_id TEXT,
                    role TEXT NOT NULL CHECK(role IN ('system','user','assistant')),
                    status TEXT NOT NULL DEFAULT 'complete',
                    input_tokens INTEGER DEFAULT 0,
                    output_tokens INTEGER DEFAULT 0,
                    reasoning_tokens INTEGER,
                    cost_usd REAL,
                    created_at TEXT NOT NULL
                 );"
            ).unwrap();
        }

        store.repair_message_blocks_foreign_key().unwrap();
        let target: String = store
            .conn()
            .query_row(
                "PRAGMA foreign_key_list(assistant_message_blocks)",
                [],
                |row| row.get(2),
            )
            .unwrap();
        assert_eq!(target, "assistant_messages");

        let conn = store.conn();
        conn.execute(
            "INSERT INTO assistant_conversations (id, title, provider_id, model_id, created_at, updated_at) VALUES ('c1', '', 'p1', 'm1', 'now', 'now')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO assistant_messages (id, conversation_id, role, created_at) VALUES ('m1', 'c1', 'user', 'now')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO assistant_message_blocks (id, message_id, block_type, content) VALUES ('b1', 'm1', 'text', 'hello')",
            [],
        ).unwrap();
    }
}
