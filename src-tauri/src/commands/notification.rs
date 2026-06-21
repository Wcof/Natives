use crate::{db, emit_db_state_changed, Error, Result};
use serde_json::Value as JsonValue;
use tauri::State;

use crate::AppState;

#[tauri::command]
pub fn notification_send(
    title: String,
    body: String,
    level: Option<String>,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<()> {
    let pool_conn = state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;
    let lvl = level.as_deref().unwrap_or("info");
    let id = db::create_notification(conn, &title, &body, lvl, None)?;
    emit_db_state_changed(&app_handle, "notification", serde_json::json!({ "action": "send", "id": id }));
    Ok(())
}

#[tauri::command]
pub fn notification_list(
    unread_only: Option<bool>,
    state: State<'_, AppState>,
) -> Result<Vec<JsonValue>> {
    let pool_conn = state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;
    db::list_notifications(conn, unread_only.unwrap_or(false))
}

#[tauri::command]
pub fn notification_mark_read(id: i64, app_handle: tauri::AppHandle, state: State<'_, AppState>) -> Result<()> {
    let pool_conn = state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;
    db::mark_notification_read(conn, id)?;
    emit_db_state_changed(&app_handle, "notification", serde_json::json!({ "action": "mark_read", "id": id }));
    Ok(())
}

#[tauri::command]
pub fn notification_mark_all_read(app_handle: tauri::AppHandle, state: State<'_, AppState>) -> Result<()> {
    let pool_conn = state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;
    db::mark_all_notifications_read(conn)?;
    emit_db_state_changed(&app_handle, "notification", serde_json::json!({ "action": "mark_all_read" }));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_placeholder() {
        assert!(true);
    }
}
