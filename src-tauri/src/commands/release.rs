use crate::{release_wizard, Error, Result};
use serde_json::Value as JsonValue;

#[tauri::command]
pub fn release_inspect(project_path: String) -> Result<JsonValue> {
    let inspection = release_wizard::inspect_project(&project_path)?;
    serde_json::to_value(inspection).map_err(|e| Error::Internal(e.to_string()))
}

#[tauri::command]
pub fn release_prepare(project_path: String, version: String) -> Result<JsonValue> {
    release_wizard::prepare_release(&project_path, &version)
}

#[tauri::command]
pub fn release_get_sequence(project_path: String, version: String) -> Result<JsonValue> {
    release_wizard::get_sequence(&project_path, &version)
}

#[tauri::command]
pub fn release_execute(project_path: String, command: String) -> Result<JsonValue> {
    release_wizard::execute_command(&project_path, &command)
}
