use crate::{env_manager, provider_key_manager, Error, Result};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::AppState;

// ── Data types ──

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderKey {
    pub id: String,
    pub provider_id: String,
    pub label: String,
    pub api_key: String,
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
    pub keys: Vec<ProviderKey>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddProviderInput {
    pub preset_name: String,
    pub name: String,
    pub website_url: String,
    pub base_url: String,
    pub keys: Vec<AddKeyInput>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddKeyInput {
    pub label: String,
    pub api_key: String,
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
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTestResult {
    pub success: bool,
    pub error: Option<String>,
}

// ── Commands ──

/// List all saved providers with their API keys (decrypted).
#[tauri::command]
pub fn list_providers(state: State<'_, AppState>) -> Result<Vec<UserProvider>> {
    let pool_conn = state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;

    ensure_tables(conn)?;

    // Fetch providers
    let mut pstmt = conn.prepare(
        "SELECT id, preset_name, name, website_url, base_url, created_at, updated_at FROM user_providers ORDER BY created_at DESC"
    ).map_err(|e| Error::Internal(e.to_string()))?;

    let providers: Vec<(String, String, String, String, String, String, String)> = pstmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
            ))
        })
        .map_err(|e| Error::Internal(e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

    // Fetch all keys
    let mut kstmt = conn.prepare(
        "SELECT id, provider_id, label, api_key_encrypted, dek_encrypted, created_at FROM provider_api_keys ORDER BY created_at ASC"
    ).map_err(|e| Error::Internal(e.to_string()))?;

    let all_keys: Vec<(String, String, String, String, String, String)> = kstmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })
        .map_err(|e| Error::Internal(e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

    // Assemble
    let result = providers
        .into_iter()
        .map(|(id, preset_name, name, website_url, base_url, created_at, updated_at)| {
            let keys: Vec<ProviderKey> = all_keys
                .iter()
                .filter(|(_, pid, _, _, _, _)| pid == &id)
                .map(|(kid, _, label, encrypted, dek, kcreated)| {
                    let api_key = if !dek.is_empty() {
                        provider_key_manager::envelope_decrypt(encrypted, dek, conn).unwrap_or_default()
                    } else {
                        // Legacy fallback for keys without KEK-DEK
                        match env_manager::get_encryption_key(conn) {
                            Ok(ek) => env_manager::decrypt(encrypted, &ek).unwrap_or_default(),
                            Err(_) => String::new(),
                        }
                    };
                    ProviderKey {
                        id: kid.clone(),
                        provider_id: id.clone(),
                        label: label.clone(),
                        api_key,
                        created_at: kcreated.clone(),
                    }
                })
                .collect();

            UserProvider { id, preset_name, name, website_url, base_url, keys, created_at, updated_at }
        })
        .collect();

    Ok(result)
}

/// Add a new provider with optional initial keys.
#[tauri::command]
pub fn add_provider(state: State<'_, AppState>, input: AddProviderInput) -> Result<UserProvider> {
    let pool_conn = state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;

    ensure_tables(conn)?;

    let id = uuid_v4();
    let now = chrono_now();

    conn.execute(
        "INSERT INTO user_providers (id, preset_name, name, website_url, base_url, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![id, input.preset_name, input.name, input.website_url, input.base_url, now, now],
    ).map_err(|e| Error::Internal(e.to_string()))?;

    let mut keys = Vec::new();
    for kin in input.keys {
        let kid = uuid_v4();
        let (encrypted, dek_encrypted) = if kin.api_key.is_empty() {
            (String::new(), String::new())
        } else {
            provider_key_manager::envelope_encrypt(&kin.api_key, conn)?
        };
        conn.execute(
            "INSERT INTO provider_api_keys (id, provider_id, label, api_key_encrypted, dek_encrypted, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![kid, id, kin.label, encrypted, dek_encrypted, now],
        ).map_err(|e| Error::Internal(e.to_string()))?;
        keys.push(ProviderKey { id: kid, provider_id: id.clone(), label: kin.label, api_key: kin.api_key, created_at: now.clone() });
    }

    Ok(UserProvider { id, preset_name: input.preset_name, name: input.name, website_url: input.website_url, base_url: input.base_url, keys, created_at: now.clone(), updated_at: now })
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
    let (encrypted, dek_encrypted) = if input.api_key.is_empty() {
        (String::new(), String::new())
    } else {
        provider_key_manager::envelope_encrypt(&input.api_key, conn)?
    };

    conn.execute(
        "INSERT INTO provider_api_keys (id, provider_id, label, api_key_encrypted, dek_encrypted, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![kid, input.provider_id, input.label, encrypted, dek_encrypted, now],
    ).map_err(|e| Error::Internal(e.to_string()))?;

    Ok(ProviderKey { id: kid, provider_id: input.provider_id, label: input.label, api_key: input.api_key, created_at: now })
}

/// Delete a provider key by ID.
#[tauri::command]
pub fn delete_provider_key(state: State<'_, AppState>, id: String) -> Result<()> {
    let pool_conn = state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;

    conn.execute("DELETE FROM provider_api_keys WHERE id = ?1", params![id])
        .map_err(|e| Error::Internal(e.to_string()))?;
    Ok(())
}

/// Delete a provider and all its keys.
#[tauri::command]
pub fn delete_provider(state: State<'_, AppState>, id: String) -> Result<()> {
    let pool_conn = state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;

    conn.execute("DELETE FROM provider_api_keys WHERE provider_id = ?1", params![id])
        .map_err(|e| Error::Internal(e.to_string()))?;
    conn.execute("DELETE FROM user_providers WHERE id = ?1", params![id])
        .map_err(|e| Error::Internal(e.to_string()))?;
    Ok(())
}

/// Test a provider connection by making a minimal API request.
/// The test is executed entirely in Rust, never exposing the API key to the frontend.
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
            // Fallback to legacy encryption
            let encryption_key = env_manager::get_encryption_key(conn)?;
            env_manager::decrypt(&api_key_encrypted, &encryption_key)?
        }
    } else {
        let encryption_key = env_manager::get_encryption_key(conn)?;
        env_manager::decrypt(&api_key_encrypted, &encryption_key)?
    };

    // Make the test request via Rust (NEVER exposing key to frontend)
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| Error::Internal(format!("Failed to build client: {e}")))?;

    let request_url = format!("{}/v1/models", base_url.trim_end_matches('/'));

    let response = client
        .get(&request_url)
        .header("Authorization", format!("Bearer {}", api_key))
        .send()
        .await;

    match response {
        Ok(resp) => {
            if resp.status().is_success() {
                Ok(ProviderTestResult {
                    success: true,
                    error: None,
                })
            } else {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                Ok(ProviderTestResult {
                    success: false,
                    error: Some(format!("HTTP {}: {}", status, body.chars().take(200).collect::<String>())),
                })
            }
        }
        Err(e) => Ok(ProviderTestResult {
            success: false,
            error: Some(format!("Connection failed: {e}")),
        }),
    }
}

// ── Helpers ──

fn ensure_tables(conn: &rusqlite::Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS user_providers (
            id TEXT PRIMARY KEY,
            preset_name TEXT NOT NULL,
            name TEXT NOT NULL,
            website_url TEXT NOT NULL DEFAULT '',
            base_url TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS provider_api_keys (
            id TEXT PRIMARY KEY,
            provider_id TEXT NOT NULL REFERENCES user_providers(id) ON DELETE CASCADE,
            label TEXT NOT NULL DEFAULT '',
            api_key_encrypted TEXT NOT NULL DEFAULT '',
            dek_encrypted TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL
        );"
    ).map_err(|e| Error::Internal(e.to_string()))
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
