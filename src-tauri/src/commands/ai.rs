//! AI Resources 命令（AIR-001 ~ AIR-016）。

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::ai::facade;
use crate::ai::model::{Connection, Credential, Model, Provider};
use crate::AppState;
use crate::{Error, Result};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateCredentialInput {
    pub provider_id: String,
    pub label: String,
    pub secret: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteCredentialInput {
    pub provider_id: String,
    pub credential_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthCheckResult {
    pub reachable: bool,
    pub error: Option<String>,
}

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
pub fn ai_list_connections(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<Vec<Connection>> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    facade::list_connections(&conn, &provider_id)
}

#[tauri::command]
pub fn ai_list_credentials(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<Vec<Credential>> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    facade::list_credentials(&conn, &provider_id)
}

#[tauri::command]
pub fn ai_create_credential(
    state: State<'_, AppState>,
    input: CreateCredentialInput,
) -> Result<Credential> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    facade::create_credential(&conn, &input.provider_id, &input.label, &input.secret)
}

#[tauri::command]
pub fn ai_delete_credential(
    state: State<'_, AppState>,
    input: DeleteCredentialInput,
) -> Result<bool> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    facade::delete_credential(&conn, &input.provider_id, &input.credential_id)
}

#[tauri::command]
pub fn ai_list_models(state: State<'_, AppState>, connection_id: String) -> Result<Vec<Model>> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    facade::list_models(&conn, &connection_id)
}

#[tauri::command]
pub async fn ai_check_connection_health(base_url: String) -> Result<HealthCheckResult> {
    let (reachable, error) = facade::check_connection_health(&base_url).await;
    Ok(HealthCheckResult { reachable, error })
}
