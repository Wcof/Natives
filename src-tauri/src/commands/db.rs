use crate::{Error, Result};
use serde_json::Value as JsonValue;
use tauri::State;

use crate::AppState;

#[tauri::command]
pub fn db_get(key: String, state: State<'_, AppState>) -> Result<Option<JsonValue>> {
    let _db = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    // TODO: Implement in Loop 3
    Err(Error::NotImplemented(format!("db:get({key})")))
}

#[tauri::command]
pub fn db_set(key: String, value: JsonValue, state: State<'_, AppState>) -> Result<()> {
    let _db = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    // TODO: Implement in Loop 3
    Err(Error::NotImplemented(format!("db:set({key})")))
}

#[tauri::command]
pub fn db_delete(key: String, state: State<'_, AppState>) -> Result<()> {
    let _db = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    // TODO: Implement in Loop 3
    Err(Error::NotImplemented(format!("db:delete({key})")))
}

#[tauri::command]
pub fn db_list(prefix: Option<String>, state: State<'_, AppState>) -> Result<Vec<JsonValue>> {
    let _db = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    // TODO: Implement in Loop 3
    Err(Error::NotImplemented(format!("db:list({prefix:?})")))
}
