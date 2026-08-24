use crate::{commands::menubar::MENUBAR_LABEL, Error, Result};
use tauri::Manager;

/// Signal from frontend that theme CSS variables are applied.
/// This is the FOUC guard — the window stays hidden until this is called.
///
/// MB-P0-02: the signal is resolved against the *calling* window's label:
/// - `main`: re-assert macOS chrome, then show+focus the main window.
/// - `menubar`: mark the popup theme-ready, then show+focus the popup (the
///   popup is created `visible(false)` and must not appear before this).
#[tauri::command]
pub fn theme_ready_signal(window: tauri::Window) -> Result<()> {
    match window.label() {
        "main" => {
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
            // PERF-01: window visible = 用户感知的冷启动结束；输出阶段耗时表。
            crate::startup_timing::record("window_ready", 0);
            crate::startup_timing::report();
        }
        MENUBAR_LABEL => {
            crate::commands::menubar::mark_theme_ready();
            window.show().map_err(|e| Error::Internal(e.to_string()))?;
            window
                .set_focus()
                .map_err(|e| Error::Internal(e.to_string()))?;
        }
        other => {
            return Err(Error::InvalidInput(format!(
                "theme_ready_signal called from unsupported window '{other}'"
            )));
        }
    }
    Ok(())
}

/// Open a secondary widget window for the control hub.
///
/// DEPRECATED — the legacy ControlHubWidget surface is being retired in favor
/// of the menubar popup (MB-P0-05). Kept registered so existing `mode=widget`
/// renderer references keep resolving; new code must use the `menubar`
/// commands instead. The old frontend surface will be removed separately.
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
