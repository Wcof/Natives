use super::*;

pub(crate) fn provider_test_body(provider_type: &str, model: &str) -> Result<serde_json::Value> {
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
            // Reasoning/thinking models may consume the budget before final text.
            "max_tokens": 64,
            "stream": false,
        })),
        "openai_responses" => Ok(serde_json::json!({
            "model": model,
            "input": "Reply with exactly: ok",
            "max_output_tokens": 64,
            "stream": false,
        })),
        "openai_chat_completions" => Ok(serde_json::json!({
            "model": model,
            // Keep the prompt short; reasoning models may spend the budget on
            // reasoning_content before producing final content.
            "messages": [
                { "role": "user", "content": "Reply with exactly: ok" }
            ],
            "max_tokens": 64,
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

fn provider_test_is_rate_limited(result: &ProviderTestResult) -> bool {
    result.error.as_deref().is_some_and(|err| {
        let lower = err.to_ascii_lowercase();
        lower.contains("http_status=429")
            || lower.contains("rate limited")
            || lower.contains("too many requests")
    })
}

#[derive(Clone)]
pub(crate) struct ProviderRateLimitRoute {
    provider_id: String,
    key_id: String,
}

async fn daemon_rate_limit_call(
    method: &str,
    route: &ProviderRateLimitRoute,
    retry_after_ms: Option<u64>,
) -> std::result::Result<(), String> {
    let socket = std::env::var("NATIVES_DAEMON_SOCKET")
        .map_err(|_| "Native daemon is unavailable; provider test was not sent".to_string())?;
    let bootstrap = std::env::var("NATIVES_DAEMON_BOOTSTRAP")
        .map_err(|_| "Native daemon is unavailable; provider test was not sent".to_string())?;
    let mut client = natives_agent_daemon::DaemonClient::connect(
        socket,
        &bootstrap,
        natives_agent_daemon::client_protocol_version(),
    )
    .await
    .map_err(|error| format!("Native daemon rate limiter is unavailable: {error}"))?;
    client
        .call(
            method,
            serde_json::json!({
                "provider_id": route.provider_id,
                "key_id": route.key_id,
                "retry_after_ms": retry_after_ms,
            }),
        )
        .await
        .map_err(|error| format!("Native daemon rate limiter rejected provider test: {error}"))?;
    Ok(())
}

async fn acquire_provider_test_slot(route: Option<&ProviderRateLimitRoute>) -> Result<()> {
    if let Some(route) = route {
        daemon_rate_limit_call("engine.rateLimit.acquire", route, None)
            .await
            .map_err(Error::Internal)?;
    }
    Ok(())
}

async fn record_provider_test_rate_limit(
    route: Option<&ProviderRateLimitRoute>,
    headers: &reqwest::header::HeaderMap,
) {
    if let Some(route) = route {
        let retry_after_ms = provider_adapters::http_stream::retry_after_ms(headers);
        let _ = daemon_rate_limit_call("engine.rateLimit.cooldown", route, retry_after_ms).await;
    }
}

async fn execute_provider_test_with_retry(
    provider_type: &str,
    base_url: &str,
    api_key: &str,
    model: Option<&str>,
    route: Option<&ProviderRateLimitRoute>,
) -> ProviderTestResult {
    let first = execute_provider_test(provider_type, base_url, api_key, model, route).await;
    if first.success || !provider_test_is_rate_limited(&first) {
        return first;
    }
    execute_provider_test(provider_type, base_url, api_key, model, route).await
}

/// Execute the actual provider test request (shared by provider_test and test_provider_raw).
pub(crate) async fn execute_provider_test(
    provider_type: &str,
    base_url: &str,
    api_key: &str,
    model: Option<&str>,
    route: Option<&ProviderRateLimitRoute>,
) -> ProviderTestResult {
    let client =
        match crate::credential_broker::outbound_http_client(std::time::Duration::from_secs(15)) {
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
        if let Err(error) = acquire_provider_test_slot(route).await {
            return ProviderTestResult {
                success: false,
                error: Some(error.to_string()),
            };
        }
        let response = request.json(&body).send().await;

        return match response {
            Ok(resp) => {
                let status = resp.status();
                if status == 429 {
                    record_provider_test_rate_limit(route, resp.headers()).await;
                }
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
                                        "Provider returned an empty assistant payload. Check protocol/model. Response keys: {keys}"
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
    if let Err(error) = acquire_provider_test_slot(route).await {
        return ProviderTestResult {
            success: false,
            error: Some(error.to_string()),
        };
    }
    let response = request.send().await;

    match response {
        Ok(resp) => {
            let status = resp.status();
            if status == 429 {
                record_provider_test_rate_limit(route, resp.headers()).await;
            }
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
        let conn: &rusqlite::Connection = &pool_conn;
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
    let route = ProviderRateLimitRoute {
        provider_id: provider_id.clone(),
        key_id: key_id.clone(),
    };
    let result = execute_provider_test_with_retry(
        &protocol,
        &base_url,
        &api_key,
        model.as_deref(),
        Some(&route),
    )
    .await;
    let now = chrono_now();
    let status = if result.success {
        "valid"
    } else if provider_test_is_rate_limited(&result) {
        "rate_limited"
    } else {
        "invalid"
    };
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
    let route = raw_provider_rate_limit_route(&protocol, &input.base_url, &input.api_key);
    Ok(execute_provider_test_with_retry(
        &protocol,
        &input.base_url,
        &input.api_key,
        input.model.as_deref(),
        Some(&route),
    )
    .await)
}

fn raw_provider_rate_limit_route(
    protocol: &str,
    base_url: &str,
    api_key: &str,
) -> ProviderRateLimitRoute {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    api_key.hash(&mut hasher);
    ProviderRateLimitRoute {
        provider_id: format!("raw:{protocol}:{}", base_url.trim_end_matches('/')),
        key_id: format!("raw:{:016x}", hasher.finish()),
    }
}
