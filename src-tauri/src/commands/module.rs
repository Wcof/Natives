use crate::{module_manager, permission_center, emit_db_state_changed, Error, Result};
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
pub fn module_install(path_or_zip: String, app_handle: tauri::AppHandle, state: State<'_, AppState>) -> Result<JsonValue> {
    let pool_conn = state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;
    let module_id = module_manager::install_module(conn, &modules_dir(), &path_or_zip)?;
    emit_db_state_changed(&app_handle, "module", serde_json::json!({ "action": "install", "moduleId": module_id }));
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
    let pool_conn = state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;
    permission_center::grant_permission(conn, &module_id, &permission, None)
}

#[tauri::command]
pub fn module_revoke_permission(
    module_id: String,
    permission: String,
    state: State<'_, AppState>,
) -> Result<()> {
    let pool_conn = state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;
    permission_center::revoke_permission(conn, &module_id, &permission, None)
}

#[tauri::command]
pub fn module_list_permissions(
    module_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<JsonValue>> {
    let pool_conn = state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;
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
    let pool_conn = state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;
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
    let pool_conn = state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;
    permission_center::approve_all_permissions(conn, &module_id, None)
}

#[tauri::command]
pub fn module_uninstall(module_id: String, app_handle: tauri::AppHandle, state: State<'_, AppState>) -> Result<()> {
    let pool_conn = state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;
    module_manager::uninstall_module(conn, &modules_dir(), &module_id)?;
    emit_db_state_changed(&app_handle, "module", serde_json::json!({ "action": "uninstall", "moduleId": module_id }));
    Ok(())
}

#[tauri::command]
pub fn module_list(state: State<'_, AppState>) -> Result<Vec<JsonValue>> {
    let pool_conn = state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;
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
pub fn module_enable(module_id: String, app_handle: tauri::AppHandle, state: State<'_, AppState>) -> Result<()> {
    let pool_conn = state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;
    module_manager::enable_module(conn, &module_id)?;
    emit_db_state_changed(&app_handle, "module", serde_json::json!({ "action": "enable", "moduleId": module_id }));
    Ok(())
}

#[tauri::command]
pub fn module_disable(module_id: String, app_handle: tauri::AppHandle, state: State<'_, AppState>) -> Result<()> {
    let pool_conn = state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;
    module_manager::disable_module(conn, &module_id)?;
    emit_db_state_changed(&app_handle, "module", serde_json::json!({ "action": "disable", "moduleId": module_id }));
    Ok(())
}

#[tauri::command]
pub fn module_update(module_id: String, source: Option<String>, app_handle: tauri::AppHandle, state: State<'_, AppState>) -> Result<JsonValue> {
    let pool_conn = state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;
    let mdir = modules_dir();
    if let Some(src) = source {
        // Full update: read new manifest → replace files → re-sync permissions
        let updated_id = module_manager::update_module(conn, &mdir, &module_id, &src)?;
        emit_db_state_changed(&app_handle, "module", serde_json::json!({ "action": "update", "moduleId": updated_id }));
        Ok(serde_json::json!({ "moduleId": updated_id, "ok": true }))
    } else {
        // No source provided: just re-sync DB from existing files on disk
        module_manager::sync_modules_to_db(conn, &mdir)?;
        emit_db_state_changed(&app_handle, "module", serde_json::json!({ "action": "update", "moduleId": module_id }));
        Ok(serde_json::json!({ "moduleId": module_id, "ok": true }))
    }
}

/// Write an AI-generated module to disk and hot-sync into the running system.
/// This is the primary entry point for the "AI App Engine" — the AI brain calls
/// this command to materialize generated HTML/JS/Tailwind code as a live module.
#[tauri::command]
pub fn write_generated_module(
    module_id: String,
    name: String,
    html_content: String,
    permissions: Vec<String>,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<JsonValue> {
    let pool_conn = state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;
    module_manager::write_generated_module(
        conn,
        &modules_dir(),
        &module_id,
        &name,
        &html_content,
        &permissions,
    )?;
    // Emit hot-reload event so frontend menu refreshes immediately
    emit_db_state_changed(
        &app_handle,
        "module",
        serde_json::json!({ "action": "generated", "moduleId": module_id }),
    );
    Ok(serde_json::json!({ "moduleId": module_id, "ok": true }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_placeholder() {
        assert!(true);
    }
}
