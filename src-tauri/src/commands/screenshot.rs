use crate::{screenshot, Result};

#[tauri::command]
pub fn screenshot_start_watching(app: tauri::AppHandle) -> Result<()> {
    screenshot::start_watching(app)
}

#[tauri::command]
pub fn screenshot_stop_watching() -> Result<()> {
    // TODO: implement stop mechanism
    Ok(())
}

#[tauri::command]
pub fn screenshot_save_annotated(data_url: String, target_path: Option<String>) -> Result<String> {
    screenshot::save_annotated(&data_url, target_path.as_deref())
}
