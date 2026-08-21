//! Apps 命令（APP-001 ~ APP-018）。

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::apps::facade;
use crate::apps::model::{App, RuntimeInstance, RuntimeSpec, Surface};
use crate::creative_app::local::LocalRuntimeHandle;
use crate::creative_app::service::MutationLock;
use crate::AppState;
use crate::{Error, Result};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAppInput {
    pub title: String,
    pub source: String,
    pub source_id: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppHealthResult {
    pub healthy: bool,
    pub status: String,
    pub port: Option<u16>,
}

#[tauri::command]
pub fn apps_list(state: State<'_, AppState>) -> Result<Vec<App>> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    facade::list_apps(&conn)
}

#[tauri::command]
pub fn apps_get(state: State<'_, AppState>, id: String) -> Result<Option<App>> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    facade::get_app(&conn, &id)
}

#[tauri::command]
pub fn apps_create(state: State<'_, AppState>, input: CreateAppInput) -> Result<App> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    facade::create_app(
        &conn,
        &input.title,
        &input.source,
        &input.source_id,
        input.description.as_deref(),
        input.icon.as_deref(),
    )
}

#[tauri::command]
pub fn apps_delete(state: State<'_, AppState>, id: String) -> Result<bool> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    facade::delete_app(&conn, &id)
}

#[tauri::command]
pub fn apps_list_instances(
    state: State<'_, AppState>,
    application_id: String,
) -> Result<Vec<RuntimeInstance>> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    facade::list_runtime_instances(&conn, &application_id)
}

#[tauri::command]
pub fn apps_list_surfaces(
    state: State<'_, AppState>,
    application_id: String,
) -> Result<Vec<Surface>> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    facade::list_surfaces(&conn, &application_id)
}

#[tauri::command]
pub fn apps_active_spec(
    state: State<'_, AppState>,
    application_id: String,
) -> Result<Option<RuntimeSpec>> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    facade::active_runtime_spec(&conn, &application_id)
}

#[tauri::command]
pub async fn apps_start(
    id: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    lock: State<'_, MutationLock>,
    local_runtime: State<'_, LocalRuntimeHandle>,
) -> Result<bool> {
    let _ = crate::commands::creative_app::creative_app_start(
        id,
        app_handle,
        state,
        lock,
        local_runtime,
    )
    .await?;
    Ok(true)
}

#[tauri::command]
pub async fn apps_stop(
    id: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    lock: State<'_, MutationLock>,
    local_runtime: State<'_, LocalRuntimeHandle>,
    browser: State<'_, crate::creative_app::browser::BrowserStateHandle>,
) -> Result<bool> {
    let _ = crate::commands::creative_app::creative_app_stop(
        id,
        app_handle,
        state,
        lock,
        local_runtime,
        browser,
    )
    .await?;
    Ok(true)
}

#[tauri::command]
pub async fn apps_restart(
    id: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    lock: State<'_, MutationLock>,
    local_runtime: State<'_, LocalRuntimeHandle>,
) -> Result<bool> {
    let _ = crate::commands::creative_app::creative_app_restart(
        id,
        app_handle,
        state,
        lock,
        local_runtime,
    )
    .await?;
    Ok(true)
}

#[tauri::command]
pub async fn apps_kill(
    id: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    lock: State<'_, MutationLock>,
    local_runtime: State<'_, LocalRuntimeHandle>,
    browser: State<'_, crate::creative_app::browser::BrowserStateHandle>,
) -> Result<bool> {
    apps_stop(id, app_handle, state, lock, local_runtime, browser).await
}

#[tauri::command]
pub fn apps_health(state: State<'_, AppState>, application_id: String) -> Result<AppHealthResult> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    let instances = facade::list_runtime_instances(&conn, &application_id)?;
    if let Some(inst) = instances.first() {
        let healthy = inst.status == "running";
        let port = inst.current_port.map(|p| p as u16);
        Ok(AppHealthResult {
            healthy,
            status: inst.status.clone(),
            port,
        })
    } else {
        Ok(AppHealthResult {
            healthy: false,
            status: "stopped".to_string(),
            port: None,
        })
    }
}
