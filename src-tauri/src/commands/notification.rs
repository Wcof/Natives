use crate::{Error, Result};
use serde_json::Value as JsonValue;

#[tauri::command]
pub fn notification_send(title: String, body: String, level: Option<String>) -> Result<()> {
    // TODO: Implement in Loop 3 — write to DB + broadcast
    Err(Error::NotImplemented(format!("notification:send({title})")))
}

#[tauri::command]
pub fn notification_list(unread_only: Option<bool>) -> Result<Vec<JsonValue>> {
    // TODO: Implement in Loop 3
    Err(Error::NotImplemented("notification:list".into()))
}

#[tauri::command]
pub fn notification_mark_read(id: u32) -> Result<()> {
    // TODO: Implement in Loop 3
    Err(Error::NotImplemented(format!("notification:markRead({id})")))
}

#[tauri::command]
pub fn notification_mark_all_read() -> Result<()> {
    // TODO: Implement in Loop 3
    Err(Error::NotImplemented("notification:markAllRead".into()))
}
