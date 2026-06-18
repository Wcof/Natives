use crate::{archive, Error, Result};
use serde_json::Value as JsonValue;

#[tauri::command]
pub fn archive_list(archive_path: String) -> Result<JsonValue> {
    let listing = archive::list_archive(&archive_path)?;
    serde_json::to_value(listing).map_err(|e| Error::Internal(e.to_string()))
}
