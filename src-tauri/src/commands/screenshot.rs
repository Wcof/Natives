use crate::{screenshot, Result};
use std::sync::atomic::Ordering;
use tauri::State;

use crate::AppState;

#[tauri::command]
pub fn screenshot_start_watching(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<()> {
    // Reset stop flag so a previously stopped watcher can be restarted
    state.screenshot_stop_flag.store(false, Ordering::Relaxed);
    screenshot::start_watching(app, state.screenshot_stop_flag.clone())
}

#[tauri::command]
pub fn screenshot_stop_watching(state: State<'_, AppState>) -> Result<()> {
    state.screenshot_stop_flag.store(true, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
pub fn screenshot_save_annotated(data_url: String, target_path: Option<String>) -> Result<String> {
    screenshot::save_annotated(&data_url, target_path.as_deref())
}
