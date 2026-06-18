use crate::{git, Error, Result};
use serde_json::Value as JsonValue;

#[tauri::command]
pub fn git_status(dir_path: String) -> Result<JsonValue> {
    let status = git::git_status(&dir_path)?;
    serde_json::to_value(status).map_err(|e| Error::Internal(e.to_string()))
}

#[tauri::command]
pub fn git_diff(file_path: String) -> Result<String> {
    git::git_diff(&file_path)
}
