//! db_migrations — Host SQLite schema migration **registry** (W3 split).
//!
//! The ordered facade `apply` runs every migration step in order, guarded by
//! `current_version`, and preserves legacy repair statements in their original
//! position. Individual `migrate_vN` steps live in `super::migrations_steps`
//! so this file stays a thin, single source of ordering.

use super::{backfill_creative_identity, migrations_steps::*};
use crate::Error;
use rusqlite::{Connection, OptionalExtension};

/// Read current schema version from settings; absent = v1.
fn current_version(conn: &Connection) -> Result<i32, Error> {
    let stored: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = '_schema_version'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(Error::Database)?;
    Ok(stored.and_then(|v| v.parse::<i32>().ok()).unwrap_or(1))
}

/// Run all pending migrations for the Host DB, preserving legacy repairs.
pub fn apply(conn: &Connection) -> Result<(), Error> {
    let current_version = current_version(conn)?;

    // Migration v1→v2: v1 schema already includes all 10 tables created at init,
    // so this is a no-op for now. Future migrations go here.
    if current_version < 2 {
        migrate_v2(conn)?;
    }

    // Migration v2→v3: builtin_tools table for extensible built-in tool registry.
    // Each row = one tool (terminal, editor, browser…) with enabled flag and driver choice.
    if current_version < 3 {
        migrate_v3(conn)?;
    }

    // Migration v3→v4: structured usage_stats and skill_usage tables.
    // Replaces the previous JSON-blob approach (settings key "usage:cached")
    // with proper relational rows for queryability and source breadcrumbs.
    if current_version < 4 {
        migrate_v4(conn)?;
    }

    // Migration v4→v5: provider_api_keys 加 dek_encrypted 列（信封加密）
    // 原迁移在 init_kek() 里有时序问题——list_providers 可能在 init_kek 之前调用
    if current_version < 5 {
        migrate_v5(conn)?;
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
        migrate_v6(conn)?;
    }

    // Migration v6→v7: external creative apps (GitHub container) + encrypted env.
    // Does not alter `modules` — dual-source storage stays on separate tables (ADR-0013).
    if current_version < 7 {
        migrate_v7(conn)?;
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
        migrate_v8(conn)?;
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
        migrate_v9(conn)?;
    }

    // Migration v9→v10: creative draft store (ADR-0014).
    if current_version < 10 {
        migrate_v10(conn)?;
    }

    // Migration v10→v11: capability secrets (ADR-0016 decision 7).
    if current_version < 11 {
        migrate_v11(conn)?;
    }

    // Migration v11→v12: unified Application identity + RuntimeInstance (batch 1).
    if current_version < 12 {
        migrate_v12(conn)?;
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
    if current_version < 13 {
        migrate_v13(conn)?;
    }

    // Migration v13→v14: local app volume identity (batch 4).
    if current_version < 14 {
        migrate_v14(conn)?;
    }

    // Migration v14→v15 (batch 1 CR-101): repair ghost `applications` identities.
    if current_version < 15 {
        migrate_v15(conn)?;
    }

    // Migration v15→v16 (batch 1 CR-102): one active runtime instance and one
    // active startup plan per application as DATABASE invariants.
    if current_version < 16 {
        migrate_v16(conn)?;
    }

    // Migration v16→v17 (batch 1 CR-103): promote `startup_plans` to a versioned
    // LaunchProfile base.
    if current_version < 17 {
        migrate_v17(conn)?;
    }

    // Migration v17→v18 (batch 2 CR-201): operation journal for lifecycle
    // mutations.
    if current_version < 18 {
        migrate_v18(conn)?;
    }

    // Migration v18→v19 (batch 5 CR-501): Surface, Endpoint, Window tables.
    if current_version < 19 {
        migrate_v19(conn)?;
    }

    // Migration v19→v20 (batch 6 CR-601): BrowserProfile table.
    if current_version < 20 {
        migrate_v20(conn)?;
    }

    // Migration v20→v21 (batch 6 CR-602/603): OAuth allowlist + app grants.
    if current_version < 21 {
        migrate_v21(conn)?;
    }

    // Migration v21→v22 (batch 7 CR-701): service_instances table.
    if current_version < 22 {
        migrate_v22(conn)?;
    }

    // Migration v22→v23: creative proposal inbox + executable approval records
    // (T06), window_instances URL/reconcile truth (T07), and real browser
    // profiles + grant history (T08).
    if current_version < 23 {
        migrate_v23(conn)?;
    }

    // Migration v23→v24 (T09): non-owned app records.
    if current_version < 24 {
        migrate_v24(conn)?;
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
