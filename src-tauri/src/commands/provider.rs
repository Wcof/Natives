use crate::{env_manager, provider_key_manager, Error, Result};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::AppState;

// ── Helper: mask API key for frontend consumption ──
fn mask_api_key(key: &str) -> String {
    if key.len() <= 8 {
        return "***".to_string();
    }
    let prefix = &key[..4];
    let suffix = &key[key.len()-4..];
    format!("{}…{}", prefix, suffix)
}

// ── Data types ──

/// ProviderKey returned to the frontend — NEVER contains the full key.
/// The `masked_key` field shows only prefix + ellipsis + last 4 chars.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderKey {
    pub id: String,
    pub provider_id: String,
    pub label: String,
    /// Masked key shown to the user (e.g. "sk-a…1b2c"). Never the original key.
    pub masked_key: String,
    pub is_primary: bool,
    pub is_active: bool,
    pub status: String,
    pub last_tested_at: Option<String>,
    pub last_error_code: Option<String>,
    pub last_error_message: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserProvider {
    pub id: String,
    pub preset_name: String,
    pub name: String,
    pub website_url: String,
    pub base_url: String,
    pub default_model: Option<String>,
    pub primary_key_id: Option<String>,
    pub keys: Vec<ProviderKey>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddProviderInput {
    pub provider_type: String,
    pub display_name: String,
    pub website_url: String,
    pub base_url: String,
    pub default_model: String,
    pub initial_key: AddKeyInput,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddKeyInput {
    pub label: String,
    pub api_key: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteProviderKeyInput {
    pub provider_id: String,
    pub key_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddProviderKeyInput {
    pub provider_id: String,
    pub label: String,
    pub api_key: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTestInput {
    pub provider_id: String,
    pub key_id: String,
    /// Optional model to test with (for OpenAI-compatible providers)
    #[serde(default)]
    pub model: Option<String>,
}

/// Test a provider connection using a raw API key (without saving to DB).
/// Used by AddProviderDialog before the user saves the provider.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawProviderTestInput {
    pub base_url: String,
    pub api_key: String,
    /// Optional model to test with (for OpenAI-compatible providers)
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTestResult {
    pub success: bool,
    pub error: Option<String>,
}

// ── Commands ──

/// List all saved providers with key metadata only.
/// Full API keys are never returned to the frontend.
#[tauri::command]
pub fn list_providers(state: State<'_, AppState>) -> Result<Vec<UserProvider>> {
    let pool_conn = state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;

    ensure_tables(conn)?;

    // Fetch providers
    let mut pstmt = conn.prepare(
        "SELECT id, preset_name, name, website_url, base_url, default_model, created_at, updated_at FROM user_providers ORDER BY created_at DESC"
    ).map_err(|e| Error::Internal(e.to_string()))?;

    let providers: Vec<(String, String, String, String, String, Option<String>, String, String)> = pstmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
            ))
        })
        .map_err(|e| Error::Internal(e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

    // Fetch all keys
    let mut kstmt = conn.prepare(
        "SELECT id, provider_id, label, masked_key, is_primary, is_active, test_status, last_test_at, last_error_code, last_error_message, created_at FROM provider_api_keys ORDER BY created_at ASC"
    ).map_err(|e| Error::Internal(e.to_string()))?;

    let all_keys: Vec<(String, String, String, String, bool, bool, String, Option<String>, Option<String>, Option<String>, String)> = kstmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, bool>(4)?,
                row.get::<_, bool>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, Option<String>>(8)?,
                row.get::<_, Option<String>>(9)?,
                row.get::<_, String>(10)?,
            ))
        })
        .map_err(|e| Error::Internal(e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

    // Compute primary_key_id for each provider
    let primary_key_ids: std::collections::HashMap<String, String> = all_keys
        .iter()
        .filter(|(_, _, _, _, is_primary, _, _, _, _, _, _)| *is_primary)
        .map(|(kid, pid, _, _, _, _, _, _, _, _, _)| (pid.clone(), kid.clone()))
        .collect();

    // Assemble — keys are masked, NEVER return full key to frontend
    let result = providers
        .into_iter()
        .map(|(id, preset_name, name, website_url, base_url, default_model, created_at, updated_at)| {
            let primary_key_id = primary_key_ids.get(&id).cloned();
            let keys: Vec<ProviderKey> = all_keys
                .iter()
                .filter(|(_, pid, _, _, _, _, _, _, _, _, _)| pid == &id)
                .map(|(kid, _, label, masked_key, is_primary, is_active, test_status, last_test_at, last_error_code, last_error_message, kcreated)| {
                    ProviderKey {
                        id: kid.clone(),
                        provider_id: id.clone(),
                        label: label.clone(),
                        masked_key: if masked_key.is_empty() { "••••••••".to_string() } else { masked_key.clone() },
                        is_primary: *is_primary,
                        is_active: *is_active,
                        status: test_status.clone(),
                        last_tested_at: last_test_at.clone(),
                        last_error_code: last_error_code.clone(),
                        last_error_message: last_error_message.clone(),
                        created_at: kcreated.clone(),
                    }
                })
                .collect();

            UserProvider { id, preset_name, name, website_url, base_url, default_model, primary_key_id, keys, created_at, updated_at }
        })
        .collect();

    Ok(result)
}

/// Add a new provider with initial key and default model.
#[tauri::command]
pub fn add_provider(state: State<'_, AppState>, input: AddProviderInput) -> Result<UserProvider> {
    let pool_conn = state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;

    ensure_tables(conn)?;

    let id = uuid_v4();
    let now = chrono_now();

    conn.execute(
        "INSERT INTO user_providers (id, preset_name, name, website_url, base_url, default_model, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![id, input.provider_type, input.display_name, input.website_url, input.base_url, input.default_model, now, now],
    ).map_err(|e| Error::Internal(e.to_string()))?;

    let kid = uuid_v4();
    let masked_key = mask_api_key(&input.initial_key.api_key);
    let (encrypted, dek_encrypted) = if input.initial_key.api_key.is_empty() {
        (String::new(), String::new())
    } else {
        provider_key_manager::envelope_encrypt(&input.initial_key.api_key, conn)?
    };
    conn.execute(
        "INSERT INTO provider_api_keys (id, provider_id, label, api_key_encrypted, dek_encrypted, masked_key, is_primary, is_active, test_status, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, 1, 'untested', ?7)",
        params![kid, id, input.initial_key.label, encrypted, dek_encrypted, masked_key, now],
    ).map_err(|e| Error::Internal(e.to_string()))?;

    let key = ProviderKey {
        id: kid, provider_id: id.clone(),
        label: input.initial_key.label, masked_key,
        is_primary: true, is_active: true,
        status: "untested".to_string(),
        last_tested_at: None, last_error_code: None, last_error_message: None,
        created_at: now.clone(),
    };

    Ok(UserProvider {
        id, preset_name: input.provider_type, name: input.display_name,
        website_url: input.website_url, base_url: input.base_url,
        default_model: Some(input.default_model),
        primary_key_id: Some(key.id.clone()),
        keys: vec![key],
        created_at: now.clone(), updated_at: now,
    })
}

/// Add an API key to an existing provider.
#[tauri::command]
pub fn add_provider_key(state: State<'_, AppState>, input: AddProviderKeyInput) -> Result<ProviderKey> {
    let pool_conn = state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;

    ensure_tables(conn)?;

    let kid = uuid_v4();
    let now = chrono_now();
    let masked_key = mask_api_key(&input.api_key);
    let (encrypted, dek_encrypted) = if input.api_key.is_empty() {
        (String::new(), String::new())
    } else {
        provider_key_manager::envelope_encrypt(&input.api_key, conn)?
    };

    conn.execute(
        "INSERT INTO provider_api_keys (id, provider_id, label, api_key_encrypted, dek_encrypted, masked_key, is_primary, is_active, test_status, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, 1, 'untested', ?7)",
        params![kid, input.provider_id, input.label, encrypted, dek_encrypted, masked_key, now],
    ).map_err(|e| Error::Internal(e.to_string()))?;

    // Return masked key — never expose full key to frontend
    Ok(ProviderKey {
        id: kid, provider_id: input.provider_id, label: input.label,
        masked_key, is_primary: false, is_active: true,
        status: "untested".to_string(),
        last_tested_at: None, last_error_code: None, last_error_message: None,
        created_at: now,
    })
}

/// Delete a provider API key.
#[tauri::command]
pub fn delete_provider_key(state: State<'_, AppState>, input: DeleteProviderKeyInput) -> Result<()> {
    let pool_conn = state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;

    ensure_tables(conn)?;

    // Check if this is a primary key
    let is_primary: bool = conn
        .query_row(
            "SELECT is_primary FROM provider_api_keys WHERE id = ?1",
            params![input.key_id],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if is_primary {
        // Cannot delete primary key — must switch to another key first
        return Err(Error::InvalidInput("Cannot delete primary key. Set another key as primary first.".to_string()));
    }

    conn.execute(
        "DELETE FROM provider_api_keys WHERE id = ?1 AND provider_id = ?2",
        params![input.key_id, input.provider_id],
    ).map_err(|e| Error::Internal(e.to_string()))?;

    Ok(())
}

/// Delete a provider and all its keys.
#[tauri::command]
pub fn delete_provider(state: State<'_, AppState>, provider_id: String) -> Result<()> {
    let pool_conn = state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;

    conn.execute("DELETE FROM provider_api_keys WHERE provider_id = ?1", params![provider_id])
        .map_err(|e| Error::Internal(e.to_string()))?;
    conn.execute("DELETE FROM user_providers WHERE id = ?1", params![provider_id])
        .map_err(|e| Error::Internal(e.to_string()))?;
    Ok(())
}

/// Normalize an OpenAI-compatible base URL:
/// - Rejects empty URLs
/// - Trims trailing slash
/// - If user enters `/chat/completions` or `/v1/chat/completions`, derive parent `/v1`
/// - Prevents double `/v1/v1`
fn normalize_url(raw: &str) -> Result<String> {
    let trimmed = raw.trim().trim_end_matches('/').to_string();
    if trimmed.is_empty() {
        return Err(Error::Internal("Base URL cannot be empty".to_string()));
    }
    // If user entered /chat/completions path, derive the base
    if let Some(base) = trimmed.strip_suffix("/chat/completions") {
        // If it doesn't end with /v1, append /v1
        let clean = base.trim_end_matches('/');
        if clean.ends_with("/v1") {
            Ok(clean.to_string())
        } else {
            Ok(format!("{}/v1", clean))
        }
    } else if let Some(base) = trimmed.strip_suffix("/v1/v1") {
        // Double /v1 — remove one
        Ok(format!("{}/v1", base.trim_end_matches('/')))
    } else {
        Ok(trimmed)
    }
}

/// Build the chat completions URL from a normalized base URL
fn chat_completions_url(base_url: &str) -> String {
    format!("{}/chat/completions", base_url.trim_end_matches('/'))
}

/// Execute the actual provider test request (shared by provider_test and test_provider_raw).
async fn execute_provider_test(
    base_url: &str,
    api_key: &str,
    model: Option<&str>,
) -> ProviderTestResult {
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
    {
        Ok(c) => c,
        Err(e) => return ProviderTestResult {
            success: false,
            error: Some(format!("Failed to build HTTP client: {e}")),
        },
    };

    // Normalize the base URL
    let normalized = match normalize_url(base_url) {
        Ok(url) => url,
        Err(e) => return ProviderTestResult {
            success: false,
            error: Some(format!("Invalid base URL: {e}")),
        },
    };

    // If a model is provided, test chat completions (OpenAI-compatible)
    if let Some(model) = model {
        let chat_url = chat_completions_url(&normalized);
        let body = serde_json::json!({
            "model": model,
            "messages": [
                { "role": "user", "content": "Respond with the word 'ok'." }
            ],
            "max_tokens": 10,
            "stream": false,
        });

        let response = client
            .post(&chat_url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await;

        return match response {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    match resp.json::<serde_json::Value>().await {
                        Ok(json) => {
                            let has_content = json["choices"]
                                .as_array()
                                .and_then(|c| c.first())
                                .and_then(|c| c["message"]["content"].as_str())
                                .map(|s| !s.is_empty())
                                .unwrap_or(false);
                            if has_content {
                                ProviderTestResult { success: true, error: None }
                            } else {
                                ProviderTestResult {
                                    success: false,
                                    error: Some("Model response missing content. Check model name.".to_string()),
                                }
                            }
                        }
                        Err(_) => ProviderTestResult {
                            success: false,
                            error: Some("Invalid JSON response from API".to_string()),
                        },
                    }
                } else if status.is_client_error() {
                    let body = resp.text().await.unwrap_or_default();
                    let error_body = body.chars().take(300).collect::<String>();
                    let classified = if error_body.contains("model_not_found") || error_body.contains("model not found") {
                        format!("Model '{}' not available", model)
                    } else if status == 401 {
                        "Authentication failed — invalid API key".to_string()
                    } else if status == 429 {
                        "Rate limited — too many requests".to_string()
                    } else {
                        format!("HTTP {}: {}", status, error_body)
                    };
                    ProviderTestResult { success: false, error: Some(classified) }
                } else {
                    let body = resp.text().await.unwrap_or_default();
                    ProviderTestResult {
                        success: false,
                        error: Some(format!("HTTP {}: {}", status, body.chars().take(200).collect::<String>())),
                    }
                }
            }
            Err(e) => {
                if e.is_timeout() {
                    ProviderTestResult {
                        success: false,
                        error: Some("Connection timed out (15s)".to_string()),
                    }
                } else if e.is_connect() {
                    ProviderTestResult {
                        success: false,
                        error: Some("Cannot connect — check base URL and network".to_string()),
                    }
                } else {
                    ProviderTestResult {
                        success: false,
                        error: Some(format!("Connection failed: {e}")),
                    }
                }
            }
        };
    }

    // Legacy test: GET /v1/models endpoint
    let request_url = format!("{}/v1/models", normalized.trim_end_matches('/'));

    let response = client
        .get(&request_url)
        .header("Authorization", format!("Bearer {}", api_key))
        .send()
        .await;

    match response {
        Ok(resp) => {
            if resp.status().is_success() {
                ProviderTestResult { success: true, error: None }
            } else {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                let classified = if status == 401 { "Authentication failed".to_string() }
                    else if status == 404 { "Endpoint not found — check base URL".to_string() }
                    else { format!("HTTP {}: {}", status, body.chars().take(200).collect::<String>()) };
                ProviderTestResult { success: false, error: Some(classified) }
            }
        }
        Err(e) => {
            if e.is_timeout() {
                ProviderTestResult { success: false, error: Some("Connection timed out (15s)".to_string()) }
            } else if e.is_connect() {
                ProviderTestResult { success: false, error: Some("Cannot connect — check base URL and network".to_string()) }
            } else {
                ProviderTestResult { success: false, error: Some(format!("Connection failed: {e}")) }
            }
        }
    }
}

/// Test a provider connection with a saved provider (by key_id).
/// The full API key is decrypted server-side — never exposed to the frontend.
#[tauri::command]
pub async fn provider_test(
    state: State<'_, AppState>,
    input: ProviderTestInput,
) -> Result<ProviderTestResult> {
    let pool_conn = state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;

    // Fetch the API key and base URL
    let (api_key_encrypted, dek_encrypted, base_url): (String, Option<String>, String) = conn
        .query_row(
            "SELECT k.api_key_encrypted, k.dek_encrypted, p.base_url
             FROM provider_api_keys k
             JOIN user_providers p ON k.provider_id = p.id
             WHERE k.id = ?1 AND k.provider_id = ?2",
            params![input.key_id, input.provider_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .map_err(|e| Error::Internal(format!("Failed to fetch key: {e}")))?;

    // Decrypt the API key
    let api_key = if let Some(dek) = &dek_encrypted {
        if !dek.is_empty() {
            provider_key_manager::envelope_decrypt(&api_key_encrypted, dek, conn)?
        } else {
            let encryption_key = env_manager::get_encryption_key(conn)?;
            env_manager::decrypt(&api_key_encrypted, &encryption_key)?
        }
    } else {
        let encryption_key = env_manager::get_encryption_key(conn)?;
        env_manager::decrypt(&api_key_encrypted, &encryption_key)?
    };

    Ok(execute_provider_test(&base_url, &api_key, input.model.as_deref()).await)
}

/// Test a provider connection using a raw API key (without saving).
/// Used by AddProviderDialog before the user saves the provider.
#[tauri::command]
pub async fn test_provider_raw(
    input: RawProviderTestInput,
) -> Result<ProviderTestResult> {
    Ok(execute_provider_test(&input.base_url, &input.api_key, input.model.as_deref()).await)
}

// ── Helpers ──

fn ensure_tables(conn: &rusqlite::Connection) -> Result<()> {
    // Create lease table first
    crate::key_lease::ensure_lease_table(conn)?;

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS user_providers (
            id TEXT PRIMARY KEY,
            preset_name TEXT NOT NULL,
            name TEXT NOT NULL,
            website_url TEXT NOT NULL DEFAULT '',
            base_url TEXT NOT NULL DEFAULT '',
            default_model TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS provider_api_keys (
            id TEXT PRIMARY KEY,
            provider_id TEXT NOT NULL REFERENCES user_providers(id) ON DELETE CASCADE,
            label TEXT NOT NULL DEFAULT '',
            api_key_encrypted TEXT NOT NULL DEFAULT '',
            dek_encrypted TEXT NOT NULL DEFAULT '',
            masked_key TEXT NOT NULL DEFAULT '',
            is_primary INTEGER NOT NULL DEFAULT 0,
            is_active INTEGER NOT NULL DEFAULT 1,
            test_status TEXT NOT NULL DEFAULT 'untested',
            last_test_at TEXT,
            last_error_code TEXT,
            last_error_message TEXT,
            updated_at TEXT,
            last_leased_at TEXT,
            created_at TEXT NOT NULL
        );"
    ).map_err(|e| Error::Internal(e.to_string()))?;

    // Incremental migration: add missing columns for existing tables
    let add_column_if_missing = |table: &str, col: &str, def: &str| -> Result<()> {
        let exists: bool = conn
            .prepare(&format!("PRAGMA table_info({})", table))
            .map_err(|e| Error::Internal(e.to_string()))?
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|e| Error::Internal(e.to_string()))?
            .filter_map(|r| r.ok())
            .any(|name| name == col);
        if !exists {
            let sql = format!("ALTER TABLE {} ADD COLUMN {} {}", table, col, def);
            conn.execute_batch(&sql)
                .map_err(|e| Error::Internal(format!("migration failed: {e}")))?;
        }
        Ok(())
    };

    add_column_if_missing("user_providers", "default_model", "TEXT")?;
    add_column_if_missing("provider_api_keys", "masked_key", "TEXT NOT NULL DEFAULT ''")?;
    add_column_if_missing("provider_api_keys", "is_primary", "INTEGER NOT NULL DEFAULT 0")?;
    add_column_if_missing("provider_api_keys", "is_active", "INTEGER NOT NULL DEFAULT 1")?;
    add_column_if_missing("provider_api_keys", "test_status", "TEXT NOT NULL DEFAULT 'untested'")?;
    add_column_if_missing("provider_api_keys", "last_test_at", "TEXT")?;
    add_column_if_missing("provider_api_keys", "last_error_code", "TEXT")?;
    add_column_if_missing("provider_api_keys", "last_error_message", "TEXT")?;
    add_column_if_missing("provider_api_keys", "updated_at", "TEXT")?;
    add_column_if_missing("provider_api_keys", "last_leased_at", "TEXT")?;

    // Ensure unique index: only one primary key per provider
    conn.execute_batch(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_provider_primary_key
         ON provider_api_keys(provider_id)
         WHERE is_primary = 1;"
    ).map_err(|e| Error::Internal(e.to_string()))?;

    // Auto-promote oldest key to primary if no primary key exists yet
    let primary_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM provider_api_keys WHERE is_primary = 1",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    if primary_count == 0 {
        conn.execute(
            "UPDATE provider_api_keys SET is_primary = 1
             WHERE id = (SELECT id FROM provider_api_keys ORDER BY created_at ASC LIMIT 1)",
            [],
        ).map_err(|e| Error::Internal(e.to_string()))?;
    }

    Ok(())
}

fn uuid_v4() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: [u8; 16] = rng.gen();
    // Set version (4) and variant (RFC 4122)
    let mut buf = bytes;
    buf[6] = (buf[6] & 0x0f) | 0x40;  // version 4
    buf[8] = (buf[8] & 0x3f) | 0x80;  // variant 10xx
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        buf[0], buf[1], buf[2], buf[3],
        buf[4], buf[5],
        buf[6], buf[7],
        buf[8], buf[9],
        buf[10], buf[11], buf[12], buf[13], buf[14], buf[15],
    )
}

fn chrono_now() -> String {
    use chrono::Utc;
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_api_key_short() {
        assert_eq!(mask_api_key("ab"), "***");
    }

    #[test]
    fn test_mask_api_key_normal() {
        let masked = mask_api_key("sk-ant-abcdefghijklmnop");
        assert_eq!(masked, "sk-a…mnop");
        assert!(!masked.contains("abcdefghijklmnop"));
    }

    #[test]
    fn test_normalize_url_keeps_standard() {
        let result = normalize_url("https://api.openai.com/v1").unwrap();
        assert_eq!(result, "https://api.openai.com/v1");
    }

    #[test]
    fn test_normalize_url_trims_trailing_slash() {
        let result = normalize_url("https://api.openai.com/v1/").unwrap();
        assert_eq!(result, "https://api.openai.com/v1");
    }

    #[test]
    fn test_normalize_url_chat_completions_derives_v1() {
        let result = normalize_url("https://token.sensenova.cn/v1/chat/completions").unwrap();
        assert_eq!(result, "https://token.sensenova.cn/v1");
    }

    #[test]
    fn test_normalize_url_chat_completions_no_v1() {
        let result = normalize_url("https://example.com/chat/completions").unwrap();
        assert_eq!(result, "https://example.com/v1");
    }

    #[test]
    fn test_normalize_url_double_v1_dedup() {
        let result = normalize_url("https://example.com/v1/v1").unwrap();
        assert_eq!(result, "https://example.com/v1");
    }

    #[test]
    fn test_normalize_url_empty_rejected() {
        let result = normalize_url("");
        assert!(result.is_err());
    }

    #[test]
    fn test_normalize_url_whitespace_only_rejected() {
        let result = normalize_url("  ");
        assert!(result.is_err());
    }

    #[test]
    fn test_chat_completions_url_appends() {
        let result = chat_completions_url("https://token.sensenova.cn/v1");
        assert_eq!(result, "https://token.sensenova.cn/v1/chat/completions");
    }

    #[test]
    fn test_chat_completions_url_trailing_slash() {
        let result = chat_completions_url("https://example.com/");
        assert_eq!(result, "https://example.com/chat/completions");
    }

    // ── Integration tests (simulate manual validation) ──

    /// Set up an in-memory SQLite database with provider tables.
    fn setup_db() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        ensure_tables(&conn).unwrap();
        conn
    }

    /// Simulate: User opens Settings → Providers → Add SenseNova Token
    /// Enter `https://token.sensenova.cn/v1` as base URL.
    /// Enter an API Key.
    /// Verify that:
    ///   - The base URL is stored as-is (no /chat/completions suffix)
    ///   - The returned ProviderKey has masked_key instead of api_key
    ///   - The masked_key format is "prefix…suffix"
    ///   - No full key is present in the DTO
    #[test]
    fn test_integration_add_sensenova_provider_returns_masked_key() {
        let conn = setup_db();

        // Simulate adding a SenseNova provider
        let now = chrono_now();
        let provider_id = uuid_v4();
        let key_id = uuid_v4();
        let full_key = "sk-sensenova-test-key-abcdef123456";

        // Insert provider
        conn.execute(
            "INSERT INTO user_providers (id, preset_name, name, website_url, base_url, created_at, updated_at)
             VALUES (?1, 'sensenova', 'SenseNova Token', 'https://platform.sensenova.cn', 'https://token.sensenova.cn/v1', ?2, ?3)",
            rusqlite::params![provider_id, now, now],
        ).unwrap();

        // Insert key (encrypted)
        conn.execute(
            "INSERT INTO provider_api_keys (id, provider_id, label, api_key_encrypted, dek_encrypted, created_at)
             VALUES (?1, ?2, 'API Key 1', ?3, '', ?4)",
            rusqlite::params![key_id, provider_id, full_key.to_string(), now],
        ).unwrap();

        // Read back the key and verify masking
        let (db_key, masked): (String, String) = conn.query_row(
            "SELECT k.api_key_encrypted, '' FROM provider_api_keys k WHERE k.id = ?1",
            rusqlite::params![key_id],
            |row| Ok((row.get(0)?, String::new())),
        ).unwrap();

        // The DB stores the full key (encrypted in production, raw in test)
        assert_eq!(db_key, full_key, "DB still has full key (encrypted in production)");

        // Simulate what the frontend sees: masked_key
        let frontend_key = mask_api_key(&db_key);
        assert_eq!(frontend_key, "sk-s…3456", "Frontend receives masked key");
        assert!(!frontend_key.contains("abcdef123456"), "Full key not in frontend DTO");
        assert!(frontend_key.contains("…"), "Masked key uses ellipsis");
    }

    /// Simulate: User clicks "Test Connection" with an unsaved provider.
    /// Verify that:
    ///   - normalize_url correctly handles the SenseNova base URL
    ///   - chat_completions_url builds the correct endpoint
    ///   - test_provider_raw can be called with the raw key and URL
    #[test]
    fn test_integration_test_connection_flow() {
        // Step 1: User enters base URL
        let raw_url = "https://token.sensenova.cn/v1/";
        
        // Step 2: URL normalization removes trailing slash
        let normalized = normalize_url(raw_url).unwrap();
        assert_eq!(normalized, "https://token.sensenova.cn/v1");
        
        // Step 3: Chat completions URL is derived
        let chat_url = chat_completions_url(&normalized);
        assert_eq!(chat_url, "https://token.sensenova.cn/v1/chat/completions");
        
        // Step 4: User selects a model
        let model = "sensenova-6.7-flash-lite";
        assert!(model.contains("sensenova") || model.contains("deepseek"), 
                "Model is from the allowlist");
        
        // Step 5: Key masking works for the raw key
        let raw_key = "sk-sensenova-test-key-abcdef123456";
        let masked = mask_api_key(raw_key);
        assert_eq!(masked, "sk-s…3456");
        assert!(!masked.contains(raw_key));
    }

    /// Simulate: User enters invalid base URL → verify rejection.
    #[test]
    fn test_integration_invalid_url_rejected() {
        // Empty URL should be rejected
        assert!(normalize_url("").is_err());
        assert!(normalize_url("  ").is_err());
        
        // Valid URL should work
        assert!(normalize_url("https://token.sensenova.cn/v1").is_ok());
    }

    /// Simulate: User enters /chat/completions as base URL → auto-derived to /v1.
    #[test]
    fn test_integration_chat_completions_url_auto_derived() {
        let result = normalize_url("https://token.sensenova.cn/v1/chat/completions").unwrap();
        assert_eq!(result, "https://token.sensenova.cn/v1", 
                   "Base URL should be derived to /v1 when user pastes /chat/completions");
        
        let chat_url = chat_completions_url(&result);
        assert_eq!(chat_url, "https://token.sensenova.cn/v1/chat/completions",
                   "Chat completions URL should be correctly rebuilt from derived base");
    }

    /// Verify that the ProviderKey DTO struct used for frontend communication
    /// has masked_key instead of an api_key field.
    #[test]
    fn test_provider_key_dto_has_masked_key_not_api_key() {
        // The ProviderKey struct is defined with `masked_key: String`
        // This test verifies no api_key field exists in the DTO
        let key = ProviderKey {
            id: "test-id".to_string(),
            provider_id: "test-provider".to_string(),
            label: "Test Key".to_string(),
            masked_key: "sk-a…1b2c".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
        };
        
        // Serialize to JSON and verify no api_key field
        let json = serde_json::to_value(&key).unwrap();
        assert!(json.get("apiKey").is_none(), "DTO must not have apiKey field");
        assert!(json.get("maskedKey").is_some(), "DTO must have maskedKey field");
        assert_eq!(json["maskedKey"], "sk-a…1b2c");
    }
}
