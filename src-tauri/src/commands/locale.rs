use crate::{db, emit_db_state_changed, Error, Result};
use tauri::State;

use crate::AppState;

const LOCALE_KEY: &str = "settings:locale";
const DEFAULT_LOCALE: &str = "zh";

#[tauri::command]
pub fn get_locale(state: State<'_, AppState>) -> Result<String> {
    let pool_conn = state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;
    Ok(db::get_setting(conn, LOCALE_KEY)?
        .unwrap_or_else(|| DEFAULT_LOCALE.to_string()))
}

#[tauri::command]
pub fn set_locale(locale: String, app_handle: tauri::AppHandle, state: State<'_, AppState>) -> Result<()> {
    let pool_conn = state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;
    db::set_setting(conn, LOCALE_KEY, &locale)?;
    emit_db_state_changed(&app_handle, "locale", serde_json::json!({ "locale": locale }));
    Ok(())
}
