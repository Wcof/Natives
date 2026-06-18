use crate::{Error, Result};

#[tauri::command]
pub fn screenshot_start_watching() -> Result<()> {
    // TODO: Implement in Loop 12
    Err(Error::NotImplemented("screenshot:startWatching".into()))
}

#[tauri::command]
pub fn screenshot_stop_watching() -> Result<()> {
    // TODO: Implement in Loop 12
    Err(Error::NotImplemented("screenshot:stopWatching".into()))
}

#[tauri::command]
pub fn screenshot_save_annotated(data_url: String, target_path: Option<String>) -> Result<String> {
    // TODO: Implement in Loop 12
    Err(Error::NotImplemented("screenshot:saveAnnotated".into()))
}
