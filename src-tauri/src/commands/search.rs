use crate::{Error, Result};
use serde_json::Value as JsonValue;

#[tauri::command]
pub fn search_grep(query: String, root: String, options: Option<JsonValue>) -> Result<JsonValue> {
    // TODO: Implement in Loop 10
    Err(Error::NotImplemented(format!("search:grep({query}, {root})")))
}

#[tauri::command]
pub fn search_files(query: String, root: String, options: Option<JsonValue>) -> Result<JsonValue> {
    // TODO: Implement in Loop 10
    Err(Error::NotImplemented(format!("search:files({query}, {root})")))
}

#[tauri::command]
pub fn search_spotlight(query: String, root: String) -> Result<JsonValue> {
    // TODO: Implement in Loop 10
    Err(Error::NotImplemented(format!("search:spotlight({query}, {root})")))
}
