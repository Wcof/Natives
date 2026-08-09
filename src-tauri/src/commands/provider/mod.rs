//! Provider configuration and connection-testing commands (Host).
//!
//! Split from a single 2256-line file into domain modules (ARCH-002):
//! `crud` (provider/key CRUD), `discover` (model discovery) and `test`
//! (connection testing) share the helpers below; each command is re-exported
//! so `commands::provider::*` paths in the invoke handler stay unchanged.

use crate::{env_manager, provider_key_manager, Error, Result};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::AppState;

mod crud;
mod discover;
mod test;

pub use crud::*;
pub use discover::*;
pub use test::*;

#[cfg(test)]
mod provider_tests;

fn mask_api_key(key: &str) -> String {
    if key.len() <= 8 {
        return "***".to_string();
    }
    let prefix = &key[..4];
    let suffix = &key[key.len() - 4..];
    format!("{}…{}", prefix, suffix)
}

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
    pub api_protocol: String,
    pub name: String,
    pub website_url: String,
    pub base_url: String,
    pub default_model: Option<String>,
    pub primary_key_id: Option<String>,
    pub keys: Vec<ProviderKey>,
    pub models: Vec<DiscoveredModel>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddProviderInput {
    pub provider_type: String,
    #[serde(default)]
    pub api_protocol: Option<String>,
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
    #[serde(default)]
    pub provider_type: String,
    #[serde(default)]
    pub api_protocol: Option<String>,
    pub base_url: String,
    pub api_key: String,
    /// Optional model to test with (for OpenAI-compatible providers)
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDiscoveryInput {
    #[serde(default)]
    pub provider_type: String,
    #[serde(default)]
    pub api_protocol: Option<String>,
    pub base_url: String,
    pub api_key: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDiscoverySavedInput {
    pub provider_id: String,
    pub key_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredModel {
    pub id: String,
    pub display_name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProviderDefaultsInput {
    pub provider_id: String,
    pub default_model: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetPrimaryKeyInput {
    pub provider_id: String,
    pub key_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTestResult {
    pub success: bool,
    pub error: Option<String>,
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


fn is_anthropic_protocol(provider_type: &str) -> bool {
    matches!(
        provider_type.trim().to_ascii_lowercase().as_str(),
        "anthropic" | "claude" | "anthropic_messages" | "anthropic-native"
    )
}


fn normalize_api_protocol(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "anthropic" | "claude" | "anthropic-native" | "anthropic_messages" => "anthropic_messages",
        "openai_responses" | "responses" => "openai_responses",
        "gemini" | "google" | "gemini_generate_content" => "gemini_generate_content",
        "ollama" | "ollama_chat" => "ollama_chat",
        "openai" | "openai_compatible" | "openai-compatible" | "openai_chat_completions" => {
            "openai_chat_completions"
        }
        other => other,
    }
    .to_string()
}


fn required_api_protocol(value: Option<&str>) -> Result<String> {
    let raw = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            Error::InvalidInput(
                "api_protocol is required; select a protocol explicitly".to_string(),
            )
        })?;
    let protocol = normalize_api_protocol(raw);
    match protocol.as_str() {
        "openai_chat_completions"
        | "openai_responses"
        | "anthropic_messages"
        | "gemini_generate_content"
        | "ollama_chat" => Ok(protocol),
        _ => Err(Error::InvalidInput(format!(
            "Unsupported api_protocol={protocol}; select an explicit supported protocol"
        ))),
    }
}


fn anthropic_url(base_url: &str, endpoint: &str) -> Result<String> {
    let base = base_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err(Error::Internal("Base URL cannot be empty".to_string()));
    }
    let base = base
        .strip_suffix("/v1/messages")
        .or_else(|| base.strip_suffix("/v1/models"))
        .unwrap_or(base);
    if base.ends_with("/v1") {
        Ok(format!("{base}/{endpoint}"))
    } else {
        Ok(format!("{base}/v1/{endpoint}"))
    }
}


fn provider_test_url(provider_type: &str, base_url: &str) -> Result<String> {
    match normalize_api_protocol(provider_type).as_str() {
        "anthropic_messages" => anthropic_url(base_url, "messages"),
        "openai_responses" => Ok(format!("{}/responses", normalize_url(base_url)?.trim_end_matches('/'))),
        "openai_chat_completions" => Ok(chat_completions_url(&normalize_url(base_url)?)),
        protocol => Err(Error::InvalidInput(format!(
            "Provider test unsupported for api_protocol={protocol}; select openai_chat_completions, openai_responses, or anthropic_messages"
        ))),
    }
}


fn models_url(provider_type: &str, base_url: &str) -> Result<String> {
    match normalize_api_protocol(provider_type).as_str() {
        "anthropic_messages" => anthropic_url(base_url, "models"),
        "openai_chat_completions" | "openai_responses" => {
            Ok(format!("{}/models", normalize_url(base_url)?.trim_end_matches('/')))
        }
        protocol => Err(Error::InvalidInput(format!(
            "Model discovery unsupported for api_protocol={protocol}; select openai_chat_completions, openai_responses, or anthropic_messages"
        ))),
    }
}


fn non_empty_text(value: &serde_json::Value) -> bool {
    value.as_str().is_some_and(|text| !text.trim().is_empty())
}

/// Whether a provider test response proves the endpoint/model is usable.
///
/// Reasoning-first OpenAI-compatible models (e.g. SenseNova deepseek-v4-flash)
/// often return empty `message.content` while filling `reasoning_content` /
/// `reasoning` when `max_tokens` is small. Treat those as success — the key,
/// base URL, protocol, and model are all valid. cc-switch connectivity checks
/// similarly accept any reachable HTTP response rather than requiring final
/// assistant text.
fn provider_response_has_content(provider_type: &str, value: &serde_json::Value) -> bool {
    match normalize_api_protocol(provider_type).as_str() {
        "anthropic_messages" => {
            // Prefer real text/thinking blocks; fall back to any well-formed content array
            // (connectivity proof). Auth/protocol/model are already validated by HTTP 200.
            value["content"].as_array().is_some_and(|blocks| {
                !blocks.is_empty()
                    && blocks.iter().any(|block| {
                        non_empty_text(&block["text"])
                            || non_empty_text(&block["thinking"])
                            || block.get("type").is_some()
                    })
            }) || (value.get("id").is_some() && value.get("model").is_some())
        }
        "openai_responses" => {
            non_empty_text(&value["output_text"])
                || value["output"]
                    .as_array()
                    .is_some_and(|items| !items.is_empty())
                || (value.get("id").is_some() && value.get("model").is_some())
        }
        _ => {
            let message = &value["choices"][0]["message"];
            let content = &message["content"];
            non_empty_text(content)
                || content.as_array().is_some_and(|blocks| {
                    blocks.iter().any(|block| non_empty_text(&block["text"]))
                })
                // Reasoning-only completion still proves connectivity/auth/model.
                || non_empty_text(&message["reasoning_content"])
                || non_empty_text(&message["reasoning"])
                // Some gateways put text under delta-shaped fields even on non-stream.
                || non_empty_text(&value["choices"][0]["text"])
                // Last resort: HTTP 200 with a well-formed choice is enough for "test connection".
                || value["choices"].as_array().is_some_and(|choices| {
                    !choices.is_empty() && choices[0].get("message").is_some()
                })
        }
    }
}


fn ensure_tables(conn: &rusqlite::Connection) -> Result<()> {
    // Create lease table first
    crate::key_lease::ensure_lease_table(conn)?;

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS user_providers (
            id TEXT PRIMARY KEY,
            preset_name TEXT NOT NULL,
            api_protocol TEXT NOT NULL DEFAULT 'openai_chat_completions',
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
        );",
    )
    .map_err(|e| Error::Internal(e.to_string()))?;

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
    add_column_if_missing(
        "user_providers",
        "api_protocol",
        "TEXT NOT NULL DEFAULT 'openai_chat_completions'",
    )?;
    conn.execute_batch(
        "UPDATE user_providers
         SET api_protocol = CASE
             WHEN lower(preset_name) IN ('anthropic', 'claude', 'anthropic_messages') THEN 'anthropic_messages'
             WHEN lower(preset_name) IN ('openai_responses', 'responses') THEN 'openai_responses'
             WHEN lower(preset_name) IN ('gemini', 'google', 'gemini_generate_content') THEN 'gemini_generate_content'
             WHEN lower(preset_name) IN ('ollama', 'ollama_chat') THEN 'ollama_chat'
             ELSE 'openai_chat_completions'
         END
         WHERE api_protocol IS NULL
            OR api_protocol = ''
            OR api_protocol IN ('openai_compatible', 'anthropic', 'claude');"
    ).map_err(|e| Error::Internal(e.to_string()))?;
    add_column_if_missing(
        "provider_api_keys",
        "masked_key",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    add_column_if_missing(
        "provider_api_keys",
        "is_primary",
        "INTEGER NOT NULL DEFAULT 0",
    )?;
    add_column_if_missing(
        "provider_api_keys",
        "is_active",
        "INTEGER NOT NULL DEFAULT 1",
    )?;
    add_column_if_missing(
        "provider_api_keys",
        "test_status",
        "TEXT NOT NULL DEFAULT 'untested'",
    )?;
    add_column_if_missing("provider_api_keys", "last_test_at", "TEXT")?;
    add_column_if_missing("provider_api_keys", "last_error_code", "TEXT")?;
    add_column_if_missing("provider_api_keys", "last_error_message", "TEXT")?;
    add_column_if_missing("provider_api_keys", "updated_at", "TEXT")?;
    add_column_if_missing("provider_api_keys", "last_leased_at", "TEXT")?;

    // Ensure unique index: only one primary key per provider
    conn.execute_batch(
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_provider_primary_key
         ON provider_api_keys(provider_id)
         WHERE is_primary = 1;",
    )
    .map_err(|e| Error::Internal(e.to_string()))?;

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
        )
        .map_err(|e| Error::Internal(e.to_string()))?;
    }

    Ok(())
}


fn uuid_v4() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let bytes: [u8; 16] = rng.gen();
    // Set version (4) and variant (RFC 4122)
    let mut buf = bytes;
    buf[6] = (buf[6] & 0x0f) | 0x40; // version 4
    buf[8] = (buf[8] & 0x3f) | 0x80; // variant 10xx
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
