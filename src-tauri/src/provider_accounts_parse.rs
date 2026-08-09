//! Sub2API account import parsing/validation helpers (W3 split from
//! provider_accounts.rs). Pure functions + import DTOs; command handlers stay
//! in `provider_accounts.rs`.

use crate::{Error, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub const MAX_IMPORT_BYTES: usize = 10 * 1024 * 1024;
pub const MAX_IMPORT_ACCOUNTS: usize = 2_000;

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

pub fn empty_object() -> Value {
    json!({})
}

#[derive(Clone)]
pub struct ParsedAccount {
    pub name: String,
    pub platform: String,
    pub account_type: String,
    pub credentials: Value,
    pub extra: Value,
    pub proxy_key: Option<String>,
    pub concurrency: i64,
    pub priority: i64,
    pub expires_at: Option<String>,
    pub fingerprint: String,
}

pub struct ParsedSource {
    pub accounts: Vec<Value>,
    pub proxies: Vec<Value>,
}

pub fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}
pub fn digest(input: &str) -> String {
    hex::encode(Sha256::digest(input.as_bytes()))
}

pub fn supported(platform: &str, account_type: &str) -> bool {
    matches!(
        (platform, account_type),
        ("openai", "oauth" | "apikey" | "upstream")
            | ("anthropic", "apikey" | "upstream")
            | ("gemini", "apikey" | "upstream")
    )
}

pub fn field<'a>(value: &'a Value, name: &str) -> Option<&'a str> {
    value.get(name)?.as_str()
}

pub fn collect_accounts(
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

pub fn parse_source(source: &str) -> Result<ParsedSource> {
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

pub fn parse_account(value: &Value) -> std::result::Result<ParsedAccount, String> {
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

pub fn jwt_expiry(token: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let bytes = URL_SAFE_NO_PAD.decode(token.split('.').nth(1)?).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    chrono::DateTime::<chrono::Utc>::from_timestamp(value.get("exp")?.as_i64()?, 0)
}

pub fn account_extra(value: &Value) -> Value {
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

pub fn expiry_to_rfc3339(value: &Value) -> Option<String> {
    if let Some(value) = value.as_str() {
        return Some(value.to_string());
    }
    chrono::DateTime::<chrono::Utc>::from_timestamp(value.as_i64()?, 0)
        .map(|value| value.to_rfc3339())
}

pub fn valid_proxy_url(value: &str) -> bool {
    let value = value.trim();
    !value.chars().any(char::is_control)
        && (value.starts_with("http://")
            || value.starts_with("https://")
            || value.starts_with("socks5://")
            || value.starts_with("socks5h://"))
}

pub fn imported_proxy_url(proxy: &Value) -> Option<String> {
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

pub fn public_proxy_config(value: Value) -> Value {
    // Password-bearing URLs stay encrypted in SQLite and never cross IPC.
    json!({"enabled": value.get("enabled").and_then(Value::as_bool).unwrap_or(false)})
}

pub fn merge_credentials(mut incoming: Value, existing: Option<Value>) -> Value {
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

pub fn preview(source: &str, existing: &std::collections::HashSet<String>) -> Result<ImportPreview> {
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
}
