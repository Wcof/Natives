//! Tauri commands for dual-source Creative Apps (ADR-0013).
//!
//! All async commands open rusqlite connections only in short synchronous
//! scopes (or inside `spawn_blocking`) so `Connection` is never held across
//! `.await` — Connection is !Send.

use crate::creative_app::browser::{self, BrowserStateHandle};
use crate::creative_app::docker;
use crate::creative_app::install;
use crate::creative_app::model::*;
use crate::creative_app::service::{self, MutationLock};
use crate::creative_app::store;
use crate::db::DbPool;
use crate::{Error, Result};
use tauri::State;

use crate::AppState;

fn modules_dir() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".natives")
        .join("modules")
}

fn conn(pool: &DbPool) -> Result<r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>> {
    pool.get()
        .map_err(|e| Error::Internal(format!("db: {e}")))
}

#[tauri::command]
pub fn creative_app_list(state: State<'_, AppState>) -> Result<Vec<CreativeAppSummary>> {
    let c = conn(&state.db)?;
    service::CreativeAppService::list(&c)
}

#[tauri::command]
pub async fn creative_app_start(
    id: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    lock: State<'_, MutationLock>,
) -> Result<CreativeAppSummary> {
    let pool = state.db.clone();
    let lock = lock.inner().clone();
    let _guard = lock.lock().await;
    let handle = app_handle.clone();
    tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Handle::current();
        let c = conn(&pool)?;
        // Re-implement start dispatch without holding conn across await in async fn
        if store::get_app(&c, &id)?.is_some() {
            rt.block_on(install::start_app(&c, &handle, &id))
        } else {
            crate::module_manager::enable_module(&c, &id)?;
            crate::emit_db_state_changed(
                &handle,
                "module",
                serde_json::json!({ "action": "enable", "moduleId": id }),
            );
            crate::emit_db_state_changed(
                &handle,
                "creative-app",
                serde_json::json!({ "action": "start", "id": id }),
            );
            service::CreativeAppService::get_summary(&c, &id)
        }
    })
    .await
    .map_err(|e| Error::Internal(format!("start join: {e}")))?
}

#[tauri::command]
pub async fn creative_app_stop(
    id: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    lock: State<'_, MutationLock>,
) -> Result<CreativeAppSummary> {
    let pool = state.db.clone();
    let lock = lock.inner().clone();
    let _guard = lock.lock().await;
    let handle = app_handle.clone();
    tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Handle::current();
        let c = conn(&pool)?;
        if store::get_app(&c, &id)?.is_some() {
            rt.block_on(install::stop_app(&c, &handle, &id))
        } else {
            crate::module_manager::disable_module(&c, &id)?;
            crate::emit_db_state_changed(
                &handle,
                "module",
                serde_json::json!({ "action": "disable", "moduleId": id }),
            );
            crate::emit_db_state_changed(
                &handle,
                "creative-app",
                serde_json::json!({ "action": "stop", "id": id }),
            );
            service::CreativeAppService::get_summary(&c, &id)
        }
    })
    .await
    .map_err(|e| Error::Internal(format!("stop join: {e}")))?
}

#[tauri::command]
pub async fn creative_app_delete(
    id: String,
    options: Option<DeleteOptions>,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    lock: State<'_, MutationLock>,
) -> Result<DeleteResult> {
    let pool = state.db.clone();
    let lock = lock.inner().clone();
    let _guard = lock.lock().await;
    let opts = options.unwrap_or_default();
    let handle = app_handle.clone();
    tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Handle::current();
        let c = conn(&pool)?;
        if store::get_app(&c, &id)?.is_some() {
            rt.block_on(install::delete_app(&c, &handle, &id, opts))
        } else {
            crate::module_manager::uninstall_module(&c, &modules_dir(), &id)?;
            crate::emit_db_state_changed(
                &handle,
                "module",
                serde_json::json!({ "action": "uninstall", "moduleId": id }),
            );
            crate::emit_db_state_changed(
                &handle,
                "creative-app",
                serde_json::json!({ "action": "deleted", "id": id }),
            );
            Ok(DeleteResult {
                ok: true,
                warnings: vec![],
            })
        }
    })
    .await
    .map_err(|e| Error::Internal(format!("delete join: {e}")))?
}

#[tauri::command]
pub fn creative_app_get_open_target(
    id: String,
    state: State<'_, AppState>,
) -> Result<OpenTarget> {
    let c = conn(&state.db)?;
    service::CreativeAppService::get_open_target(&c, &id)
}

#[tauri::command]
pub fn creative_app_inspect_github(
    request: InspectGithubRequest,
    state: State<'_, AppState>,
) -> Result<InspectGithubResult> {
    let c = conn(&state.db)?;
    install::inspect(&c, &request)
}

#[tauri::command]
pub async fn creative_app_install_github(
    request: InstallGithubRequest,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    lock: State<'_, MutationLock>,
) -> Result<CreativeAppSummary> {
    let pool = state.db.clone();
    let lock = lock.inner().clone();
    let _guard = lock.lock().await;
    let handle = app_handle.clone();
    tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Handle::current();
        let c = conn(&pool)?;
        rt.block_on(install::install_github(&c, &handle, request))
    })
    .await
    .map_err(|e| Error::Internal(format!("install join: {e}")))?
}

#[tauri::command]
pub async fn creative_app_logs(
    id: String,
    tail: Option<u32>,
    state: State<'_, AppState>,
) -> Result<String> {
    let pool = state.db.clone();
    let tail = tail.unwrap_or(200) as usize;
    let cfg_json = {
        let c = conn(&pool)?;
        let rec = store::get_app(&c, &id)?.ok_or_else(|| Error::NotFound(id.clone()))?;
        rec.runtime_config_json
    };
    let cfg = store::parse_runtime_config(&cfg_json)?;
    match cfg {
        RuntimeConfig::DockerCompose {
            project_name,
            compose_file,
            ..
        } => {
            docker::compose_logs(
                &project_name,
                &std::path::PathBuf::from(compose_file),
                tail,
            )
            .await
        }
        RuntimeConfig::DockerRun { container_name, .. } => {
            docker::docker_logs(&container_name, tail).await
        }
    }
}

#[tauri::command]
pub async fn creative_app_reconcile(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<u32> {
    let pool = state.db.clone();
    let handle = app_handle.clone();
    tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Handle::current();
        let c = conn(&pool)?;
        let n = rt.block_on(install::reconcile_all(&c, Some(&handle)))?;
        Ok(n as u32)
    })
    .await
    .map_err(|e| Error::Internal(format!("reconcile join: {e}")))?
}

#[tauri::command]
pub fn creative_app_github_token_status(
    state: State<'_, AppState>,
) -> Result<GithubTokenStatus> {
    let c = conn(&state.db)?;
    store::github_token_status(&c)
}

#[tauri::command]
pub fn creative_app_github_token_set(
    token: String,
    state: State<'_, AppState>,
) -> Result<GithubTokenStatus> {
    let c = conn(&state.db)?;
    store::set_github_token(&c, token.trim())?;
    store::github_token_status(&c)
}

#[tauri::command]
pub fn creative_app_github_token_clear(state: State<'_, AppState>) -> Result<GithubTokenStatus> {
    let c = conn(&state.db)?;
    store::clear_github_token(&c)?;
    store::github_token_status(&c)
}

#[tauri::command]
pub async fn creative_app_docker_status() -> Result<DockerEngineStatus> {
    Ok(docker::engine_status().await)
}

// ── Child browser ──────────────────────────────────────────────

#[tauri::command]
pub fn creative_app_browser_show(
    app_id: String,
    url: String,
    bounds: BrowserBounds,
    app_handle: tauri::AppHandle,
    browser: State<'_, BrowserStateHandle>,
) -> Result<()> {
    browser::browser_show(&app_handle, &browser, &app_id, &url, bounds)
}

#[tauri::command]
pub fn creative_app_browser_set_bounds(
    bounds: BrowserBounds,
    app_handle: tauri::AppHandle,
) -> Result<()> {
    browser::browser_set_bounds(&app_handle, bounds)
}

#[tauri::command]
pub fn creative_app_browser_back(app_handle: tauri::AppHandle) -> Result<()> {
    browser::browser_back(&app_handle)
}

#[tauri::command]
pub fn creative_app_browser_forward(app_handle: tauri::AppHandle) -> Result<()> {
    browser::browser_forward(&app_handle)
}

#[tauri::command]
pub fn creative_app_browser_reload(app_handle: tauri::AppHandle) -> Result<()> {
    browser::browser_reload(&app_handle)
}

#[tauri::command]
pub fn creative_app_browser_hide(app_handle: tauri::AppHandle) -> Result<()> {
    browser::browser_hide(&app_handle)
}

#[tauri::command]
pub fn creative_app_browser_close(
    app_handle: tauri::AppHandle,
    browser: State<'_, BrowserStateHandle>,
) -> Result<()> {
    browser::browser_close(&app_handle, &browser)
}

#[tauri::command]
pub fn creative_app_browser_current(
    browser: State<'_, BrowserStateHandle>,
) -> Result<serde_json::Value> {
    browser::browser_current(&browser)
}
