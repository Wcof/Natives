//! Apps 域 Tauri IPC 命令（APP-020）。
//!
//! 提供应用中心唯一公共 IPC 契约：
//! - Query：`apps_list`, `apps_list_views`, `apps_get`, `apps_get_view`, `apps_list_instances`,
//!          `apps_list_surfaces`, `apps_active_spec`, `apps_system_discover`, `apps_local_inspect`, `apps_local_logs`
//! - Register：`apps_register_local`, `apps_register_system`, `apps_register_web`
//! - Mutate：`apps_update_metadata`, `apps_update_system_spec`, `apps_update_web_spec`,
//!           `apps_remove`, `apps_set_sidebar_visibility`, `apps_set_sidebar_order`
//! - Lifecycle：`apps_open`, `apps_start`, `apps_stop`, `apps_restart`, `apps_force_stop`, `apps_resolve_orphan`
//! - System Control（APPV2-T06）：`apps_system_hide`, `apps_system_unhide`
//! - Web Surface：`apps_web_close`, `apps_web_hide`, `apps_web_reload`, `apps_web_back`, `apps_web_forward`, `apps_web_clear_data`

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

use crate::apps::model::{
    App, AppKind, AppView, RegisterLocalProjectInput, RegisterSystemApplicationInput,
    RegisterWebApplicationInput, RegistrationOrigin, RuntimeInstance, RuntimeSpec, Surface,
    UpdateAppMetadataInput, UpdateSystemApplicationSpecInput, UpdateWebApplicationSpecInput,
};
use crate::apps::mutation_lock::MutationLock;
use crate::apps::repository::AppRepository;
use crate::apps::service::{AppsService, AppsServiceDeps};
use crate::apps::system::SystemAppCandidate;
use crate::apps::{local, system, web};
use crate::creative_app::browser::BrowserStateHandle;
use crate::creative_app::local::LocalRuntimeHandle;
use crate::creative_app::model::LocalProjectScanResult;
use crate::creative_app::model_runtime::BrowserBounds;
use crate::AppState;
use crate::{Error, Result};

fn service(
    state: &AppState,
    app_handle: &AppHandle,
    locks: &MutationLock,
    local_runtime: &LocalRuntimeHandle,
) -> AppsService {
    let browser = Arc::new(std::sync::Mutex::new(
        crate::creative_app::browser::BrowserState::default(),
    ));

    AppsService::new(AppsServiceDeps {
        db: state.db.clone(),
        locks: locks.clone(),
        local_runtime: local_runtime.clone(),
        browser,
        app_handle: app_handle.clone(),
        host_http_port: 0,
    })
}

// ── Query ─────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn apps_list(state: State<'_, AppState>) -> Result<Vec<App>> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    AppRepository::list(&conn)
}

#[tauri::command]
pub fn apps_list_views(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    locks: State<'_, MutationLock>,
    local_runtime: State<'_, LocalRuntimeHandle>,
) -> Result<Vec<AppView>> {
    let svc = service(&state, &app_handle, &locks, &local_runtime);
    svc.list_views()
}

#[tauri::command]
pub fn apps_get(state: State<'_, AppState>, id: String) -> Result<Option<App>> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    AppRepository::get(&conn, &id)
}

#[tauri::command]
pub fn apps_get_view(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    locks: State<'_, MutationLock>,
    local_runtime: State<'_, LocalRuntimeHandle>,
    id: String,
) -> Result<Option<AppView>> {
    let svc = service(&state, &app_handle, &locks, &local_runtime);
    svc.get_view(&id)
}

#[tauri::command]
pub fn apps_list_instances(
    state: State<'_, AppState>,
    application_id: String,
) -> Result<Vec<RuntimeInstance>> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    AppRepository::list_instances(&conn, &application_id)
}

#[tauri::command]
pub fn apps_list_surfaces(
    state: State<'_, AppState>,
    application_id: String,
) -> Result<Vec<Surface>> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    AppRepository::list_surfaces(&conn, &application_id)
}

#[tauri::command]
pub fn apps_active_spec(
    state: State<'_, AppState>,
    application_id: String,
) -> Result<Option<RuntimeSpec>> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    AppRepository::active_spec(&conn, &application_id)
}

#[tauri::command]
pub async fn apps_system_discover(state: State<'_, AppState>) -> Result<Vec<SystemAppCandidate>> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        let conn = db.get().map_err(|e| Error::Internal(e.to_string()))?;
        if let Some(driver) = system::driver() {
            driver.discover(&conn)
        } else {
            Ok(Vec::new())
        }
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))?
}

#[tauri::command]
pub fn apps_local_inspect(
    state: State<'_, AppState>,
    project_root: String,
) -> Result<LocalProjectScanResult> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    local::inspect(&conn, &project_root)
}

#[tauri::command]
pub fn apps_local_logs(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    locks: State<'_, MutationLock>,
    local_runtime: State<'_, LocalRuntimeHandle>,
    id: String,
    limit: Option<usize>,
) -> Result<Vec<serde_json::Value>> {
    let svc = service(&state, &app_handle, &locks, &local_runtime);
    svc.local_logs(&id, limit.unwrap_or(100))
}

// ── Register ──────────────────────────────────────────────────────────────

#[tauri::command]
pub fn apps_register_local(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    locks: State<'_, MutationLock>,
    local_runtime: State<'_, LocalRuntimeHandle>,
    input: RegisterLocalProjectInput,
) -> Result<AppView> {
    let svc = service(&state, &app_handle, &locks, &local_runtime);
    svc.register_local(input)
}

#[tauri::command]
pub fn apps_register_system(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    locks: State<'_, MutationLock>,
    local_runtime: State<'_, LocalRuntimeHandle>,
    input: RegisterSystemApplicationInput,
) -> Result<AppView> {
    let svc = service(&state, &app_handle, &locks, &local_runtime);
    svc.register_system(input)
}

#[tauri::command]
pub fn apps_register_web(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    locks: State<'_, MutationLock>,
    local_runtime: State<'_, LocalRuntimeHandle>,
    input: RegisterWebApplicationInput,
) -> Result<AppView> {
    let svc = service(&state, &app_handle, &locks, &local_runtime);
    svc.register_web(input)
}

// ── Mutate ────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn apps_update_metadata(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    locks: State<'_, MutationLock>,
    local_runtime: State<'_, LocalRuntimeHandle>,
    input: UpdateAppMetadataInput,
) -> Result<AppView> {
    let svc = service(&state, &app_handle, &locks, &local_runtime);
    svc.update_metadata(input)
}

#[tauri::command]
pub fn apps_update_system_spec(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    locks: State<'_, MutationLock>,
    local_runtime: State<'_, LocalRuntimeHandle>,
    input: UpdateSystemApplicationSpecInput,
) -> Result<AppView> {
    let svc = service(&state, &app_handle, &locks, &local_runtime);
    svc.update_system_spec(input)
}

#[tauri::command]
pub fn apps_update_web_spec(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    locks: State<'_, MutationLock>,
    local_runtime: State<'_, LocalRuntimeHandle>,
    input: UpdateWebApplicationSpecInput,
) -> Result<AppView> {
    let svc = service(&state, &app_handle, &locks, &local_runtime);
    svc.update_web_spec(input)
}

#[tauri::command]
pub fn apps_set_sidebar_visibility(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    locks: State<'_, MutationLock>,
    local_runtime: State<'_, LocalRuntimeHandle>,
    id: String,
    show: bool,
) -> Result<AppView> {
    let svc = service(&state, &app_handle, &locks, &local_runtime);
    svc.set_sidebar(&id, Some(show), None)
}

#[tauri::command]
pub fn apps_set_sidebar_order(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    locks: State<'_, MutationLock>,
    local_runtime: State<'_, LocalRuntimeHandle>,
    id: String,
    order: Option<i64>,
) -> Result<AppView> {
    let svc = service(&state, &app_handle, &locks, &local_runtime);
    svc.set_sidebar(&id, None, order)
}

#[tauri::command]
pub async fn apps_remove(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    locks: State<'_, MutationLock>,
    local_runtime: State<'_, LocalRuntimeHandle>,
    id: String,
    risk_level: Option<u8>,
) -> Result<bool> {
    let svc = service(&state, &app_handle, &locks, &local_runtime);
    svc.remove(&id, risk_level.unwrap_or(1)).await?;
    Ok(true)
}

// ── Lifecycle ─────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn apps_open(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    locks: State<'_, MutationLock>,
    local_runtime: State<'_, LocalRuntimeHandle>,
    id: String,
    // APPV2-T03：Renderer 实测内容区矩形（Web 目标「先 bounds 后 show」；其它 kind 忽略）。
    bounds: Option<BrowserBounds>,
) -> Result<bool> {
    let svc = service(&state, &app_handle, &locks, &local_runtime);
    svc.open(&id, bounds).await
}

/// APPV2-T03：Web Surface 动态内容区 bounds。
///
/// Host 端验证（finite / 正宽高 / 最小尺寸 / 主窗口内容范围 clamp）后下发
/// `browser_set_bounds`；返回实际生效 bounds。仅对已存在 label 生效
/// （未打开的 surface 在 open 时携带 bounds）。
#[tauri::command]
pub async fn apps_web_set_bounds(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    locks: State<'_, MutationLock>,
    local_runtime: State<'_, LocalRuntimeHandle>,
    id: String,
    bounds: BrowserBounds,
) -> Result<BrowserBounds> {
    let svc = service(&state, &app_handle, &locks, &local_runtime);
    svc.web_set_bounds(&id, bounds).await
}

#[tauri::command]
pub async fn apps_start(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    locks: State<'_, MutationLock>,
    local_runtime: State<'_, LocalRuntimeHandle>,
    id: String,
) -> Result<bool> {
    let svc = service(&state, &app_handle, &locks, &local_runtime);
    svc.start(&id).await
}

#[tauri::command]
pub async fn apps_stop(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    locks: State<'_, MutationLock>,
    local_runtime: State<'_, LocalRuntimeHandle>,
    id: String,
    risk_level: Option<u8>,
) -> Result<bool> {
    let svc = service(&state, &app_handle, &locks, &local_runtime);
    // APPV2-T06：system kind 走验证后的 graceful terminate（超时返回 typed Err）。
    svc.stop(&id, risk_level.unwrap_or(1)).await
}

/// APPV2-T06：隐藏系统应用（AppKit hide 语义）。
///
/// 返回是否真实执行（目标未运行 = `false`，不算错误）。仅 system kind 可用，
/// 其它 kind 返回 typed `InvalidInput`。
#[tauri::command]
pub async fn apps_system_hide(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    locks: State<'_, MutationLock>,
    local_runtime: State<'_, LocalRuntimeHandle>,
    id: String,
) -> Result<bool> {
    let svc = service(&state, &app_handle, &locks, &local_runtime);
    svc.hide_system(&id).await
}

/// APPV2-T06：恢复（取消隐藏）系统应用。口径同 [`apps_system_hide`]。
#[tauri::command]
pub async fn apps_system_unhide(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    locks: State<'_, MutationLock>,
    local_runtime: State<'_, LocalRuntimeHandle>,
    id: String,
) -> Result<bool> {
    let svc = service(&state, &app_handle, &locks, &local_runtime);
    svc.unhide_system(&id).await
}

/// Read the live macOS process/window state without changing it.
#[tauri::command]
pub async fn apps_system_observe(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    locks: State<'_, MutationLock>,
    local_runtime: State<'_, LocalRuntimeHandle>,
    id: String,
) -> Result<system::SystemRunningState> {
    let svc = service(&state, &app_handle, &locks, &local_runtime);
    svc.observe_system(&id).await
}

/// Move/resize one standard macOS window once; no persistent tracking.
#[tauri::command]
pub async fn apps_system_dock(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    locks: State<'_, MutationLock>,
    local_runtime: State<'_, LocalRuntimeHandle>,
    id: String,
    bounds: BrowserBounds,
) -> Result<system::DockResult> {
    let svc = service(&state, &app_handle, &locks, &local_runtime);
    svc.dock_system(&id, bounds).await
}

#[tauri::command]
pub fn apps_system_open_accessibility_settings() -> Result<bool> {
    #[cfg(target_os = "macos")]
    {
        open::that("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
            .map_err(|e| crate::Error::Internal(e.to_string()))?;
        Ok(true)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(system::unsupported(
            "Accessibility settings are only available on macOS",
        ))
    }
}

#[tauri::command]
pub fn apps_activate_host(app_handle: AppHandle) -> Result<bool> {
    let main = app_handle
        .get_webview_window("main")
        .ok_or_else(|| Error::NotFound("main window".into()))?;
    main.show().map_err(|e| Error::Internal(e.to_string()))?;
    main.set_focus()
        .map_err(|e| Error::Internal(e.to_string()))?;
    Ok(true)
}

#[tauri::command]
pub async fn apps_restart(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    locks: State<'_, MutationLock>,
    local_runtime: State<'_, LocalRuntimeHandle>,
    id: String,
) -> Result<bool> {
    let svc = service(&state, &app_handle, &locks, &local_runtime);
    svc.restart(&id).await
}

#[tauri::command]
pub async fn apps_force_stop(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    locks: State<'_, MutationLock>,
    local_runtime: State<'_, LocalRuntimeHandle>,
    id: String,
) -> Result<bool> {
    let svc = service(&state, &app_handle, &locks, &local_runtime);
    svc.force_stop(&id).await
}

#[tauri::command]
pub async fn apps_resolve_orphan(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    locks: State<'_, MutationLock>,
    local_runtime: State<'_, LocalRuntimeHandle>,
    id: String,
    action: String,
) -> Result<bool> {
    let svc = service(&state, &app_handle, &locks, &local_runtime);
    svc.local_resolve_orphan(&id, &action).await?;
    Ok(true)
}

// ── Web Surface ───────────────────────────────────────────────────────────

#[tauri::command]
pub fn apps_web_close(
    app_handle: AppHandle,
    browser: State<'_, BrowserStateHandle>,
    id: String,
) -> Result<bool> {
    web::close(&app_handle, &browser, &id)?;
    Ok(true)
}

#[tauri::command]
pub fn apps_web_hide(app_handle: AppHandle, id: String) -> Result<bool> {
    web::hide(&app_handle, &id)?;
    Ok(true)
}

#[tauri::command]
pub fn apps_web_reload(app_handle: AppHandle, id: String) -> Result<bool> {
    web::reload(&app_handle, &id)?;
    Ok(true)
}

#[tauri::command]
pub fn apps_web_back(app_handle: AppHandle, id: String) -> Result<bool> {
    web::back(&app_handle, &id)?;
    Ok(true)
}

#[tauri::command]
pub fn apps_web_forward(app_handle: AppHandle, id: String) -> Result<bool> {
    web::forward(&app_handle, &id)?;
    Ok(true)
}

#[tauri::command]
pub fn apps_web_clear_data(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    locks: State<'_, MutationLock>,
    local_runtime: State<'_, LocalRuntimeHandle>,
    id: String,
) -> Result<bool> {
    let svc = service(&state, &app_handle, &locks, &local_runtime);
    svc.web_clear_data(&id)?;
    Ok(true)
}

// ── Legacy Compat ─────────────────────────────────────────────────────────

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
pub fn apps_create(state: State<'_, AppState>, input: CreateAppInput) -> Result<App> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    let kind = match input.source.as_str() {
        "remote" | "web" => AppKind::WebApplication,
        "native" | "system" => AppKind::SystemApplication,
        _ => AppKind::LocalProject,
    };
    let app_id = match kind {
        AppKind::LocalProject => AppRepository::register_local(
            &conn,
            &input.title,
            &input.source_id,
            input.description.as_deref(),
            input.icon.as_deref(),
            RegistrationOrigin::Manual,
        )?,
        AppKind::SystemApplication => AppRepository::register_system(
            &conn,
            &input.title,
            &input.source_id,
            None,
            "macos",
            None,
            RegistrationOrigin::Manual,
        )?,
        AppKind::WebApplication => {
            AppRepository::register_web(&conn, &input.title, &input.source_id, &[], None, false)?
        }
    };
    AppRepository::get(&conn, &app_id)?.ok_or_else(|| Error::NotFound(app_id))
}

#[tauri::command]
pub async fn apps_delete(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    locks: State<'_, MutationLock>,
    local_runtime: State<'_, LocalRuntimeHandle>,
    id: String,
) -> Result<bool> {
    let svc = service(&state, &app_handle, &locks, &local_runtime);
    svc.remove(&id, 0).await?;
    Ok(true)
}

#[tauri::command]
pub async fn apps_kill(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    locks: State<'_, MutationLock>,
    local_runtime: State<'_, LocalRuntimeHandle>,
    id: String,
) -> Result<bool> {
    let svc = service(&state, &app_handle, &locks, &local_runtime);
    svc.force_stop(&id).await
}

#[tauri::command]
pub fn apps_health(state: State<'_, AppState>, application_id: String) -> Result<AppHealthResult> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    let instances = AppRepository::list_instances(&conn, &application_id)?;
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
