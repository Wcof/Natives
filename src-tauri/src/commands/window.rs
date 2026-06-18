use crate::{Error, Result};
use tauri::Manager;

#[tauri::command]
pub fn window_minimize(app: tauri::AppHandle) -> Result<()> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| Error::NotFound("main window".into()))?;
    window.minimize().map_err(|e| Error::Internal(e.to_string()))
}

#[tauri::command]
pub fn window_maximize(app: tauri::AppHandle) -> Result<()> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| Error::NotFound("main window".into()))?;
    if window.is_maximized().unwrap_or(false) {
        window.unmaximize().map_err(|e| Error::Internal(e.to_string()))
    } else {
        window.maximize().map_err(|e| Error::Internal(e.to_string()))
    }
}

#[tauri::command]
pub fn window_close(app: tauri::AppHandle) -> Result<()> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| Error::NotFound("main window".into()))?;
    window.close().map_err(|e| Error::Internal(e.to_string()))
}

#[tauri::command]
pub fn window_is_maximized(app: tauri::AppHandle) -> Result<bool> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| Error::NotFound("main window".into()))?;
    window.is_maximized().map_err(|e| Error::Internal(e.to_string()))
}
