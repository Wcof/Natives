use crate::{module_manager, permission_center, Error, Result};
use serde_json::Value as JsonValue;
use tauri::State;

use crate::AppState;

fn modules_dir() -> std::path::PathBuf {
    dirs_or_home().join(".natives").join("modules")
}

fn dirs_or_home() -> std::path::PathBuf {
    dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."))
}

#[tauri::command]
pub fn module_scan() -> Result<Vec<JsonValue>> {
    let dir = modules_dir();
    let results = module_manager::scan_modules(&dir);
    serde_json::to_value(results)
        .map(|v| {
            if let JsonValue::Array(arr) = v {
                arr
            } else {
                vec![]
            }
        })
        .map_err(|e| Error::Internal(e.to_string()))
}

#[tauri::command]
pub fn module_install(path_or_zip: String, state: State<'_, AppState>) -> Result<JsonValue> {
    let guard = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| Error::Internal("database not initialized".into()))?;
    let module_id = module_manager::install_module(conn, &modules_dir(), &path_or_zip)?;
    Ok(serde_json::json!({ "moduleId": module_id }))
}

#[tauri::command]
pub fn module_read_manifest(source: String) -> Result<JsonValue> {
    let manifest =
        module_manager::read_manifest_from_source(&modules_dir(), &source)
            .map_err(|e| Error::InvalidInput(e))?;
    serde_json::to_value(manifest).map_err(|e| Error::Internal(e.to_string()))
}

#[tauri::command]
pub fn module_grant_permission(
    module_id: String,
    permission: String,
    state: State<'_, AppState>,
) -> Result<()> {
    let guard = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| Error::Internal("database not initialized".into()))?;
    permission_center::grant_permission(conn, &module_id, &permission, None)
}

#[tauri::command]
pub fn module_revoke_permission(
    module_id: String,
    permission: String,
    state: State<'_, AppState>,
) -> Result<()> {
    let guard = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| Error::Internal("database not initialized".into()))?;
    permission_center::revoke_permission(conn, &module_id, &permission, None)
}

#[tauri::command]
pub fn module_list_permissions(
    module_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<JsonValue>> {
    let guard = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| Error::Internal("database not initialized".into()))?;
    let records = permission_center::list_permissions(conn, &module_id)?;
    serde_json::to_value(records)
        .map(|v| {
            if let JsonValue::Array(arr) = v {
                arr
            } else {
                vec![]
            }
        })
        .map_err(|e| Error::Internal(e.to_string()))
}

#[tauri::command]
pub fn module_get_audit_log(
    module_id: Option<String>,
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> Result<Vec<JsonValue>> {
    let guard = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| Error::Internal("database not initialized".into()))?;
    let entries =
        permission_center::get_audit_log(conn, module_id.as_deref(), limit.unwrap_or(50))?;
    serde_json::to_value(entries)
        .map(|v| {
            if let JsonValue::Array(arr) = v {
                arr
            } else {
                vec![]
            }
        })
        .map_err(|e| Error::Internal(e.to_string()))
}

#[tauri::command]
pub fn module_approve_all_permissions(
    module_id: String,
    state: State<'_, AppState>,
) -> Result<()> {
    let guard = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| Error::Internal("database not initialized".into()))?;
    permission_center::approve_all_permissions(conn, &module_id, None)
}

#[tauri::command]
pub fn module_uninstall(module_id: String, state: State<'_, AppState>) -> Result<()> {
    let guard = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| Error::Internal("database not initialized".into()))?;
    module_manager::uninstall_module(conn, &modules_dir(), &module_id)
}

#[tauri::command]
pub fn module_list(state: State<'_, AppState>) -> Result<Vec<JsonValue>> {
    let guard = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| Error::Internal("database not initialized".into()))?;
    let modules = module_manager::list_modules(conn)?;
    serde_json::to_value(modules)
        .map(|v| {
            if let JsonValue::Array(arr) = v {
                arr
            } else {
                vec![]
            }
        })
        .map_err(|e| Error::Internal(e.to_string()))
}

#[tauri::command]
pub fn module_enable(module_id: String, state: State<'_, AppState>) -> Result<()> {
    let guard = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| Error::Internal("database not initialized".into()))?;
    module_manager::enable_module(conn, &module_id)
}

#[tauri::command]
pub fn module_disable(module_id: String, state: State<'_, AppState>) -> Result<()> {
    let guard = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| Error::Internal("database not initialized".into()))?;
    module_manager::disable_module(conn, &module_id)
}

#[tauri::command]
pub fn module_update(module_id: String, state: State<'_, AppState>) -> Result<()> {
    // TODO: Full update flow (read new manifest, replace files, re-sync)
    // For now, re-sync the module from disk
    let guard = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| Error::Internal("database not initialized".into()))?;
    module_manager::sync_modules_to_db(conn, &modules_dir())
}
