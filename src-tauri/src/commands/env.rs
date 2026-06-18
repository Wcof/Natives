use crate::{env_manager, Error, Result};
use serde_json::Value as JsonValue;
use tauri::State;

use crate::AppState;

#[tauri::command]
pub fn env_get_variables(profile_id: String, state: State<'_, AppState>) -> Result<JsonValue> {
    let guard = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| Error::Internal("database not initialized".into()))?;
    let encryption_key = env_manager::get_encryption_key(conn)?;
    let id: i64 = profile_id
        .parse()
        .map_err(|_| Error::InvalidInput("invalid profile id".into()))?;
    let vars = env_manager::get_variables(conn, id, &encryption_key)?;
    serde_json::to_value(vars).map_err(|e| Error::Internal(e.to_string()))
}

#[tauri::command]
pub fn env_get_default_profile(state: State<'_, AppState>) -> Result<String> {
    let guard = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| Error::Internal("database not initialized".into()))?;
    match env_manager::get_default_profile(conn)? {
        Some(profile) => Ok(profile.name),
        None => Ok(String::new()),
    }
}

#[tauri::command]
pub fn env_list_profiles(state: State<'_, AppState>) -> Result<Vec<JsonValue>> {
    let guard = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| Error::Internal("database not initialized".into()))?;
    let profiles = env_manager::list_profiles(conn)?;
    serde_json::to_value(profiles)
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
pub fn env_create_profile(name: String, state: State<'_, AppState>) -> Result<()> {
    let guard = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| Error::Internal("database not initialized".into()))?;
    env_manager::create_profile(conn, &name)
}

#[tauri::command]
pub fn env_delete_profile(name: String, state: State<'_, AppState>) -> Result<()> {
    let guard = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| Error::Internal("database not initialized".into()))?;
    env_manager::delete_profile(conn, &name)
}

#[tauri::command]
pub fn env_set_default_profile(name: String, state: State<'_, AppState>) -> Result<()> {
    let guard = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| Error::Internal("database not initialized".into()))?;
    env_manager::set_default_profile(conn, &name)
}

#[tauri::command]
pub fn env_set_variable(
    profile_id: String,
    key: String,
    value: String,
    state: State<'_, AppState>,
) -> Result<()> {
    let guard = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| Error::Internal("database not initialized".into()))?;
    let encryption_key = env_manager::get_encryption_key(conn)?;
    let id: i64 = profile_id
        .parse()
        .map_err(|_| Error::InvalidInput("invalid profile id".into()))?;
    env_manager::set_variable(conn, id, &key, &value, &encryption_key)
}

#[tauri::command]
pub fn env_delete_variable(
    profile_id: String,
    key: String,
    state: State<'_, AppState>,
) -> Result<()> {
    let guard = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| Error::Internal("database not initialized".into()))?;
    let id: i64 = profile_id
        .parse()
        .map_err(|_| Error::InvalidInput("invalid profile id".into()))?;
    env_manager::delete_variable(conn, id, &key)
}

#[tauri::command]
pub fn env_encrypt(text: String, state: State<'_, AppState>) -> Result<String> {
    let guard = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| Error::Internal("database not initialized".into()))?;
    let encryption_key = env_manager::get_encryption_key(conn)?;
    env_manager::encrypt(&text, &encryption_key)
}

#[tauri::command]
pub fn env_decrypt(encrypted: String, state: State<'_, AppState>) -> Result<String> {
    let guard = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| Error::Internal("database not initialized".into()))?;
    let encryption_key = env_manager::get_encryption_key(conn)?;
    env_manager::decrypt(&encrypted, &encryption_key)
}
