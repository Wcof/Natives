use crate::{Error, Result};
use serde_json::Value as JsonValue;

#[tauri::command]
pub fn usage_refresh() -> Result<JsonValue> {
    // TODO: Implement in Loop 11
    Err(Error::NotImplemented("usage:refresh".into()))
}
