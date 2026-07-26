//! MCP connector configuration CRUD + JSON import (ADR-0016).
//!
//! This table is the trusted configuration source for `mcp_runtime`. Secrets
//! never land here: env values may be `secret:<id>` references (resolved by
//! the Host-side encrypted store at spawn time), plaintext Authorization
//! headers are rejected, and bearer/OAuth access tokens stay in the in-memory
//! `McpCredentialStore` via `mcp.auth.set`.

use super::store;
use agent_core::mcp::{McpServerConfig, McpTransport};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Map, Value};
use std::collections::HashMap;

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

const SELECT_COLS: &str = "id, name, transport, command, args_json, env_json, url, headers_json, \
     auth_mode, oauth_config_json, trusted, enabled, source, hub_ref, created_at, updated_at";

/// Env keys that look secret-bearing must use `secret:<id>` references so the
/// plaintext lives only in the Host encrypted store. Heuristic, documented in
/// ADR-0016; literal non-secret values (PATH, LANG…) pass through.
const SECRETLIKE_KEY_MARKERS: &[&str] = &["TOKEN", "KEY", "SECRET", "PASSWORD", "CREDENTIAL"];

fn row_to_json(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    let args: String = row.get("args_json")?;
    let env: String = row.get("env_json")?;
    let headers: String = row.get("headers_json")?;
    let oauth: String = row.get("oauth_config_json")?;
    // env values are either literals or secret refs; expose only key names +
    // whether each value is a secret ref. Never echo literal values back out.
    let env_map: HashMap<String, String> = serde_json::from_str(&env).unwrap_or_default();
    let env_summary: Vec<Value> = env_map
        .iter()
        .map(|(k, v)| {
            json!({
                "key": k,
                "isSecretRef": v.starts_with("secret:"),
            })
        })
        .collect();
    Ok(json!({
        "id": row.get::<_, String>("id")?,
        "name": row.get::<_, String>("name")?,
        "transport": row.get::<_, String>("transport")?,
        "command": row.get::<_, Option<String>>("command")?,
        "args": serde_json::from_str::<Value>(&args).unwrap_or_else(|_| json!([])),
        "env": env_summary,
        "url": row.get::<_, Option<String>>("url")?,
        "headerKeys": serde_json::from_str::<HashMap<String, String>>(&headers)
            .map(|m| m.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default(),
        "authMode": row.get::<_, String>("auth_mode")?,
        "oauthConfig": serde_json::from_str::<Value>(&oauth).unwrap_or_else(|_| json!({})),
        "trusted": row.get::<_, i64>("trusted")? != 0,
        "enabled": row.get::<_, i64>("enabled")? != 0,
        "source": row.get::<_, String>("source")?,
        "hubRef": row.get::<_, Option<String>>("hub_ref")?,
        "createdAt": row.get::<_, String>("created_at")?,
        "updatedAt": row.get::<_, String>("updated_at")?,
    }))
}

pub fn list(params_value: &Value) -> Result<Value, String> {
    let enabled_only = params_value
        .get("enabledOnly")
        .or_else(|| params_value.get("enabled_only"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let data = store()?;
    let conn = data.conn()?;
    let sql = if enabled_only {
        format!("SELECT {SELECT_COLS} FROM capability_mcp_server WHERE enabled = 1 ORDER BY name")
    } else {
        format!("SELECT {SELECT_COLS} FROM capability_mcp_server ORDER BY name")
    };
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let mut rows = stmt
        .query_map([], row_to_json)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    // Join live runtime status (single authority: mcp_runtime).
    let runtime = crate::mcp_runtime::global_mcp();
    for row in &mut rows {
        let id = row.get("id").and_then(Value::as_str).unwrap_or_default();
        row["runtimeStatus"] = json!(runtime.server_status(id).unwrap_or_else(|| "stopped".into()));
    }
    Ok(json!({ "servers": rows }))
}

pub fn get(params_value: &Value) -> Result<Value, String> {
    let id = required_str(params_value, "id")?;
    let data = store()?;
    let conn = data.conn()?;
    let server = conn
        .query_row(
            &format!("SELECT {SELECT_COLS} FROM capability_mcp_server WHERE id = ?1"),
            params![id],
            row_to_json,
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("mcp server not found: {id}"))?;
    Ok(json!({ "server": server }))
}

pub fn create(params_value: &Value) -> Result<Value, String> {
    let draft = validate_draft(params_value, None)?;
    let data = store()?;
    let conn = data.conn()?;
    let now = now_iso();
    conn.execute(
        "INSERT INTO capability_mcp_server
            (id, name, transport, command, args_json, env_json, url, headers_json,
             auth_mode, oauth_config_json, trusted, enabled, source, hub_ref,
             created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?15)",
        params![
            draft.id,
            draft.name,
            draft.transport,
            draft.command,
            draft.args_json,
            draft.env_json,
            draft.url,
            draft.headers_json,
            draft.auth_mode,
            draft.oauth_config_json,
            draft.trusted as i64,
            draft.enabled as i64,
            draft.source,
            draft.hub_ref,
            now,
        ],
    )
    .map_err(|e| {
        if e.to_string().contains("UNIQUE") {
            format!("mcp server id already exists: {}", draft.id)
        } else {
            e.to_string()
        }
    })?;
    drop(conn);
    sync_runtime_registration(&draft.id)?;
    get(&json!({ "id": draft.id }))
}

pub fn update(params_value: &Value) -> Result<Value, String> {
    let id = required_str(params_value, "id")?.to_string();
    let data = store()?;
    let conn = data.conn()?;
    let existing: Option<Value> = conn
        .query_row(
            &format!("SELECT {SELECT_COLS} FROM capability_mcp_server WHERE id = ?1"),
            params![id],
            |row| {
                // Re-read raw JSON columns for merge (row_to_json redacts env).
                let raw = json!({
                    "id": row.get::<_, String>("id")?,
                    "name": row.get::<_, String>("name")?,
                    "transport": row.get::<_, String>("transport")?,
                    "command": row.get::<_, Option<String>>("command")?,
                    "args": serde_json::from_str::<Value>(&row.get::<_, String>("args_json")?)
                        .unwrap_or_else(|_| json!([])),
                    "env": serde_json::from_str::<Value>(&row.get::<_, String>("env_json")?)
                        .unwrap_or_else(|_| json!({})),
                    "url": row.get::<_, Option<String>>("url")?,
                    "headers": serde_json::from_str::<Value>(&row.get::<_, String>("headers_json")?)
                        .unwrap_or_else(|_| json!({})),
                    "authMode": row.get::<_, String>("auth_mode")?,
                    "oauthConfig": serde_json::from_str::<Value>(
                        &row.get::<_, String>("oauth_config_json")?
                    ).unwrap_or_else(|_| json!({})),
                    "trusted": row.get::<_, i64>("trusted")? != 0,
                    "enabled": row.get::<_, i64>("enabled")? != 0,
                    "source": row.get::<_, String>("source")?,
                    "hubRef": row.get::<_, Option<String>>("hub_ref")?,
                });
                Ok(raw)
            },
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some(existing) = existing else {
        return Err(format!("mcp server not found: {id}"));
    };
    drop(conn);

    // Merge patch over existing raw values, then run full validation.
    let mut merged = existing.as_object().cloned().unwrap_or_default();
    for key in [
        "name",
        "transport",
        "command",
        "args",
        "env",
        "url",
        "headers",
        "authMode",
        "oauthConfig",
        "trusted",
        "enabled",
    ] {
        if let Some(v) = params_value.get(key) {
            merged.insert(key.to_string(), v.clone());
        }
    }
    merged.insert("id".into(), json!(id));
    let draft = validate_draft(&Value::Object(merged), Some(&id))?;

    // A running server must not mutate under its own feet: stop first.
    let runtime = crate::mcp_runtime::global_mcp();
    if matches!(runtime.server_status(&id).as_deref(), Some("running")) {
        runtime
            .stop(&id)
            .map_err(|e| format!("stop before update failed: {e}"))?;
    }

    let data = store()?;
    let conn = data.conn()?;
    let changed = conn
        .execute(
            "UPDATE capability_mcp_server SET
                name = ?2, transport = ?3, command = ?4, args_json = ?5, env_json = ?6,
                url = ?7, headers_json = ?8, auth_mode = ?9, oauth_config_json = ?10,
                trusted = ?11, enabled = ?12, updated_at = ?13
             WHERE id = ?1",
            params![
                id,
                draft.name,
                draft.transport,
                draft.command,
                draft.args_json,
                draft.env_json,
                draft.url,
                draft.headers_json,
                draft.auth_mode,
                draft.oauth_config_json,
                draft.trusted as i64,
                draft.enabled as i64,
                now_iso(),
            ],
        )
        .map_err(|e| e.to_string())?;
    if changed == 0 {
        return Err(format!("mcp server not found: {id}"));
    }
    drop(conn);
    sync_runtime_registration(&id)?;
    get(&json!({ "id": id }))
}

pub fn delete(params_value: &Value) -> Result<Value, String> {
    let id = required_str(params_value, "id")?;
    let runtime = crate::mcp_runtime::global_mcp();
    if matches!(runtime.server_status(id).as_deref(), Some("running")) {
        runtime
            .stop(id)
            .map_err(|e| format!("stop before delete failed: {e}"))?;
    }
    let data = store()?;
    let conn = data.conn()?;
    let changed = conn
        .execute("DELETE FROM capability_mcp_server WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    if changed == 0 {
        return Err(format!("mcp server not found: {id}"));
    }
    runtime.remove_server(id);
    Ok(json!({ "deleted": id }))
}

/// Import the standard `{"mcpServers": {name: {...}}}` format (Claude Desktop
/// / Cursor compatible). Imported servers default to untrusted + enabled.
pub fn import_json(params_value: &Value) -> Result<Value, String> {
    let raw = required_str(params_value, "json")?;
    let parsed: Value =
        serde_json::from_str(raw).map_err(|e| format!("invalid JSON: {e}"))?;
    let servers = parsed
        .get("mcpServers")
        .and_then(Value::as_object)
        .ok_or("missing mcpServers object")?;

    let mut imported = Vec::new();
    let mut errors = Vec::new();
    for (name, entry) in servers {
        let mut draft = Map::new();
        draft.insert("id".into(), json!(sanitize_id(name)));
        draft.insert("name".into(), json!(name));
        draft.insert("trusted".into(), json!(false));
        draft.insert("enabled".into(), json!(true));
        draft.insert("source".into(), json!("import_json"));
        if let Some(command) = entry.get("command") {
            draft.insert("transport".into(), json!("stdio"));
            draft.insert("command".into(), command.clone());
            if let Some(args) = entry.get("args") {
                draft.insert("args".into(), args.clone());
            }
            if let Some(env) = entry.get("env") {
                draft.insert("env".into(), env.clone());
            }
        } else if let Some(url) = entry.get("url") {
            let transport = match entry.get("type").and_then(Value::as_str) {
                Some("sse") => "sse",
                _ => "http",
            };
            draft.insert("transport".into(), json!(transport));
            draft.insert("url".into(), url.clone());
            if let Some(headers) = entry.get("headers") {
                draft.insert("headers".into(), headers.clone());
            }
        } else {
            errors.push(json!({ "name": name, "error": "entry needs command or url" }));
            continue;
        }
        match create(&Value::Object(draft)) {
            Ok(_) => imported.push(name.clone()),
            Err(e) => errors.push(json!({ "name": name, "error": e })),
        }
    }
    Ok(json!({
        "imported": imported,
        "skipped": errors.len(),
        "errors": errors,
    }))
}

/// Enabled connector configs projected into runtime form, for bootstrap and
/// for run-level resolution. Secret refs stay unresolved here — spawn-time
/// resolution goes through the Host broker (never persisted, never logged).
pub fn enabled_runtime_configs() -> Result<Vec<McpServerConfig>, String> {
    let data = store()?;
    let conn = data.conn()?;
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {SELECT_COLS} FROM capability_mcp_server WHERE enabled = 1"
        ))
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>("id")?,
                row.get::<_, String>("transport")?,
                row.get::<_, Option<String>>("command")?,
                row.get::<_, String>("args_json")?,
                row.get::<_, Option<String>>("url")?,
                row.get::<_, String>("headers_json")?,
                row.get::<_, i64>("trusted")?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let mut configs = Vec::new();
    for (id, transport, command, args_json, url, headers_json, trusted) in rows {
        let transport = match transport.as_str() {
            "stdio" => McpTransport::Stdio,
            "sse" => McpTransport::Sse,
            _ => McpTransport::Http,
        };
        let headers: HashMap<String, String> =
            serde_json::from_str(&headers_json).unwrap_or_default();
        configs.push(McpServerConfig {
            id,
            transport,
            command,
            args: serde_json::from_str(&args_json).ok(),
            url,
            trusted: trusted != 0,
            auth_token: None,
            headers: if headers.is_empty() { None } else { Some(headers) },
        });
    }
    Ok(configs)
}

/// Raw env map for one server (values may be `secret:<id>` references).
/// Callers resolving references must never persist or log the plaintext.
pub fn env_for_server(id: &str) -> Result<HashMap<String, String>, String> {
    let data = store()?;
    let conn = data.conn()?;
    let env_json: String = conn
        .query_row(
            "SELECT env_json FROM capability_mcp_server WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("mcp server not found: {id}"))?;
    Ok(serde_json::from_str(&env_json).unwrap_or_default())
}

/// Keep the runtime registry in sync after create/update. Registration errors
/// for untrusted configs are expected (registry enforces its own gates) and
/// reported to the caller.
fn sync_runtime_registration(id: &str) -> Result<(), String> {
    let configs = enabled_runtime_configs()?;
    let runtime = crate::mcp_runtime::global_mcp();
    match configs.into_iter().find(|c| c.id == id) {
        Some(config) => {
            // Untrusted registration failures are surfaced but do not roll
            // back the DB row — the row is the user's intent; the runtime
            // gate stays authoritative for execution.
            if let Err(error) = runtime.register_server(config) {
                eprintln!("[capability] runtime registration deferred for '{id}': {error}");
            }
        }
        None => runtime.remove_server(id),
    }
    Ok(())
}

struct ValidatedDraft {
    id: String,
    name: String,
    transport: String,
    command: Option<String>,
    args_json: String,
    env_json: String,
    url: Option<String>,
    headers_json: String,
    auth_mode: String,
    oauth_config_json: String,
    trusted: bool,
    enabled: bool,
    source: String,
    hub_ref: Option<String>,
}

fn validate_draft(params: &Value, existing_id: Option<&str>) -> Result<ValidatedDraft, String> {
    let id = match existing_id {
        Some(id) => id.to_string(),
        None => params
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| sanitize_id(params.get("name").and_then(Value::as_str).unwrap_or(""))),
    };
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err("id must be non-empty [a-zA-Z0-9_-]".into());
    }
    let name = required_str(params, "name")?.to_string();
    let transport = required_str(params, "transport")?;
    if !matches!(transport, "stdio" | "http" | "sse") {
        return Err(format!("invalid transport: {transport}"));
    }
    let command = params
        .get("command")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string);
    let url = params
        .get("url")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string);
    match transport {
        "stdio" => {
            if command.is_none() {
                return Err("stdio transport requires command".into());
            }
        }
        _ => {
            let Some(u) = url.as_deref() else {
                return Err("http/sse transport requires url".into());
            };
            if !(u.starts_with("http://") || u.starts_with("https://")) {
                return Err("url must be http(s)".into());
            }
        }
    }

    let args: Vec<String> = params
        .get("args")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default();

    let env: HashMap<String, String> = match params.get("env") {
        Some(Value::Object(map)) => {
            let mut out = HashMap::new();
            for (k, v) in map {
                let Some(v) = v.as_str() else {
                    return Err(format!("env value for {k} must be a string"));
                };
                let key_upper = k.to_ascii_uppercase();
                let secretlike = SECRETLIKE_KEY_MARKERS
                    .iter()
                    .any(|marker| key_upper.contains(marker));
                if secretlike && !v.starts_with("secret:") {
                    return Err(format!(
                        "env '{k}' looks secret-bearing; store it via capability:secret:set and pass a 'secret:<id>' reference"
                    ));
                }
                out.insert(k.clone(), v.to_string());
            }
            out
        }
        _ => HashMap::new(),
    };

    let headers: HashMap<String, String> = match params.get("headers") {
        Some(Value::Object(map)) => {
            let mut out = HashMap::new();
            for (k, v) in map {
                if k.eq_ignore_ascii_case("authorization") {
                    return Err(
                        "plaintext Authorization headers are rejected; use auth_mode bearer/oauth with mcp.auth.set".into(),
                    );
                }
                let Some(v) = v.as_str() else {
                    return Err(format!("header value for {k} must be a string"));
                };
                out.insert(k.clone(), v.to_string());
            }
            out
        }
        _ => HashMap::new(),
    };

    let auth_mode = params
        .get("authMode")
        .or_else(|| params.get("auth_mode"))
        .and_then(Value::as_str)
        .unwrap_or("none");
    if !matches!(auth_mode, "none" | "bearer" | "oauth") {
        return Err(format!("invalid auth_mode: {auth_mode}"));
    }
    let oauth_config = params
        .get("oauthConfig")
        .or_else(|| params.get("oauth_config"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    if let Some(obj) = oauth_config.as_object() {
        for key in obj.keys() {
            if key.to_ascii_lowercase().contains("secret") {
                return Err("oauth_config must not contain client secrets".into());
            }
        }
    }

    let source = params
        .get("source")
        .and_then(Value::as_str)
        .unwrap_or("manual");
    if !matches!(source, "manual" | "import_json" | "hub") {
        return Err(format!("invalid source: {source}"));
    }

    Ok(ValidatedDraft {
        id,
        name,
        transport: transport.to_string(),
        command,
        args_json: serde_json::to_string(&args).unwrap_or_else(|_| "[]".into()),
        env_json: serde_json::to_string(&env).unwrap_or_else(|_| "{}".into()),
        url,
        headers_json: serde_json::to_string(&headers).unwrap_or_else(|_| "{}".into()),
        auth_mode: auth_mode.to_string(),
        oauth_config_json: serde_json::to_string(&oauth_config).unwrap_or_else(|_| "{}".into()),
        trusted: params.get("trusted").and_then(Value::as_bool).unwrap_or(false),
        enabled: params.get("enabled").and_then(Value::as_bool).unwrap_or(true),
        source: source.to_string(),
        hub_ref: params
            .get("hubRef")
            .or_else(|| params.get("hub_ref"))
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

pub(super) fn sanitize_id(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn required_str<'a>(params: &'a Value, key: &str) -> Result<&'a str, String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| format!("{key} required"))
}
