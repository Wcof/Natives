#![allow(dead_code)]
use crate::{Error, Result};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::params;
use rusqlite::Connection;
use rusqlite::OptionalExtension;
use std::path::Path;

/// Current host schema version after all incremental migrations. Kept in sync
/// with the last `_schema_version` write in `apply_migrations`; tests assert
/// against it so a future migration does not leave a stale literal behind.
pub const SCHEMA_VERSION: &str = "23";

/// Map a source-table `state` string to a runtime_instances.status for the
/// v12 backfill. Terminal / unknown states produce no instance.
fn instance_status_for_state(state: &str) -> Option<&'static str> {
    Some(match state {
        "running" => "running",
        "starting" => "starting",
        "stopping" => "stopping",
        "start_failed" => "failed",
        "cleanup_failed" => "cleanup_failed",
        "orphaned" => "orphaned",
        _ => return None,
    })
}

/// Extract pgid / pid from a local process_identity_json during backfill.
pub(crate) fn parse_identity(json: Option<&str>) -> (Option<i32>, Option<u32>) {
    let Some(s) = json else {
        return (None, None);
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(s) else {
        return (None, None);
    };
    let pgid = v
        .get("processGroupId")
        .and_then(|x| x.as_i64())
        .map(|x| x as i32);
    let pid = v.get("pid").and_then(|x| x.as_i64()).map(|x| x as u32);
    (pgid, pid)
}

/// Database connection pool type alias
pub type DbPool = Pool<SqliteConnectionManager>;

use lazy_static::lazy_static;
use std::sync::Mutex;

lazy_static! {
    static ref ASSISTANT_DB_POOL: Mutex<Option<DbPool>> = Mutex::new(None);
    /// 主 natives.db pool（全局持有，供 runtime 等无法 access AppState 的模块使用）
    static ref MAIN_DB_POOL: Mutex<Option<DbPool>> = Mutex::new(None);
}

/// 注册主 natives.db pool（lib.rs setup 钩子调用）
pub fn register_main_pool(pool: DbPool) {
    let mut guard = MAIN_DB_POOL.lock().unwrap();
    *guard = Some(pool);
}

/// 获取主 natives.db pool 的连接（runtime 等无 State 上下文场景）
pub fn get_main_conn() -> Result<r2d2::PooledConnection<SqliteConnectionManager>> {
    let guard = MAIN_DB_POOL.lock().unwrap();
    match guard.as_ref() {
        Some(pool) => pool
            .get()
            .map_err(|e| Error::Internal(format!("Failed to get main DB connection: {e}"))),
        None => Err(Error::Internal("main DB pool not initialized".into())),
    }
}

/// Initialize the assistant database pool at ~/.natives/assistant.db.
/// This is a separate SQLite database isolated from the core natives.db.
pub fn init_assistant_db() -> Result<()> {
    let data_dir = dirs::home_dir()
        .ok_or_else(|| Error::Internal("Cannot find home dir".to_string()))?
        .join(".natives");
    std::fs::create_dir_all(&data_dir)
        .map_err(|e| Error::Internal(format!("Cannot create .natives dir: {e}")))?;
    let db_path = data_dir.join("assistant.db");
    let pool = init_db_pool(&db_path)?;

    // Create assistant-specific tables
    let conn = pool
        .get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS assistant_sessions (
            id TEXT PRIMARY KEY,
            project_id TEXT,
            title TEXT NOT NULL DEFAULT '',
            model_id TEXT NOT NULL DEFAULT '',
            provider_id TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            summary TEXT NOT NULL DEFAULT '',
            token_used INTEGER NOT NULL DEFAULT 0,
            status TEXT NOT NULL DEFAULT 'active'
        );

        CREATE TABLE IF NOT EXISTS assistant_messages (
            id TEXT PRIMARY KEY,
            session_id TEXT NOT NULL REFERENCES assistant_sessions(id) ON DELETE CASCADE,
            role TEXT NOT NULL,
            content TEXT NOT NULL DEFAULT '',
            tool_calls TEXT,
            tool_result TEXT,
            status TEXT NOT NULL DEFAULT '',
            token_count INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            sequence INTEGER NOT NULL DEFAULT 0
        );

        CREATE INDEX IF NOT EXISTS idx_assistant_messages_session
            ON assistant_messages(session_id, sequence);
        ",
    )?;

    // 增量迁移：assistant_sessions 加 runtime_override / sdk_session_id 字段（Slice B）
    // R-D3：用 PRAGMA table_info 检查列存在，ALTER TABLE ADD COLUMN 补齐，禁 DROP
    let existing_cols: Vec<String> = conn
        .prepare("PRAGMA table_info(assistant_sessions)")
        .map_err(Error::Database)?
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(Error::Database)?
        .filter_map(|r| r.ok())
        .collect();
    if !existing_cols.iter().any(|c| c == "runtime_override") {
        conn.execute(
            "ALTER TABLE assistant_sessions ADD COLUMN runtime_override TEXT",
            [],
        )
        .map_err(Error::Database)?;
    }
    if !existing_cols.iter().any(|c| c == "sdk_session_id") {
        conn.execute(
            "ALTER TABLE assistant_sessions ADD COLUMN sdk_session_id TEXT",
            [],
        )
        .map_err(Error::Database)?;
    }

    // scheduled_tasks + task_runs 表（Job 任务模块复用扩展；
    // DDL 与条件补列的单一来源在 jobs::store::ensure_schema）
    crate::jobs::store::ensure_schema(&conn)?;

    let mut guard = ASSISTANT_DB_POOL.lock().unwrap();
    *guard = Some(pool);
    Ok(())
}

/// Get a connection from the assistant database pool.
pub fn get_assistant_db_conn() -> Result<r2d2::PooledConnection<SqliteConnectionManager>> {
    let guard = ASSISTANT_DB_POOL.lock().unwrap();
    match guard.as_ref() {
        Some(pool) => pool
            .get()
            .map_err(|e| Error::Internal(format!("Failed to get assistant DB connection: {e}"))),
        None => {
            drop(guard);
            init_assistant_db()?;
            let guard = ASSISTANT_DB_POOL.lock().unwrap();
            match guard.as_ref() {
                Some(pool) => pool.get().map_err(|e| {
                    Error::Internal(format!("Failed to get assistant DB connection: {e}"))
                }),
                None => Err(Error::Internal(
                    "Failed to initialize assistant DB".to_string(),
                )),
            }
        }
    }
}

/// Initialize the SQLite database with WAL mode, foreign keys, and all tables.
/// Kept for standalone DB initialization (e.g., tests, CLI tools).
#[allow(dead_code)]
pub fn init_db(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;",
    )?;
    create_tables(&conn)?;
    apply_migrations(&conn)?;
    Ok(conn)
}

/// Initialize a connection pool (size 4, idle timeout 30s).
/// Each connection gets WAL mode, foreign keys, and busy_timeout set.
pub fn init_db_pool(path: &Path) -> Result<DbPool> {
    let manager = SqliteConnectionManager::file(path).with_init(|conn| {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
                 PRAGMA foreign_keys = ON;
                 PRAGMA busy_timeout = 5000;",
        )?;
        Ok(())
    });
    let pool = Pool::builder()
        .max_size(4)
        .idle_timeout(Some(std::time::Duration::from_secs(30)))
        .build(manager)
        .map_err(|e| Error::Internal(format!("failed to create DB pool: {e}")))?;

    // Run schema creation/migration on one connection
    let conn = pool
        .get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    create_tables(&conn)?;
    apply_migrations(&conn)?;

    Ok(pool)
}

pub fn create_tables(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        -- 1. Installed plugin registry
        CREATE TABLE IF NOT EXISTS modules (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            version TEXT NOT NULL,
            entry TEXT NOT NULL,
            type TEXT NOT NULL,
            description TEXT,
            author TEXT,
            icon TEXT,
            enabled INTEGER DEFAULT 1,
            min_natives_version TEXT,
            state TEXT DEFAULT 'installed',
            created_at TEXT,
            updated_at TEXT
        );

        -- 2. Per-module permission grants
        CREATE TABLE IF NOT EXISTS module_permissions (
            module_id TEXT NOT NULL REFERENCES modules(id) ON DELETE CASCADE,
            permission TEXT NOT NULL,
            granted INTEGER DEFAULT 0,
            PRIMARY KEY (module_id, permission)
        );

        -- 3. Key-value app settings
        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at TEXT
        );

        -- 4. Per-module key-value storage
        CREATE TABLE IF NOT EXISTS module_data (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            module_id TEXT NOT NULL,
            key TEXT NOT NULL,
            value TEXT,
            UNIQUE(module_id, key)
        );

        -- 5. Workshop/marketplace cache
        CREATE TABLE IF NOT EXISTS workshop_cache (
            id TEXT PRIMARY KEY,
            name TEXT,
            version TEXT,
            description TEXT,
            author TEXT,
            icon TEXT,
            permissions TEXT,
            installed INTEGER DEFAULT 0
        );

        -- 6. Environment configuration profiles
        CREATE TABLE IF NOT EXISTS env_profiles (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE,
            is_default INTEGER DEFAULT 0,
            created_at TEXT
        );

        -- 7. Encrypted env vars per profile
        CREATE TABLE IF NOT EXISTS env_variables (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            profile_id INTEGER NOT NULL REFERENCES env_profiles(id) ON DELETE CASCADE,
            key TEXT NOT NULL,
            value_encrypted TEXT NOT NULL,
            UNIQUE(profile_id, key)
        );

        -- 8. Notification queue
        CREATE TABLE IF NOT EXISTS notifications (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            module_id TEXT,
            title TEXT,
            body TEXT,
            level TEXT DEFAULT 'info',
            read INTEGER DEFAULT 0,
            created_at TEXT
        );

        -- 9. Sidebar ordering
        CREATE TABLE IF NOT EXISTS module_order (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            module_id TEXT NOT NULL REFERENCES modules(id) ON DELETE CASCADE UNIQUE,
            sort_order INTEGER NOT NULL
        );

        -- 10. Permission audit trail
        CREATE TABLE IF NOT EXISTS permission_audit_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            module_id TEXT NOT NULL,
            permission TEXT NOT NULL,
            action TEXT NOT NULL CHECK(action IN ('grant','revoke','deny','approve')),
            granted INTEGER NOT NULL,
            reason TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        -- 11. Kernel-owned module contracts (KI-1 audit tree)
        CREATE TABLE IF NOT EXISTS module_contracts (
            module_id TEXT PRIMARY KEY,
            contract_id TEXT NOT NULL,
            schema_version TEXT NOT NULL,
            content_hash TEXT NOT NULL,
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        -- 12. User-configured LLM providers (native_runtime / provider commands)
        CREATE TABLE IF NOT EXISTS user_providers (
            id TEXT PRIMARY KEY,
            preset_name TEXT NOT NULL,
            api_protocol TEXT NOT NULL DEFAULT 'openai_chat_completions',
            name TEXT NOT NULL,
            website_url TEXT NOT NULL DEFAULT '',
            base_url TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        -- 13. Provider API keys (envelope-encrypted, KEK-DEK)
        CREATE TABLE IF NOT EXISTS provider_api_keys (
            id TEXT PRIMARY KEY,
            provider_id TEXT NOT NULL REFERENCES user_providers(id) ON DELETE CASCADE,
            label TEXT NOT NULL DEFAULT '',
            api_key_encrypted TEXT NOT NULL DEFAULT '',
            dek_encrypted TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL
        );

        -- 14. Imported Sub2API account pools. Credentials and proxy passwords
        -- are envelope-encrypted and never projected to the renderer.
        CREATE TABLE IF NOT EXISTS provider_account_proxies (
            id TEXT PRIMARY KEY,
            provider_id TEXT NOT NULL REFERENCES user_providers(id) ON DELETE CASCADE,
            proxy_key_hash TEXT NOT NULL,
            config_encrypted TEXT NOT NULL,
            dek_encrypted TEXT NOT NULL,
            created_at TEXT NOT NULL,
            UNIQUE(provider_id, proxy_key_hash)
        );
        CREATE TABLE IF NOT EXISTS provider_accounts (
            id TEXT PRIMARY KEY,
            provider_id TEXT NOT NULL REFERENCES user_providers(id) ON DELETE CASCADE,
            name TEXT NOT NULL DEFAULT '',
            platform TEXT NOT NULL,
            account_type TEXT NOT NULL,
            credentials_encrypted TEXT NOT NULL,
            dek_encrypted TEXT NOT NULL,
            extra_json TEXT NOT NULL DEFAULT '{}',
            proxy_id TEXT REFERENCES provider_account_proxies(id) ON DELETE SET NULL,
            concurrency INTEGER NOT NULL DEFAULT 1,
            priority INTEGER NOT NULL DEFAULT 0,
            expires_at TEXT,
            status TEXT NOT NULL DEFAULT 'active',
            identity_fingerprint TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            UNIQUE(provider_id, identity_fingerprint)
        );
        CREATE INDEX IF NOT EXISTS idx_provider_accounts_pool
            ON provider_accounts(provider_id, status, priority);

        -- 15. Host-owned routing configuration. Runtime health remains in assistant.db.
        CREATE TABLE IF NOT EXISTS provider_routing_settings (
            id INTEGER PRIMARY KEY CHECK(id=1),
            enabled INTEGER NOT NULL DEFAULT 0,
            local_enabled INTEGER NOT NULL DEFAULT 0,
            local_port INTEGER NOT NULL DEFAULT 15721,
            local_token_encrypted TEXT,
            local_token_dek_encrypted TEXT,
            rectifier_json TEXT NOT NULL DEFAULT '{}',
            global_proxy_json TEXT NOT NULL DEFAULT '{}',
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS provider_route_bindings (
            id TEXT PRIMARY KEY,
            position INTEGER NOT NULL DEFAULT 0,
            provider_id TEXT NOT NULL REFERENCES user_providers(id) ON DELETE CASCADE,
            credential_kind TEXT NOT NULL CHECK(credential_kind IN ('api_key', 'sub2api_pool')),
            credential_id TEXT,
            model_id TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_provider_route_bindings_position
            ON provider_route_bindings(position);

        -- Indexes
        CREATE INDEX IF NOT EXISTS idx_notifications_unread ON notifications(read, created_at);
        CREATE INDEX IF NOT EXISTS idx_audit_log_module ON permission_audit_log(module_id, created_at);
        ",
    )?;
    Ok(())
}

pub fn apply_migrations(conn: &Connection) -> Result<()> {
    // Migration system: incremental ALTER TABLE ADD COLUMN only.
    // Never DROP TABLE or rebuild — that would lose user data.
    //
    // Version is tracked in settings table with key '_schema_version'.
    // If absent, schema is considered v1 (initial state from create_tables).

    let current_version: i32 = conn
        .query_row(
            "SELECT value FROM settings WHERE key = '_schema_version'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(Error::Database)?
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);

    // Migration v1→v2: v1 schema already includes all 10 tables created at init,
    // so this is a no-op for now. Future migrations go here.
    if current_version < 2 {
        // v2: reserved migration slot — no schema changes needed yet.
        // Future migrations that require column additions or new tables should
        // increment this version and add their DDL here.
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('_schema_version', '2')",
            [],
        )
        .map_err(Error::Database)?;
    }

    // Migration v2→v3: builtin_tools table for extensible built-in tool registry.
    // Each row = one tool (terminal, editor, browser…) with enabled flag and driver choice.
    if current_version < 3 {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS builtin_tools (
                id TEXT PRIMARY KEY,
                enabled INTEGER NOT NULL DEFAULT 0,
                driver TEXT NOT NULL DEFAULT 'native',
                updated_at TEXT
            );

            -- Seed default rows for known built-in tools (all disabled by default)
            INSERT OR IGNORE INTO builtin_tools (id, enabled, driver) VALUES ('terminal', 0, 'native');

            INSERT OR REPLACE INTO settings (key, value) VALUES ('_schema_version', '3');
            ",
        )
        .map_err(Error::Database)?;
    }

    // Migration v3→v4: structured usage_stats and skill_usage tables.
    // Replaces the previous JSON-blob approach (settings key "usage:cached")
    // with proper relational rows for queryability and source breadcrumbs.
    if current_version < 4 {
        conn.execute_batch(
            "
            -- Per-model daily usage statistics with source breadcrumb
            CREATE TABLE IF NOT EXISTS usage_stats (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                date TEXT NOT NULL,           -- YYYY-MM-DD
                source TEXT NOT NULL,         -- 'claude' | 'codex' | 'rtk'
                source_path TEXT,             -- breadcrumb: e.g. '~/.claude/stats-cache.json'
                model TEXT NOT NULL,          -- model identifier
                input_tokens INTEGER NOT NULL DEFAULT 0,
                output_tokens INTEGER NOT NULL DEFAULT 0,
                cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
                cache_read_tokens INTEGER NOT NULL DEFAULT 0,
                request_count INTEGER NOT NULL DEFAULT 0,
                cost_usd REAL NOT NULL DEFAULT 0.0,
                UNIQUE(date, source, model)
            );

            CREATE INDEX IF NOT EXISTS idx_usage_stats_date ON usage_stats(date);
            CREATE INDEX IF NOT EXISTS idx_usage_stats_source ON usage_stats(source);

            -- Skill invocation tracking with source breadcrumb
            CREATE TABLE IF NOT EXISTS skill_usage (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                date TEXT NOT NULL,           -- YYYY-MM-DD
                source TEXT NOT NULL,         -- 'claude-log' | 'codex-session' | 'manual'
                source_path TEXT,             -- breadcrumb: log file path or session ID
                skill_name TEXT NOT NULL,
                trigger_count INTEGER NOT NULL DEFAULT 1,
                UNIQUE(date, source, skill_name)
            );

            CREATE INDEX IF NOT EXISTS idx_skill_usage_date ON skill_usage(date);
            CREATE INDEX IF NOT EXISTS idx_skill_usage_skill ON skill_usage(skill_name);

            INSERT OR REPLACE INTO settings (key, value) VALUES ('_schema_version', '4');
            ",
        )
        .map_err(Error::Database)?;
    }

    // Migration v4→v5: provider_api_keys 加 dek_encrypted 列（信封加密）
    // 原迁移在 init_kek() 里有时序问题——list_providers 可能在 init_kek 之前调用
    if current_version < 5 {
        // PRAGMA table_info 检查列是否已存在（幂等）
        let existing_cols: Vec<String> = conn
            .prepare("PRAGMA table_info(provider_api_keys)")
            .map_err(Error::Database)?
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(Error::Database)?
            .filter_map(|r| r.ok())
            .collect();
        if !existing_cols.iter().any(|c| c == "dek_encrypted") {
            conn.execute_batch(
                "ALTER TABLE provider_api_keys ADD COLUMN dek_encrypted TEXT NOT NULL DEFAULT '';",
            )
            .map_err(Error::Database)?;
        }
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('_schema_version', '5')",
            [],
        )
        .map_err(Error::Database)?;
    }

    // Cache tables must be ensured independently of the schema marker. Some existing
    // installations already have a later version marker but never received this table.
    // `CREATE TABLE IF NOT EXISTS` makes startup repair safe and non-destructive.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS usage_dashboard_snapshots (
            time_zone TEXT PRIMARY KEY,
            schema_version INTEGER NOT NULL,
            generated_at_ms INTEGER NOT NULL,
            coverage_start_ms INTEGER NOT NULL,
            coverage_end_ms INTEGER NOT NULL,
            payload_json TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )",
        [],
    )
    .map_err(Error::Database)?;

    let provider_cols: Vec<String> = conn
        .prepare("PRAGMA table_info(user_providers)")
        .map_err(Error::Database)?
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(Error::Database)?
        .filter_map(|r| r.ok())
        .collect();
    if !provider_cols.iter().any(|c| c == "api_protocol") {
        conn.execute_batch(
            "ALTER TABLE user_providers ADD COLUMN api_protocol TEXT NOT NULL DEFAULT 'openai_chat_completions';",
        )
        .map_err(Error::Database)?;
    }
    conn.execute_batch(
        "UPDATE user_providers
         SET api_protocol = CASE
             WHEN lower(preset_name) IN ('anthropic', 'claude', 'anthropic_messages') THEN 'anthropic_messages'
             WHEN lower(preset_name) IN ('openai_responses', 'responses') THEN 'openai_responses'
             WHEN lower(preset_name) IN ('gemini', 'google', 'gemini_generate_content') THEN 'gemini_generate_content'
             WHEN lower(preset_name) IN ('ollama', 'ollama_chat') THEN 'ollama_chat'
             ELSE 'openai_chat_completions'
         END
         WHERE api_protocol IS NULL
            OR api_protocol = ''
            OR api_protocol IN ('openai_compatible', 'anthropic', 'claude');",
    )
    .map_err(Error::Database)?;

    // Migration v5→v6: record the version only when this database has not advanced
    // beyond it. Never downgrade a newer marker.
    if current_version < 6 {
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('_schema_version', '6')",
            [],
        )
        .map_err(Error::Database)?;
    }

    // Migration v6→v7: external creative apps (GitHub container) + encrypted env.
    // Does not alter `modules` — dual-source storage stays on separate tables (ADR-0013).
    if current_version < 7 {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS external_creative_apps (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                description TEXT,
                icon TEXT,
                version TEXT NOT NULL,
                owner TEXT NOT NULL,
                repo TEXT NOT NULL,
                repository_url TEXT NOT NULL,
                release_tag TEXT NOT NULL,
                release_id INTEGER,
                runtime TEXT NOT NULL,
                state TEXT NOT NULL,
                open_url TEXT,
                health_url TEXT,
                host_port INTEGER,
                runtime_config_json TEXT NOT NULL DEFAULT '{}',
                last_error TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_external_creative_apps_state
                ON external_creative_apps(state);
            CREATE INDEX IF NOT EXISTS idx_external_creative_apps_updated
                ON external_creative_apps(updated_at);

            CREATE TABLE IF NOT EXISTS creative_app_env (
                app_id TEXT NOT NULL REFERENCES external_creative_apps(id) ON DELETE CASCADE,
                key TEXT NOT NULL,
                value_encrypted TEXT NOT NULL,
                PRIMARY KEY (app_id, key)
            );

            INSERT OR REPLACE INTO settings (key, value) VALUES ('_schema_version', '7');
            ",
        )
        .map_err(Error::Database)?;
    }

    // Repair path: ensure v7 tables exist even if marker was advanced without DDL.
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS external_creative_apps (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            description TEXT,
            icon TEXT,
            version TEXT NOT NULL,
            owner TEXT NOT NULL,
            repo TEXT NOT NULL,
            repository_url TEXT NOT NULL,
            release_tag TEXT NOT NULL,
            release_id INTEGER,
            runtime TEXT NOT NULL,
            state TEXT NOT NULL,
            open_url TEXT,
            health_url TEXT,
            host_port INTEGER,
            runtime_config_json TEXT NOT NULL DEFAULT '{}',
            last_error TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS creative_app_env (
            app_id TEXT NOT NULL REFERENCES external_creative_apps(id) ON DELETE CASCADE,
            key TEXT NOT NULL,
            value_encrypted TEXT NOT NULL,
            PRIMARY KEY (app_id, key)
        );
        ",
    )
    .map_err(Error::Database)?;

    // Migration v7→v8: local creative projects (third source).
    // Independent of external_creative_apps / modules (ADR-0013 extension).
    if current_version < 8 {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS local_creative_apps (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                description TEXT,
                icon TEXT,
                canonical_project_root TEXT NOT NULL UNIQUE,
                device_id TEXT NOT NULL,
                device_name TEXT NOT NULL,
                project_kind TEXT NOT NULL,
                launch_mode TEXT NOT NULL,
                launch_plan_json TEXT NOT NULL DEFAULT '{}',
                plan_fingerprint TEXT NOT NULL DEFAULT '',
                state TEXT NOT NULL,
                status_detail_json TEXT,
                open_url TEXT,
                current_port INTEGER,
                process_identity_json TEXT,
                auto_open INTEGER NOT NULL DEFAULT 1,
                startup_timeout_ms INTEGER NOT NULL DEFAULT 60000,
                last_started_at TEXT,
                last_exit_reason TEXT,
                last_error TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_local_creative_apps_state
                ON local_creative_apps(state);
            CREATE INDEX IF NOT EXISTS idx_local_creative_apps_updated
                ON local_creative_apps(updated_at);

            CREATE TABLE IF NOT EXISTS local_creative_env (
                app_id TEXT NOT NULL REFERENCES local_creative_apps(id) ON DELETE CASCADE,
                key TEXT NOT NULL,
                value_encrypted TEXT NOT NULL,
                PRIMARY KEY (app_id, key)
            );

            INSERT OR REPLACE INTO settings (key, value) VALUES ('_schema_version', '8');
            ",
        )
        .map_err(Error::Database)?;
    }

    // Repair path for v8 tables.
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS local_creative_apps (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            description TEXT,
            icon TEXT,
            canonical_project_root TEXT NOT NULL UNIQUE,
            device_id TEXT NOT NULL,
            device_name TEXT NOT NULL,
            project_kind TEXT NOT NULL,
            launch_mode TEXT NOT NULL,
            launch_plan_json TEXT NOT NULL DEFAULT '{}',
            plan_fingerprint TEXT NOT NULL DEFAULT '',
            state TEXT NOT NULL,
            status_detail_json TEXT,
            open_url TEXT,
            current_port INTEGER,
            process_identity_json TEXT,
            auto_open INTEGER NOT NULL DEFAULT 1,
            startup_timeout_ms INTEGER NOT NULL DEFAULT 60000,
            last_started_at TEXT,
            last_exit_reason TEXT,
            last_error TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS local_creative_env (
            app_id TEXT NOT NULL REFERENCES local_creative_apps(id) ON DELETE CASCADE,
            key TEXT NOT NULL,
            value_encrypted TEXT NOT NULL,
            PRIMARY KEY (app_id, key)
        );
        ",
    )
    .map_err(Error::Database)?;

    // Migration v8→v9: Sub2API account pools and provider routing settings.
    if current_version < 9 {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS provider_account_proxies (
                id TEXT PRIMARY KEY,
                provider_id TEXT NOT NULL REFERENCES user_providers(id) ON DELETE CASCADE,
                proxy_key_hash TEXT NOT NULL,
                config_encrypted TEXT NOT NULL,
                dek_encrypted TEXT NOT NULL,
                created_at TEXT NOT NULL,
                UNIQUE(provider_id, proxy_key_hash)
            );
            CREATE TABLE IF NOT EXISTS provider_accounts (
                id TEXT PRIMARY KEY,
                provider_id TEXT NOT NULL REFERENCES user_providers(id) ON DELETE CASCADE,
                name TEXT NOT NULL DEFAULT '',
                platform TEXT NOT NULL,
                account_type TEXT NOT NULL,
                credentials_encrypted TEXT NOT NULL,
                dek_encrypted TEXT NOT NULL,
                extra_json TEXT NOT NULL DEFAULT '{}',
                proxy_id TEXT REFERENCES provider_account_proxies(id) ON DELETE SET NULL,
                concurrency INTEGER NOT NULL DEFAULT 1,
                priority INTEGER NOT NULL DEFAULT 0,
                expires_at TEXT,
                status TEXT NOT NULL DEFAULT 'active',
                identity_fingerprint TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                UNIQUE(provider_id, identity_fingerprint)
            );
            CREATE INDEX IF NOT EXISTS idx_provider_accounts_pool
                ON provider_accounts(provider_id, status, priority);
            CREATE TABLE IF NOT EXISTS provider_routing_settings (
                id INTEGER PRIMARY KEY CHECK(id=1),
                enabled INTEGER NOT NULL DEFAULT 0,
                local_enabled INTEGER NOT NULL DEFAULT 0,
                local_port INTEGER NOT NULL DEFAULT 15721,
                local_token_encrypted TEXT,
                local_token_dek_encrypted TEXT,
                rectifier_json TEXT NOT NULL DEFAULT '{}',
                global_proxy_json TEXT NOT NULL DEFAULT '{}',
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS provider_route_bindings (
                id TEXT PRIMARY KEY,
                position INTEGER NOT NULL DEFAULT 0,
                provider_id TEXT NOT NULL REFERENCES user_providers(id) ON DELETE CASCADE,
                credential_kind TEXT NOT NULL CHECK(credential_kind IN ('api_key', 'sub2api_pool')),
                credential_id TEXT,
                model_id TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_provider_route_bindings_position
                ON provider_route_bindings(position);
            INSERT OR IGNORE INTO provider_routing_settings (id, updated_at) VALUES (1, datetime('now'));
            INSERT OR REPLACE INTO settings (key, value) VALUES ('_schema_version', '9');
            ",
        )
        .map_err(Error::Database)?;
    }

    // Migration v9→v10: creative draft store (ADR-0014).
    //
    // Drafts are deliberately kept out of `modules`: a draft has no contract_id,
    // no sidebar entry and no domain namespace until the user publishes it.
    // Revision content lives on disk under ~/.natives/drafts/<draft_id>/;
    // only metadata is relational.
    if current_version < 10 {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS creative_drafts (
                draft_id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                intent TEXT NOT NULL,
                conversation_id TEXT,
                origin_module_id TEXT,
                current_revision INTEGER NOT NULL DEFAULT 0,
                state TEXT NOT NULL DEFAULT 'drafting'
                    CHECK(state IN ('drafting','generating','ready','publishing','published','archived')),
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS creative_draft_revisions (
                draft_id TEXT NOT NULL REFERENCES creative_drafts(draft_id) ON DELETE CASCADE,
                revision INTEGER NOT NULL,
                content_hash TEXT NOT NULL,
                created_at TEXT NOT NULL,
                PRIMARY KEY (draft_id, revision)
            );
            CREATE INDEX IF NOT EXISTS idx_creative_drafts_state
                ON creative_drafts(state);
            CREATE INDEX IF NOT EXISTS idx_creative_drafts_conversation
                ON creative_drafts(conversation_id);
            INSERT OR REPLACE INTO settings (key, value) VALUES ('_schema_version', '10');
            ",
        )
        .map_err(Error::Database)?;
    }

    // Migration v10→v11: capability secrets (ADR-0016 decision 7).
    //
    // Host-owned encrypted store for capability-library secrets: MCP env vars,
    // bearer tokens and OAuth refresh tokens. `owner_ref` points at the
    // capability MCP server id. Column semantics reuse the provider_api_keys
    // KEK-DEK envelope (see provider_key_manager):
    // - `ciphertext` = BASE64(nonce || AES-256-GCM ciphertext) under a per-row DEK
    // - `nonce`      = BASE64(kek_nonce || DEK wrapped by the provider KEK)
    // The daemon only reads rows via NativesDbBroker and never persists plaintext.
    if current_version < 11 {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS capability_secrets (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL CHECK(kind IN ('mcp_env','mcp_bearer','mcp_oauth_refresh')),
                owner_ref TEXT NOT NULL,
                key_name TEXT,
                ciphertext TEXT NOT NULL,
                nonce TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_capability_secrets_owner
                ON capability_secrets(owner_ref);
            CREATE UNIQUE INDEX IF NOT EXISTS idx_capability_secrets_identity
                ON capability_secrets(kind, owner_ref, COALESCE(key_name, ''));
            INSERT OR REPLACE INTO settings (key, value) VALUES ('_schema_version', '11');
            ",
        )
        .map_err(Error::Database)?;
    }

    // Migration v11→v12: unified Application identity + RuntimeInstance (batch 1).
    //
    // `modules` / `external_creative_apps` / `local_creative_apps` stay the
    // source detail. `applications` gives every app one identity; `startup_plans`
    // keeps the versioned plan; `runtime_instances` records the current runtime
    // (one active instance per app — the CAS batch 2 promotes to real owner);
    // `preview_targets` will bind previews to instances (batch 6).
    if current_version < 12 {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS applications (
                id TEXT PRIMARY KEY,
                source TEXT NOT NULL,
                source_id TEXT NOT NULL,
                title TEXT NOT NULL,
                description TEXT,
                icon TEXT,
                version TEXT NOT NULL DEFAULT '1',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_applications_source_id
                ON applications(source, source_id);
            CREATE INDEX IF NOT EXISTS idx_applications_updated
                ON applications(updated_at);

            CREATE TABLE IF NOT EXISTS startup_plans (
                id TEXT PRIMARY KEY,
                application_id TEXT NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
                plan_version INTEGER NOT NULL DEFAULT 1,
                plan_json TEXT NOT NULL,
                is_active INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_startup_plans_application
                ON startup_plans(application_id);

            CREATE TABLE IF NOT EXISTS runtime_instances (
                id TEXT PRIMARY KEY,
                application_id TEXT NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
                plan_id TEXT REFERENCES startup_plans(id) ON DELETE SET NULL,
                status TEXT NOT NULL,
                cleanup_status TEXT,
                owner_kind TEXT NOT NULL,
                pgid INTEGER,
                compose_project TEXT,
                resolved_urls_json TEXT,
                current_port INTEGER,
                pid INTEGER,
                failure TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_runtime_instances_application
                ON runtime_instances(application_id);
            CREATE INDEX IF NOT EXISTS idx_runtime_instances_status
                ON runtime_instances(status);

            CREATE TABLE IF NOT EXISTS preview_targets (
                id TEXT PRIMARY KEY,
                runtime_instance_id TEXT NOT NULL REFERENCES runtime_instances(id) ON DELETE CASCADE,
                url TEXT NOT NULL,
                kind TEXT NOT NULL,
                selected INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_preview_targets_instance
                ON preview_targets(runtime_instance_id);

            INSERT OR REPLACE INTO settings (key, value) VALUES ('_schema_version', '12');
            ",
        )
        .map_err(Error::Database)?;
    }

    // Repair path for v12 tables when a database carries an advanced marker.
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS applications (
            id TEXT PRIMARY KEY,
            source TEXT NOT NULL,
            source_id TEXT NOT NULL,
            title TEXT NOT NULL,
            description TEXT,
            icon TEXT,
            version TEXT NOT NULL DEFAULT '1',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_applications_source_id
            ON applications(source, source_id);
        CREATE TABLE IF NOT EXISTS startup_plans (
            id TEXT PRIMARY KEY,
            application_id TEXT NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
            plan_version INTEGER NOT NULL DEFAULT 1,
            plan_json TEXT NOT NULL,
            is_active INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_startup_plans_application
            ON startup_plans(application_id);
        CREATE TABLE IF NOT EXISTS runtime_instances (
            id TEXT PRIMARY KEY,
            application_id TEXT NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
            plan_id TEXT REFERENCES startup_plans(id) ON DELETE SET NULL,
            status TEXT NOT NULL,
            cleanup_status TEXT,
            owner_kind TEXT NOT NULL,
            pgid INTEGER,
            compose_project TEXT,
            resolved_urls_json TEXT,
            current_port INTEGER,
            pid INTEGER,
            failure TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_runtime_instances_application
            ON runtime_instances(application_id);
        CREATE TABLE IF NOT EXISTS preview_targets (
            id TEXT PRIMARY KEY,
            runtime_instance_id TEXT NOT NULL REFERENCES runtime_instances(id) ON DELETE CASCADE,
            url TEXT NOT NULL,
            kind TEXT NOT NULL,
            selected INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_preview_targets_instance
            ON preview_targets(runtime_instance_id);
        ",
    )
    .map_err(Error::Database)?;

    // Idempotent backfill of existing sources into the unified identity / plans /
    // runtime instances. Extracted so tests can prove idempotency.
    backfill_creative_identity(conn)?;

    // Migration v12→v13: runtime instance bookkeeping columns (batch 2).
    // last_heartbeat / owner_pid / exit_code / resource_ledger_json give the
    // instance row enough to answer "who owns it, is it alive, what leaked".
    // Column adds are guarded by PRAGMA so re-running is safe (repair path).
    if current_version < 13 {
        let mut cols = std::collections::HashSet::new();
        {
            let mut stmt = conn
                .prepare("PRAGMA table_info(runtime_instances)")
                .map_err(Error::Database)?;
            let rows = stmt
                .query_map([], |r| r.get::<_, String>(1))
                .map_err(Error::Database)?;
            for r in rows {
                cols.insert(r.map_err(Error::Database)?);
            }
        }
        if !cols.contains("last_heartbeat") {
            conn.execute(
                "ALTER TABLE runtime_instances ADD COLUMN last_heartbeat TEXT",
                [],
            )
            .map_err(Error::Database)?;
        }
        if !cols.contains("owner_pid") {
            conn.execute(
                "ALTER TABLE runtime_instances ADD COLUMN owner_pid INTEGER",
                [],
            )
            .map_err(Error::Database)?;
        }
        if !cols.contains("exit_code") {
            conn.execute(
                "ALTER TABLE runtime_instances ADD COLUMN exit_code INTEGER",
                [],
            )
            .map_err(Error::Database)?;
        }
        if !cols.contains("resource_ledger_json") {
            conn.execute(
                "ALTER TABLE runtime_instances ADD COLUMN resource_ledger_json TEXT",
                [],
            )
            .map_err(Error::Database)?;
        }
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('_schema_version', '13')",
            [],
        )
        .map_err(Error::Database)?;
    }

    // Migration v13→v14: local app volume identity (batch 4). Persisted so a
    // volume re-mount / disconnect can be recognized across restarts.
    if current_version < 14 {
        let has_volume = conn
            .prepare("PRAGMA table_info(local_creative_apps)")
            .map_err(Error::Database)?
            .query_map([], |r| r.get::<_, String>(1))
            .map_err(Error::Database)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::Database)?
            .iter()
            .any(|c| c == "volume_identity");
        if !has_volume {
            conn.execute(
                "ALTER TABLE local_creative_apps ADD COLUMN volume_identity TEXT",
                [],
            )
            .map_err(Error::Database)?;
        }
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('_schema_version', '14')",
            [],
        )
        .map_err(Error::Database)?;
    }

    // Migration v14→v15 (batch 1 CR-101): repair ghost `applications` identities.
    //
    // Older read paths used find-or-create on the browser open/close flow, which
    // fabricated a fake `local_project` application row for a GitHub app. This
    // migration only deletes rows with NO source row AND no dependent
    // startup_plans/runtime_instances (double gate); every deleted row JSON is
    // backed up to `creative_identity_reports`. Rows with dependencies or
    // cross-source collisions are reported and left untouched.
    if current_version < 15 {
        repair_creative_identity_ghosts(conn)?;
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('_schema_version', '15')",
            [],
        )
        .map_err(Error::Database)?;
    }

    // Migration v15→v16 (batch 1 CR-102): one active runtime instance and one
    // active startup plan per application as DATABASE invariants.
    //
    // Existing duplicates (from the old double-start race) are reconciled first:
    // the newest active row is kept, the rest are demoted (runtime duplicates →
    // orphaned, plan duplicates → is_active=0) and each demotion is audited in
    // `creative_identity_reports`. Only then are the partial unique indexes
    // created, so a concurrent second start is rejected by the DB (#06/#07).
    if current_version < 16 {
        repair_creative_active_invariants(conn)?;
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('_schema_version', '16')",
            [],
        )
        .map_err(Error::Database)?;
    }

    // Migration v16→v17 (batch 1 CR-103): promote `startup_plans` to a versioned
    // LaunchProfile base.
    //
    // Adds nullable schema_version / driver_kind / ownership_mode columns,
    // backfills them from the stored plan JSON (read/write both derive them; the
    // columns are the persisted mirror), and repairs the earlier Compose backfill
    // that classified a local docker_compose instance as `local_process`.
    if current_version < 17 {
        upgrade_startup_plans_v1(conn)?;
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('_schema_version', '17')",
            [],
        )
        .map_err(Error::Database)?;
    }

    // Migration v17→v18 (batch 2 CR-201): operation journal for lifecycle
    // mutations.
    //
    // install/start/stop/restart/delete each record a durable operation row
    // (kind / phase / redacted input / error / timestamps) so every external
    // side effect is traceable to an operation and partial failures have a
    // recovery carrier. Additive and idempotent — no source detail is touched.
    // `application_id` is nullable with ON DELETE SET NULL so a delete
    // operation survives the removal of its own application row (audit trail).
    if current_version < 18 {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS operations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                application_id TEXT REFERENCES applications(id) ON DELETE SET NULL,
                runtime_instance_id TEXT REFERENCES runtime_instances(id) ON DELETE SET NULL,
                kind TEXT NOT NULL,
                phase TEXT NOT NULL,
                actor TEXT NOT NULL DEFAULT 'user',
                redacted_input_json TEXT,
                error_code TEXT,
                error_message TEXT,
                started_at TEXT NOT NULL,
                finished_at TEXT,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_operations_application_active
                ON operations(application_id, phase);
            CREATE INDEX IF NOT EXISTS idx_operations_updated_at
                ON operations(updated_at);
            INSERT OR REPLACE INTO settings (key, value) VALUES ('_schema_version', '18');
            ",
        )
        .map_err(Error::Database)?;
    }

    // Migration v18→v19 (batch 5 CR-501): Surface, Endpoint, Window tables.
    //
    // application_surfaces: each app has one main surface (backfilled from
    // existing applications) and optionally embed surfaces for child WebViews.
    //
    // runtime_endpoints: each runtime instance can have zero or more endpoints
    // (preview URL, API, health check). Backfilled from preview_targets.
    //
    // window_instances: each window maps 1:1 to a Tauri WebView/WebviewWindow.
    // Backfilled from active browser_show entries.
    if current_version < 19 {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS application_surfaces (
                id TEXT PRIMARY KEY,
                application_id TEXT NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
                kind TEXT NOT NULL DEFAULT 'main',
                label TEXT NOT NULL DEFAULT 'Main',
                title TEXT,
                url TEXT,
                bounds_json TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_surfaces_application
                ON application_surfaces(application_id);

            CREATE TABLE IF NOT EXISTS runtime_endpoints (
                id TEXT PRIMARY KEY,
                runtime_instance_id TEXT NOT NULL REFERENCES runtime_instances(id) ON DELETE CASCADE,
                kind TEXT NOT NULL DEFAULT 'preview',
                url TEXT NOT NULL,
                port INTEGER,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_endpoints_runtime
                ON runtime_endpoints(runtime_instance_id);

            CREATE TABLE IF NOT EXISTS window_instances (
                id TEXT PRIMARY KEY,
                application_id TEXT NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
                surface_id TEXT NOT NULL REFERENCES application_surfaces(id) ON DELETE CASCADE,
                runtime_instance_id TEXT REFERENCES runtime_instances(id) ON DELETE SET NULL,
                label TEXT NOT NULL,
                state TEXT NOT NULL DEFAULT 'closed',
                bounds_json TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_windows_application
                ON window_instances(application_id);
            CREATE INDEX IF NOT EXISTS idx_windows_surface
                ON window_instances(surface_id);

            INSERT OR REPLACE INTO settings (key, value) VALUES ('_schema_version', '19');
            ",
        )
        .map_err(Error::Database)?;
        // Backfill main surfaces for existing applications and preview
        // endpoints for active runtimes (idempotent, additive).
        if let Err(e) = crate::creative_app::surface_store::backfill_v19(conn) {
            eprintln!("warning: surface backfill failed: {e}");
        }
    }

    // Migration v19→v20 (batch 6 CR-601): BrowserProfile table.
    //
    // Profiles store metadata only — never cookie content. The platform_store_key
    // identifies the WKWebsiteDataStore for future use when per-profile isolation
    // becomes possible on the platform.
    if current_version < 20 {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS browser_profiles (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                platform_store_key TEXT NOT NULL,
                is_default INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            -- Seed the default profile
            INSERT OR IGNORE INTO browser_profiles (id, name, platform_store_key, is_default, created_at, updated_at)
                VALUES ('default', 'Default', 'default', 1, datetime('now'), datetime('now'));
            INSERT OR REPLACE INTO settings (key, value) VALUES ('_schema_version', '20');
            ",
        )
        .map_err(Error::Database)?;
    }

    // Migration v20→v21 (batch 6 CR-602/603): OAuth allowlist + app grants.
    //
    // oauth_allowlist: per-app domain allowlist for OAuth popup windows.
    // app_grants: per-app capability grants (upload, download, clipboard, window_open).
    if current_version < 21 {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS oauth_allowlist (
                id TEXT PRIMARY KEY,
                application_id TEXT NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
                domain TEXT NOT NULL,
                created_at TEXT NOT NULL,
                UNIQUE(application_id, domain)
            );

            CREATE TABLE IF NOT EXISTS app_grants (
                id TEXT PRIMARY KEY,
                application_id TEXT NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
                kind TEXT NOT NULL,
                policy TEXT NOT NULL DEFAULT 'default_deny',
                path TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                UNIQUE(application_id, kind)
            );

            INSERT OR REPLACE INTO settings (key, value) VALUES ('_schema_version', '21');
            ",
        )
        .map_err(Error::Database)?;
    }

    // Migration v21→v22 (batch 7 CR-701): service_instances table.
    //
    // A runtime can expose multiple services (e.g. Compose web + db); each
    // service gets a row with its readiness state. Single-service runtimes get
    // a "main" service row.
    if current_version < 22 {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS service_instances (
                id TEXT PRIMARY KEY,
                runtime_instance_id TEXT NOT NULL REFERENCES runtime_instances(id) ON DELETE CASCADE,
                name TEXT NOT NULL,
                readiness TEXT NOT NULL DEFAULT 'starting',
                required INTEGER NOT NULL DEFAULT 1,
                endpoint_id TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                UNIQUE(runtime_instance_id, name)
            );
            CREATE INDEX IF NOT EXISTS idx_services_runtime
                ON service_instances(runtime_instance_id);

                        INSERT OR REPLACE INTO settings (key, value) VALUES ('_schema_version', '22');
            ",
        )
        .map_err(Error::Database)?;
        // Backfill a "main" service row for active runtimes (idempotent).
        if let Err(e) = crate::creative_app::service_store::backfill_v22(conn) {
            eprintln!("warning: service backfill failed: {e}");
        }
    }

    // Migration v22→v23: creative proposal inbox + executable approval records
    // (T06), window_instances URL/reconcile truth (T07), and real browser
    // profiles + grant history (T08). Incremental tables/columns only — no
    // DROP, no user-data rebuild.
    if current_version < 23 {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS creative_proposal_inbox (
                proposal_id TEXT PRIMARY KEY,
                envelope_json TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                failure TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                approved_at TEXT,
                approver TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_proposal_inbox_status
                ON creative_proposal_inbox(status);

            CREATE TABLE IF NOT EXISTS creative_executable_approval (
                id TEXT PRIMARY KEY,
                canonical_path TEXT NOT NULL,
                file_identity TEXT NOT NULL,
                scope TEXT NOT NULL,
                approver TEXT NOT NULL,
                approved_at TEXT NOT NULL,
                proposal_id TEXT NOT NULL
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_executable_approval_path
                ON creative_executable_approval(canonical_path);

            CREATE TABLE IF NOT EXISTS browser_profile_bindings (
                application_id TEXT PRIMARY KEY REFERENCES applications(id) ON DELETE CASCADE,
                profile_id TEXT NOT NULL REFERENCES browser_profiles(id) ON DELETE CASCADE,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS grant_events (
                id TEXT PRIMARY KEY,
                application_id TEXT NOT NULL REFERENCES applications(id) ON DELETE CASCADE,
                kind TEXT NOT NULL,
                event TEXT NOT NULL,
                policy TEXT,
                path TEXT,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_grant_events_app
                ON grant_events(application_id, created_at);
            ",
        )
        .map_err(Error::Database)?;
        // T07: window_instances gets URL + reconcile truth.
        //
        // url: the content URL a window is currently showing (NULL when closed).
        // last_error: reconcile/operation detail so the UI can show why a
        //   window is closed (honest state — never a fabricated value, R-F2).
        // reconcile_state: 'ok' | 'missing' | 'orphaned' — explicit outcome of
        //   the DB↔WebView reconcile sweep on Host restart.
        let cols = {
            let mut stmt = conn
                .prepare("PRAGMA table_info(window_instances)")
                .map_err(Error::Database)?;
            let rows = stmt
                .query_map([], |r| r.get::<_, String>(1))
                .map_err(Error::Database)?;
            rows.filter_map(|r| r.ok()).collect::<Vec<_>>()
        };
        if !cols.iter().any(|c| c == "url") {
            conn.execute_batch("ALTER TABLE window_instances ADD COLUMN url TEXT;")
                .map_err(Error::Database)?;
        }
        if !cols.iter().any(|c| c == "last_error") {
            conn.execute_batch("ALTER TABLE window_instances ADD COLUMN last_error TEXT;")
                .map_err(Error::Database)?;
        }
        if !cols.iter().any(|c| c == "reconcile_state") {
            conn.execute_batch(
                "ALTER TABLE window_instances ADD COLUMN reconcile_state TEXT NOT NULL DEFAULT 'ok';",
            )
            .map_err(Error::Database)?;
        }
        // T08: repair non-hex platform_store_key rows (pre-v23 placeholders
        // like 'default') to a deterministic 16-byte identifier from the
        // profile id, so a profile's cookies survive restart.
        let profiles: Vec<(String, String)> = {
            let mut stmt = conn
                .prepare("SELECT id, platform_store_key FROM browser_profiles")
                .map_err(Error::Database)?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(Error::Database)?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row.map_err(Error::Database)?);
            }
            out
        };
        let is_hex_key = |key: &str| key.len() == 32 && key.chars().all(|c| c.is_ascii_hexdigit());
        for (id, key) in profiles {
            if !is_hex_key(&key) {
                let repaired = crate::creative_app::profile_store::store_key_for_id(&id);
                conn.execute(
                    "UPDATE browser_profiles SET platform_store_key = ?1 WHERE id = ?2",
                    rusqlite::params![repaired, id],
                )
                .map_err(Error::Database)?;
            }
        }
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('_schema_version', '23')",
            [],
        )
        .map_err(Error::Database)?;
    }

    // Repair path for v9 tables when a database carries an advanced marker.
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS provider_account_proxies (
            id TEXT PRIMARY KEY,
            provider_id TEXT NOT NULL REFERENCES user_providers(id) ON DELETE CASCADE,
            proxy_key_hash TEXT NOT NULL,
            config_encrypted TEXT NOT NULL,
            dek_encrypted TEXT NOT NULL,
            created_at TEXT NOT NULL,
            UNIQUE(provider_id, proxy_key_hash)
        );
        CREATE TABLE IF NOT EXISTS provider_accounts (
            id TEXT PRIMARY KEY,
            provider_id TEXT NOT NULL REFERENCES user_providers(id) ON DELETE CASCADE,
            name TEXT NOT NULL DEFAULT '',
            platform TEXT NOT NULL,
            account_type TEXT NOT NULL,
            credentials_encrypted TEXT NOT NULL,
            dek_encrypted TEXT NOT NULL,
            extra_json TEXT NOT NULL DEFAULT '{}',
            proxy_id TEXT REFERENCES provider_account_proxies(id) ON DELETE SET NULL,
            concurrency INTEGER NOT NULL DEFAULT 1,
            priority INTEGER NOT NULL DEFAULT 0,
            expires_at TEXT,
            status TEXT NOT NULL DEFAULT 'active',
            identity_fingerprint TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            UNIQUE(provider_id, identity_fingerprint)
        );
        CREATE INDEX IF NOT EXISTS idx_provider_accounts_pool
            ON provider_accounts(provider_id, status, priority);
        CREATE TABLE IF NOT EXISTS provider_routing_settings (
            id INTEGER PRIMARY KEY CHECK(id=1),
            enabled INTEGER NOT NULL DEFAULT 0,
            local_enabled INTEGER NOT NULL DEFAULT 0,
            local_port INTEGER NOT NULL DEFAULT 15721,
            local_token_encrypted TEXT,
            local_token_dek_encrypted TEXT,
            rectifier_json TEXT NOT NULL DEFAULT '{}',
            global_proxy_json TEXT NOT NULL DEFAULT '{}',
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS provider_route_bindings (
            id TEXT PRIMARY KEY,
            position INTEGER NOT NULL DEFAULT 0,
            provider_id TEXT NOT NULL REFERENCES user_providers(id) ON DELETE CASCADE,
            credential_kind TEXT NOT NULL CHECK(credential_kind IN ('api_key', 'sub2api_pool')),
            credential_id TEXT,
            model_id TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_provider_route_bindings_position
            ON provider_route_bindings(position);
        INSERT OR IGNORE INTO provider_routing_settings (id, updated_at) VALUES (1, datetime('now'));
        ",
    )
    .map_err(Error::Database)?;

    // Repair path for v10 tables when a database carries an advanced marker.
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS creative_drafts (
            draft_id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            intent TEXT NOT NULL,
            conversation_id TEXT,
            origin_module_id TEXT,
            current_revision INTEGER NOT NULL DEFAULT 0,
            state TEXT NOT NULL DEFAULT 'drafting'
                CHECK(state IN ('drafting','generating','ready','publishing','published','archived')),
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS creative_draft_revisions (
            draft_id TEXT NOT NULL REFERENCES creative_drafts(draft_id) ON DELETE CASCADE,
            revision INTEGER NOT NULL,
            content_hash TEXT NOT NULL,
            created_at TEXT NOT NULL,
            PRIMARY KEY (draft_id, revision)
        );
        CREATE INDEX IF NOT EXISTS idx_creative_drafts_state
            ON creative_drafts(state);
        CREATE INDEX IF NOT EXISTS idx_creative_drafts_conversation
            ON creative_drafts(conversation_id);
        ",
    )
    .map_err(Error::Database)?;

    Ok(())
}

/// Idempotent backfill of the three existing sources into the unified
/// `applications` identity, `startup_plans`, and `runtime_instances` for rows
/// that are currently non-terminal. Safe to run repeatedly (INSERT OR IGNORE +
/// deterministic ids); extracted so tests can prove idempotency.
pub(crate) fn backfill_creative_identity(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        INSERT OR IGNORE INTO applications (id, source, source_id, title, description, icon, version, created_at, updated_at)
        SELECT 'app-internal-' || id, 'internal', id, COALESCE(name, id), description, icon,
               COALESCE(NULLIF(version, ''), '1'),
               COALESCE(created_at, datetime('now')), COALESCE(updated_at, datetime('now'))
        FROM modules;

        INSERT OR IGNORE INTO applications (id, source, source_id, title, description, icon, version, created_at, updated_at)
        SELECT 'app-external-' || id, 'external_github', id, COALESCE(title, id), description, icon,
               COALESCE(NULLIF(version, ''), '1'),
               COALESCE(created_at, datetime('now')), COALESCE(updated_at, datetime('now'))
        FROM external_creative_apps;

        INSERT OR IGNORE INTO applications (id, source, source_id, title, description, icon, version, created_at, updated_at)
        SELECT 'app-local-' || id, 'local_project', id, COALESCE(title, id), description, icon, '1',
               COALESCE(created_at, datetime('now')), COALESCE(updated_at, datetime('now'))
        FROM local_creative_apps;

        INSERT OR IGNORE INTO startup_plans (id, application_id, plan_version, plan_json, is_active, created_at, updated_at)
        SELECT 'plan-local-' || a.id, a.id, 1, l.launch_plan_json, 1,
               COALESCE(l.created_at, datetime('now')), COALESCE(l.updated_at, datetime('now'))
        FROM local_creative_apps l
        JOIN applications a ON a.source = 'local_project' AND a.source_id = l.id;

        INSERT OR IGNORE INTO startup_plans (id, application_id, plan_version, plan_json, is_active, created_at, updated_at)
        SELECT 'plan-external-' || a.id, a.id, 1, e.runtime_config_json, 1,
               COALESCE(e.created_at, datetime('now')), COALESCE(e.updated_at, datetime('now'))
        FROM external_creative_apps e
        JOIN applications a ON a.source = 'external_github' AND a.source_id = e.id;
        ",
    )
    .map_err(Error::Database)?;

    // Backfill runtime_instances for rows that are currently non-terminal, so
    // every active resource is traceable to one instance even after migration.
    {
        let now = chrono::Utc::now().to_rfc3339();
        let mut stmt = conn
            .prepare(
                "SELECT l.id, l.state, l.current_port, l.open_url, l.process_identity_json,
                        l.launch_plan_json
                 FROM local_creative_apps l
                 JOIN applications a ON a.source = 'local_project' AND a.source_id = l.id",
            )
            .map_err(Error::Database)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })
            .map_err(Error::Database)?;
        for r in rows {
            let (source_id, state, port, open_url, ident_json, plan_json) =
                r.map_err(Error::Database)?;
            let Some(status) = instance_status_for_state(&state) else {
                continue;
            };
            let application_id = conn
                .query_row(
                    "SELECT id FROM applications WHERE source = 'local_project' AND source_id = ?1",
                    params![source_id],
                    |row| row.get::<_, String>(0),
                )
                .map_err(Error::Database)?;
            let (pgid, pid) = parse_identity(ident_json.as_deref());
            let plan_id: Option<String> = conn
                .query_row(
                    "SELECT id FROM startup_plans WHERE application_id = ?1 AND is_active = 1 LIMIT 1",
                    params![application_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(Error::Database)?;
            let runtime = serde_json::from_str::<serde_json::Value>(&plan_json)
                .ok()
                .and_then(|v| {
                    v.get("runtime")
                        .and_then(|r| r.as_str())
                        .map(str::to_string)
                });
            let owner_kind = match runtime.as_deref() {
                Some("static_http") => "host_http",
                // Local Compose plans own a docker_compose project, NOT a local
                // process (batch 1 CR-103 fixes the earlier misclassification).
                Some("docker_compose") => "docker_compose",
                _ => "local_process",
            };
            let urls = open_url
                .as_deref()
                .map(|u| serde_json::json!([u]).to_string());
            conn.execute(
                "INSERT OR IGNORE INTO runtime_instances
                    (id, application_id, plan_id, status, cleanup_status, owner_kind, pgid, compose_project,
                     resolved_urls_json, current_port, pid, failure, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6, NULL, ?7, ?8, ?9, NULL, ?10, ?10)",
                params![
                    format!("ri-backfill-{application_id}"),
                    application_id,
                    plan_id,
                    status,
                    owner_kind,
                    pgid,
                    urls,
                    port,
                    pid,
                    now,
                ],
            )
            .map_err(Error::Database)?;
        }
    }
    {
        let now = chrono::Utc::now().to_rfc3339();
        let mut stmt = conn
            .prepare(
                "SELECT e.id, e.state, e.host_port, e.open_url, e.runtime_config_json
                 FROM external_creative_apps e
                 JOIN applications a ON a.source = 'external_github' AND a.source_id = e.id",
            )
            .map_err(Error::Database)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })
            .map_err(Error::Database)?;
        for r in rows {
            let (source_id, state, port, open_url, cfg_json) = r.map_err(Error::Database)?;
            let Some(status) = instance_status_for_state(&state) else {
                continue;
            };
            let application_id = conn
                .query_row(
                    "SELECT id FROM applications WHERE source = 'external_github' AND source_id = ?1",
                    params![source_id],
                    |row| row.get::<_, String>(0),
                )
                .map_err(Error::Database)?;
            let plan_id: Option<String> = conn
                .query_row(
                    "SELECT id FROM startup_plans WHERE application_id = ?1 AND is_active = 1 LIMIT 1",
                    params![application_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(Error::Database)?;
            let owner_kind = if serde_json::from_str::<serde_json::Value>(&cfg_json)
                .ok()
                .and_then(|v| v.get("kind").and_then(|k| k.as_str()).map(str::to_string))
                .as_deref()
                == Some("docker_compose")
            {
                "docker_compose"
            } else {
                "docker_run"
            };
            let urls = open_url
                .as_deref()
                .map(|u| serde_json::json!([u]).to_string());
            conn.execute(
                "INSERT OR IGNORE INTO runtime_instances
                    (id, application_id, plan_id, status, cleanup_status, owner_kind, pgid, compose_project,
                     resolved_urls_json, current_port, pid, failure, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, NULL, ?5, NULL, NULL, ?6, ?7, NULL, NULL, ?8, ?8)",
                params![
                    format!("ri-backfill-{application_id}"),
                    application_id,
                    plan_id,
                    status,
                    owner_kind,
                    urls,
                    port,
                    now,
                ],
            )
            .map_err(Error::Database)?;
        }
    }
    Ok(())
}

/// Insert an immutable audit/report row for a creative identity repair.
/// Deterministic `id` keeps re-runs idempotent (INSERT OR IGNORE).
#[allow(clippy::too_many_arguments)] // pre-existing parameter list
fn insert_identity_report(
    conn: &Connection,
    id: &str,
    kind: &str,
    application_id: Option<&str>,
    source: Option<&str>,
    source_id: Option<&str>,
    action: &str,
    payload_json: &str,
    ts: &str,
) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO creative_identity_reports
            (id, kind, application_id, source, source_id, action, payload_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            id,
            kind,
            application_id,
            source,
            source_id,
            action,
            payload_json,
            ts
        ],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// Batch 1 CR-101: delete confirmed ghost `applications` rows and audit the rest.
///
/// A ghost is an applications row whose source detail row is gone (no matching
/// row in `modules` / `external_creative_apps` / `local_creative_apps`). We only
/// delete ghosts with no dependent `startup_plans` / `runtime_instances` /
/// `preview_targets` (double gate per upgrade plan T01); every deletion backs
/// up the full row JSON into `creative_identity_reports`. Ghosts that still
/// carry dependent data are quarantined (reported, kept), and a ghost whose
/// source_id collides with a real row of another source is also reported.
/// Idempotent: after the first run the deletable set is empty.
pub(crate) fn repair_creative_identity_ghosts(conn: &Connection) -> Result<usize> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS creative_identity_reports (
            id TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            application_id TEXT,
            source TEXT,
            source_id TEXT,
            action TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        ",
    )
    .map_err(Error::Database)?;
    let ts = chrono::Utc::now().to_rfc3339();

    let candidates: Vec<(String, String, String)> = {
        let mut stmt = conn
            .prepare(
                "SELECT a.id, a.source, a.source_id
                 FROM applications a
                 WHERE NOT EXISTS (SELECT 1 FROM modules m WHERE m.id = a.source_id AND a.source = 'internal')
                   AND NOT EXISTS (SELECT 1 FROM external_creative_apps e WHERE e.id = a.source_id AND a.source = 'external_github')
                   AND NOT EXISTS (SELECT 1 FROM local_creative_apps l WHERE l.id = a.source_id AND a.source = 'local_project')",
            )
            .map_err(Error::Database)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map_err(Error::Database)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(Error::Database)?);
        }
        out
    };

    let mut deleted = 0usize;
    for (id, source, source_id) in candidates {
        let row_json: String = conn
            .query_row(
                "SELECT json_object('id', id, 'source', source, 'source_id', source_id,
                                    'title', title, 'version', version,
                                    'created_at', created_at, 'updated_at', updated_at)
                 FROM applications WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .map_err(Error::Database)?;
        // Double gate: dependent rows must be empty before we may delete.
        let has_deps: i64 = conn
            .query_row(
                "SELECT
                    (SELECT COUNT(*) FROM startup_plans sp WHERE sp.application_id = ?1)
                  + (SELECT COUNT(*) FROM runtime_instances ri WHERE ri.application_id = ?1)
                  + (SELECT COUNT(*) FROM preview_targets pt
                        JOIN runtime_instances ri2 ON ri2.id = pt.runtime_instance_id
                     WHERE ri2.application_id = ?1)",
                params![id],
                |r| r.get(0),
            )
            .map_err(Error::Database)?;
        if has_deps > 0 {
            insert_identity_report(
                conn,
                &format!("ghost-quarantine-{id}"),
                "ghost_application_with_dependencies",
                Some(&id),
                Some(&source),
                Some(&source_id),
                "quarantined",
                &row_json,
                &ts,
            )?;
            continue;
        }
        // Cross-source collision: the same source_id exists as a REAL row in a
        // different source table. The real row lives in its own table, so
        // deletion is still safe, but the collision is worth an audit record.
        let cross: i64 = conn
            .query_row(
                "SELECT
                    (SELECT COUNT(*) FROM modules m WHERE m.id = ?2 AND ?1 <> 'internal')
                  + (SELECT COUNT(*) FROM external_creative_apps e WHERE e.id = ?2 AND ?1 <> 'external_github')
                  + (SELECT COUNT(*) FROM local_creative_apps l WHERE l.id = ?2 AND ?1 <> 'local_project')",
                params![source, source_id],
                |r| r.get(0),
            )
            .map_err(Error::Database)?;
        if cross > 0 {
            insert_identity_report(
                conn,
                &format!("collision-{id}"),
                "cross_source_collision",
                Some(&id),
                Some(&source),
                Some(&source_id),
                "reported",
                &row_json,
                &ts,
            )?;
        }
        insert_identity_report(
            conn,
            &format!("ghost-{id}"),
            "ghost_application",
            Some(&id),
            Some(&source),
            Some(&source_id),
            "deleted",
            &row_json,
            &ts,
        )?;
        conn.execute("DELETE FROM applications WHERE id = ?1", params![id])
            .map_err(Error::Database)?;
        deleted += 1;
    }
    Ok(deleted)
}

/// Batch 1 CR-102: reconcile duplicate active runtime/plan rows, then create the
/// partial unique indexes that make "one active per application" a DB invariant.
///
/// Duplicates are NOT deleted and NOT silently marked stopped (invariant #8):
/// the newest active row is kept and older runtime duplicates are demoted to
/// `orphaned` (outside the index scope), while older plan duplicates get
/// `is_active=0`. Every demotion is audited in `creative_identity_reports`.
/// Idempotent: after the first run there are no duplicates left to demote.
pub(crate) fn repair_creative_active_invariants(conn: &Connection) -> Result<usize> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS creative_identity_reports (
            id TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            application_id TEXT,
            source TEXT,
            source_id TEXT,
            action TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        ",
    )
    .map_err(Error::Database)?;
    let ts = chrono::Utc::now().to_rfc3339();
    let mut fixed = 0usize;

    // 1) One active startup_plan per application.
    let dup_plan_apps: Vec<String> = {
        let mut stmt = conn
            .prepare(
                "SELECT application_id FROM startup_plans WHERE is_active = 1
                 GROUP BY application_id HAVING COUNT(*) > 1",
            )
            .map_err(Error::Database)?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(Error::Database)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::Database)?
    };
    for app in dup_plan_apps {
        let ids: Vec<String> = {
            let mut stmt = conn
                .prepare(
                    "SELECT id FROM startup_plans
                     WHERE application_id = ?1 AND is_active = 1
                     ORDER BY updated_at DESC, id",
                )
                .map_err(Error::Database)?;
            let rows = stmt
                .query_map(params![app], |r| r.get::<_, String>(0))
                .map_err(Error::Database)?;
            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(Error::Database)?
        };
        for id in ids.into_iter().skip(1) {
            let row_json: String = conn
                .query_row(
                    "SELECT json_object('id', id, 'application_id', application_id,
                                        'plan_version', plan_version, 'is_active', is_active,
                                        'updated_at', updated_at)
                     FROM startup_plans WHERE id = ?1",
                    params![id],
                    |r| r.get(0),
                )
                .map_err(Error::Database)?;
            conn.execute(
                "UPDATE startup_plans SET is_active = 0, updated_at = ?2 WHERE id = ?1",
                params![id, ts],
            )
            .map_err(Error::Database)?;
            insert_identity_report(
                conn,
                &format!("plan-dedup-{id}"),
                "duplicate_active_plan",
                Some(&app),
                None,
                None,
                "demoted",
                &row_json,
                &ts,
            )?;
            fixed += 1;
        }
    }

    // 2) One active runtime_instance per application (over the index scope).
    let dup_runtime_apps: Vec<String> = {
        let mut stmt = conn
            .prepare(
                "SELECT application_id FROM runtime_instances
                 WHERE status IN ('starting','running','stopping')
                 GROUP BY application_id HAVING COUNT(*) > 1",
            )
            .map_err(Error::Database)?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(Error::Database)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::Database)?
    };
    for app in dup_runtime_apps {
        let ids: Vec<String> = {
            let mut stmt = conn
                .prepare(
                    "SELECT id FROM runtime_instances
                     WHERE application_id = ?1 AND status IN ('starting','running','stopping')
                     ORDER BY updated_at DESC, id",
                )
                .map_err(Error::Database)?;
            let rows = stmt
                .query_map(params![app], |r| r.get::<_, String>(0))
                .map_err(Error::Database)?;
            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(Error::Database)?
        };
        for id in ids.into_iter().skip(1) {
            let row_json: String = conn
                .query_row(
                    "SELECT json_object('id', id, 'application_id', application_id,
                                        'status', status, 'owner_kind', owner_kind,
                                        'updated_at', updated_at)
                     FROM runtime_instances WHERE id = ?1",
                    params![id],
                    |r| r.get(0),
                )
                .map_err(Error::Database)?;
            // Demote to orphaned (NOT stopped — the resource is unproven; see
            // invariant #8). Orphaned is outside the index scope so the partial
            // unique index below remains satisfiable.
            conn.execute(
                "UPDATE runtime_instances
                 SET status = 'orphaned', cleanup_status = NULL, updated_at = ?2
                 WHERE id = ?1",
                params![id, ts],
            )
            .map_err(Error::Database)?;
            insert_identity_report(
                conn,
                &format!("runtime-dedup-{id}"),
                "duplicate_active_runtime",
                Some(&app),
                None,
                None,
                "orphaned",
                &row_json,
                &ts,
            )?;
            fixed += 1;
        }
    }

    // 3) Partial unique indexes — safe now that duplicates are gone.
    conn.execute_batch(
        "
        CREATE UNIQUE INDEX IF NOT EXISTS idx_runtime_instances_one_active
            ON runtime_instances(application_id)
            WHERE status IN ('starting','running','stopping');
        CREATE UNIQUE INDEX IF NOT EXISTS idx_startup_plans_one_active
            ON startup_plans(application_id) WHERE is_active = 1;
        ",
    )
    .map_err(Error::Database)?;

    Ok(fixed)
}

/// Derive a LaunchProfile `driver_kind` from a stored plan JSON. Handles the
/// local LaunchPlan shape (`runtime`, camelCase) and the external RuntimeConfig
/// shape (`kind`, snake_case). Kept self-contained because migrations must run
/// against historical schemas without depending on domain modules.
fn plan_driver_kind_from_json(json: &str) -> &'static str {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return "unknown";
    };
    if let Some(r) = v.get("runtime").and_then(|r| r.as_str()) {
        return match r {
            "static_http" => "local_static",
            "docker_compose" => "docker_compose",
            _ => "node_dev_server",
        };
    }
    if let Some(k) = v.get("kind").and_then(|k| k.as_str()) {
        return if k == "docker_compose" {
            "docker_compose"
        } else {
            "docker_run"
        };
    }
    "unknown"
}

/// Batch 1 CR-103: promote `startup_plans` to the versioned LaunchProfile base.
///
/// Adds nullable `schema_version` / `driver_kind` / `ownership_mode` columns
/// (read-upgrader derives them from plan_json when NULL), backfills existing
/// rows, and repairs the Compose backfill that misclassified a local
/// docker_compose instance as `local_process`. Additive and idempotent: the
/// column adds are PRAGMA-guarded and the backfill only touches NULL rows.
pub(crate) fn upgrade_startup_plans_v1(conn: &Connection) -> Result<usize> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS creative_identity_reports (
            id TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            application_id TEXT,
            source TEXT,
            source_id TEXT,
            action TEXT NOT NULL,
            payload_json TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        ",
    )
    .map_err(Error::Database)?;

    // 1) Add the versioned columns (guarded so re-running is safe).
    let plan_cols: Vec<String> = {
        let mut stmt = conn
            .prepare("PRAGMA table_info(startup_plans)")
            .map_err(Error::Database)?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(1))
            .map_err(Error::Database)?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(Error::Database)?
    };
    if !plan_cols.iter().any(|c| c == "schema_version") {
        conn.execute(
            "ALTER TABLE startup_plans ADD COLUMN schema_version INTEGER",
            [],
        )
        .map_err(Error::Database)?;
    }
    if !plan_cols.iter().any(|c| c == "driver_kind") {
        conn.execute("ALTER TABLE startup_plans ADD COLUMN driver_kind TEXT", [])
            .map_err(Error::Database)?;
    }
    if !plan_cols.iter().any(|c| c == "ownership_mode") {
        conn.execute(
            "ALTER TABLE startup_plans ADD COLUMN ownership_mode TEXT",
            [],
        )
        .map_err(Error::Database)?;
    }

    // 2) Backfill columns for legacy rows (only those still missing them).
    let ts = chrono::Utc::now().to_rfc3339();
    let mut fixed = 0usize;
    let missing: Vec<(String, String)> = {
        let mut stmt = conn
            .prepare(
                "SELECT id, plan_json FROM startup_plans
                 WHERE schema_version IS NULL OR driver_kind IS NULL OR ownership_mode IS NULL",
            )
            .map_err(Error::Database)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(Error::Database)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(Error::Database)?);
        }
        out
    };
    for (id, plan_json) in missing {
        let dk = plan_driver_kind_from_json(&plan_json);
        let sv = serde_json::from_str::<serde_json::Value>(&plan_json)
            .ok()
            .and_then(|v| v.get("schemaVersion").and_then(|s| s.as_u64()))
            .unwrap_or(1);
        conn.execute(
            "UPDATE startup_plans
             SET schema_version = ?2, driver_kind = ?3, ownership_mode = 'managed', updated_at = ?4
             WHERE id = ?1",
            params![id, sv as i64, dk, ts],
        )
        .map_err(Error::Database)?;
        insert_identity_report(
            conn,
            &format!("plan-upgrade-{id}"),
            "plan_versioned_columns",
            None,
            None,
            None,
            "backfilled",
            &serde_json::json!({ "driverKind": dk, "schemaVersion": sv }).to_string(),
            &ts,
        )?;
        fixed += 1;
    }

    // 3) Repair the earlier Compose backfill that classified local docker_compose
    //    instances as `local_process` (#34).
    let misclassified: Vec<(String, String, String)> = {
        let mut stmt = conn
            .prepare(
                "SELECT ri.id, ri.application_id, l.launch_plan_json
                 FROM runtime_instances ri
                 JOIN applications a ON a.id = ri.application_id
                 JOIN local_creative_apps l ON l.id = a.source_id AND a.source = 'local_project'
                 WHERE ri.owner_kind = 'local_process'
                   AND json_extract(l.launch_plan_json, '$.runtime') = 'docker_compose'",
            )
            .map_err(Error::Database)?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map_err(Error::Database)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(Error::Database)?);
        }
        out
    };
    for (rid, app_id, plan_json) in misclassified {
        conn.execute(
            "UPDATE runtime_instances SET owner_kind = 'docker_compose', updated_at = ?2 WHERE id = ?1",
            params![rid, ts],
        )
        .map_err(Error::Database)?;
        insert_identity_report(
            conn,
            &format!("owner-repair-{rid}"),
            "plan_owner_repaired",
            Some(&app_id),
            None,
            None,
            "repaired",
            &serde_json::json!({ "instanceId": rid, "driverKind": plan_driver_kind_from_json(&plan_json) })
                .to_string(),
            &ts,
        )?;
        fixed += 1;
    }

    Ok(fixed)
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)] // 该文件 test 模块后仍有生产 CRUD 代码（历史结构）
mod tests {
    use super::*;

    #[test]
    fn repairs_snapshot_table_when_schema_marker_is_already_newer() {
        let conn = Connection::open_in_memory().expect("open in-memory database");
        create_tables(&conn).expect("create base tables");
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES ('_schema_version', '8')",
            [],
        )
        .expect("set newer schema marker");

        apply_migrations(&conn).expect("repair migrations");

        let table_exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'usage_dashboard_snapshots'",
                [],
                |row| row.get(0),
            )
            .expect("query sqlite schema");
        assert_eq!(table_exists, 1);

        let local_exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'local_creative_apps'",
                [],
                |row| row.get(0),
            )
            .expect("query local_creative_apps");
        assert_eq!(local_exists, 1);
    }

    /// Batch 1: the three-source backfill into applications / startup_plans /
    /// runtime_instances is idempotent and covers active rows.
    #[test]
    fn creative_identity_backfill_is_idempotent() {
        let conn = Connection::open_in_memory().expect("open in-memory database");
        create_tables(&conn).expect("create base tables");
        apply_migrations(&conn).expect("apply migrations");

        conn.execute(
            "INSERT INTO modules (id, name, version, entry, type, enabled, state, created_at, updated_at)
             VALUES ('mod1', 'M', '1', 'index.html', 'web-module', 1, 'installed', 't', 't')",
            [],
        )
        .expect("insert module");
        conn.execute(
            "INSERT INTO local_creative_apps
                (id, title, canonical_project_root, device_id, device_name, project_kind, launch_mode,
                 launch_plan_json, plan_fingerprint, state, current_port, open_url, process_identity_json,
                 startup_timeout_ms, created_at, updated_at)
             VALUES ('loc1', 'Local', '/tmp/x', 'd', 'n', 'html', 'smart',
                     '{\"schemaVersion\":1,\"runtime\":\"static_http\",\"program\":\"internal\"}', 'fp',
                     'running', 5173, 'http://127.0.0.1:5173/', '{\"pid\":100,\"processGroupId\":100}',
                     60000, 't', 't')",
            [],
        )
        .expect("insert local app");
        conn.execute(
            "INSERT INTO external_creative_apps
                (id, title, version, owner, repo, repository_url, release_tag, runtime, state, host_port,
                 open_url, runtime_config_json, created_at, updated_at)
             VALUES ('ext1', 'Ext', '1', 'o', 'r', 'http://x', 'v1', 'docker_compose', 'running', 8080,
                     'http://127.0.0.1:8080/',
                     '{\"kind\":\"docker_compose\",\"projectName\":\"natives-ext1\",\"composeFile\":\"/tmp/c.yml\",\"service\":\"web\",\"containerPort\":80,\"hostPort\":8080,\"openPath\":\"/\"}',
                     't', 't')",
            [],
        )
        .expect("insert external app");

        backfill_creative_identity(&conn).expect("first backfill");
        backfill_creative_identity(&conn).expect("second backfill (idempotency)");

        let count = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).expect("count") };
        assert_eq!(count("SELECT COUNT(*) FROM applications"), 3);
        assert_eq!(count("SELECT COUNT(*) FROM startup_plans"), 2);
        assert_eq!(count("SELECT COUNT(*) FROM runtime_instances"), 2);

        // The running local app got a real instance with owner_kind/port/pgid.
        let (status, kind, port, pgid): (String, String, Option<i64>, Option<i64>) = conn
            .query_row(
                "SELECT ri.status, ri.owner_kind, ri.current_port, ri.pgid
                 FROM runtime_instances ri
                 JOIN applications a ON a.id = ri.application_id
                 WHERE a.source_id = 'loc1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .expect("local instance");
        assert_eq!(status, "running");
        assert_eq!(kind, "host_http");
        assert_eq!(port, Some(5173));
        assert_eq!(pgid, Some(100));

        // The external running app got a docker_compose instance.
        let (status, kind): (String, String) = conn
            .query_row(
                "SELECT ri.status, ri.owner_kind
                 FROM runtime_instances ri
                 JOIN applications a ON a.id = ri.application_id
                 WHERE a.source_id = 'ext1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("external instance");
        assert_eq!(status, "running");
        assert_eq!(kind, "docker_compose");

        // Re-running again must not create duplicates.
        backfill_creative_identity(&conn).expect("third backfill");
        assert_eq!(count("SELECT COUNT(*) FROM applications"), 3);
        assert_eq!(count("SELECT COUNT(*) FROM runtime_instances"), 2);
    }

    /// Batch 1 CR-101: a ghost application (no source row, no dependent rows)
    /// is deleted and its full row JSON is backed up to the report table.
    #[test]
    fn repair_ghost_identity_deletes_and_backs_up() {
        let conn = Connection::open_in_memory().expect("open in-memory database");
        create_tables(&conn).expect("create base tables");
        apply_migrations(&conn).expect("apply migrations");

        // A fake local_project identity the old browser show path would fabricate
        // for a GitHub app (no matching source row anywhere).
        conn.execute(
            "INSERT INTO applications (id, source, source_id, title, version, created_at, updated_at)
             VALUES ('ghost-1', 'local_project', 'ext-ghost', 'Ghost', '1', 't', 't')",
            [],
        )
        .expect("insert ghost");

        let deleted = repair_creative_identity_ghosts(&conn).expect("repair");
        assert_eq!(deleted, 1);

        let remaining: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM applications WHERE id = 'ghost-1'",
                [],
                |r| r.get(0),
            )
            .expect("count");
        assert_eq!(remaining, 0);

        // The row JSON was backed up with an audit record.
        let (kind, action, payload): (String, String, String) = conn
            .query_row(
                "SELECT kind, action, payload_json FROM creative_identity_reports WHERE id = 'ghost-ghost-1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .expect("report row");
        assert_eq!(kind, "ghost_application");
        assert_eq!(action, "deleted");
        assert!(payload.contains("ext-ghost"));

        // Idempotent: a second run finds nothing new and adds no duplicate rows.
        let deleted2 = repair_creative_identity_ghosts(&conn).expect("repair again");
        assert_eq!(deleted2, 0);
        let reports: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM creative_identity_reports WHERE kind = 'ghost_application'",
                [],
                |r| r.get(0),
            )
            .expect("count reports");
        assert_eq!(reports, 1);
    }

    /// Batch 1 CR-101: a ghost with dependent rows is quarantined, never deleted.
    #[test]
    fn repair_quarantines_ghost_with_dependencies() {
        let conn = Connection::open_in_memory().expect("open in-memory database");
        create_tables(&conn).expect("create base tables");
        apply_migrations(&conn).expect("apply migrations");

        conn.execute(
            "INSERT INTO applications (id, source, source_id, title, version, created_at, updated_at)
             VALUES ('ghost-2', 'local_project', 'ext-ghost', 'Ghost', '1', 't', 't')",
            [],
        )
        .expect("insert ghost");
        // Dependent startup_plan keeps the row from being deletable.
        conn.execute(
            "INSERT INTO startup_plans (id, application_id, plan_version, plan_json, is_active, created_at, updated_at)
             VALUES ('plan-ghost', 'ghost-2', 1, '{}', 1, 't', 't')",
            [],
        )
        .expect("insert dependent plan");

        let deleted = repair_creative_identity_ghosts(&conn).expect("repair");
        assert_eq!(deleted, 0, "ghost with dependencies must not be deleted");
        let remaining: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM applications WHERE id = 'ghost-2'",
                [],
                |r| r.get(0),
            )
            .expect("count");
        assert_eq!(remaining, 1);
        let quarantined: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM creative_identity_reports
                 WHERE kind = 'ghost_application_with_dependencies' AND action = 'quarantined'",
                [],
                |r| r.get(0),
            )
            .expect("count reports");
        assert_eq!(quarantined, 1);
    }

    /// Batch 1 CR-101: a ghost whose source_id collides with a real row of a
    /// different source is deleted (the real row lives in its own table) but the
    /// collision is recorded for audit.
    #[test]
    fn repair_reports_cross_source_collision() {
        let conn = Connection::open_in_memory().expect("open in-memory database");
        create_tables(&conn).expect("create base tables");
        apply_migrations(&conn).expect("apply migrations");

        // A real external GitHub app.
        conn.execute(
            "INSERT INTO external_creative_apps
                (id, title, version, owner, repo, repository_url, release_tag, runtime, state, runtime_config_json, created_at, updated_at)
             VALUES ('ext-1', 'Ext', '1', 'o', 'r', 'http://x', 'v1', 'docker_compose', 'installed_stopped', '{}', 't', 't')",
            [],
        )
        .expect("insert external");
        // The ghost local_project identity the old browser path created for it.
        conn.execute(
            "INSERT INTO applications (id, source, source_id, title, version, created_at, updated_at)
             VALUES ('ghost-3', 'local_project', 'ext-1', 'Ghost', '1', 't', 't')",
            [],
        )
        .expect("insert ghost");

        let deleted = repair_creative_identity_ghosts(&conn).expect("repair");
        assert_eq!(deleted, 1);
        let collision: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM creative_identity_reports WHERE kind = 'cross_source_collision'",
                [],
                |r| r.get(0),
            )
            .expect("count reports");
        assert_eq!(collision, 1);
        // The real external app row is untouched.
        let ext: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM external_creative_apps WHERE id = 'ext-1'",
                [],
                |r| r.get(0),
            )
            .expect("count");
        assert_eq!(ext, 1);
    }

    /// Batch 1 CR-102: the v16 migration leaves the two active unique indexes in
    /// place so a second active instance / plan is a DB-level violation.
    #[test]
    fn migration_creates_active_unique_indexes() {
        let conn = Connection::open_in_memory().expect("open in-memory database");
        create_tables(&conn).expect("create base tables");
        apply_migrations(&conn).expect("apply migrations");

        let index = |name: &str| -> i64 {
            conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name=?1",
                rusqlite::params![name],
                |r| r.get(0),
            )
            .expect("index exists")
        };
        assert_eq!(index("idx_runtime_instances_one_active"), 1);
        assert_eq!(index("idx_startup_plans_one_active"), 1);

        conn.execute(
            "INSERT INTO applications (id, source, source_id, title, version, created_at, updated_at)
             VALUES ('app-1', 'local_project', 'loc1', 'L', '1', 't', 't')",
            [],
        )
        .expect("insert app");
        conn.execute(
            "INSERT INTO runtime_instances (id, application_id, status, owner_kind, created_at, updated_at)
             VALUES ('ri-1', 'app-1', 'starting', 'local_process', 't', 't')",
            [],
        )
        .expect("insert active instance");
        // A second active instance violates the partial unique index.
        let err = conn
            .execute(
                "INSERT INTO runtime_instances (id, application_id, status, owner_kind, created_at, updated_at)
                 VALUES ('ri-2', 'app-1', 'starting', 'local_process', 'u', 'u')",
                [],
            )
            .expect_err("second active instance must be rejected by the DB");
        assert!(
            matches!(&err, rusqlite::Error::SqliteFailure(f, _)
                if f.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE),
            "expected UNIQUE constraint violation, got {err:?}"
        );

        // Same for active plans.
        conn.execute(
            "INSERT INTO startup_plans (id, application_id, plan_version, plan_json, is_active, created_at, updated_at)
             VALUES ('plan-1', 'app-1', 1, '{}', 1, 't', 't')",
            [],
        )
        .expect("insert active plan");
        let err = conn
            .execute(
                "INSERT INTO startup_plans (id, application_id, plan_version, plan_json, is_active, created_at, updated_at)
                 VALUES ('plan-2', 'app-1', 1, '{}', 1, 'u', 'u')",
                [],
            )
            .expect_err("second active plan must be rejected by the DB");
        assert!(matches!(&err, rusqlite::Error::SqliteFailure(..)));
    }

    /// Batch 1 CR-102: the repair reconciles pre-existing duplicate active rows
    /// (kept newest, demoted rest audited) before the indexes are recreated, and
    /// never fabricates a stopped status for an unproven resource.
    #[test]
    fn repair_active_invariants_dedups_duplicates() {
        let conn = Connection::open_in_memory().expect("open in-memory database");
        create_tables(&conn).expect("create base tables");
        apply_migrations(&conn).expect("apply migrations");

        // Simulate a pre-v16 database: no active unique indexes yet.
        conn.execute("DROP INDEX idx_runtime_instances_one_active", [])
            .expect("drop runtime index");
        conn.execute("DROP INDEX idx_startup_plans_one_active", [])
            .expect("drop plan index");
        conn.execute(
            "INSERT INTO applications (id, source, source_id, title, version, created_at, updated_at)
             VALUES ('app-1', 'local_project', 'loc1', 'L', '1', 't', 't')",
            [],
        )
        .expect("insert app");
        // Two active instances + two active plans for the same app.
        conn.execute(
            "INSERT INTO runtime_instances (id, application_id, status, owner_kind, created_at, updated_at)
             VALUES ('ri-old', 'app-1', 'running', 'local_process', 't', 't')",
            [],
        )
        .expect("insert older running instance");
        conn.execute(
            "INSERT INTO runtime_instances (id, application_id, status, owner_kind, created_at, updated_at)
             VALUES ('ri-new', 'app-1', 'starting', 'local_process', 'u', 'u')",
            [],
        )
        .expect("insert newer starting instance");
        conn.execute(
            "INSERT INTO startup_plans (id, application_id, plan_version, plan_json, is_active, created_at, updated_at)
             VALUES ('plan-old', 'app-1', 1, '{}', 1, 't', 't')",
            [],
        )
        .expect("insert older active plan");
        conn.execute(
            "INSERT INTO startup_plans (id, application_id, plan_version, plan_json, is_active, created_at, updated_at)
             VALUES ('plan-new', 'app-1', 1, '{}', 1, 'u', 'u')",
            [],
        )
        .expect("insert newer active plan");

        let fixed = repair_creative_active_invariants(&conn).expect("repair");
        assert_eq!(fixed, 2, "one runtime + one plan duplicate demoted");

        // The newest active rows were kept; the older ones were demoted (not
        // deleted, not fabricated as stopped).
        let kept_runtime: String = conn
            .query_row(
                "SELECT id FROM runtime_instances WHERE application_id='app-1'
                 AND status IN ('starting','running','stopping')",
                [],
                |r| r.get(0),
            )
            .expect("kept runtime");
        assert_eq!(kept_runtime, "ri-new");
        let old_runtime_status: String = conn
            .query_row(
                "SELECT status FROM runtime_instances WHERE id='ri-old'",
                [],
                |r| r.get(0),
            )
            .expect("old runtime status");
        assert_eq!(
            old_runtime_status, "orphaned",
            "unproven duplicate must be orphaned, never fabricated stopped"
        );
        let kept_plan: String = conn
            .query_row(
                "SELECT id FROM startup_plans WHERE application_id='app-1' AND is_active=1",
                [],
                |r| r.get(0),
            )
            .expect("kept plan");
        assert_eq!(kept_plan, "plan-new");
        let old_plan_active: i64 = conn
            .query_row(
                "SELECT is_active FROM startup_plans WHERE id='plan-old'",
                [],
                |r| r.get(0),
            )
            .expect("old plan active");
        assert_eq!(old_plan_active, 0);

        // The indexes were recreated and now reject a second active instance.
        let err = conn
            .execute(
                "INSERT INTO runtime_instances (id, application_id, status, owner_kind, created_at, updated_at)
                 VALUES ('ri-3', 'app-1', 'starting', 'local_process', 'x', 'x')",
                [],
            )
            .expect_err("index must reject a second active instance");
        assert!(matches!(&err, rusqlite::Error::SqliteFailure(..)));

        // Idempotent re-run: no further demotions, no duplicate report rows.
        let fixed2 = repair_creative_active_invariants(&conn).expect("repair again");
        assert_eq!(fixed2, 0);
        let reports: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM creative_identity_reports WHERE kind IN
                 ('duplicate_active_runtime','duplicate_active_plan')",
                [],
                |r| r.get(0),
            )
            .expect("count reports");
        assert_eq!(reports, 2);
    }

    /// Batch 1 CR-103: migration v17 backfills the LaunchProfile v1 columns from
    /// stored plan JSON and repairs the Compose backfill that classified a local
    /// docker_compose instance as `local_process` (#34).
    #[test]
    fn upgrade_startup_plans_v1_backfills_and_repairs_compose_owner() {
        let conn = Connection::open_in_memory().expect("open in-memory database");
        create_tables(&conn).expect("create base tables");
        apply_migrations(&conn).expect("apply migrations");

        // A local compose app whose backfilled instance was misclassified.
        conn.execute(
            "INSERT INTO local_creative_apps
                (id, title, canonical_project_root, device_id, device_name, project_kind, launch_mode,
                 launch_plan_json, plan_fingerprint, state, startup_timeout_ms, created_at, updated_at)
             VALUES ('loc-compose', 'Compose', '/tmp/c', 'd', 'n', 'other', 'smart',
                     '{\"schemaVersion\":1,\"runtime\":\"docker_compose\",\"program\":\"internal\",\"compose\":{\"composeFile\":\"docker-compose.yml\",\"projectSeed\":\"p\"}}',
                     'fp', 'running', 60000, 't', 't')",
            [],
        )
        .expect("insert local compose app");
        conn.execute(
            "INSERT INTO applications (id, source, source_id, title, version, created_at, updated_at)
             VALUES ('app-compose', 'local_project', 'loc-compose', 'Compose', '1', 't', 't')",
            [],
        )
        .expect("insert app");
        conn.execute(
            "INSERT INTO startup_plans (id, application_id, plan_version, plan_json, is_active, created_at, updated_at)
             VALUES ('plan-compose', 'app-compose', 1,
                     '{\"schemaVersion\":1,\"runtime\":\"docker_compose\",\"program\":\"internal\"}',
                     1, 't', 't')",
            [],
        )
        .expect("insert plan with NULL versioned columns");
        conn.execute(
            "INSERT INTO runtime_instances (id, application_id, status, owner_kind, created_at, updated_at)
             VALUES ('ri-compose', 'app-compose', 'running', 'local_process', 't', 't')",
            [],
        )
        .expect("insert misclassified instance");

        let fixed = upgrade_startup_plans_v1(&conn).expect("upgrade v17");
        assert!(
            fixed >= 2,
            "expected plan backfill + owner repair, got {fixed}"
        );

        // Plan columns backfilled from JSON.
        let (dk, sv, om): (String, i64, String) = conn
            .query_row(
                "SELECT driver_kind, schema_version, ownership_mode FROM startup_plans WHERE id = 'plan-compose'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .expect("plan columns");
        assert_eq!(dk, "docker_compose");
        assert_eq!(sv, 1);
        assert_eq!(om, "managed");

        // The misclassified instance now owns a docker_compose project.
        let kind: String = conn
            .query_row(
                "SELECT owner_kind FROM runtime_instances WHERE id = 'ri-compose'",
                [],
                |r| r.get(0),
            )
            .expect("owner kind");
        assert_eq!(kind, "docker_compose");

        // Idempotent re-run finds nothing new.
        let fixed2 = upgrade_startup_plans_v1(&conn).expect("upgrade again");
        assert_eq!(fixed2, 0);
    }

    /// Batch 2 CR-201: migration v18 creates the `operations` journal table with
    /// FK-linked app/instance columns and active-phase indexes, and is idempotent.
    #[test]
    fn migration_creates_operations_journal() {
        let conn = Connection::open_in_memory().expect("open in-memory database");
        create_tables(&conn).expect("create base tables");
        apply_migrations(&conn).expect("apply migrations");

        let version: String = conn
            .query_row(
                "SELECT value FROM settings WHERE key = '_schema_version'",
                [],
                |r| r.get(0),
            )
            .expect("schema version");
        assert_eq!(
            version, SCHEMA_VERSION,
            "migration must bump the tracked version"
        );

        conn.execute(
            "INSERT INTO operations (kind, phase, actor, started_at, updated_at)
             VALUES ('start', 'pending', 'user', 't', 't')",
            [],
        )
        .expect("operations table accepts a minimal journal row");

        // Application id can be NULL (install before the app identity exists) and
        // the FK is satisfied once an application row exists.
        conn.execute(
            "INSERT INTO applications (id, source, source_id, title, version, created_at, updated_at)
             VALUES ('app-op', 'local_project', 'loc', 'L', '1', 't', 't')",
            [],
        )
        .expect("insert app");
        conn.execute(
            "INSERT INTO operations (application_id, kind, phase, started_at, updated_at)
             VALUES ('app-op', 'stop', 'running', 't', 't')",
            [],
        )
        .expect("operation links to an application");

        // Repeat migration is a no-op (no error, no duplicate table).
        apply_migrations(&conn).expect("re-apply migrations");

        // Deleting the application SET NULLs the operation's application_id so a
        // delete operation journal survives its own app row (audit trail).
        conn.execute(
            "INSERT INTO operations (application_id, kind, phase, started_at, updated_at)
             VALUES ('app-op', 'delete', 'succeeded', 't', 't')",
            [],
        )
        .expect("insert delete operation");
        conn.execute("DELETE FROM applications WHERE id = 'app-op'", [])
            .expect("delete app");
        let orphaned_app: Option<String> = conn
            .query_row(
                "SELECT application_id FROM operations WHERE kind = 'delete'",
                [],
                |r| r.get(0),
            )
            .expect("delete operation row");
        assert_eq!(orphaned_app, None, "FK SET NULL keeps the delete audit row");
    }
}

// ──────────────────────────────────────────────
// Module data CRUD (used by db:get/set/delete/list)
// Maps to the "module_data" table with a fixed module_id
// The Electron preload's db.get/set uses a single flat namespace,
// stored in module_data with module_id = '_app'.
// ──────────────────────────────────────────────

const APP_MODULE_ID: &str = "_app";

/// Generic module_data read (used by state persistence)
pub fn db_get_module_data(
    conn: &Connection,
    module_id: &str,
    key: &str,
) -> Result<Option<serde_json::Value>> {
    let mut stmt = conn
        .prepare("SELECT value FROM module_data WHERE module_id = ?1 AND key = ?2")
        .map_err(Error::Database)?;
    let result: Option<String> = stmt
        .query_row(rusqlite::params![module_id, key], |row| {
            row.get::<_, String>(0)
        })
        .optional()
        .map_err(Error::Database)?;
    match result {
        Some(s) => {
            let v: serde_json::Value =
                serde_json::from_str(&s).unwrap_or(serde_json::Value::String(s));
            Ok(Some(v))
        }
        None => Ok(None),
    }
}

/// Generic module_data write (used by state persistence)
pub fn db_set_module_data(
    conn: &Connection,
    module_id: &str,
    key: &str,
    value: &serde_json::Value,
) -> Result<()> {
    let serialized = serde_json::to_string(value).map_err(Error::Json)?;
    conn.execute(
        "INSERT INTO module_data (module_id, key, value) VALUES (?1, ?2, ?3)
         ON CONFLICT(module_id, key) DO UPDATE SET value = excluded.value",
        rusqlite::params![module_id, key, serialized],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// Generic module_data delete (used by state persistence)
pub fn db_delete_module_data(conn: &Connection, module_id: &str, key: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM module_data WHERE module_id = ?1 AND key = ?2",
        rusqlite::params![module_id, key],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// db:get — read a value from module_data
pub fn db_get(conn: &Connection, key: &str) -> Result<Option<serde_json::Value>> {
    let mut stmt = conn
        .prepare("SELECT value FROM module_data WHERE module_id = ?1 AND key = ?2")
        .map_err(Error::Database)?;
    let result: Option<String> = stmt
        .query_row(rusqlite::params![APP_MODULE_ID, key], |row| {
            row.get::<_, String>(0)
        })
        .optional()
        .map_err(Error::Database)?;

    match result {
        Some(s) => {
            let v: serde_json::Value =
                serde_json::from_str(&s).unwrap_or(serde_json::Value::String(s));
            Ok(Some(v))
        }
        None => Ok(None),
    }
}

/// db:set — upsert a value into module_data
pub fn db_set(conn: &Connection, key: &str, value: &serde_json::Value) -> Result<()> {
    let serialized = serde_json::to_string(value).map_err(Error::Json)?;
    conn.execute(
        "INSERT INTO module_data (module_id, key, value) VALUES (?1, ?2, ?3)
         ON CONFLICT(module_id, key) DO UPDATE SET value = excluded.value",
        rusqlite::params![APP_MODULE_ID, key, serialized],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// db:delete — remove a key from module_data
pub fn db_delete(conn: &Connection, key: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM module_data WHERE module_id = ?1 AND key = ?2",
        rusqlite::params![APP_MODULE_ID, key],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// db:list — list keys in module_data, optionally filtered by prefix
pub fn db_list(conn: &Connection, prefix: Option<&str>) -> Result<Vec<String>> {
    let stmt = match prefix {
        Some(_) => {
            let mut s = conn
                .prepare("SELECT key FROM module_data WHERE module_id = ?1 AND key LIKE ?2")
                .map_err(Error::Database)?;
            let pattern = format!("{}%", prefix.unwrap());
            let rows = s
                .query_map(rusqlite::params![APP_MODULE_ID, pattern], |row| {
                    row.get::<_, String>(0)
                })
                .map_err(Error::Database)?;
            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(Error::Database)
        }
        None => {
            let mut s = conn
                .prepare("SELECT key FROM module_data WHERE module_id = ?1")
                .map_err(Error::Database)?;
            let rows = s
                .query_map(rusqlite::params![APP_MODULE_ID], |row| {
                    row.get::<_, String>(0)
                })
                .map_err(Error::Database)?;
            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(Error::Database)
        }
    }?;
    Ok(stmt)
}

// ──────────────────────────────────────────────
// Settings helpers (used by theme, locale, etc.)
// ──────────────────────────────────────────────

/// Read a setting value by key
pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>> {
    let mut stmt = conn
        .prepare("SELECT value FROM settings WHERE key = ?1")
        .map_err(Error::Database)?;
    let result: Option<String> = stmt
        .query_row(rusqlite::params![key], |row| row.get::<_, String>(0))
        .optional()
        .map_err(Error::Database)?;
    Ok(result)
}

/// Write a setting value (upsert)
pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        rusqlite::params![key, value, now],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// Delete a setting value by key
pub fn delete_setting(conn: &Connection, key: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM settings WHERE key = ?1",
        rusqlite::params![key],
    )
    .map_err(Error::Database)?;
    Ok(())
}

// ──────────────────────────────────────────────
// Notifications
// ──────────────────────────────────────────────

/// Create a notification and return its ID
pub fn create_notification(
    conn: &Connection,
    title: &str,
    body: &str,
    level: &str,
    module_id: Option<&str>,
) -> Result<i64> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO notifications (module_id, title, body, level, read, created_at) VALUES (?1, ?2, ?3, ?4, 0, ?5)",
        rusqlite::params![module_id, title, body, level, now],
    )
    .map_err(Error::Database)?;
    Ok(conn.last_insert_rowid())
}

/// List notifications, optionally filtered by unread-only
pub fn list_notifications(conn: &Connection, unread_only: bool) -> Result<Vec<serde_json::Value>> {
    let sql = if unread_only {
        "SELECT id, module_id, title, body, level, read, created_at FROM notifications WHERE read = 0 ORDER BY created_at DESC"
    } else {
        "SELECT id, module_id, title, body, level, read, created_at FROM notifications ORDER BY created_at DESC"
    };
    let mut stmt = conn.prepare(sql).map_err(Error::Database)?;
    let rows = stmt
        .query_map([], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, i64>(0)?,
                "moduleId": row.get::<_, Option<String>>(1)?,
                "title": row.get::<_, String>(2)?,
                "body": row.get::<_, String>(3)?,
                "level": row.get::<_, String>(4)?,
                "read": row.get::<_, i64>(5)? != 0,
                "createdAt": row.get::<_, String>(6)?,
            }))
        })
        .map_err(Error::Database)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::Database)
}

/// Mark a notification as read
pub fn mark_notification_read(conn: &Connection, id: i64) -> Result<()> {
    conn.execute(
        "UPDATE notifications SET read = 1 WHERE id = ?1",
        rusqlite::params![id],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// Mark all notifications as read
pub fn mark_all_notifications_read(conn: &Connection) -> Result<()> {
    conn.execute("UPDATE notifications SET read = 1 WHERE read = 0", [])
        .map_err(Error::Database)?;
    Ok(())
}

// ──────────────────────────────────────────────
// Builtin tools CRUD (extensible tool registry)
// ──────────────────────────────────────────────

/// List all builtin tools from DB
pub fn list_builtin_tools(conn: &Connection) -> Result<Vec<serde_json::Value>> {
    let mut stmt = conn
        .prepare("SELECT id, enabled, driver FROM builtin_tools")
        .map_err(Error::Database)?;
    let rows = stmt
        .query_map([], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "enabled": row.get::<_, i64>(1)? != 0,
                "driver": row.get::<_, String>(2)?,
            }))
        })
        .map_err(Error::Database)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::Database)
}

/// Update a builtin tool's enabled/driver state
pub fn update_builtin_tool(conn: &Connection, id: &str, enabled: bool, driver: &str) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO builtin_tools (id, enabled, driver, updated_at) VALUES (?1, ?2, ?3, datetime('now'))",
        rusqlite::params![id, enabled as i64, driver],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// Ensure a builtin tool row exists (seed from frontend registry)
pub fn seed_builtin_tool(conn: &Connection, id: &str, driver: &str) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO builtin_tools (id, enabled, driver) VALUES (?1, 0, ?2)",
        rusqlite::params![id, driver],
    )
    .map_err(Error::Database)?;
    Ok(())
}

// ──────────────────────────────────────────────
// Usage stats CRUD (structured per-model daily stats)
// ──────────────────────────────────────────────

/// Upsert a daily model usage stat row with source breadcrumb.
#[allow(clippy::too_many_arguments)] // pre-existing parameter list
pub fn upsert_usage_stat(
    conn: &Connection,
    date: &str,
    source: &str,
    source_path: Option<&str>,
    model: &str,
    input_tokens: u64,
    output_tokens: u64,
    cache_creation_tokens: u64,
    cache_read_tokens: u64,
    request_count: u64,
    cost_usd: f64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO usage_stats (date, source, source_path, model, input_tokens, output_tokens,
            cache_creation_tokens, cache_read_tokens, request_count, cost_usd)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(date, source, model) DO UPDATE SET
            input_tokens = excluded.input_tokens,
            output_tokens = excluded.output_tokens,
            cache_creation_tokens = excluded.cache_creation_tokens,
            cache_read_tokens = excluded.cache_read_tokens,
            request_count = excluded.request_count,
            cost_usd = excluded.cost_usd,
            source_path = excluded.source_path",
        rusqlite::params![
            date,
            source,
            source_path,
            model,
            input_tokens,
            output_tokens,
            cache_creation_tokens,
            cache_read_tokens,
            request_count,
            cost_usd
        ],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// Query usage stats, optionally filtered by source and date range.
/// Returns array of JSON objects with all columns.
#[allow(dead_code)]
pub fn query_usage_stats(
    conn: &Connection,
    source: Option<&str>,
    from_date: Option<&str>,
    to_date: Option<&str>,
) -> Result<Vec<serde_json::Value>> {
    let mut sql = "SELECT date, source, source_path, model, input_tokens, output_tokens, cache_creation_tokens, cache_read_tokens, request_count, cost_usd FROM usage_stats WHERE 1=1".to_string();
    if source.is_some() {
        sql.push_str(" AND source = ?");
    }
    if from_date.is_some() {
        sql.push_str(" AND date >= ?");
    }
    if to_date.is_some() {
        sql.push_str(" AND date <= ?");
    }
    sql.push_str(" ORDER BY date DESC, model");

    let mut stmt = conn.prepare(&sql).map_err(Error::Database)?;
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    if let Some(s) = source {
        params.push(Box::new(s.to_string()));
    }
    if let Some(d) = from_date {
        params.push(Box::new(d.to_string()));
    }
    if let Some(d) = to_date {
        params.push(Box::new(d.to_string()));
    }

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok(serde_json::json!({
                "date": row.get::<_, String>(0)?,
                "source": row.get::<_, String>(1)?,
                "sourcePath": row.get::<_, Option<String>>(2)?,
                "model": row.get::<_, String>(3)?,
                "inputTokens": row.get::<_, u64>(4)?,
                "outputTokens": row.get::<_, u64>(5)?,
                "cacheCreationTokens": row.get::<_, u64>(6)?,
                "cacheReadTokens": row.get::<_, u64>(7)?,
                "requestCount": row.get::<_, u64>(8)?,
                "costUsd": row.get::<_, f64>(9)?,
            }))
        })
        .map_err(Error::Database)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::Database)
}

// ──────────────────────────────────────────────
// Skill usage CRUD (skill invocation tracking)
// ──────────────────────────────────────────────

/// Upsert a daily skill usage row with source breadcrumb.
pub fn upsert_skill_usage(
    conn: &Connection,
    date: &str,
    source: &str,
    source_path: Option<&str>,
    skill_name: &str,
    trigger_count: u64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO skill_usage (date, source, source_path, skill_name, trigger_count)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(date, source, skill_name) DO UPDATE SET
            trigger_count = excluded.trigger_count,
            source_path = excluded.source_path",
        rusqlite::params![date, source, source_path, skill_name, trigger_count],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// Query skill usage stats, optionally filtered by skill name and date range.
#[allow(dead_code)]
pub fn query_skill_usage(
    conn: &Connection,
    skill_name: Option<&str>,
    from_date: Option<&str>,
    to_date: Option<&str>,
) -> Result<Vec<serde_json::Value>> {
    let mut sql =
        "SELECT date, source, source_path, skill_name, trigger_count FROM skill_usage WHERE 1=1"
            .to_string();
    if skill_name.is_some() {
        sql.push_str(" AND skill_name = ?");
    }
    if from_date.is_some() {
        sql.push_str(" AND date >= ?");
    }
    if to_date.is_some() {
        sql.push_str(" AND date <= ?");
    }
    sql.push_str(" ORDER BY date DESC, skill_name");

    let mut stmt = conn.prepare(&sql).map_err(Error::Database)?;
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    if let Some(s) = skill_name {
        params.push(Box::new(s.to_string()));
    }
    if let Some(d) = from_date {
        params.push(Box::new(d.to_string()));
    }
    if let Some(d) = to_date {
        params.push(Box::new(d.to_string()));
    }

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok(serde_json::json!({
                "date": row.get::<_, String>(0)?,
                "source": row.get::<_, String>(1)?,
                "sourcePath": row.get::<_, Option<String>>(2)?,
                "skillName": row.get::<_, String>(3)?,
                "triggerCount": row.get::<_, u64>(4)?,
            }))
        })
        .map_err(Error::Database)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::Database)
}
