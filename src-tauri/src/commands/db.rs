use crate::{db, Error, Result};
use serde_json::Value as JsonValue;
use tauri::{Emitter, State};

use crate::AppState;

#[derive(Clone, serde::Serialize)]
struct DbPayload {
    channel: String,
    data: serde_json::Value,
}

#[tauri::command]
pub fn db_get(key: String, state: State<'_, AppState>) -> Result<Option<JsonValue>> {
    let guard = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| Error::Internal("database not initialized".into()))?;
    db::db_get(conn, &key)
}

#[tauri::command]
pub fn db_set(key: String, value: JsonValue, app_handle: tauri::AppHandle, state: State<'_, AppState>) -> Result<()> {
    let guard = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| Error::Internal("database not initialized".into()))?;
    db::db_set(conn, &key, &value)?;

    // Broadcast change to all webviews so live theme/locale/config updates are reflected
    let payload = DbPayload {
        channel: "module_data".into(),
        data: serde_json::json!({ "key": key }),
    };
    let _ = app_handle.emit("db-state-changed", payload);
    Ok(())
}

#[tauri::command]
pub fn db_delete(key: String, app_handle: tauri::AppHandle, state: State<'_, AppState>) -> Result<()> {
    let guard = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| Error::Internal("database not initialized".into()))?;
    db::db_delete(conn, &key)?;

    // Broadcast deletion event
    let payload = DbPayload {
        channel: "module_data".into(),
        data: serde_json::json!({ "key": key, "deleted": true }),
    };
    let _ = app_handle.emit("db-state-changed", payload);
    Ok(())
}

#[tauri::command]
pub fn db_list(prefix: Option<String>, state: State<'_, AppState>) -> Result<Vec<String>> {
    let guard = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| Error::Internal("database not initialized".into()))?;
    db::db_list(conn, prefix.as_deref())
}
