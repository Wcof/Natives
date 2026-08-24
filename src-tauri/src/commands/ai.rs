//! AI Resources 命令（AIR-001 ~ AIR-016 / API-001..004 / OAU-008）。

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::ai::facade;
use crate::ai::model::{
    AiResourcesSummary, Connection, Credential, CredentialStatus, DeleteImpact, Model,
    ModelAvailability, ModelSource, Provider, QuotaSnapshot, UpstreamProtocol,
};
use crate::ai::store;
use crate::secrets::store::SecretStore;
use crate::AppState;
use crate::{Error, Result};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProviderInput {
    pub id: Option<String>,
    pub preset_key: Option<String>,
    pub name: String,
    pub website_url: String,
    pub icon_key: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProviderInput {
    pub id: String,
    pub name: Option<String>,
    pub website_url: Option<String>,
    pub icon_key: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateConnectionInput {
    pub id: Option<String>,
    pub provider_id: String,
    pub name: String,
    pub base_url: String,
    pub upstream_protocol: String,
    pub models_url: Option<String>,
    pub proxy_url: Option<String>,
    pub headers_json: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateConnectionInput {
    pub id: String,
    pub name: Option<String>,
    pub base_url: Option<String>,
    pub upstream_protocol: Option<String>,
    pub models_url: Option<Option<String>>,
    pub proxy_url: Option<Option<String>>,
    pub headers_json: Option<Option<String>>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateApiKeyCredentialInput {
    pub provider_id: String,
    pub label: String,
    pub api_key: String,
    pub priority: Option<u32>,
    pub concurrency_limit: Option<u32>,
    pub connection_ids: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCredentialPolicyInput {
    pub id: String,
    pub label: Option<String>,
    pub priority: Option<u32>,
    pub concurrency_limit: Option<u32>,
    pub status: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteCredentialInput {
    pub provider_id: String,
    pub credential_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoverModelsInput {
    pub base_url: String,
    pub api_key: String,
    pub models_url: Option<String>,
    pub headers_json: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmDiscoveredModelsInput {
    pub provider_id: Option<String>,
    pub connection_id: Option<String>,
    pub models: Vec<crate::ai::discovery::DiscoveredModel>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateManualModelInput {
    pub provider_id: Option<String>,
    pub connection_id: Option<String>,
    pub model_id: String,
    pub display_name: String,
    pub capabilities_json: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthCheckResult {
    pub reachable: bool,
    pub error: Option<String>,
}

// ── Providers ──

#[tauri::command]
pub fn ai_list_providers(state: State<'_, AppState>) -> Result<Vec<Provider>> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    facade::list_providers(&conn)
}

#[tauri::command]
pub fn ai_get_provider(state: State<'_, AppState>, id: String) -> Result<Option<Provider>> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    facade::get_provider(&conn, &id)
}

#[tauri::command]
pub fn ai_create_provider(
    state: State<'_, AppState>,
    input: CreateProviderInput,
) -> Result<Provider> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    facade::create_provider(
        &conn,
        input.id.as_deref(),
        input.preset_key.as_deref(),
        &input.name,
        &input.website_url,
        input.icon_key.as_deref(),
        input.enabled.unwrap_or(true),
    )
}

#[tauri::command]
pub fn ai_update_provider(
    state: State<'_, AppState>,
    input: UpdateProviderInput,
) -> Result<Provider> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    facade::update_provider(
        &conn,
        &input.id,
        input.name.as_deref(),
        input.website_url.as_deref(),
        input.icon_key.as_deref(),
        input.enabled,
    )
}

#[tauri::command]
pub fn ai_delete_provider(state: State<'_, AppState>, id: String) -> Result<bool> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    facade::delete_provider(&conn, &id)
}

#[tauri::command]
pub fn ai_get_provider_delete_impact(
    state: State<'_, AppState>,
    id: String,
) -> Result<DeleteImpact> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    facade::get_provider_delete_impact(&conn, &id)
}

// ── Connections ──

#[tauri::command]
pub fn ai_list_connections(
    state: State<'_, AppState>,
    provider_id: Option<String>,
) -> Result<Vec<Connection>> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    store::list_connections(&conn, provider_id.as_deref())
}

#[tauri::command]
pub fn ai_get_connection(state: State<'_, AppState>, id: String) -> Result<Option<Connection>> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    facade::get_connection(&conn, &id)
}

#[tauri::command]
pub fn ai_create_connection(
    state: State<'_, AppState>,
    input: CreateConnectionInput,
) -> Result<Connection> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    let protocol = UpstreamProtocol::from_str(&input.upstream_protocol)
        .unwrap_or(UpstreamProtocol::OpenaiChatCompletions);
    facade::create_connection(
        &conn,
        input.id.as_deref(),
        &input.provider_id,
        &input.name,
        &input.base_url,
        protocol,
        input.models_url.as_deref(),
        input.proxy_url.as_deref(),
        input.headers_json.as_deref(),
        input.enabled.unwrap_or(true),
    )
}

#[tauri::command]
pub fn ai_update_connection(
    state: State<'_, AppState>,
    input: UpdateConnectionInput,
) -> Result<Connection> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    let proto = input
        .upstream_protocol
        .as_deref()
        .and_then(UpstreamProtocol::from_str);
    let models_url = input.models_url.as_ref().map(|m| m.as_deref());
    let proxy_url = input.proxy_url.as_ref().map(|p| p.as_deref());
    let headers_json = input.headers_json.as_ref().map(|h| h.as_deref());

    facade::update_connection(
        &conn,
        &input.id,
        input.name.as_deref(),
        input.base_url.as_deref(),
        proto,
        models_url,
        proxy_url,
        headers_json,
        input.enabled,
    )
}

#[tauri::command]
pub fn ai_delete_connection(state: State<'_, AppState>, id: String) -> Result<bool> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    facade::delete_connection(&conn, &id)
}

// ── Credentials ──

#[tauri::command]
pub fn ai_list_credentials(
    state: State<'_, AppState>,
    provider_id: Option<String>,
) -> Result<Vec<Credential>> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    store::list_credentials(&conn, provider_id.as_deref())
}

#[tauri::command]
pub fn ai_get_credential(state: State<'_, AppState>, id: String) -> Result<Option<Credential>> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    facade::get_credential(&conn, &id)
}

#[tauri::command]
pub fn ai_create_api_key_credential(
    state: State<'_, AppState>,
    input: CreateApiKeyCredentialInput,
) -> Result<Credential> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    let conns = input.connection_ids.unwrap_or_default();
    facade::create_api_key_credential(
        &conn,
        &input.provider_id,
        &input.label,
        &input.api_key,
        input.priority.unwrap_or(0),
        input.concurrency_limit.unwrap_or(10),
        &conns,
    )
}

#[tauri::command]
pub fn ai_update_credential_policy(
    state: State<'_, AppState>,
    input: UpdateCredentialPolicyInput,
) -> Result<Credential> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    let status = input.status.as_deref().map(CredentialStatus::from_str);
    store::update_credential_policy(
        &conn,
        &input.id,
        input.label.as_deref(),
        input.priority,
        input.concurrency_limit,
        status,
    )
}

#[tauri::command]
pub fn ai_delete_credential(
    state: State<'_, AppState>,
    input: DeleteCredentialInput,
) -> Result<bool> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    facade::delete_credential(&conn, &input.provider_id, &input.credential_id)
}

// ── Models ──

#[tauri::command]
pub async fn ai_discover_models(
    input: DiscoverModelsInput,
) -> Result<Vec<crate::ai::discovery::DiscoveredModel>> {
    crate::ai::discovery::discover_models(
        &input.base_url,
        &input.api_key,
        input.models_url.as_deref(),
        input.headers_json.as_deref(),
    )
    .await
    .map_err(Error::Internal)
}

#[tauri::command]
pub fn ai_confirm_discovered_models(
    state: State<'_, AppState>,
    input: ConfirmDiscoveredModelsInput,
) -> Result<Vec<Model>> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    let mut saved = Vec::new();
    for m in input.models {
        let model = facade::upsert_model(
            &conn,
            input.provider_id.as_deref(),
            input.connection_id.as_deref(),
            None,
            &m.id,
            &m.display_name,
            m.source,
            m.capabilities.as_deref(),
            m.availability,
        )?;
        saved.push(model);
    }
    Ok(saved)
}

#[tauri::command]
pub fn ai_list_models(
    state: State<'_, AppState>,
    connection_id: Option<String>,
    credential_id: Option<String>,
) -> Result<Vec<Model>> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    facade::list_models(&conn, connection_id.as_deref(), credential_id.as_deref())
}

#[tauri::command]
pub fn ai_create_manual_model(
    state: State<'_, AppState>,
    input: CreateManualModelInput,
) -> Result<Model> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    facade::upsert_model(
        &conn,
        input.provider_id.as_deref(),
        input.connection_id.as_deref(),
        None,
        &input.model_id,
        &input.display_name,
        ModelSource::Manual,
        input.capabilities_json.as_deref(),
        ModelAvailability::Available,
    )
}

#[tauri::command]
pub fn ai_delete_model(state: State<'_, AppState>, id: String) -> Result<bool> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    store::delete_model(&conn, &id)
}

// ── OAuth Commands ──

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OauthStartInput {
    pub provider_id: String,
    pub account_label: Option<String>,
}

#[tauri::command]
pub fn ai_oauth_list_presets() -> Vec<crate::ai::OauthProviderPreset> {
    crate::ai::OAUTH_PRESETS
        .iter()
        .map(|p| (*p).clone())
        .collect()
}

#[tauri::command]
pub async fn ai_oauth_start(
    state: State<'_, AppState>,
    input: OauthStartInput,
) -> Result<crate::ai::OauthSessionInfo> {
    let service = crate::ai::OauthService::new();
    let session = service
        .start_oauth_session(&input.provider_id, input.account_label)
        .await?;
    let info = session.to_info();
    let mut info_with_url = info;
    if session.flow == crate::ai::OauthFlowType::Pkce {
        let preset = crate::ai::get_oauth_preset(&session.provider_id)
            .ok_or_else(|| Error::NotFound(format!("Preset {} not found", session.provider_id)))?;
        let scopes_encoded = preset.scopes.join(" ");
        let authorize_url = format!(
            "{}?client_id={}&response_type=code&redirect_uri={}&state={}&code_challenge={}&code_challenge_method=S256&scope={}",
            preset.authorize_url,
            crate::ai::oauth::pkce::url_encode(preset.client_id),
            crate::ai::oauth::pkce::url_encode(&session.redirect_uri),
            crate::ai::oauth::pkce::url_encode(&session.state),
            crate::ai::oauth::pkce::url_encode(session.pkce_verifier.as_deref().unwrap_or("")),
            crate::ai::oauth::pkce::url_encode(&scopes_encoded),
        );
        info_with_url.authorize_url = Some(authorize_url);
    }
    state.oauth_sessions.insert(session);
    Ok(info_with_url)
}

#[tauri::command]
pub async fn ai_oauth_poll(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<crate::ai::OauthSessionInfo> {
    let service = crate::ai::OauthService::new();
    let bundle_opt = service
        .poll_device_session(&session_id, &state.oauth_sessions)
        .await?;
    if let Some(bundle) = bundle_opt {
        let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
        service.persist_oauth_success(&conn, &session_id, bundle, &state.oauth_sessions)?;
    }
    state
        .oauth_sessions
        .get_info(&session_id)
        .ok_or_else(|| Error::NotFound(format!("Session {session_id} not found")))
}

#[tauri::command]
pub async fn ai_oauth_submit_callback(
    state: State<'_, AppState>,
    callback_url: String,
) -> Result<crate::ai::OauthSessionInfo> {
    let service = crate::ai::OauthService::new();
    let (session_id, bundle) = service
        .exchange_callback_url(&callback_url, &state.oauth_sessions)
        .await?;
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    service.persist_oauth_success(&conn, &session_id, bundle, &state.oauth_sessions)?;
    state
        .oauth_sessions
        .get_info(&session_id)
        .ok_or_else(|| Error::NotFound(format!("Session {session_id} not found")))
}

#[tauri::command]
pub fn ai_oauth_cancel(state: State<'_, AppState>, session_id: String) -> Result<bool> {
    Ok(state.oauth_sessions.cancel(&session_id))
}

#[tauri::command]
pub async fn ai_oauth_refresh(
    state: State<'_, AppState>,
    credential_id: String,
) -> Result<Credential> {
    let (provider_id, secret_ref_str, secret_revision) = {
        let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
        let cred = store::get_credential(&conn, &credential_id)?
            .ok_or_else(|| Error::NotFound(format!("Credential {credential_id} not found")))?;
        (cred.provider_id, cred.secret_ref, cred.secret_revision)
    };

    let store = crate::secrets::keychain::KeychainSecretStore::default();
    let old_bytes = store
        .read(&crate::secrets::store::SecretRef::new(
            secret_ref_str.clone(),
        ))
        .map_err(|e| Error::Internal(format!("Failed to read secret from Keychain: {e}")))?;

    let old_bundle: crate::ai::oauth::session::OauthTokenBundle =
        serde_json::from_slice(&old_bytes)
            .map_err(|e| Error::Internal(format!("Failed to parse token bundle: {e}")))?;

    let refresh_token = old_bundle
        .refresh_token
        .ok_or_else(|| Error::InvalidInput("No refresh token in credential".into()))?;

    let service = crate::ai::OauthService::new();
    let new_bundle = service
        .refresh_credential_async(&provider_id, &refresh_token)
        .await?;

    let new_bytes = serde_json::to_vec(&new_bundle).map_err(|e| Error::Internal(e.to_string()))?;
    let next_revision = secret_revision + 1;
    let new_ref_str = format!("natives/ai/credential/{}/v{}", credential_id, next_revision);
    let new_ref = crate::secrets::store::SecretRef::new(new_ref_str.clone());

    store
        .write(&new_ref, &new_bytes)
        .map_err(|e| Error::Internal(e.to_string()))?;

    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    let now = chrono::Utc::now();
    let expires_at = new_bundle
        .expires_in_secs
        .map(|secs| (now + chrono::Duration::seconds(secs as i64)).to_rfc3339());

    store::update_credential_status(
        &conn,
        &credential_id,
        CredentialStatus::Active,
        Some(expires_at.as_deref()),
        Some(Some(&now.to_rfc3339())),
        Some(expires_at.as_deref()),
    )?;
    let _ = store.delete(&crate::secrets::store::SecretRef::new(secret_ref_str));

    store::get_credential(&conn, &credential_id)?
        .ok_or_else(|| Error::Internal("Failed to read back refreshed credential".into()))
}

// ── Quota & Summary ──

#[tauri::command]
pub fn ai_get_quota(
    state: State<'_, AppState>,
    credential_id: String,
) -> Result<Option<QuotaSnapshot>> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    store::get_quota_snapshot(&conn, &credential_id)
}

#[tauri::command]
pub fn ai_run_migration(state: State<'_, AppState>) -> Result<crate::ai::MigrationReport> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    crate::ai::run_ai_resources_migration(&conn)
}

#[tauri::command]
pub fn ai_get_summary(state: State<'_, AppState>) -> Result<AiResourcesSummary> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    facade::get_ai_resources_summary(&conn)
}

#[tauri::command]
pub async fn ai_check_connection_health(base_url: String) -> Result<HealthCheckResult> {
    let (reachable, error) = facade::check_connection_health(&base_url).await;
    Ok(HealthCheckResult { reachable, error })
}
