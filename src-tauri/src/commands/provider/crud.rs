use super::*;

/// List all saved providers with key metadata only.
/// Full API keys are never returned to the frontend.
#[tauri::command]
pub fn list_providers(state: State<'_, AppState>) -> Result<Vec<UserProvider>> {
    let pool_conn = state
        .db
        .get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &pool_conn;

    ensure_tables(conn)?;

    // Fetch providers
    let mut pstmt = conn.prepare(
        "SELECT id, preset_name, api_protocol, name, website_url, base_url, default_model, created_at, updated_at FROM user_providers ORDER BY created_at DESC"
    ).map_err(|e| Error::Internal(e.to_string()))?;

    #[allow(clippy::type_complexity)] // pre-existing type shape
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
    #[allow(clippy::type_complexity)] // pre-existing type shape
    let mut kstmt = conn.prepare(
        "SELECT id, provider_id, label, masked_key, is_primary, is_active, test_status, last_test_at, last_error_code, last_error_message, created_at FROM provider_api_keys ORDER BY created_at ASC"
    ).map_err(|e| Error::Internal(e.to_string()))?;

    #[allow(clippy::type_complexity)] // pre-existing type shape
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
        let assistant = crate::db::get_main_conn()?;
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

    // Keep assistant.db in sync so AssistantWorkbench provider.list / run.start
    // can see the same provider/model pair that Settings just saved.
    mirror_provider_to_assistant(
        &id,
        &provider_type,
        &display_name,
        &base_url,
        input.website_url.trim(),
        &default_model,
        &kid,
        key_label,
        &encrypted,
        &masked_key,
        &now,
        &[DiscoveredModel {
            id: default_model.clone(),
            display_name: Some(default_model.clone()),
        }],
    )?;

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
    let conn: &rusqlite::Connection = &pool_conn;

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
    let now = chrono_now();
    let assistant = crate::db::get_main_conn()?;
    let _ = assistant.execute(
        "UPDATE assistant_provider_configs SET default_model = ?1, updated_at = ?2 WHERE id = ?3",
        params![model, now, input.provider_id],
    );
    assistant.execute(
        "INSERT OR IGNORE INTO assistant_model_cache (id, provider_id, model_id, display_name, capabilities, context_window, max_output, source, discovered_at) VALUES (?1, ?2, ?3, ?4, '{}', 0, 0, 'manual', datetime('now'))",
        params![format!("{}:{}", input.provider_id, model), input.provider_id, model, model],
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
    let conn: &rusqlite::Connection = &pool_conn;

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
    let conn: &rusqlite::Connection = &pool_conn;

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

    // Best-effort cleanup of the assistant mirror so the picker does not keep
    // a deleted provider around after Settings removes it.
    if let Ok(assistant) = crate::db::get_main_conn() {
        let _ = assistant.execute(
            "DELETE FROM assistant_model_cache WHERE provider_id = ?1",
            params![provider_id],
        );
        let _ = assistant.execute(
            "DELETE FROM assistant_provider_keys WHERE provider_id = ?1",
            params![provider_id],
        );
        let _ = assistant.execute(
            "DELETE FROM assistant_provider_configs WHERE id = ?1",
            params![provider_id],
        );
    }
    Ok(())
}

/// Mirror a settings-owned provider into assistant.db tables used by
/// `provider.list` / `run.start`. Settings writes natives.db; assistant reads
/// assistant.db — without this mirror the picker stays empty after add.
#[allow(clippy::too_many_arguments)] // pre-existing parameter list
fn mirror_provider_to_assistant(
    provider_id: &str,
    provider_type: &str,
    display_name: &str,
    base_url: &str,
    website_url: &str,
    default_model: &str,
    key_id: &str,
    key_label: &str,
    encrypted_key: &str,
    masked_key: &str,
    now: &str,
    models: &[DiscoveredModel],
) -> Result<()> {
    let assistant = crate::db::get_main_conn()?;

    // Defensive: Settings pool init historically only created session tables.
    // DataStore migrations own the full schema, but ensure picker tables exist
    // even if this write races ahead of host store init.
    assistant
        .execute_batch(
            "
            CREATE TABLE IF NOT EXISTS assistant_provider_configs (
                id TEXT PRIMARY KEY,
                provider_type TEXT NOT NULL,
                display_name TEXT NOT NULL,
                api_base_url TEXT NOT NULL,
                website_url TEXT NOT NULL DEFAULT '',
                organization_id TEXT,
                project_id TEXT,
                proxy_url TEXT,
                timeout_secs INTEGER,
                default_model TEXT,
                health_status TEXT NOT NULL DEFAULT 'unknown',
                last_test_at TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS assistant_provider_keys (
                id TEXT PRIMARY KEY,
                provider_id TEXT NOT NULL REFERENCES assistant_provider_configs(id) ON DELETE CASCADE,
                encrypted_key TEXT NOT NULL,
                masked_key TEXT NOT NULL,
                label TEXT,
                is_active INTEGER NOT NULL DEFAULT 1,
                is_primary INTEGER NOT NULL DEFAULT 0,
                test_status TEXT NOT NULL DEFAULT 'untested',
                last_test_at TEXT,
                last_test_ok INTEGER,
                last_error_code TEXT,
                last_error_message TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT
            );
            CREATE TABLE IF NOT EXISTS assistant_model_cache (
                id TEXT PRIMARY KEY,
                provider_id TEXT NOT NULL,
                model_id TEXT NOT NULL,
                display_name TEXT,
                capabilities TEXT NOT NULL,
                context_window INTEGER NOT NULL,
                max_output INTEGER NOT NULL,
                source TEXT NOT NULL DEFAULT 'api_discovery',
                discovered_at TEXT NOT NULL
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_model_cache_provider_model
                ON assistant_model_cache(provider_id, model_id);
            ",
        )
        .map_err(|e| Error::Internal(format!("assistant provider schema ensure failed: {e}")))?;

    // Collapse historical duplicates then enforce uniqueness even on older DBs.
    let _ = assistant.execute_batch(
        "DELETE FROM assistant_model_cache
         WHERE rowid NOT IN (
             SELECT MIN(rowid) FROM assistant_model_cache GROUP BY provider_id, model_id
         );
         CREATE UNIQUE INDEX IF NOT EXISTS idx_model_cache_provider_model
             ON assistant_model_cache(provider_id, model_id);",
    );

    assistant
        .execute(
            "INSERT INTO assistant_provider_configs
             (id, provider_type, display_name, api_base_url, website_url, default_model, health_status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'unknown', ?7, ?7)
             ON CONFLICT(id) DO UPDATE SET
                provider_type = excluded.provider_type,
                display_name = excluded.display_name,
                api_base_url = excluded.api_base_url,
                website_url = excluded.website_url,
                default_model = excluded.default_model,
                updated_at = excluded.updated_at",
            params![
                provider_id,
                provider_type,
                display_name,
                base_url,
                website_url,
                default_model,
                now
            ],
        )
        .map_err(|e| Error::Internal(format!("assistant provider mirror failed: {e}")))?;

    assistant
        .execute(
            "INSERT INTO assistant_provider_keys
             (id, provider_id, encrypted_key, masked_key, label, is_active, is_primary, test_status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 1, 1, 'untested', ?6, ?6)
             ON CONFLICT(id) DO UPDATE SET
                encrypted_key = excluded.encrypted_key,
                masked_key = excluded.masked_key,
                label = excluded.label,
                is_active = 1,
                is_primary = 1,
                updated_at = excluded.updated_at",
            params![
                key_id,
                provider_id,
                encrypted_key,
                masked_key,
                key_label,
                now
            ],
        )
        .map_err(|e| Error::Internal(format!("assistant key mirror failed: {e}")))?;

    // Ensure at most one primary key is marked for this provider in assistant.db.
    let _ = assistant.execute(
        "UPDATE assistant_provider_keys SET is_primary = 0 WHERE provider_id = ?1 AND id != ?2",
        params![provider_id, key_id],
    );

    if !models.is_empty() {
        for model in models {
            let model_id = model.id.trim();
            if model_id.is_empty() {
                continue;
            }
            let display = model
                .display_name
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(model_id);
            assistant
                .execute(
                    "INSERT OR IGNORE INTO assistant_model_cache
                     (id, provider_id, model_id, display_name, capabilities, context_window, max_output, source, discovered_at)
                     VALUES (?1, ?2, ?3, ?4, '{}', 0, 0, 'manual', ?5)",
                    params![
                        format!("{provider_id}:{model_id}"),
                        provider_id,
                        model_id,
                        display,
                        now
                    ],
                )
                .map_err(|e| Error::Internal(format!("assistant model mirror failed: {e}")))?;
        }
    } else if !default_model.trim().is_empty() {
        assistant
            .execute(
                "INSERT OR IGNORE INTO assistant_model_cache
                 (id, provider_id, model_id, display_name, capabilities, context_window, max_output, source, discovered_at)
                 VALUES (?1, ?2, ?3, ?4, '{}', 0, 0, 'manual', ?5)",
                params![
                    format!("{provider_id}:{default_model}"),
                    provider_id,
                    default_model,
                    default_model,
                    now
                ],
            )
            .map_err(|e| Error::Internal(format!("assistant default model mirror failed: {e}")))?;
    }

    Ok(())
}
