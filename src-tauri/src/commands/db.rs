use crate::{db, Error, Result};
use serde_json::Value as JsonValue;
use tauri::{Emitter, State};

use crate::AppState;

#[tauri::command]
pub fn db_get(key: String, state: State<'_, AppState>) -> Result<Option<JsonValue>> {
    let guard = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| Error::Internal("database not initialized".into()))?;
    db::db_get(conn, &key)
}

#[tauri::command]
pub fn db_set(key: String, value: JsonValue, state: State<'_, AppState>) -> Result<()> {
    let guard = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| Error::Internal("database not initialized".into()))?;
    db::db_set(conn, &key, &value)
}

#[tauri::command]
pub fn db_delete(key: String, state: State<'_, AppState>) -> Result<()> {
    let guard = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| Error::Internal("database not initialized".into()))?;
    db::db_delete(conn, &key)
}

#[tauri::command]
pub fn db_list(prefix: Option<String>, state: State<'_, AppState>) -> Result<Vec<String>> {
    let guard = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| Error::Internal("database not initialized".into()))?;
    db::db_list(conn, prefix.as_deref())
}
