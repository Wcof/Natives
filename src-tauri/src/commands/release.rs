use crate::{Error, Result};
use serde_json::Value as JsonValue;

#[tauri::command]
pub fn release_inspect(project_path: String) -> Result<JsonValue> {
    // TODO: Implement in Loop 12
    Err(Error::NotImplemented(format!("release:inspect({project_path})")))
}

#[tauri::command]
pub fn release_prepare(project_path: String, version: String) -> Result<JsonValue> {
    // TODO: Implement in Loop 12
    Err(Error::NotImplemented(format!("release:prepare({project_path}, {version})")))
}

#[tauri::command]
pub fn release_get_sequence(project_path: String, version: String) -> Result<JsonValue> {
    // TODO: Implement in Loop 12
    Err(Error::NotImplemented(format!("release:getSequence({project_path}, {version})")))
}

#[tauri::command]
pub fn release_execute(project_path: String, command: String) -> Result<JsonValue> {
    // TODO: Implement in Loop 12
    Err(Error::NotImplemented(format!("release:execute({project_path}, {command})")))
}
