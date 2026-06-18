use crate::{db, Error, Result};
use serde_json::Value as JsonValue;
use tauri::{Emitter, State};

use crate::AppState;

#[tauri::command]
pub fn notification_send(
    title: String,
    body: String,
    level: Option<String>,
    state: State<'_, AppState>,
) -> Result<()> {
    let guard = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| Error::Internal("database not initialized".into()))?;
    let lvl = level.as_deref().unwrap_or("info");
    let _id = db::create_notification(conn, &title, &body, lvl, None)?;
    Ok(())
}

#[tauri::command]
pub fn notification_list(
    unread_only: Option<bool>,
    state: State<'_, AppState>,
) -> Result<Vec<JsonValue>> {
    let guard = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| Error::Internal("database not initialized".into()))?;
    db::list_notifications(conn, unread_only.unwrap_or(false))
}

#[tauri::command]
pub fn notification_mark_read(id: i64, state: State<'_, AppState>) -> Result<()> {
    let guard = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| Error::Internal("database not initialized".into()))?;
    db::mark_notification_read(conn, id)
}

#[tauri::command]
pub fn notification_mark_all_read(state: State<'_, AppState>) -> Result<()> {
    let guard = state.db.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let conn = guard
        .as_ref()
        .ok_or_else(|| Error::Internal("database not initialized".into()))?;
    db::mark_all_notifications_read(conn)
}
