//! db_migrations_steps — individual Host SQLite migration steps (W3 split).
//!
//! Each version is an independent, idempotent `migrate_vN` step. The ordered
//! registry lives in `db_migrations::apply`; this file only holds the steps
//! so the registry stays the single source of ordering.

use super::{
    repair_creative_active_invariants, repair_creative_identity_ghosts, upgrade_startup_plans_v1,
};
use crate::Error;
use rusqlite::Connection;

pub(super) fn migrate_v2(conn: &Connection) -> Result<(), Error> {
    // v2: reserved migration slot — no schema changes needed yet.
    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES ('_schema_version', '2')",
        [],
    )
    .map_err(Error::Database)?;
    Ok(())
}

pub(super) fn migrate_v3(conn: &Connection) -> Result<(), Error> {
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
    Ok(())
}

pub(super) fn migrate_v4(conn: &Connection) -> Result<(), Error> {
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
    Ok(())
}

pub(super) fn migrate_v5(conn: &Connection) -> Result<(), Error> {
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
    Ok(())
}

pub(super) fn migrate_v6(conn: &Connection) -> Result<(), Error> {
    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES ('_schema_version', '6')",
        [],
    )
    .map_err(Error::Database)?;
    Ok(())
}

pub(super) fn migrate_v7(conn: &Connection) -> Result<(), Error> {
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
    Ok(())
}

pub(super) fn migrate_v8(conn: &Connection) -> Result<(), Error> {
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
    Ok(())
}

pub(super) fn migrate_v9(conn: &Connection) -> Result<(), Error> {
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
    Ok(())
}

pub(super) fn migrate_v10(conn: &Connection) -> Result<(), Error> {
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
    Ok(())
}

pub(super) fn migrate_v11(conn: &Connection) -> Result<(), Error> {
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
    Ok(())
}

pub(super) fn migrate_v12(conn: &Connection) -> Result<(), Error> {
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
    Ok(())
}

pub(super) fn migrate_v13(conn: &Connection) -> Result<(), Error> {
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
    Ok(())
}

pub(super) fn migrate_v14(conn: &Connection) -> Result<(), Error> {
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
    Ok(())
}

pub(super) fn migrate_v15(conn: &Connection) -> Result<(), Error> {
    repair_creative_identity_ghosts(conn)?;
    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES ('_schema_version', '15')",
        [],
    )
    .map_err(Error::Database)?;
    Ok(())
}

pub(super) fn migrate_v16(conn: &Connection) -> Result<(), Error> {
    repair_creative_active_invariants(conn)?;
    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES ('_schema_version', '16')",
        [],
    )
    .map_err(Error::Database)?;
    Ok(())
}

pub(super) fn migrate_v17(conn: &Connection) -> Result<(), Error> {
    upgrade_startup_plans_v1(conn)?;
    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES ('_schema_version', '17')",
        [],
    )
    .map_err(Error::Database)?;
    Ok(())
}

pub(super) fn migrate_v18(conn: &Connection) -> Result<(), Error> {
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
    Ok(())
}

pub(super) fn migrate_v19(conn: &Connection) -> Result<(), Error> {
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
    Ok(())
}

pub(super) fn migrate_v20(conn: &Connection) -> Result<(), Error> {
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
    Ok(())
}

pub(super) fn migrate_v21(conn: &Connection) -> Result<(), Error> {
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
    Ok(())
}

pub(super) fn migrate_v22(conn: &Connection) -> Result<(), Error> {
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
    Ok(())
}

pub(super) fn migrate_v23(conn: &Connection) -> Result<(), Error> {
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
    Ok(())
}

pub(super) fn migrate_v24(conn: &Connection) -> Result<(), Error> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS non_owned_apps (
            id TEXT PRIMARY KEY,
            ownership TEXT NOT NULL,
            url TEXT NOT NULL,
            approved_origins_json TEXT NOT NULL DEFAULT '[]',
            title TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_non_owned_ownership
            ON non_owned_apps(ownership);
        INSERT OR REPLACE INTO settings (key, value) VALUES ('_schema_version', '24');
        ",
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// Migration v24→v25 (FIL-005/008): persistent file metadata index + FTS5.
///
/// - `file_index`：非敏感文件元数据（path/kind/size/mtime/thumbnail）持久化，
///   供 indexed search 与 Home/Files 查询复用，避免每次全量扫描。
/// - FTS5 虚拟表存文件内容（content separated：正文与元数据分离，R-P1）。
///   索引由 file_indexer 模块负责（initial scan + fs_watch 增量），
///   本步只建 schema，保证幂等。
pub(super) fn migrate_v25(conn: &Connection) -> Result<(), Error> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS file_index (
            path TEXT PRIMARY KEY,
            kind TEXT NOT NULL DEFAULT 'file',
            size INTEGER NOT NULL DEFAULT 0,
            mtime_ms INTEGER NOT NULL DEFAULT 0,
            has_thumbnail INTEGER NOT NULL DEFAULT 0,
            indexed_at_ms INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_file_index_kind
            ON file_index(kind);
        CREATE INDEX IF NOT EXISTS idx_file_index_mtime
            ON file_index(mtime_ms);
        INSERT OR REPLACE INTO settings (key, value) VALUES ('_schema_version', '25');
        ",
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// Migration v25→v26 (FIL-008): FTS5 content index（content separated）。
///
/// FTS5 表不参与元数据查询，正文与元数据分离；索引内容由 file_indexer
/// 写入（initial scan / fs_watch 增量 / debounce / coalesce）。
pub(super) fn migrate_v26(conn: &Connection) -> Result<(), Error> {
    conn.execute_batch(
        "
        CREATE VIRTUAL TABLE IF NOT EXISTS file_content_fts USING fts5(
            path UNINDEXED,
            content,
            tokenize = 'unicode61'
        );
        INSERT OR REPLACE INTO settings (key, value) VALUES ('_schema_version', '26');
        ",
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// Migration v26→v27 (Workspace V2 design system).
///
/// Introduces the seven workspace tables (the single authority for workspace
/// state — the renderer never writes these directly, only through typed IPC):
///
/// - `workspaces`               — workspace root entity.
/// - `workspace_tabs`           — tabs inside a workspace.
/// - `workspace_context_items`  — context items pinned to a workspace.
/// - `workspace_widgets`        — widget instances (legacy Home widgets land here).
/// - `workspace_layouts`        — per-breakpoint responsive layouts (lg/md/sm).
/// - `workspace_view_states`    — keyed view state (split/window/scroll).
/// - `workspace_tool_profiles`  — tool-profile bindings scoped to a workspace.
///
/// The step also performs two compat migrations, both idempotent:
///
/// 1. Legacy `settings:home_workspace` (schemaVersion 1, single JSON document)
///    is imported into a `home` workspace row + `workspace_widgets` +
///    `workspace_layouts` rows, then the legacy key is removed so the new
///    tables are the only authority going forward.
/// 2. `settings:theme` legacy aliases (`terminal-volt` / `frosted-jasmine`)
///    are normalized to `dark` / `light` (V-002/V-004).
pub(super) fn migrate_v27(conn: &Connection) -> Result<(), Error> {
    super::migration_v27::migrate_v27(conn)
}

/// Migration v27→v28 (personal-creative → application-center, Phase B).
///
/// Delegates to the standalone v28 module (DDL + backfill + version marker),
/// mirroring the `migrate_v27` proxy so this file stays the ordering registry.
pub(super) fn migrate_v28(conn: &Connection) -> Result<(), Error> {
    super::migration_v28::migrate_v28(conn)
}

/// Migration v28→v29 (PWSV2 — Personal Workspace V2, ADR-0021 §PWSV2).
///
/// Delegates to the standalone v29 module (additive columns, the
/// `workspace_open_tabs` / `workspace_templates` tables, `enabled` backfill,
/// version marker). This file stays the ordering registry.
pub(super) fn migrate_v29(conn: &Connection) -> Result<(), Error> {
    super::migration_v29::migrate_v29(conn)
}
