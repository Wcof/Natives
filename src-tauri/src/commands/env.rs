use crate::{emit_db_state_changed, env_manager, Error, Result};
use tauri::State;

use crate::AppState;

#[tauri::command]
pub fn env_get_default_profile(state: State<'_, AppState>) -> Result<String> {
    let pool_conn = state
        .db
        .get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &pool_conn;
    match env_manager::get_default_profile(conn)? {
        Some(profile) => Ok(profile.name),
        None => Ok(String::new()),
    }
}

#[tauri::command]
pub fn env_list_profiles(state: State<'_, AppState>) -> Result<Vec<env_manager::EnvProfile>> {
    let pool_conn = state
        .db
        .get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &pool_conn;
    env_manager::list_profiles(conn)
}

#[tauri::command]
pub fn env_create_profile(
    name: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<()> {
    let pool_conn = state
        .db
        .get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &pool_conn;
    env_manager::create_profile(conn, &name)?;
    emit_db_state_changed(
        &app_handle,
        "env",
        serde_json::json!({ "action": "create_profile", "name": name }),
    );
    Ok(())
}

#[tauri::command]
pub fn env_delete_profile(
    name: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<()> {
    let pool_conn = state
        .db
        .get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &pool_conn;
    env_manager::delete_profile(conn, &name)?;
    emit_db_state_changed(
        &app_handle,
        "env",
        serde_json::json!({ "action": "delete_profile", "name": name }),
    );
    Ok(())
}

#[tauri::command]
pub fn env_set_default_profile(
    name: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<()> {
    let pool_conn = state
        .db
        .get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &pool_conn;
    env_manager::set_default_profile(conn, &name)?;
    emit_db_state_changed(
        &app_handle,
        "env",
        serde_json::json!({ "action": "set_default_profile", "name": name }),
    );
    Ok(())
}

#[tauri::command]
pub fn env_set_variable(
    profile_id: String,
    key: String,
    value: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<()> {
    let pool_conn = state
        .db
        .get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &pool_conn;
    let encryption_key = env_manager::get_encryption_key(conn)?;
    let id: i64 = profile_id
        .parse()
        .map_err(|_| Error::InvalidInput("invalid profile id".into()))?;
    env_manager::set_variable(conn, id, &key, &value, &encryption_key)?;
    emit_db_state_changed(
        &app_handle,
        "env",
        serde_json::json!({ "action": "set_variable", "profileId": profile_id, "key": key }),
    );
    Ok(())
}

#[tauri::command]
pub fn env_delete_variable(
    profile_id: String,
    key: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<()> {
    let pool_conn = state
        .db
        .get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &pool_conn;
    let id: i64 = profile_id
        .parse()
        .map_err(|_| Error::InvalidInput("invalid profile id".into()))?;
    env_manager::delete_variable(conn, id, &key)?;
    emit_db_state_changed(
        &app_handle,
        "env",
        serde_json::json!({ "action": "delete_variable", "profileId": profile_id, "key": key }),
    );
    Ok(())
}

// Decryption stays Host-only for provider use and terminal environment injection.
