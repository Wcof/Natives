use crate::{Error, Result};

#[tauri::command]
pub fn thumbnail_generate(file_path: String, width: u32) -> Result<String> {
    // TODO: Implement in Loop 9
    Err(Error::NotImplemented(format!("thumbnail:generate({file_path}, {width})")))
}
