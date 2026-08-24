//! Migration v30→v31: AI Resources & Local Proxy schema（ADR-0020 / plan3 03-data-security-contracts §1）。
//!
//! 新增表：
//! - `ai_providers`: 厂商身份
//! - `ai_connections`: 真实上游端点配置（无 Secret）
//! - `ai_credentials`: 统一 API Key / OAuth 凭证元数据（Secret 在 OS Keychain）
//! - `ai_credential_connections`: 凭证与连接多对多关联
//! - `ai_models`: 模型目录与来源证明
//! - `ai_quota_snapshots` & `ai_quota_windows`: 真实额度快照
//! - `proxy_settings`: 本地代理配置单例
//! - `proxy_routes` & `proxy_route_targets`: 本地模型路由规则
//! - `proxy_usage_records`: 代理用量审计

use rusqlite::Connection;

use crate::{Error, Result};

pub(super) fn migrate_v31(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        -- 1. AI Providers 厂商表
        CREATE TABLE IF NOT EXISTS ai_providers (
            id TEXT PRIMARY KEY,
            preset_key TEXT,
            name TEXT NOT NULL,
            website_url TEXT NOT NULL DEFAULT '',
            icon_key TEXT,
            enabled INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_ai_providers_enabled ON ai_providers(enabled);

        -- 2. AI Connections 真实上游端点表（绝不存 Secret）
        CREATE TABLE IF NOT EXISTS ai_connections (
            id TEXT PRIMARY KEY,
            provider_id TEXT NOT NULL REFERENCES ai_providers(id) ON DELETE CASCADE,
            name TEXT NOT NULL,
            base_url TEXT NOT NULL,
            upstream_protocol TEXT NOT NULL DEFAULT 'openai_chat_completions',
            models_url TEXT,
            proxy_url TEXT,
            headers_json TEXT,
            enabled INTEGER NOT NULL DEFAULT 1,
            health_status TEXT NOT NULL DEFAULT 'unknown',
            last_checked_at TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_ai_connections_provider ON ai_connections(provider_id);

        -- 3. AI Credentials 凭证元数据表（Secret 在 Keychain）
        CREATE TABLE IF NOT EXISTS ai_credentials (
            id TEXT PRIMARY KEY,
            provider_id TEXT NOT NULL REFERENCES ai_providers(id) ON DELETE CASCADE,
            kind TEXT NOT NULL,
            label TEXT NOT NULL,
            secret_ref TEXT NOT NULL,
            secret_revision INTEGER NOT NULL DEFAULT 1,
            masked_identity TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL DEFAULT 'active',
            priority INTEGER NOT NULL DEFAULT 0,
            concurrency_limit INTEGER NOT NULL DEFAULT 10,
            expires_at TEXT,
            last_refreshed_at TEXT,
            next_refresh_at TEXT,
            identity_fingerprint TEXT,
            metadata_json TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_ai_credentials_provider ON ai_credentials(provider_id);
        CREATE INDEX IF NOT EXISTS idx_ai_credentials_status ON ai_credentials(status);
        CREATE UNIQUE INDEX IF NOT EXISTS idx_ai_credentials_fingerprint
            ON ai_credentials(provider_id, identity_fingerprint)
            WHERE identity_fingerprint IS NOT NULL;

        -- 4. Credential 与 Connection 多对多绑定
        CREATE TABLE IF NOT EXISTS ai_credential_connections (
            credential_id TEXT NOT NULL REFERENCES ai_credentials(id) ON DELETE CASCADE,
            connection_id TEXT NOT NULL REFERENCES ai_connections(id) ON DELETE CASCADE,
            PRIMARY KEY(credential_id, connection_id)
        );

        -- 5. AI Models 模型目录表
        CREATE TABLE IF NOT EXISTS ai_models (
            id TEXT PRIMARY KEY,
            provider_id TEXT REFERENCES ai_providers(id) ON DELETE CASCADE,
            connection_id TEXT REFERENCES ai_connections(id) ON DELETE CASCADE,
            source_credential_id TEXT REFERENCES ai_credentials(id) ON DELETE SET NULL,
            model_id TEXT NOT NULL,
            display_name TEXT NOT NULL,
            source TEXT NOT NULL DEFAULT 'discovered',
            capabilities_json TEXT,
            availability TEXT NOT NULL DEFAULT 'available',
            discovered_at TEXT NOT NULL,
            last_seen_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_ai_models_connection ON ai_models(connection_id);
        CREATE INDEX IF NOT EXISTS idx_ai_models_cred ON ai_models(source_credential_id);

        -- 6. Quota Snapshots 额度快照
        CREATE TABLE IF NOT EXISTS ai_quota_snapshots (
            id TEXT PRIMARY KEY,
            credential_id TEXT NOT NULL REFERENCES ai_credentials(id) ON DELETE CASCADE,
            provider_adapter TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'unknown',
            plan_name TEXT,
            error_category TEXT,
            error_message TEXT,
            fetched_at TEXT NOT NULL,
            expires_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_ai_quota_cred ON ai_quota_snapshots(credential_id);

        -- 7. Quota Windows 额度窗口
        CREATE TABLE IF NOT EXISTS ai_quota_windows (
            id TEXT PRIMARY KEY,
            snapshot_id TEXT NOT NULL REFERENCES ai_quota_snapshots(id) ON DELETE CASCADE,
            label TEXT NOT NULL,
            remaining REAL,
            limit_value REAL,
            used REAL,
            unit TEXT,
            reset_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_ai_quota_windows_snapshot ON ai_quota_windows(snapshot_id);

        -- 8. Proxy Settings 本地代理配置单例
        CREATE TABLE IF NOT EXISTS proxy_settings (
            id TEXT PRIMARY KEY,
            enabled_intent INTEGER NOT NULL DEFAULT 0,
            bind_host TEXT NOT NULL DEFAULT '127.0.0.1',
            port_mode TEXT NOT NULL DEFAULT 'dynamic',
            configured_port INTEGER NOT NULL DEFAULT 15721,
            effective_port INTEGER NOT NULL DEFAULT 0,
            access_secret_ref TEXT NOT NULL DEFAULT '',
            grace_timeout_ms INTEGER NOT NULL DEFAULT 5000,
            max_concurrency INTEGER NOT NULL DEFAULT 64,
            max_request_body_bytes INTEGER NOT NULL DEFAULT 33554432,
            updated_at TEXT NOT NULL
        );

        -- 9. Proxy Routes 本地模型路由表
        CREATE TABLE IF NOT EXISTS proxy_routes (
            id TEXT PRIMARY KEY,
            local_model TEXT NOT NULL UNIQUE,
            enabled INTEGER NOT NULL DEFAULT 1,
            strategy TEXT NOT NULL DEFAULT 'ordered',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        -- 10. Proxy Route Targets 路由目标表
        CREATE TABLE IF NOT EXISTS proxy_route_targets (
            id TEXT PRIMARY KEY,
            route_id TEXT NOT NULL REFERENCES proxy_routes(id) ON DELETE CASCADE,
            position INTEGER NOT NULL DEFAULT 0,
            connection_id TEXT NOT NULL REFERENCES ai_connections(id) ON DELETE CASCADE,
            model_id TEXT NOT NULL,
            credential_selector_json TEXT NOT NULL,
            priority INTEGER NOT NULL DEFAULT 0,
            enabled INTEGER NOT NULL DEFAULT 1
        );
        CREATE INDEX IF NOT EXISTS idx_proxy_targets_route ON proxy_route_targets(route_id);

        -- 11. Proxy Usage Records 用量记录表
        CREATE TABLE IF NOT EXISTS proxy_usage_records (
            id TEXT PRIMARY KEY,
            route_id TEXT,
            connection_id TEXT,
            credential_id TEXT,
            inbound_protocol TEXT NOT NULL,
            upstream_protocol TEXT NOT NULL,
            local_model TEXT NOT NULL,
            upstream_model TEXT NOT NULL,
            prompt_tokens INTEGER NOT NULL DEFAULT 0,
            completion_tokens INTEGER NOT NULL DEFAULT 0,
            total_tokens INTEGER NOT NULL DEFAULT 0,
            reasoning_tokens INTEGER,
            cached_tokens INTEGER,
            latency_ms INTEGER NOT NULL DEFAULT 0,
            status TEXT NOT NULL DEFAULT 'success',
            error_code TEXT,
            created_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_proxy_usage_created ON proxy_usage_records(created_at);
        CREATE INDEX IF NOT EXISTS idx_proxy_usage_route ON proxy_usage_records(route_id);

        -- 12. 更新版本标号为 31
        INSERT OR REPLACE INTO settings (key, value) VALUES ('_schema_version', '31');
        ",
    )
    .map_err(Error::Database)?;

    Ok(())
}
