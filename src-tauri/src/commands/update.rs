use crate::{Error, Result};
use serde_json::Value as JsonValue;

#[tauri::command]
pub fn update_check() -> Result<JsonValue> {
    // TODO: Implement in Loop 12
    Err(Error::NotImplemented("update:check".into()))
}

#[tauri::command]
pub fn update_mute(version: String) -> Result<()> {
    // TODO: Implement in Loop 12
    Err(Error::NotImplemented(format!("update:mute({version})")))
}

#[tauri::command]
pub fn update_get_muted() -> Result<Vec<String>> {
    // TODO: Implement in Loop 12
    Err(Error::NotImplemented("update:getMuted".into()))
}
