use crate::{env_manager, Error, Result};
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

// ── Commands ──

/// List all saved providers with their API keys (decrypted).
#[tauri::command]
pub fn list_providers(state: State<'_, AppState>) -> Result<Vec<UserProvider>> {
    let pool_conn = state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;

    ensure_tables(conn)?;
    let encryption_key = env_manager::get_encryption_key(conn)?;

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
        "SELECT id, provider_id, label, api_key_encrypted, created_at FROM provider_api_keys ORDER BY created_at ASC"
    ).map_err(|e| Error::Internal(e.to_string()))?;

    let all_keys: Vec<(String, String, String, String, String)> = kstmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
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
                .filter(|(_, pid, _, _, _)| pid == &id)
                .map(|(kid, _, label, encrypted, kcreated)| {
                    let api_key = env_manager::decrypt(encrypted, &encryption_key).unwrap_or_default();
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
    let encryption_key = env_manager::get_encryption_key(conn)?;

    let id = uuid_v4();
    let now = chrono_now();

    conn.execute(
        "INSERT INTO user_providers (id, preset_name, name, website_url, base_url, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![id, input.preset_name, input.name, input.website_url, input.base_url, now, now],
    ).map_err(|e| Error::Internal(e.to_string()))?;

    let mut keys = Vec::new();
    for kin in input.keys {
        let kid = uuid_v4();
        let encrypted = if kin.api_key.is_empty() {
            String::new()
        } else {
            env_manager::encrypt(&kin.api_key, &encryption_key)?
        };
        conn.execute(
            "INSERT INTO provider_api_keys (id, provider_id, label, api_key_encrypted, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![kid, id, kin.label, encrypted, now],
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
    let encryption_key = env_manager::get_encryption_key(conn)?;

    let kid = uuid_v4();
    let now = chrono_now();
    let encrypted = if input.api_key.is_empty() {
        String::new()
    } else {
        env_manager::encrypt(&input.api_key, &encryption_key)?
    };

    conn.execute(
        "INSERT INTO provider_api_keys (id, provider_id, label, api_key_encrypted, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![kid, input.provider_id, input.label, encrypted, now],
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
