use crate::{Error, Result};
use serde_json::Value as JsonValue;

#[tauri::command]
pub fn git_status(dir_path: String) -> Result<JsonValue> {
    // TODO: Implement in Loop 10
    Err(Error::NotImplemented(format!("git:status({dir_path})")))
}

#[tauri::command]
pub fn git_diff(file_path: String) -> Result<String> {
    // TODO: Implement in Loop 10
    Err(Error::NotImplemented(format!("git:diff({file_path})")))
}
