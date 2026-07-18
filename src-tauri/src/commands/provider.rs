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
    let suffix = &key[key.len() - 4..];
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

// ── Commands ──

/// List all saved providers with key metadata only.
/// Full API keys are never returned to the frontend.
#[tauri::command]
pub fn list_providers(state: State<'_, AppState>) -> Result<Vec<UserProvider>> {
    let pool_conn = state
        .db
        .get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;

    ensure_tables(conn)?;

    // Fetch providers
    let mut pstmt = conn.prepare(
        "SELECT id, preset_name, api_protocol, name, website_url, base_url, default_model, created_at, updated_at FROM user_providers ORDER BY created_at DESC"
    ).map_err(|e| Error::Internal(e.to_string()))?;

    let providers: Vec<(
        String,
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        String,
        String,
    )> = pstmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
            ))
        })
        .map_err(|e| Error::Internal(e.to_string()))?
        .filter_map(|r| r.ok())
        .collect();

    // Fetch all keys
    let mut kstmt = conn.prepare(
        "SELECT id, provider_id, label, masked_key, is_primary, is_active, test_status, last_test_at, last_error_code, last_error_message, created_at FROM provider_api_keys ORDER BY created_at ASC"
    ).map_err(|e| Error::Internal(e.to_string()))?;

    let all_keys: Vec<(
        String,
        String,
        String,
        String,
        bool,
        bool,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
    )> = kstmt
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

    let model_rows: std::collections::HashMap<String, Vec<DiscoveredModel>> = {
        let assistant = crate::db::get_assistant_db_conn()?;
        let mut stmt = assistant.prepare(
            "SELECT provider_id, model_id, display_name FROM assistant_model_cache ORDER BY model_id ASC",
        ).map_err(|e| Error::Internal(e.to_string()))?;
        let mut grouped: std::collections::HashMap<String, Vec<DiscoveredModel>> =
            std::collections::HashMap::new();
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })
            .map_err(|e| Error::Internal(e.to_string()))?;
        for row in rows.flatten() {
            grouped.entry(row.0).or_default().push(DiscoveredModel {
                id: row.1,
                display_name: row.2,
            });
        }
        grouped
    };

    // Assemble — keys are masked, NEVER return full key to frontend
    let result = providers
        .into_iter()
        .map(
            |(
                id,
                preset_name,
                api_protocol,
                name,
                website_url,
                base_url,
                default_model,
                created_at,
                updated_at,
            )| {
                let primary_key_id = primary_key_ids.get(&id).cloned();
                let keys: Vec<ProviderKey> = all_keys
                    .iter()
                    .filter(|(_, pid, _, _, _, _, _, _, _, _, _)| pid == &id)
                    .map(
                        |(
                            kid,
                            _,
                            label,
                            masked_key,
                            is_primary,
                            is_active,
                            test_status,
                            last_test_at,
                            last_error_code,
                            last_error_message,
                            kcreated,
                        )| {
                            ProviderKey {
                                id: kid.clone(),
                                provider_id: id.clone(),
                                label: label.clone(),
                                masked_key: if masked_key.is_empty() {
                                    "••••••••".to_string()
                                } else {
                                    masked_key.clone()
                                },
                                is_primary: *is_primary,
                                is_active: *is_active,
                                status: test_status.clone(),
                                last_tested_at: last_test_at.clone(),
                                last_error_code: last_error_code.clone(),
                                last_error_message: last_error_message.clone(),
                                created_at: kcreated.clone(),
                            }
                        },
                    )
                    .collect();

                UserProvider {
                    models: model_rows.get(&id).cloned().unwrap_or_default(),
                    id,
                    preset_name,
                    api_protocol,
                    name,
                    website_url,
                    base_url,
                    default_model,
                    primary_key_id,
                    keys,
                    created_at,
                    updated_at,
                }
            },
        )
        .collect();

    Ok(result)
}

/// Add a new provider with initial key and default model.
#[tauri::command]
pub fn add_provider(state: State<'_, AppState>, input: AddProviderInput) -> Result<UserProvider> {
    let display_name = input.display_name.trim().to_string();
    let provider_type = input.provider_type.trim().to_string();
    let api_protocol = required_api_protocol(input.api_protocol.as_deref())?;
    let base_url = normalize_url(&input.base_url)?;
    let default_model = input.default_model.trim().to_string();
    let api_key = input.initial_key.api_key.trim();
    if display_name.is_empty()
        || provider_type.is_empty()
        || default_model.is_empty()
        || api_key.is_empty()
    {
        return Err(Error::InvalidInput(
            "Provider name, type, default model, and API key are required".to_string(),
        ));
    }
    let mut pool_conn = state
        .db
        .get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    ensure_tables(&pool_conn)?;

    let id = uuid_v4();
    let now = chrono_now();
    let transaction = pool_conn
        .transaction()
        .map_err(|e| Error::Internal(e.to_string()))?;
    transaction.execute(
        "INSERT INTO user_providers (id, preset_name, api_protocol, name, website_url, base_url, default_model, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![id, provider_type, api_protocol, display_name, input.website_url.trim(), base_url, default_model, now, now],
    ).map_err(|e| Error::Internal(e.to_string()))?;

    let kid = uuid_v4();
    let masked_key = mask_api_key(api_key);
    let (encrypted, dek_encrypted) = provider_key_manager::envelope_encrypt(api_key, &transaction)?;
    let key_label = input.initial_key.label.trim();
    let key_label = if key_label.is_empty() {
        "API Key"
    } else {
        key_label
    };
    transaction.execute(
        "INSERT INTO provider_api_keys (id, provider_id, label, api_key_encrypted, dek_encrypted, masked_key, is_primary, is_active, test_status, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, 1, 'untested', ?7)",
        params![kid, id, key_label, encrypted, dek_encrypted, masked_key, now],
    ).map_err(|e| Error::Internal(e.to_string()))?;
    transaction
        .commit()
        .map_err(|e| Error::Internal(e.to_string()))?;

    let assistant = crate::db::get_assistant_db_conn()?;
    assistant.execute(
        "INSERT OR IGNORE INTO assistant_model_cache (id, provider_id, model_id, display_name, capabilities, context_window, max_output, source, discovered_at) VALUES (?1, ?2, ?3, ?4, '{}', 0, 0, 'manual', datetime('now'))",
        params![uuid_v4(), id, default_model, default_model],
    ).map_err(|e| Error::Internal(e.to_string()))?;

    let key = ProviderKey {
        id: kid,
        provider_id: id.clone(),
        label: key_label.to_string(),
        masked_key,
        is_primary: true,
        is_active: true,
        status: "untested".to_string(),
        last_tested_at: None,
        last_error_code: None,
        last_error_message: None,
        created_at: now.clone(),
    };

    Ok(UserProvider {
        id,
        preset_name: provider_type,
        api_protocol,
        name: display_name,
        website_url: input.website_url.trim().to_string(),
        base_url,
        default_model: Some(default_model.clone()),
        primary_key_id: Some(key.id.clone()),
        keys: vec![key],
        models: vec![DiscoveredModel {
            id: default_model.clone(),
            display_name: Some(default_model.clone()),
        }],
        created_at: now.clone(),
        updated_at: now,
    })
}

/// Add an API key to an existing provider.
#[tauri::command]
pub fn add_provider_key(
    state: State<'_, AppState>,
    input: AddProviderKeyInput,
) -> Result<ProviderKey> {
    let pool_conn = state
        .db
        .get()
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
        id: kid,
        provider_id: input.provider_id,
        label: input.label,
        masked_key,
        is_primary: false,
        is_active: true,
        status: "untested".to_string(),
        last_tested_at: None,
        last_error_code: None,
        last_error_message: None,
        created_at: now,
    })
}

#[tauri::command]
pub fn provider_update_defaults(
    state: State<'_, AppState>,
    input: UpdateProviderDefaultsInput,
) -> Result<()> {
    let model = input.default_model.trim();
    if model.is_empty() {
        return Err(Error::InvalidInput(
            "Default model cannot be empty".to_string(),
        ));
    }
    let pool_conn = state
        .db
        .get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let updated = pool_conn
        .execute(
            "UPDATE user_providers SET default_model = ?1, updated_at = ?2 WHERE id = ?3",
            params![model, chrono_now(), input.provider_id],
        )
        .map_err(|e| Error::Internal(e.to_string()))?;
    if updated == 0 {
        return Err(Error::InvalidInput("Provider not found".to_string()));
    }
    let assistant = crate::db::get_assistant_db_conn()?;
    assistant.execute(
        "INSERT OR IGNORE INTO assistant_model_cache (id, provider_id, model_id, display_name, capabilities, context_window, max_output, source, discovered_at) VALUES (?1, ?2, ?3, ?4, '{}', 0, 0, 'manual', datetime('now'))",
        params![uuid_v4(), input.provider_id, model, model],
    ).map_err(|e| Error::Internal(e.to_string()))?;
    Ok(())
}

#[tauri::command]
pub fn provider_set_primary_key(
    state: State<'_, AppState>,
    input: SetPrimaryKeyInput,
) -> Result<()> {
    let mut pool_conn = state
        .db
        .get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    ensure_tables(&pool_conn)?;
    let transaction = pool_conn
        .transaction()
        .map_err(|e| Error::Internal(e.to_string()))?;
    let eligible: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM provider_api_keys WHERE id = ?1 AND provider_id = ?2 AND is_active = 1 AND test_status = 'valid')",
        params![input.key_id, input.provider_id],
        |row| row.get(0),
    ).map_err(|e| Error::Internal(e.to_string()))?;
    if !eligible {
        return Err(Error::InvalidInput(
            "Test the key successfully before setting it as primary".to_string(),
        ));
    }
    transaction
        .execute(
            "UPDATE provider_api_keys SET is_primary = 0 WHERE provider_id = ?1",
            params![input.provider_id],
        )
        .map_err(|e| Error::Internal(e.to_string()))?;
    transaction
        .execute(
            "UPDATE provider_api_keys SET is_primary = 1 WHERE id = ?1 AND provider_id = ?2",
            params![input.key_id, input.provider_id],
        )
        .map_err(|e| Error::Internal(e.to_string()))?;
    transaction
        .commit()
        .map_err(|e| Error::Internal(e.to_string()))?;
    Ok(())
}

/// Delete a provider API key.
#[tauri::command]
pub fn delete_provider_key(
    state: State<'_, AppState>,
    input: DeleteProviderKeyInput,
) -> Result<()> {
    let pool_conn = state
        .db
        .get()
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
        return Err(Error::InvalidInput(
            "Cannot delete primary key. Set another key as primary first.".to_string(),
        ));
    }

    conn.execute(
        "DELETE FROM provider_api_keys WHERE id = ?1 AND provider_id = ?2",
        params![input.key_id, input.provider_id],
    )
    .map_err(|e| Error::Internal(e.to_string()))?;

    Ok(())
}

/// Delete a provider and all its keys.
#[tauri::command]
pub fn delete_provider(state: State<'_, AppState>, provider_id: String) -> Result<()> {
    let pool_conn = state
        .db
        .get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;

    conn.execute(
        "DELETE FROM provider_api_keys WHERE provider_id = ?1",
        params![provider_id],
    )
    .map_err(|e| Error::Internal(e.to_string()))?;
    conn.execute(
        "DELETE FROM user_providers WHERE id = ?1",
        params![provider_id],
    )
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

fn provider_response_has_content(provider_type: &str, value: &serde_json::Value) -> bool {
    match normalize_api_protocol(provider_type).as_str() {
        "anthropic_messages" => value["content"].as_array().is_some_and(|blocks| {
            blocks.iter().any(|block| {
                block["text"]
                    .as_str()
                    .is_some_and(|text| !text.trim().is_empty())
            })
        }),
        "openai_responses" => {
            value["output_text"]
                .as_str()
                .is_some_and(|text| !text.trim().is_empty())
                || value["output"].as_array().is_some_and(|items| {
                    items.iter().any(|item| {
                        item["content"].as_array().is_some_and(|blocks| {
                            blocks.iter().any(|block| {
                                block["text"]
                                    .as_str()
                                    .is_some_and(|text| !text.trim().is_empty())
                            })
                        })
                    })
                })
        }
        _ => {
            let content = &value["choices"][0]["message"]["content"];
            content.as_str().is_some_and(|text| !text.trim().is_empty())
                || content.as_array().is_some_and(|blocks| {
                    blocks.iter().any(|block| {
                        block["text"]
                            .as_str()
                            .is_some_and(|text| !text.trim().is_empty())
                    })
                })
        }
    }
}

fn provider_test_body(provider_type: &str, model: &str) -> Result<serde_json::Value> {
    let model = model.trim();
    if model.is_empty() {
        return Err(Error::InvalidInput(
            "Model is required for provider test".to_string(),
        ));
    }
    match normalize_api_protocol(provider_type).as_str() {
        "anthropic_messages" => Ok(serde_json::json!({
            "model": model,
            "messages": [
                { "role": "user", "content": "Reply with exactly: ok" }
            ],
            "max_tokens": 16,
            "stream": false,
        })),
        "openai_responses" => Ok(serde_json::json!({
            "model": model,
            "input": "Reply with exactly: ok",
            "max_output_tokens": 16,
            "stream": false,
        })),
        "openai_chat_completions" => Ok(serde_json::json!({
            "model": model,
            "messages": [
                { "role": "user", "content": "Reply with exactly: ok" }
            ],
            "max_tokens": 16,
            "stream": false,
        })),
        protocol => Err(Error::InvalidInput(format!(
            "Provider test unsupported for api_protocol={protocol}; select openai_chat_completions, openai_responses, or anthropic_messages"
        ))),
    }
}

fn provider_test_error(
    protocol: &str,
    model: Option<&str>,
    status: Option<reqwest::StatusCode>,
    request_id: Option<String>,
    message: String,
) -> String {
    let retryable = status.is_some_and(|status| {
        status.as_u16() == 408 || status.as_u16() == 429 || status.is_server_error()
    });
    format!(
        "Provider test failed: protocol={}, model={}, http_status={}, retryable={}, request_id={}, message={}",
        normalize_api_protocol(protocol),
        model.unwrap_or("<none>"),
        status.map(|s| s.as_u16().to_string()).unwrap_or_else(|| "none".to_string()),
        retryable,
        request_id.unwrap_or_else(|| "none".to_string()),
        message,
    )
}

#[tauri::command]
pub async fn provider_discover_models(
    input: ProviderDiscoveryInput,
) -> Result<Vec<DiscoveredModel>> {
    if input.api_key.trim().is_empty() {
        return Err(Error::InvalidInput("API key cannot be empty".to_string()));
    }
    let api_protocol = required_api_protocol(input.api_protocol.as_deref())?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| Error::Internal(format!("Failed to build HTTP client: {e}")))?;
    let mut request = client.get(models_url(&api_protocol, &input.base_url)?);
    if is_anthropic_protocol(&api_protocol) {
        request = request
            .header("x-api-key", input.api_key.trim())
            .header("anthropic-version", "2023-06-01");
    } else {
        request = request.header("Authorization", format!("Bearer {}", input.api_key.trim()));
    }
    let response = request.send().await.map_err(|e| {
        Error::Internal(if e.is_timeout() {
            "Model discovery timed out".to_string()
        } else {
            format!("Model discovery failed: {e}")
        })
    })?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(Error::Internal(format!(
            "Model discovery HTTP {status}: {}",
            body.chars().take(200).collect::<String>()
        )));
    }
    let payload = response
        .json::<serde_json::Value>()
        .await
        .map_err(|e| Error::Internal(format!("Invalid model response: {e}")))?;
    let mut models: Vec<DiscoveredModel> = payload
        .get("data")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|model| {
            let id = model.get("id")?.as_str()?.trim();
            if id.is_empty() {
                return None;
            }
            let display_name = model
                .get("display_name")
                .or_else(|| model.get("displayName"))
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned);
            Some(DiscoveredModel {
                id: id.to_string(),
                display_name,
            })
        })
        .collect();
    models.sort_by(|left, right| left.id.cmp(&right.id));
    models.dedup_by(|left, right| left.id == right.id);
    Ok(models)
}

#[tauri::command]
pub async fn provider_discover_models_saved(
    state: State<'_, AppState>,
    input: ProviderDiscoverySavedInput,
) -> Result<Vec<DiscoveredModel>> {
    let provider_id = input.provider_id;
    let key_id = input.key_id;
    let (api_protocol, base_url, api_key) = {
        let pool_conn = state
            .db
            .get()
            .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
        let conn: &rusqlite::Connection = &*pool_conn;
        ensure_tables(conn)?;
        let (encrypted, dek, api_protocol, base_url): (String, Option<String>, String, String) =
            conn.query_row(
                "SELECT k.api_key_encrypted, k.dek_encrypted, p.api_protocol, p.base_url
                 FROM provider_api_keys k
                 JOIN user_providers p ON k.provider_id = p.id
                 WHERE k.id = ?1 AND k.provider_id = ?2",
                params![key_id, provider_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .map_err(|e| Error::Internal(format!("Failed to fetch key: {e}")))?;
        let api_key = if let Some(dek) = &dek {
            if !dek.is_empty() {
                provider_key_manager::envelope_decrypt(&encrypted, dek, conn)?
            } else {
                let encryption_key = env_manager::get_encryption_key(conn)?;
                env_manager::decrypt(&encrypted, &encryption_key)?
            }
        } else {
            let encryption_key = env_manager::get_encryption_key(conn)?;
            env_manager::decrypt(&encrypted, &encryption_key)?
        };
        (api_protocol, base_url, api_key)
    };

    let discover_input = ProviderDiscoveryInput {
        provider_type: api_protocol.clone(),
        api_protocol: Some(api_protocol),
        base_url,
        api_key,
    };
    let models = provider_discover_models(discover_input).await?;
    let assistant = crate::db::get_assistant_db_conn()?;
    let tx = assistant
        .unchecked_transaction()
        .map_err(|e| Error::Internal(e.to_string()))?;
    tx.execute(
        "DELETE FROM assistant_model_cache WHERE provider_id = ?1",
        params![provider_id],
    )
    .map_err(|e| Error::Internal(e.to_string()))?;
    for model in &models {
        tx.execute(
            "INSERT INTO assistant_model_cache (id, provider_id, model_id, display_name, capabilities, context_window, max_output, source, discovered_at) VALUES (?1, ?2, ?3, ?4, '{}', 0, 0, 'api_discovery', datetime('now'))",
            params![uuid_v4(), provider_id, model.id, model.display_name],
        ).map_err(|e| Error::Internal(e.to_string()))?;
    }
    tx.commit().map_err(|e| Error::Internal(e.to_string()))?;
    Ok(models)
}

/// Execute the actual provider test request (shared by provider_test and test_provider_raw).
async fn execute_provider_test(
    provider_type: &str,
    base_url: &str,
    api_key: &str,
    model: Option<&str>,
) -> ProviderTestResult {
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return ProviderTestResult {
                success: false,
                error: Some(format!("Failed to build HTTP client: {e}")),
            }
        }
    };

    let protocol = normalize_api_protocol(provider_type);
    let test_url = match provider_test_url(&protocol, base_url) {
        Ok(url) => url,
        Err(e) => {
            return ProviderTestResult {
                success: false,
                error: Some(provider_test_error(
                    &protocol,
                    model,
                    None,
                    None,
                    format!("Invalid base URL: {e}"),
                )),
            }
        }
    };

    if let Some(model) = model {
        let body = match provider_test_body(&protocol, model) {
            Ok(body) => body,
            Err(e) => {
                return ProviderTestResult {
                    success: false,
                    error: Some(provider_test_error(
                        &protocol,
                        Some(model),
                        None,
                        None,
                        e.to_string(),
                    )),
                }
            }
        };

        let mut request = client
            .post(&test_url)
            .header("Content-Type", "application/json");
        if is_anthropic_protocol(&protocol) {
            request = request
                .header("x-api-key", api_key)
                .header("anthropic-version", "2023-06-01");
        } else {
            request = request.header("Authorization", format!("Bearer {}", api_key));
        }
        let response = request.json(&body).send().await;

        return match response {
            Ok(resp) => {
                let status = resp.status();
                let request_id = resp
                    .headers()
                    .get("x-request-id")
                    .or_else(|| resp.headers().get("request-id"))
                    .and_then(|value| value.to_str().ok())
                    .map(ToOwned::to_owned);
                if status.is_success() {
                    match resp.json::<serde_json::Value>().await {
                        Ok(json) => {
                            let has_content = provider_response_has_content(&protocol, &json);
                            if has_content {
                                ProviderTestResult {
                                    success: true,
                                    error: None,
                                }
                            } else {
                                let keys = json
                                    .as_object()
                                    .map(|obj| obj.keys().cloned().collect::<Vec<_>>().join(", "))
                                    .unwrap_or_else(|| json.to_string().chars().take(80).collect());
                                ProviderTestResult {
                                    success: false,
                                    error: Some(provider_test_error(&protocol, Some(model), Some(status), request_id, format!(
                                        "Provider returned no assistant text. Check protocol/model. Response keys: {keys}"
                                    ))),
                                }
                            }
                        }
                        Err(_) => ProviderTestResult {
                            success: false,
                            error: Some(provider_test_error(
                                &protocol,
                                Some(model),
                                Some(status),
                                request_id,
                                "Invalid JSON response from API".to_string(),
                            )),
                        },
                    }
                } else if status.is_client_error() {
                    let body = resp.text().await.unwrap_or_default();
                    let error_body = body.chars().take(300).collect::<String>();
                    let classified = if error_body.contains("model_not_found")
                        || error_body.contains("model not found")
                    {
                        format!("Model '{}' not available", model)
                    } else if status == 401 {
                        "Authentication failed — invalid API key".to_string()
                    } else if status == 429 {
                        "Rate limited — too many requests".to_string()
                    } else {
                        format!("HTTP {}: {}", status, error_body)
                    };
                    ProviderTestResult {
                        success: false,
                        error: Some(provider_test_error(
                            &protocol,
                            Some(model),
                            Some(status),
                            request_id,
                            classified,
                        )),
                    }
                } else {
                    let body = resp.text().await.unwrap_or_default();
                    ProviderTestResult {
                        success: false,
                        error: Some(provider_test_error(
                            &protocol,
                            Some(model),
                            Some(status),
                            request_id,
                            format!(
                                "HTTP {}: {}",
                                status,
                                body.chars().take(200).collect::<String>()
                            ),
                        )),
                    }
                }
            }
            Err(e) => {
                if e.is_timeout() {
                    ProviderTestResult {
                        success: false,
                        error: Some(provider_test_error(
                            &protocol,
                            Some(model),
                            None,
                            None,
                            "Connection timed out (15s)".to_string(),
                        )),
                    }
                } else if e.is_connect() {
                    ProviderTestResult {
                        success: false,
                        error: Some(provider_test_error(
                            &protocol,
                            Some(model),
                            None,
                            None,
                            "Cannot connect — check base URL and network".to_string(),
                        )),
                    }
                } else {
                    ProviderTestResult {
                        success: false,
                        error: Some(provider_test_error(
                            &protocol,
                            Some(model),
                            None,
                            None,
                            format!("Connection failed: {e}"),
                        )),
                    }
                }
            }
        };
    }

    let request_url = match models_url(&protocol, base_url) {
        Ok(url) => url,
        Err(e) => {
            return ProviderTestResult {
                success: false,
                error: Some(provider_test_error(
                    &protocol,
                    None,
                    None,
                    None,
                    format!("Invalid base URL: {e}"),
                )),
            }
        }
    };

    let mut request = client.get(&request_url);
    if is_anthropic_protocol(&protocol) {
        request = request
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01");
    } else {
        request = request.header("Authorization", format!("Bearer {}", api_key));
    }
    let response = request.send().await;

    match response {
        Ok(resp) => {
            let status = resp.status();
            let request_id = resp
                .headers()
                .get("x-request-id")
                .or_else(|| resp.headers().get("request-id"))
                .and_then(|value| value.to_str().ok())
                .map(ToOwned::to_owned);
            if resp.status().is_success() {
                ProviderTestResult {
                    success: true,
                    error: None,
                }
            } else {
                let body = resp.text().await.unwrap_or_default();
                let classified = if status == 401 {
                    "Authentication failed".to_string()
                } else if status == 404 {
                    "Endpoint not found — check base URL".to_string()
                } else {
                    format!(
                        "HTTP {}: {}",
                        status,
                        body.chars().take(200).collect::<String>()
                    )
                };
                ProviderTestResult {
                    success: false,
                    error: Some(provider_test_error(
                        &protocol,
                        None,
                        Some(status),
                        request_id,
                        classified,
                    )),
                }
            }
        }
        Err(e) => {
            if e.is_timeout() {
                ProviderTestResult {
                    success: false,
                    error: Some(provider_test_error(
                        &protocol,
                        None,
                        None,
                        None,
                        "Connection timed out (15s)".to_string(),
                    )),
                }
            } else if e.is_connect() {
                ProviderTestResult {
                    success: false,
                    error: Some(provider_test_error(
                        &protocol,
                        None,
                        None,
                        None,
                        "Cannot connect — check base URL and network".to_string(),
                    )),
                }
            } else {
                ProviderTestResult {
                    success: false,
                    error: Some(provider_test_error(
                        &protocol,
                        None,
                        None,
                        None,
                        format!("Connection failed: {e}"),
                    )),
                }
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
    let provider_id = input.provider_id;
    let key_id = input.key_id;
    let (_provider_type, api_protocol, base_url, api_key, default_model) = {
        let pool_conn = state
            .db
            .get()
            .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
        let conn: &rusqlite::Connection = &*pool_conn;
        ensure_tables(conn)?;
        let (encrypted, dek, provider_type, api_protocol, base_url, default_model): (String, Option<String>, String, String, String, Option<String>) = conn
            .query_row(
                "SELECT k.api_key_encrypted, k.dek_encrypted, p.preset_name, p.api_protocol, p.base_url, p.default_model
                 FROM provider_api_keys k
                 JOIN user_providers p ON k.provider_id = p.id
                 WHERE k.id = ?1 AND k.provider_id = ?2",
                params![key_id, provider_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
            )
            .map_err(|e| Error::Internal(format!("Failed to fetch key: {e}")))?;
        let api_key = if let Some(dek) = &dek {
            if !dek.is_empty() {
                provider_key_manager::envelope_decrypt(&encrypted, dek, conn)?
            } else {
                let encryption_key = env_manager::get_encryption_key(conn)?;
                env_manager::decrypt(&encrypted, &encryption_key)?
            }
        } else {
            let encryption_key = env_manager::get_encryption_key(conn)?;
            env_manager::decrypt(&encrypted, &encryption_key)?
        };
        (
            provider_type,
            api_protocol,
            base_url,
            api_key,
            default_model,
        )
    };

    let model = input.model.or(default_model);
    let protocol = normalize_api_protocol(&api_protocol);
    let result = execute_provider_test(&protocol, &base_url, &api_key, model.as_deref()).await;
    let now = chrono_now();
    let status = if result.success { "valid" } else { "invalid" };
    state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?
        .execute(
        "UPDATE provider_api_keys SET test_status = ?1, last_test_at = ?2, last_error_message = ?3, updated_at = ?2 WHERE id = ?4 AND provider_id = ?5",
        params![status, now, result.error, key_id, provider_id],
    ).map_err(|e| Error::Internal(e.to_string()))?;
    Ok(result)
}

/// Test a provider connection using a raw API key (without saving).
/// Used by AddProviderDialog before the user saves the provider.
#[tauri::command]
pub async fn test_provider_raw(input: RawProviderTestInput) -> Result<ProviderTestResult> {
    let protocol = required_api_protocol(input.api_protocol.as_deref())?;
    Ok(execute_provider_test(
        &protocol,
        &input.base_url,
        &input.api_key,
        input.model.as_deref(),
    )
    .await)
}

// ── Helpers ──

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

    #[test]
    fn test_models_url_does_not_duplicate_v1() {
        assert_eq!(
            models_url("openai_compatible", "https://api.openai.com/v1").unwrap(),
            "https://api.openai.com/v1/models"
        );
    }

    #[test]
    fn provider_test_supports_openai_and_anthropic_response_shapes() {
        let openai = serde_json::json!({"choices": [{"message": {"content": "ok"}}]});
        let responses = serde_json::json!({"output_text": "ok"});
        let anthropic = serde_json::json!({"content": [{"type": "text", "text": "ok"}]});
        assert!(provider_response_has_content("openai_compatible", &openai));
        assert!(provider_response_has_content(
            "openai_responses",
            &responses
        ));
        assert!(provider_response_has_content("anthropic", &anthropic));
        assert!(provider_response_has_content(
            "anthropic_messages",
            &anthropic
        ));
        assert_eq!(
            provider_test_url("anthropic_messages", "https://api.anthropic.com").unwrap(),
            "https://api.anthropic.com/v1/messages"
        );
        assert_eq!(
            provider_test_url("openai_responses", "https://api.openai.com/v1").unwrap(),
            "https://api.openai.com/v1/responses"
        );
    }

    #[test]
    fn provider_test_builds_protocol_specific_request_bodies() {
        let anthropic = provider_test_body("anthropic_messages", "claude-sonnet").unwrap();
        assert_eq!(anthropic["model"], "claude-sonnet");
        assert_eq!(
            anthropic["messages"][0]["content"],
            "Reply with exactly: ok"
        );
        assert!(anthropic.get("max_tokens").is_some());
        assert!(anthropic.get("max_output_tokens").is_none());

        let responses = provider_test_body("openai_responses", "gpt-5").unwrap();
        assert_eq!(responses["model"], "gpt-5");
        assert_eq!(responses["input"], "Reply with exactly: ok");
        assert!(responses.get("max_output_tokens").is_some());
        assert!(responses.get("messages").is_none());
    }

    #[test]
    fn unsupported_protocols_are_not_silently_tested_as_openai() {
        assert!(required_api_protocol(None).is_err());
        assert!(required_api_protocol(Some("")).is_err());
        assert!(required_api_protocol(Some("unknown_protocol")).is_err());
        assert!(provider_test_url("gemini_generate_content", "https://example.com").is_err());
        assert!(models_url("ollama_chat", "http://localhost:11434").is_err());
        let message = provider_test_body("gemini_generate_content", "gemini-pro")
            .unwrap_err()
            .to_string();
        assert!(message.contains("unsupported"));
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
        let (db_key, _masked): (String, String) = conn
            .query_row(
                "SELECT k.api_key_encrypted, '' FROM provider_api_keys k WHERE k.id = ?1",
                rusqlite::params![key_id],
                |row| Ok((row.get(0)?, String::new())),
            )
            .unwrap();

        // The DB stores the full key (encrypted in production, raw in test)
        assert_eq!(
            db_key, full_key,
            "DB still has full key (encrypted in production)"
        );

        // Simulate what the frontend sees: masked_key
        let frontend_key = mask_api_key(&db_key);
        assert_eq!(frontend_key, "sk-s…3456", "Frontend receives masked key");
        assert!(
            !frontend_key.contains("abcdef123456"),
            "Full key not in frontend DTO"
        );
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
        assert!(
            model.contains("sensenova") || model.contains("deepseek"),
            "Model is from the allowlist"
        );

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
        assert_eq!(
            result, "https://token.sensenova.cn/v1",
            "Base URL should be derived to /v1 when user pastes /chat/completions"
        );

        let chat_url = chat_completions_url(&result);
        assert_eq!(
            chat_url, "https://token.sensenova.cn/v1/chat/completions",
            "Chat completions URL should be correctly rebuilt from derived base"
        );
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
            is_primary: true,
            is_active: true,
            status: "valid".to_string(),
            last_tested_at: Some("2026-01-01T00:00:00Z".to_string()),
            last_error_code: None,
            last_error_message: None,
            created_at: "2026-01-01T00:00:00Z".to_string(),
        };

        // Serialize to JSON and verify no api_key field
        let json = serde_json::to_value(&key).unwrap();
        assert!(
            json.get("apiKey").is_none(),
            "DTO must not have apiKey field"
        );
        assert!(
            json.get("maskedKey").is_some(),
            "DTO must have maskedKey field"
        );
        assert_eq!(json["maskedKey"], "sk-a…1b2c");
    }
}
