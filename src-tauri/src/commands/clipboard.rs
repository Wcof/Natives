use crate::{Error, Result};

#[tauri::command]
pub fn clipboard_write(text: String) -> Result<()> {
    // TODO: Implement with arboard or tauri plugin in Loop 2
    Err(Error::NotImplemented("clipboard:write".into()))
}

#[tauri::command]
pub fn clipboard_read() -> Result<String> {
    // TODO: Implement with arboard or tauri plugin in Loop 2
    Err(Error::NotImplemented("clipboard:read".into()))
}
