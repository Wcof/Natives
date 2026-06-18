use crate::{Error, Result};

#[tauri::command]
pub fn open_widget_window() -> Result<()> {
    // TODO: Implement in Loop 2 — open a secondary window for control hub widget
    Err(Error::NotImplemented("openWidgetWindow".into()))
}

#[tauri::command]
pub fn theme_ready_signal() -> Result<()> {
    // TODO: Implement in Loop 2 — signal that theme is applied, show window
    Ok(())
}
