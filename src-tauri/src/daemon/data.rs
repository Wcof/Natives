//! Host-owned provider mirror schema + natives.db-only legacy provider-key
//! migration.
//!
//! Authority: the Agent Daemon owns the assistant database exclusively — its
//! schema migrations and the canonical `conversation` / `message` / `run` /
//! `run_event` / `prompt_queue` tables (MODULAR §10.3 / §11.3.2:
//! cross_db_host = 0). The Host never opens or writes that database. Any
//! pre-existing `assistant_*` session/conversation tables inside it are merged
//! by the Daemon's `host_authority_migration` (`src-agent-daemon/src/storage/`),
//! never by the Host.
//!
//! Everything in this module is Host-owned and lives in the Host's own
//! natives.db:
//! - `ensure_provider_mirror_schema` creates/upgrades the Host-owned provider
//!   mirror tables (`assistant_provider_configs` / `assistant_provider_keys` /
//!   `assistant_model_cache` / `assistant_projects`). Called from
//!   `db::ensure_host_owned_tables` at natives.db init.
//! - `migrate_legacy_provider_keys` is the one-way, idempotent migration of
//!   legacy provider rows from `user_providers` / `provider_api_keys` into the
//!   mirror tables — source and target both live in natives.db, so no cross-db
//!   access and no second database handle is needed.
//!
//! Both steps are idempotent and never DROP or rebuild tables (R-D3).

use crate::Result;
use rusqlite::Connection;

/// Migrate provider keys from old legacy tables (`user_providers` /
/// `provider_api_keys`) into the Host-owned provider mirror tables
/// (`assistant_provider_configs` / `assistant_provider_keys` /
/// `assistant_model_cache`) — all on the same natives.db connection, so no
/// cross-db access is involved. Only runs if the legacy tables exist.
/// Idempotent — INSERT OR IGNORE.
///
/// The caller must already have the mirror schema in place on `natives_conn`
/// (natives.db init does so via `db::ensure_host_owned_tables` →
/// `ensure_provider_mirror_schema`).
pub fn migrate_legacy_provider_keys(natives_conn: &mut Connection) -> Result<()> {
    let has_legacy_providers: bool = natives_conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='user_providers'")
        .and_then(|mut stmt| stmt.exists([]))
        .unwrap_or(false);
    let has_legacy_keys: bool = natives_conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='provider_api_keys'")
        .and_then(|mut stmt| stmt.exists([]))
        .unwrap_or(false);
    if !has_legacy_providers && !has_legacy_keys {
        return Ok(());
    }

    let tx = natives_conn
        .transaction()
        .map_err(|e| crate::Error::Internal(format!("Legacy migration transaction failed: {e}")))?;

    // Migrate providers from user_providers (same DB: no ATTACH needed).
    if has_legacy_providers {
        let provider_sql = "INSERT OR IGNORE INTO assistant_provider_configs
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
             WHERE up.id NOT IN (SELECT id FROM assistant_provider_configs)";
        tx.execute_batch(provider_sql).map_err(|e| {
            crate::Error::Internal(format!("Legacy provider migration failed: {e}"))
        })?;
    }

    // Migrate keys from provider_api_keys (same DB).
    if has_legacy_keys {
        let keys_sql = "INSERT OR IGNORE INTO assistant_provider_keys
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
             WHERE pak.id NOT IN (SELECT id FROM assistant_provider_keys)";
        tx.execute_batch(keys_sql)
            .map_err(|e| crate::Error::Internal(format!("Legacy key migration failed: {e}")))?;
    }

    // Auto-seed assistant_model_cache with default models from user_providers.
    if has_legacy_providers {
        let model_sql = "INSERT OR IGNORE INTO assistant_model_cache
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
             WHERE up.default_model IS NOT NULL AND up.default_model != ''";
        tx.execute_batch(model_sql).map_err(|e| {
            crate::Error::Internal(format!("Default model cache seeding failed: {e}"))
        })?;
    }

    tx.commit()
        .map_err(|e| crate::Error::Internal(format!("Legacy migration commit failed: {e}")))?;

    Ok(())
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

/// v7: Unified provider key table — single source of truth for all provider/key data.
///
/// Schema only: adds columns, indexes. Data migration from legacy tables
/// is handled by `migrate_legacy_provider_keys()` after the mirror schema is
/// in place on natives.db.
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

/// Ensure the Host-owned provider mirror schema on any connection. The mirror
/// tables are Host authority and live in the Host's own natives.db (via
/// `db::ensure_host_owned_tables`); they are never created inside the Daemon's
/// assistant database.
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
