//! Local Proxy Host 命令（ADR-0020 §4 / PRX-001..004 / UI-004..005）。

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::proxy::engine::{EngineCall, ProxyEngine};
use crate::proxy::model::{PortMode, ProxySettings, ProxyStatusDTO, ProxyUsageRecord, Route};
use crate::proxy::native::NativeProxyEngine;
use crate::proxy::store;
use crate::secrets::store::SecretRef;
use crate::AppState;
use crate::{Error, Result};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProxySettingsInput {
    pub enabled_intent: Option<bool>,
    pub bind_host: Option<String>,
    pub port_mode: Option<String>,
    pub configured_port: Option<u16>,
    pub grace_timeout_ms: Option<u64>,
    pub max_concurrency: Option<u32>,
    pub max_request_body_bytes: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyChatInput {
    pub protocol: String,
    pub base_url: String,
    pub secret_ref: String,
    pub model: String,
    pub request_json: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyChatResult {
    pub ok: bool,
    pub usage_input_tokens: u64,
    pub usage_output_tokens: u64,
    pub stop_reason: String,
    pub error_category: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

#[tauri::command]
pub fn proxy_get_settings(state: State<'_, AppState>) -> Result<ProxySettings> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    store::get_proxy_settings(&conn)
}

#[tauri::command]
pub fn proxy_update_settings(
    state: State<'_, AppState>,
    input: UpdateProxySettingsInput,
) -> Result<ProxySettings> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    let mut current = store::get_proxy_settings(&conn)?;

    if let Some(enabled) = input.enabled_intent {
        current.enabled_intent = enabled;
    }
    if let Some(host) = input.bind_host {
        current.bind_host = host;
    }
    if let Some(pm) = input.port_mode {
        current.port_mode = PortMode::from_str(&pm);
    }
    if let Some(port) = input.configured_port {
        current.configured_port = port;
    }
    if let Some(grace) = input.grace_timeout_ms {
        current.grace_timeout_ms = grace;
    }
    if let Some(mc) = input.max_concurrency {
        current.max_concurrency = mc;
    }
    if let Some(mb) = input.max_request_body_bytes {
        current.max_request_body_bytes = mb;
    }

    store::save_proxy_settings(&conn, &current)?;
    Ok(current)
}

#[tauri::command]
pub fn proxy_status(state: State<'_, AppState>) -> Result<ProxyStatusDTO> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    state.proxy_runtime.get_status_dto(&conn)
}

#[tauri::command]
pub fn proxy_start(state: State<'_, AppState>) -> Result<bool> {
    state.proxy_runtime.start(state.db.clone()).map(|_| true)
}

#[tauri::command]
pub fn proxy_stop(state: State<'_, AppState>) -> Result<bool> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    state.proxy_runtime.stop(&conn).map(|_| true)
}

#[tauri::command]
pub fn proxy_restart(state: State<'_, AppState>) -> Result<bool> {
    state.proxy_runtime.restart(state.db.clone()).map(|_| true)
}

// ── Routes ──

#[tauri::command]
pub fn proxy_list_routes(state: State<'_, AppState>) -> Result<Vec<Route>> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    store::list_routes(&conn)
}

#[tauri::command]
pub fn proxy_get_route(state: State<'_, AppState>, id: String) -> Result<Option<Route>> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    store::get_route(&conn, &id)
}

#[tauri::command]
pub fn proxy_save_route(state: State<'_, AppState>, route: Route) -> Result<Route> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    store::save_route(&conn, &route)
}

#[tauri::command]
pub fn proxy_delete_route(state: State<'_, AppState>, id: String) -> Result<bool> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    store::delete_route(&conn, &id)
}

// ── Usage ──

#[tauri::command]
pub fn proxy_list_usage_records(
    state: State<'_, AppState>,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<Vec<ProxyUsageRecord>> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    store::list_usage_records(&conn, limit.unwrap_or(50), offset.unwrap_or(0))
}

// ── Internal proxy chat ──

#[tauri::command]
pub async fn proxy_chat(
    _state: State<'_, AppState>,
    input: ProxyChatInput,
) -> Result<ProxyChatResult> {
    let store = crate::secrets::keychain::KeychainSecretStore::default();
    let engine = NativeProxyEngine::new(std::sync::Arc::new(store));

    let reference = SecretRef::new(input.secret_ref);

    match engine
        .call(EngineCall {
            protocol: input.protocol,
            base_url: input.base_url,
            secret_ref: reference,
            model: input.model,
            request_json: input.request_json,
        })
        .await
    {
        Ok(crate::proxy::engine::EngineOutcome::Completed {
            usage_input_tokens,
            usage_output_tokens,
            stop_reason,
        }) => Ok(ProxyChatResult {
            ok: true,
            usage_input_tokens,
            usage_output_tokens,
            stop_reason,
            error_category: None,
            error_code: None,
            error_message: None,
        }),
        Ok(crate::proxy::engine::EngineOutcome::Failed {
            category,
            code,
            retryable: _,
        }) => Ok(ProxyChatResult {
            ok: false,
            usage_input_tokens: 0,
            usage_output_tokens: 0,
            stop_reason: String::new(),
            error_category: Some(category),
            error_code: Some(code),
            error_message: None,
        }),
        Ok(crate::proxy::engine::EngineOutcome::Unavailable { reason }) => Ok(ProxyChatResult {
            ok: false,
            usage_input_tokens: 0,
            usage_output_tokens: 0,
            stop_reason: String::new(),
            error_category: Some("unavailable".into()),
            error_code: Some("engine_unavailable".into()),
            error_message: Some(reason),
        }),
        Err(error) => Ok(ProxyChatResult {
            ok: false,
            usage_input_tokens: 0,
            usage_output_tokens: 0,
            stop_reason: String::new(),
            error_category: Some(error.category),
            error_code: Some(error.code),
            error_message: Some(error.message),
        }),
    }
}
