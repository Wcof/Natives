use super::*;

#[tauri::command]
pub async fn provider_discover_models(
    input: ProviderDiscoveryInput,
) -> Result<Vec<DiscoveredModel>> {
    if input.api_key.trim().is_empty() {
        return Err(Error::InvalidInput("API key cannot be empty".to_string()));
    }
    let api_protocol = required_api_protocol(input.api_protocol.as_deref())?;
    let client = crate::credential_broker::outbound_http_client(std::time::Duration::from_secs(15))
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
        let conn: &rusqlite::Connection = &pool_conn;
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
        // Stable id so re-discovery after DELETE is deterministic; unique index
        // on (provider_id, model_id) also guards against residual duplicates.
        tx.execute(
            "INSERT OR REPLACE INTO assistant_model_cache
             (id, provider_id, model_id, display_name, capabilities, context_window, max_output, source, discovered_at)
             VALUES (?1, ?2, ?3, ?4, '{}', 0, 0, 'api_discovery', datetime('now'))",
            params![
                format!("{provider_id}:{}", model.id),
                provider_id,
                model.id,
                model.display_name
            ],
        )
        .map_err(|e| Error::Internal(e.to_string()))?;
    }
    tx.commit().map_err(|e| Error::Internal(e.to_string()))?;
    Ok(models)
}
