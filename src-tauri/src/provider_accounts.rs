//! Host-owned Sub2API account-pool persistence.  Secret fields never cross IPC.
//!
//! W3: import parsing/validation helpers moved to [`crate::provider_accounts_parse`];
//! this file keeps the command handlers, routing settings and DTOs, and
//! re-exports the shared helpers for backwards compatibility.

use crate::{emit_db_state_changed, provider_key_manager, AppState, Error, Result};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, State};

pub use crate::provider_accounts_parse::*;

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ProviderAccountSummary {
    pub id: String,
    pub provider_id: String,
    pub name: String,
    pub platform: String,
    pub account_type: String,
    pub proxy_configured: bool,
    pub concurrency: i64,
    pub priority: i64,
    pub expires_at: Option<String>,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchDeleteRequest {
    pub provider_id: String,
    pub account_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchDeleteResult {
    pub deleted: Vec<String>,
    pub not_found: Vec<String>,
    pub failed: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSub2ApiPoolRequest {
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RoutingSettings {
    pub enabled: bool,
    pub local_enabled: bool,
    pub local_port: i64,
    pub has_local_token: bool,
    pub rectifier: Value,
    pub global_proxy: Value,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRoutingSettings {
    pub enabled: bool,
    pub local_enabled: bool,
    pub local_port: i64,
    pub local_token: Option<String>,
    #[serde(default)]
    pub clear_local_token: bool,
    #[serde(default = "empty_object")]
    pub rectifier: Value,
    #[serde(default = "empty_object")]
    pub global_proxy: Value,
}

/// Returned exactly once by a rotation command. The persistent settings API
/// deliberately exposes only `has_local_token`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalRoutingTokenIssued {
    pub token: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RouteBinding {
    pub id: String,
    pub position: i64,
    pub provider_id: String,
    pub credential_kind: String,
    pub credential_id: Option<String>,
    pub model_id: String,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[tauri::command]
pub fn provider_accounts_list(
    provider_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<ProviderAccountSummary>> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    let mut statement = conn.prepare("SELECT id, provider_id, name, platform, account_type, proxy_id IS NOT NULL, concurrency, priority, expires_at, status, created_at, updated_at FROM provider_accounts WHERE provider_id=?1 ORDER BY priority, name, id").map_err(Error::Database)?;
    let accounts = statement
        .query_map([provider_id], |row| {
            Ok(ProviderAccountSummary {
                id: row.get(0)?,
                provider_id: row.get(1)?,
                name: row.get(2)?,
                platform: row.get(3)?,
                account_type: row.get(4)?,
                proxy_configured: row.get::<_, i64>(5)? != 0,
                concurrency: row.get(6)?,
                priority: row.get(7)?,
                expires_at: row.get(8)?,
                status: row.get(9)?,
                created_at: row.get(10)?,
                updated_at: row.get(11)?,
            })
        })
        .map_err(Error::Database)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::Database)?;
    Ok(accounts)
}

#[tauri::command]
pub fn provider_accounts_preview_import(
    provider_id: String,
    source: String,
    state: State<'_, AppState>,
) -> Result<ImportPreview> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    let mut statement = conn
        .prepare("SELECT identity_fingerprint FROM provider_accounts WHERE provider_id=?1")
        .map_err(Error::Database)?;
    let existing = statement
        .query_map([provider_id], |row| row.get::<_, String>(0))
        .map_err(Error::Database)?
        .collect::<std::result::Result<std::collections::HashSet<_>, _>>()
        .map_err(Error::Database)?;
    preview(&source, &existing)
}

#[tauri::command]
pub fn provider_accounts_commit_import(
    request: ImportRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ImportResult> {
    let parsed = parse_source(&request.source)?;
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    let provider_exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM user_providers WHERE id=?1)",
            [&request.provider_id],
            |r| r.get(0),
        )
        .map_err(Error::Database)?;
    if !provider_exists {
        return Err(Error::NotFound("provider not found".into()));
    }
    let is_sub2api: bool = conn
        .query_row(
            "SELECT preset_name = 'sub2api' FROM user_providers WHERE id=?1",
            [&request.provider_id],
            |row| row.get(0),
        )
        .map_err(Error::Database)?;
    if !is_sub2api {
        return Err(Error::InvalidInput(
            "accounts require a Sub2API pool provider".into(),
        ));
    }
    let tx = conn.unchecked_transaction().map_err(Error::Database)?;
    let mut proxy_ids = std::collections::HashMap::new();
    {
        for proxy in &parsed.proxies {
            let key = field(proxy, "proxy_key")
                .or_else(|| field(proxy, "key"))
                .unwrap_or(&proxy.to_string())
                .to_string();
            let url = imported_proxy_url(proxy).ok_or_else(|| {
                Error::InvalidInput("import contains an invalid proxy URL".into())
            })?;
            let (encrypted, dek) =
                provider_key_manager::envelope_encrypt(&json!({"url": url}).to_string(), &tx)?;
            let id = uuid::Uuid::new_v4().to_string();
            tx.execute("INSERT INTO provider_account_proxies (id, provider_id, proxy_key_hash, config_encrypted, dek_encrypted, created_at) VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT(provider_id, proxy_key_hash) DO UPDATE SET config_encrypted=excluded.config_encrypted, dek_encrypted=excluded.dek_encrypted", params![id, request.provider_id, digest(&key), encrypted, dek, now()]).map_err(Error::Database)?;
            let stored: String = tx.query_row("SELECT id FROM provider_account_proxies WHERE provider_id=?1 AND proxy_key_hash=?2", params![request.provider_id, digest(&key)], |r| r.get(0)).map_err(Error::Database)?;
            proxy_ids.insert(key, stored);
        }
    }
    let mut created = 0;
    let mut updated = 0;
    let mut failed = Vec::new();
    let mut skipped = 0;
    let mut seen = std::collections::HashSet::new();
    for (index, value) in parsed.accounts.iter().enumerate() {
        let account = match parse_account(value) {
            Ok(v) => v,
            Err(error) => {
                failed.push(ImportPreviewItem {
                    index,
                    name: field(value, "name").unwrap_or("").to_string(),
                    platform: field(value, "platform").unwrap_or("").to_string(),
                    account_type: field(value, "type").unwrap_or("").to_string(),
                    action: "reject".into(),
                    error: Some(error),
                });
                continue;
            }
        };
        if !seen.insert(account.fingerprint.clone()) {
            skipped += 1;
            continue;
        }
        let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM provider_accounts WHERE provider_id=?1 AND identity_fingerprint=?2)", params![request.provider_id, account.fingerprint], |r| r.get(0)).map_err(Error::Database)?;
        let existing_credentials: Option<(String, String)> = tx.query_row(
            "SELECT credentials_encrypted, dek_encrypted FROM provider_accounts WHERE provider_id=?1 AND identity_fingerprint=?2",
            params![request.provider_id, account.fingerprint], |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional().map_err(Error::Database)?;
        let existing_plain = existing_credentials
            .and_then(|(encrypted, dek)| {
                provider_key_manager::envelope_decrypt(&encrypted, &dek, &tx).ok()
            })
            .and_then(|value| serde_json::from_str(&value).ok());
        let credentials = merge_credentials(account.credentials, existing_plain);
        let (credentials_encrypted, dek_encrypted) =
            provider_key_manager::envelope_encrypt(&credentials.to_string(), &tx)?;
        let proxy_id = account
            .proxy_key
            .as_ref()
            .and_then(|key| proxy_ids.get(key));
        let stamp = now();
        tx.execute("INSERT INTO provider_accounts (id, provider_id, name, platform, account_type, credentials_encrypted, dek_encrypted, extra_json, proxy_id, concurrency, priority, expires_at, status, identity_fingerprint, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,'active',?13,?14,?14) ON CONFLICT(provider_id, identity_fingerprint) DO UPDATE SET name=excluded.name, credentials_encrypted=excluded.credentials_encrypted, dek_encrypted=excluded.dek_encrypted, extra_json=excluded.extra_json, proxy_id=excluded.proxy_id, concurrency=excluded.concurrency, priority=excluded.priority, expires_at=excluded.expires_at, updated_at=excluded.updated_at", params![uuid::Uuid::new_v4().to_string(), request.provider_id, account.name, account.platform, account.account_type, credentials_encrypted, dek_encrypted, account.extra.to_string(), proxy_id, account.concurrency, account.priority, account.expires_at, account.fingerprint, stamp]).map_err(Error::Database)?;
        if exists {
            updated += 1
        } else {
            created += 1
        }
    }
    tx.commit().map_err(Error::Database)?;
    emit_db_state_changed(
        &app,
        "provider:accounts",
        json!({"providerId": request.provider_id}),
    );
    Ok(ImportResult {
        created,
        updated,
        skipped,
        failed,
    })
}

#[tauri::command]
pub fn provider_accounts_batch_delete(
    request: BatchDeleteRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<BatchDeleteResult> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    let tx = conn.unchecked_transaction().map_err(Error::Database)?;
    let mut deleted = Vec::new();
    let mut not_found = Vec::new();
    let mut failed = Vec::new();
    for id in request.account_ids {
        if id.trim().is_empty() {
            failed.push(id);
            continue;
        }
        if tx
            .execute(
                "DELETE FROM provider_accounts WHERE id=?1 AND provider_id=?2",
                params![id, request.provider_id],
            )
            .map_err(Error::Database)?
            == 1
        {
            deleted.push(id)
        } else {
            not_found.push(id)
        }
    }
    tx.execute("DELETE FROM provider_account_proxies WHERE provider_id=?1 AND NOT EXISTS (SELECT 1 FROM provider_accounts WHERE proxy_id=provider_account_proxies.id)", [&request.provider_id]).map_err(Error::Database)?;
    tx.commit().map_err(Error::Database)?;
    emit_db_state_changed(
        &app,
        "provider:accounts",
        json!({"providerId": request.provider_id}),
    );
    Ok(BatchDeleteResult {
        deleted,
        not_found,
        failed,
    })
}

#[tauri::command]
pub fn provider_accounts_create_pool(
    input: CreateSub2ApiPoolRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<String> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err(Error::InvalidInput("pool name is required".into()));
    }
    if name.len() > 120 {
        return Err(Error::InvalidInput("pool name is too long".into()));
    }
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    let id = uuid::Uuid::new_v4().to_string();
    let stamp = now();
    conn.execute(
        "INSERT INTO user_providers (id, preset_name, api_protocol, name, website_url, base_url, created_at, updated_at) VALUES (?1, 'sub2api', 'openai_responses', ?2, '', '', ?3, ?3)",
        params![id, name, stamp],
    ).map_err(Error::Database)?;
    emit_db_state_changed(&app, "provider:accounts", json!({"providerId": id}));
    Ok(id)
}

#[tauri::command]
pub fn provider_routing_get_settings(state: State<'_, AppState>) -> Result<RoutingSettings> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    conn.query_row("SELECT enabled, local_enabled, local_port, local_token_encrypted IS NOT NULL, rectifier_json, global_proxy_json, updated_at FROM provider_routing_settings WHERE id=1", [], |r| Ok(RoutingSettings { enabled: r.get::<_, i64>(0)? != 0, local_enabled: r.get::<_, i64>(1)? != 0, local_port: r.get(2)?, has_local_token: r.get::<_, i64>(3)? != 0, rectifier: serde_json::from_str(&r.get::<_, String>(4)?).unwrap_or_else(|_| empty_object()), global_proxy: public_proxy_config(serde_json::from_str(&r.get::<_, String>(5)?).unwrap_or_else(|_| empty_object())), updated_at: r.get(6)? })).optional().map_err(Error::Database)?.ok_or_else(|| Error::Internal("routing settings unavailable".into()))
}

#[tauri::command]
pub fn provider_routing_update_settings(
    input: UpdateRoutingSettings,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<RoutingSettings> {
    if !(1..=65535).contains(&input.local_port) {
        return Err(Error::InvalidInput("local_port must be 1..65535".into()));
    }
    if !input.rectifier.is_object() || !input.global_proxy.is_object() {
        return Err(Error::InvalidInput("routing JSON must be objects".into()));
    }
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    let (token, token_dek) = match input.local_token.filter(|v| !v.is_empty()) {
        Some(token) => {
            let values = provider_key_manager::envelope_encrypt(&token, &conn)?;
            (Some(values.0), Some(values.1))
        }
        None => (None, None),
    };
    let enabled = input
        .global_proxy
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let url = input
        .global_proxy
        .get("url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if let Some(url) = url {
        if !valid_proxy_url(url) {
            return Err(Error::InvalidInput(
                "proxy URL must use http, https, or socks5".into(),
            ));
        }
    }
    let previous: Value = conn
        .query_row(
            "SELECT global_proxy_json FROM provider_routing_settings WHERE id=1",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(Error::Database)?
        .and_then(|value| serde_json::from_str(&value).ok())
        .unwrap_or_else(empty_object);
    let global_proxy = match url {
        Some(url) => {
            let (encrypted, dek) = provider_key_manager::envelope_encrypt(url, &conn)?;
            json!({"enabled": enabled, "url_encrypted": encrypted, "dek_encrypted": dek})
        }
        None => {
            json!({"enabled": enabled, "url_encrypted": previous.get("url_encrypted").cloned(), "dek_encrypted": previous.get("dek_encrypted").cloned()})
        }
    };
    conn.execute("UPDATE provider_routing_settings SET enabled=?1, local_enabled=?2, local_port=?3, local_token_encrypted=CASE WHEN ?4 THEN NULL WHEN ?5 IS NOT NULL THEN ?5 ELSE local_token_encrypted END, local_token_dek_encrypted=CASE WHEN ?4 THEN NULL WHEN ?6 IS NOT NULL THEN ?6 ELSE local_token_dek_encrypted END, rectifier_json=?7, global_proxy_json=?8, updated_at=?9 WHERE id=1", params![input.enabled as i64, input.local_enabled as i64, input.local_port, input.clear_local_token as i64, token, token_dek, input.rectifier.to_string(), global_proxy.to_string(), now()]).map_err(Error::Database)?;
    emit_db_state_changed(&app, "provider:routing", json!({}));
    provider_routing_get_settings(state)
}

#[tauri::command]
pub fn provider_routing_rotate_local_token(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<LocalRoutingTokenIssued> {
    use rand::RngCore;

    let mut bytes = [0_u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let token = hex::encode(bytes);
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    let (encrypted, dek) = provider_key_manager::envelope_encrypt(&token, &conn)?;
    conn.execute(
        "UPDATE provider_routing_settings
         SET local_token_encrypted=?1, local_token_dek_encrypted=?2, updated_at=?3
         WHERE id=1",
        params![encrypted, dek, now()],
    )
    .map_err(Error::Database)?;
    emit_db_state_changed(&app, "provider:routing", json!({"localTokenRotated": true}));
    Ok(LocalRoutingTokenIssued { token })
}

#[tauri::command]
pub fn provider_routing_list_bindings(state: State<'_, AppState>) -> Result<Vec<RouteBinding>> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    let mut statement = conn.prepare("SELECT id, position, provider_id, credential_kind, credential_id, model_id, enabled, created_at, updated_at FROM provider_route_bindings ORDER BY position, id").map_err(Error::Database)?;
    let bindings = statement
        .query_map([], |r| {
            Ok(RouteBinding {
                id: r.get(0)?,
                position: r.get(1)?,
                provider_id: r.get(2)?,
                credential_kind: r.get(3)?,
                credential_id: r.get(4)?,
                model_id: r.get(5)?,
                enabled: r.get::<_, i64>(6)? != 0,
                created_at: r.get(7)?,
                updated_at: r.get(8)?,
            })
        })
        .map_err(Error::Database)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Error::Database)?;
    Ok(bindings)
}

#[tauri::command]
pub fn provider_routing_update_bindings(
    bindings: Vec<RouteBinding>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<RouteBinding>> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    for binding in &bindings {
        if !matches!(binding.credential_kind.as_str(), "api_key" | "sub2api_pool")
            || binding.provider_id.trim().is_empty()
            || binding.model_id.trim().is_empty()
        {
            return Err(Error::InvalidInput("invalid route binding".into()));
        }
        let provider_exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM user_providers WHERE id=?1)",
                [&binding.provider_id],
                |row| row.get(0),
            )
            .map_err(Error::Database)?;
        if !provider_exists {
            return Err(Error::InvalidInput(
                "route binding provider does not exist".into(),
            ));
        }
        if binding.credential_kind == "api_key" {
            let key_id = binding
                .credential_id
                .as_deref()
                .filter(|id| !id.trim().is_empty())
                .ok_or_else(|| {
                    Error::InvalidInput("api_key binding requires credential_id".into())
                })?;
            let owns_key: bool = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM provider_api_keys WHERE id=?1 AND provider_id=?2)",
                    params![key_id, binding.provider_id],
                    |row| row.get(0),
                )
                .map_err(Error::Database)?;
            if !owns_key {
                return Err(Error::InvalidInput(
                    "route binding key does not belong to provider".into(),
                ));
            }
        } else {
            let is_pool: bool = conn
                .query_row(
                    "SELECT preset_name = 'sub2api' FROM user_providers WHERE id=?1",
                    [&binding.provider_id],
                    |row| row.get(0),
                )
                .map_err(Error::Database)?;
            if !is_pool {
                return Err(Error::InvalidInput(
                    "sub2api_pool binding requires a Sub2API provider".into(),
                ));
            }
        }
    }
    let tx = conn.unchecked_transaction().map_err(Error::Database)?;
    tx.execute("DELETE FROM provider_route_bindings", [])
        .map_err(Error::Database)?;
    for binding in &bindings {
        let stamp = now();
        tx.execute("INSERT INTO provider_route_bindings (id, position, provider_id, credential_kind, credential_id, model_id, enabled, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)", params![binding.id, binding.position, binding.provider_id, binding.credential_kind, binding.credential_id, binding.model_id, binding.enabled as i64, binding.created_at, stamp]).map_err(Error::Database)?;
    }
    tx.commit().map_err(Error::Database)?;
    emit_db_state_changed(&app, "provider:routing", json!({}));
    provider_routing_list_bindings(state)
}
