use crate::{Error, Result};
use tauri::Manager;

/// Signal from frontend that theme CSS variables are applied.
/// This is the FOUC guard — the window stays hidden until this is called.
#[tauri::command]
pub fn theme_ready_signal(app: tauri::AppHandle) -> Result<()> {
    if let Some(window) = app.get_webview_window("main") {
        // Re-assert macOS chrome right before first paint. window-state (or a
        // prior frameless config) can leave decorations=false; without this the
        // system traffic lights never appear even though the UI reserves space.
        #[cfg(target_os = "macos")]
        {
            let _ = window.set_decorations(true);
            let _ = window.set_title_bar_style(tauri::TitleBarStyle::Overlay);
        }
        window.show().map_err(|e| Error::Internal(e.to_string()))?;
        window
            .set_focus()
            .map_err(|e| Error::Internal(e.to_string()))?;
    }
    Ok(())
}

/// Open a secondary widget window for the control hub.
#[tauri::command]
pub fn open_widget_window(app: tauri::AppHandle) -> Result<()> {
    // Check if widget window already exists
    if let Some(widget) = app.get_webview_window("widget") {
        widget.show().map_err(|e| Error::Internal(e.to_string()))?;
        widget
            .set_focus()
            .map_err(|e| Error::Internal(e.to_string()))?;
        return Ok(());
    }

    // Create new widget window
    let _widget = tauri::WebviewWindowBuilder::new(
        &app,
        "widget",
        tauri::WebviewUrl::App("index.html?mode=widget".into()),
    )
    .title("Natives Widget")
    .inner_size(400.0, 600.0)
    .decorations(false)
    .resizable(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .build()
    .map_err(|e| Error::Internal(e.to_string()))?;

    Ok(())
}
