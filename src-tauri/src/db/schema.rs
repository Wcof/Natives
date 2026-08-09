use crate::Result;
use rusqlite::Connection;

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
    // T214: migration registry split into db_migrations (per-version
    // migrate_vN steps + legacy repairs preserved in original order).
    super::db_migrations::apply(conn)
}
