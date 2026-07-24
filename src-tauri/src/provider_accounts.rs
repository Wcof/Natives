//! Host-owned Sub2API account-pool persistence.  Secret fields never cross IPC.

use crate::{emit_db_state_changed, provider_key_manager, AppState, Error, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, State};

const MAX_IMPORT_BYTES: usize = 10 * 1024 * 1024;
const MAX_IMPORT_ACCOUNTS: usize = 2_000;

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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreviewItem {
    pub index: usize,
    pub name: String,
    pub platform: String,
    pub account_type: String,
    pub action: String,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    pub accounts: Vec<ImportPreviewItem>,
    pub proxy_count: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportRequest {
    pub provider_id: String,
    pub source: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub created: usize,
    pub updated: usize,
    pub skipped: usize,
    pub failed: Vec<ImportPreviewItem>,
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

fn empty_object() -> Value {
    json!({})
}

#[derive(Clone)]
struct ParsedAccount {
    name: String,
    platform: String,
    account_type: String,
    credentials: Value,
    extra: Value,
    proxy_key: Option<String>,
    concurrency: i64,
    priority: i64,
    expires_at: Option<String>,
    fingerprint: String,
}

struct ParsedSource {
    accounts: Vec<Value>,
    proxies: Vec<Value>,
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}
fn digest(input: &str) -> String {
    hex::encode(Sha256::digest(input.as_bytes()))
}

fn supported(platform: &str, account_type: &str) -> bool {
    matches!(
        (platform, account_type),
        ("openai", "oauth" | "apikey" | "upstream")
            | ("anthropic", "apikey" | "upstream")
            | ("gemini", "apikey" | "upstream")
    )
}

fn field<'a>(value: &'a Value, name: &str) -> Option<&'a str> {
    value.get(name)?.as_str()
}

fn collect_accounts(
    value: Value,
    accounts: &mut Vec<Value>,
    proxies: &mut Vec<Value>,
) -> Result<()> {
    match value {
        Value::Array(values) => {
            for value in values {
                collect_accounts(value, accounts, proxies)?;
            }
        }
        Value::Object(object) => {
            if object.contains_key("accounts") || object.contains_key("proxies") {
                if let Some(kind) = object.get("type").and_then(Value::as_str) {
                    if !matches!(kind, "sub2api-data" | "sub2api-bundle") {
                        return Err(Error::InvalidInput("unsupported import bundle type".into()));
                    }
                }
                if let Some(version) = object.get("version").and_then(Value::as_i64) {
                    if !matches!(version, 0 | 1) {
                        return Err(Error::InvalidInput(
                            "unsupported import bundle version".into(),
                        ));
                    }
                }
            }
            if let Some(values) = object.get("proxies").and_then(Value::as_array) {
                proxies.extend(values.iter().cloned());
            }
            if let Some(values) = object.get("accounts").and_then(Value::as_array) {
                for value in values {
                    collect_accounts(value.clone(), accounts, proxies)?;
                }
            } else {
                accounts.push(Value::Object(object));
            }
        }
        value => accounts.push(value),
    }
    Ok(())
}

fn parse_source(source: &str) -> Result<ParsedSource> {
    let mut accounts = Vec::new();
    let mut proxies = Vec::new();
    let json_values = serde_json::Deserializer::from_str(source)
        .into_iter::<Value>()
        .collect::<std::result::Result<Vec<_>, _>>();
    if let Ok(values) = json_values {
        for value in values {
            collect_accounts(value, &mut accounts, &mut proxies)?;
        }
    } else {
        for line in source
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
        {
            let value =
                serde_json::from_str(line).unwrap_or_else(|_| json!({"access_token": line}));
            collect_accounts(value, &mut accounts, &mut proxies)?;
        }
    }
    if accounts.len() > MAX_IMPORT_ACCOUNTS {
        return Err(Error::InvalidInput("import exceeds 2,000 accounts".into()));
    }
    Ok(ParsedSource { accounts, proxies })
}

fn parse_account(value: &Value) -> std::result::Result<ParsedAccount, String> {
    let token = field(value, "access_token")
        .or_else(|| field(value, "accessToken"))
        .or_else(|| value.get("tokens").and_then(|v| field(v, "access_token")))
        .or_else(|| value.get("tokens").and_then(|v| field(v, "accessToken")));
    let mut platform = field(value, "platform").unwrap_or("").to_ascii_lowercase();
    let mut account_type = field(value, "type").unwrap_or("").to_ascii_lowercase();
    let mut credentials =
        if let Some(credentials) = value.get("credentials").filter(|v| v.is_object()) {
            credentials.clone()
        } else if let Some(access_token) = token {
            platform = "openai".into();
            account_type = "oauth".into();
            let mut credentials = serde_json::Map::new();
            credentials.insert("access_token".into(), json!(access_token));
            for key in [
                "refresh_token",
                "refreshToken",
                "id_token",
                "idToken",
                "chatgpt_account_id",
                "chatgptAccountId",
                "chatgpt_account_is_fedramp",
            ] {
                if let Some(v) = value.get(key) {
                    credentials.insert(key.to_string(), v.clone());
                }
            }
            if let Some(tokens) = value.get("tokens").and_then(Value::as_object) {
                for (source, target) in [
                    ("refresh_token", "refresh_token"),
                    ("refreshToken", "refresh_token"),
                    ("id_token", "id_token"),
                    ("idToken", "id_token"),
                ] {
                    if let Some(token) = tokens.get(source).filter(|token| token.is_string()) {
                        credentials.insert(target.to_string(), token.clone());
                    }
                }
            }
            Value::Object(credentials)
        } else {
            return Err("missing credentials or access_token".into());
        };
    if platform.is_empty() || account_type.is_empty() {
        return Err("missing platform or type".into());
    }
    if !supported(&platform, &account_type) {
        return Err(format!("unsupported account: {platform}/{account_type}"));
    }
    if credentials.as_object().is_none_or(|v| v.is_empty()) {
        return Err("empty credentials".into());
    }
    if let Some(map) = credentials.as_object_mut() {
        for (source, target) in [
            ("accessToken", "access_token"),
            ("refreshToken", "refresh_token"),
            ("idToken", "id_token"),
            ("chatgptAccountId", "chatgpt_account_id"),
        ] {
            if !map.contains_key(target) {
                if let Some(value) = map.get(source).cloned() {
                    map.insert(target.to_string(), value);
                }
            }
        }
        map.remove("sessionToken");
        map.remove("session_token");
    }
    let access_expiry = credentials
        .get("access_token")
        .and_then(Value::as_str)
        .and_then(jwt_expiry);
    let has_refresh = credentials
        .get("refresh_token")
        .and_then(Value::as_str)
        .is_some_and(|token| !token.trim().is_empty());
    let expires_at = value
        .get("expires_at")
        .and_then(expiry_to_rfc3339)
        .or(access_expiry.map(|time| time.to_rfc3339()));
    if platform == "openai" && account_type == "oauth" && !has_refresh {
        if expires_at.is_none() {
            return Err(
                "OAuth access token without refresh_token requires a valid exp claim".into(),
            );
        }
        if access_expiry.is_some_and(|expiry| expiry <= chrono::Utc::now())
            || expires_at
                .as_deref()
                .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
                .is_some_and(|expiry| expiry.with_timezone(&chrono::Utc) <= chrono::Utc::now())
        {
            return Err("OAuth access token is expired and has no refresh_token".into());
        }
    }
    let identity = field(value, "user_id")
        .or_else(|| field(value, "userId"))
        .or_else(|| field(value, "account_id"))
        .or_else(|| field(value, "accountId"))
        .or_else(|| field(value, "id"))
        .or_else(|| field(value, "email"))
        .or_else(|| field(value, "name"))
        .map(str::to_owned)
        .unwrap_or_else(|| credentials.to_string());
    let fingerprint = digest(&format!("{platform}:{account_type}:{identity}"));
    Ok(ParsedAccount {
        name: field(value, "name")
            .or_else(|| field(value, "email"))
            .unwrap_or("")
            .to_string(),
        platform,
        account_type,
        credentials,
        extra: account_extra(value),
        proxy_key: field(value, "proxy_key").map(str::to_owned),
        concurrency: value
            .get("concurrency")
            .and_then(Value::as_i64)
            .unwrap_or(1)
            .clamp(1, 128),
        priority: value
            .get("priority")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            .clamp(-1000, 1000),
        expires_at,
        fingerprint,
    })
}

fn jwt_expiry(token: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let bytes = URL_SAFE_NO_PAD.decode(token.split('.').nth(1)?).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    chrono::DateTime::<chrono::Utc>::from_timestamp(value.get("exp")?.as_i64()?, 0)
}

fn account_extra(value: &Value) -> Value {
    let mut extra = value
        .get("extra")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    for key in [
        "email",
        "user_id",
        "userId",
        "account_id",
        "accountId",
        "plan",
        "plan_type",
        "planType",
        "package",
        "package_type",
        "packageType",
    ] {
        if let Some(item) = value
            .get(key)
            .filter(|item| item.is_string() || item.is_number() || item.is_boolean())
        {
            extra.insert(key.to_string(), item.clone());
        }
    }
    Value::Object(extra)
}

fn expiry_to_rfc3339(value: &Value) -> Option<String> {
    if let Some(value) = value.as_str() {
        return Some(value.to_string());
    }
    chrono::DateTime::<chrono::Utc>::from_timestamp(value.as_i64()?, 0)
        .map(|value| value.to_rfc3339())
}

fn valid_proxy_url(value: &str) -> bool {
    let value = value.trim();
    !value.chars().any(char::is_control)
        && (value.starts_with("http://")
            || value.starts_with("https://")
            || value.starts_with("socks5://")
            || value.starts_with("socks5h://"))
}

fn imported_proxy_url(proxy: &Value) -> Option<String> {
    if let Some(url) = field(proxy, "url") {
        let url = url.trim();
        let url = if let Some((scheme, rest)) = url.split_once("://") {
            format!("{}://{rest}", scheme.to_ascii_lowercase())
        } else {
            url.to_string()
        };
        return valid_proxy_url(&url).then_some(url);
    }
    let scheme = field(proxy, "scheme")
        .or_else(|| field(proxy, "protocol"))
        .unwrap_or("http")
        .to_ascii_lowercase();
    let host = field(proxy, "host")?;
    let port = proxy.get("port").and_then(Value::as_i64)?;
    let auth = match (field(proxy, "username"), field(proxy, "password")) {
        (Some(username), Some(password)) if !username.is_empty() || !password.is_empty() => {
            format!("{username}:{password}@")
        }
        _ => String::new(),
    };
    let url = format!("{scheme}://{auth}{host}:{port}");
    valid_proxy_url(&url).then_some(url)
}

fn public_proxy_config(value: Value) -> Value {
    // Password-bearing URLs stay encrypted in SQLite and never cross IPC.
    json!({"enabled": value.get("enabled").and_then(Value::as_bool).unwrap_or(false)})
}

fn merge_credentials(mut incoming: Value, existing: Option<Value>) -> Value {
    let (Some(incoming), Some(existing)) = (
        incoming.as_object_mut(),
        existing.and_then(|value| value.as_object().cloned()),
    ) else {
        return incoming;
    };
    for key in ["refresh_token", "client_id"] {
        let blank = incoming
            .get(key)
            .and_then(Value::as_str)
            .is_none_or(|value| value.trim().is_empty());
        if blank {
            if let Some(value) = existing
                .get(key)
                .filter(|value| value.as_str().is_some_and(|value| !value.trim().is_empty()))
            {
                incoming.insert(key.into(), value.clone());
            }
        }
    }
    incoming.remove("sessionToken");
    incoming.remove("session_token");
    Value::Object(incoming.clone())
}

fn preview(source: &str, existing: &std::collections::HashSet<String>) -> Result<ImportPreview> {
    if source.len() > MAX_IMPORT_BYTES {
        return Err(Error::InvalidInput("import exceeds 10 MiB".into()));
    }
    let parsed = parse_source(source)?;
    let proxy_count = parsed.proxies.len();
    let mut seen = std::collections::HashSet::new();
    Ok(ImportPreview {
        proxy_count,
        accounts: parsed
            .accounts
            .iter()
            .enumerate()
            .map(|(index, v)| match parse_account(v) {
                Ok(account) => {
                    let action = if !seen.insert(account.fingerprint.clone()) {
                        "skip"
                    } else if existing.contains(&account.fingerprint) {
                        "update"
                    } else {
                        "create"
                    };
                    ImportPreviewItem {
                        index,
                        name: account.name,
                        platform: account.platform,
                        account_type: account.account_type,
                        action: action.into(),
                        error: None,
                    }
                }
                Err(error) => ImportPreviewItem {
                    index,
                    name: field(v, "name").unwrap_or("").to_string(),
                    platform: field(v, "platform").unwrap_or("").to_string(),
                    account_type: field(v, "type").unwrap_or("").to_string(),
                    action: "reject".into(),
                    error: Some(error),
                },
            })
            .collect(),
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_codex_token_without_exposing_it() {
        let token = "eyJhbGciOiJub25lIn0.eyJleHAiOjQxMDI0NDQ4MDB9.";
        let account = parse_account(&json!({"access_token":token})).unwrap();
        assert_eq!(account.platform, "openai");
        assert_ne!(account.fingerprint, "secret");
    }
    #[test]
    fn rejects_unsupported_account() {
        assert!(
            parse_account(&json!({"platform":"grok","type":"oauth","credentials":{"x":"y"}}))
                .is_err()
        );
    }
    #[test]
    fn parses_jsonl_tokens() {
        let token = "eyJhbGciOiJub25lIn0.eyJleHAiOjQxMDI0NDQ4MDB9.";
        assert_eq!(
            preview(
                &format!("{token}\n{token}"),
                &std::collections::HashSet::new()
            )
            .unwrap()
            .accounts
            .len(),
            2
        );
    }

    #[test]
    fn accepts_nested_sub2api_bundle_and_socks5h() {
        let source = json!({"type":"sub2api-data","version":1,"proxies":[{"protocol":"socks5h","host":"127.0.0.1","port":1080}],"accounts":[[{"platform":"openai","type":"apikey","credentials":{"key":"x"}}]]}).to_string();
        let parsed = parse_source(&source).unwrap();
        assert_eq!(parsed.accounts.len(), 1);
        assert_eq!(
            imported_proxy_url(&parsed.proxies[0]).as_deref(),
            Some("socks5h://127.0.0.1:1080")
        );
    }

    #[test]
    fn import_keeps_existing_refresh_token_when_update_is_access_only() {
        let merged = merge_credentials(
            json!({"access_token": "new-access", "refresh_token": ""}),
            Some(
                json!({"access_token": "old-access", "refresh_token": "keep-me", "client_id": "keep-client"}),
            ),
        );
        assert_eq!(merged["refresh_token"], "keep-me");
        assert_eq!(merged["client_id"], "keep-client");
    }

    #[test]
    fn imported_proxy_accepts_socks5_without_exposing_password() {
        let proxy = json!({"protocol":"socks5", "host":"127.0.0.1", "port":1080, "username":"u", "password":"p"});
        assert_eq!(
            imported_proxy_url(&proxy).as_deref(),
            Some("socks5://u:p@127.0.0.1:1080")
        );
        assert_eq!(
            public_proxy_config(json!({"enabled":true, "url":"socks5://u:p@127.0.0.1:1080"})),
            json!({"enabled":true})
        );
    }

    #[test]
    fn account_identity_dedupes_and_batch_delete_is_scoped_to_provider() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::create_tables(&conn).unwrap();
        crate::db::apply_migrations(&conn).unwrap();
        conn.execute(
            "INSERT INTO user_providers (id,preset_name,name,created_at,updated_at) VALUES ('p1','sub2api','Pool','now','now')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO user_providers (id,preset_name,name,created_at,updated_at) VALUES ('p2','sub2api','Other','now','now')",
            [],
        )
        .unwrap();
        for (id, provider) in [("a1", "p1"), ("a2", "p2")] {
            conn.execute(
                "INSERT INTO provider_accounts (id,provider_id,platform,account_type,credentials_encrypted,dek_encrypted,identity_fingerprint,created_at,updated_at) VALUES (?1,?2,'openai','oauth','x','y',?3,'now','now')",
                params![id, provider, format!("identity-{provider}")],
            )
            .unwrap();
        }
        assert!(conn.execute("INSERT INTO provider_accounts (id,provider_id,platform,account_type,credentials_encrypted,dek_encrypted,identity_fingerprint,created_at,updated_at) VALUES ('duplicate','p1','openai','oauth','x','y','identity-p1','now','now')", []).is_err());
        assert_eq!(
            conn.execute(
                "DELETE FROM provider_accounts WHERE id='a2' AND provider_id='p1'",
                []
            )
            .unwrap(),
            0
        );
        assert_eq!(
            conn.execute(
                "DELETE FROM provider_accounts WHERE id='a1' AND provider_id='p1'",
                []
            )
            .unwrap(),
            1
        );
    }
}
