use crate::{db, Error, Result};
use tauri::State;

use crate::AppState;

const SYSTEM_MODULE_ID: &str = "__system__";

/// Save module state to module_data table.
/// Key format: "_state:{moduleId}"
#[tauri::command]
pub fn state_save(module_id: String, state_value: String, state: State<'_, AppState>) -> Result<()> {
    let guard = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| Error::Internal("database not initialized".into()))?;
    let key = format!("_state:{module_id}");
    let json_val = serde_json::Value::String(state_value);
    db::db_set_module_data(conn, SYSTEM_MODULE_ID, &key, &json_val)
}

/// Load module state from module_data table.
#[tauri::command]
pub fn state_load(module_id: String, state: State<'_, AppState>) -> Result<Option<String>> {
    let guard = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| Error::Internal("database not initialized".into()))?;
    let key = format!("_state:{module_id}");
    match db::db_get_module_data(conn, SYSTEM_MODULE_ID, &key)? {
        Some(serde_json::Value::String(s)) => Ok(Some(s)),
        Some(v) => Ok(Some(v.to_string())),
        None => Ok(None),
    }
}

/// Clear module state from module_data table.
#[tauri::command]
pub fn state_clear(module_id: String, state: State<'_, AppState>) -> Result<()> {
    let guard = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| Error::Internal("database not initialized".into()))?;
    let key = format!("_state:{module_id}");
    db::db_delete_module_data(conn, SYSTEM_MODULE_ID, &key)
}
