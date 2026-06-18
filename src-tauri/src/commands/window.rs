use crate::{Error, Result};
use tauri::{Manager, PhysicalPosition, PhysicalSize};

fn main_window(app: &tauri::AppHandle) -> Result<tauri::WebviewWindow> {
    app.get_webview_window("main")
        .ok_or_else(|| Error::NotFound("main window".into()))
}

#[tauri::command]
pub fn window_minimize(app: tauri::AppHandle) -> Result<()> {
    main_window(&app)?
        .minimize()
        .map_err(|e| Error::Internal(e.to_string()))
}

#[tauri::command]
pub fn window_maximize(app: tauri::AppHandle) -> Result<()> {
    let win = main_window(&app)?;
    if win.is_maximized().unwrap_or(false) {
        win.unmaximize().map_err(|e| Error::Internal(e.to_string()))
    } else {
        win.maximize().map_err(|e| Error::Internal(e.to_string()))
    }
}

#[tauri::command]
pub fn window_close(app: tauri::AppHandle) -> Result<()> {
    main_window(&app)?
        .close()
        .map_err(|e| Error::Internal(e.to_string()))
}

#[tauri::command]
pub fn window_is_maximized(app: tauri::AppHandle) -> Result<bool> {
    main_window(&app)?
        .is_maximized()
        .map_err(|e| Error::Internal(e.to_string()))
}

/// Tile the window to a specific position/size (macOS-style window tiling).
/// Actions: left, right, top, bottom, fullscreen, left-half, right-half, tile
#[tauri::command]
pub fn window_tile(app: tauri::AppHandle, action: String) -> Result<()> {
    let win = main_window(&app)?;

    // Get monitor dimensions
    let monitor = win
        .current_monitor()
        .map_err(|e| Error::Internal(e.to_string()))?
        .ok_or_else(|| Error::NotFound("monitor".into()))?;

    let size = monitor.size();
    let pos = monitor.position();
    let w = size.width;
    let h = size.height;

    let (new_x, new_y, new_w, new_h) = match action.as_str() {
        "left" | "left-half" => (pos.x, pos.y, w / 2, h),
        "right" | "right-half" => (pos.x + (w as i32 / 2), pos.y, w / 2, h),
        "top" => (pos.x, pos.y, w, h / 2),
        "bottom" => (pos.x, pos.y + (h as i32 / 2), w, h / 2),
        "fullscreen" => {
            win.set_fullscreen(true)
                .map_err(|e| Error::Internal(e.to_string()))?;
            return Ok(());
        }
        "tile" => {
            // "tile" = restore from fullscreen
            win.set_fullscreen(false)
                .map_err(|e| Error::Internal(e.to_string()))?;
            return Ok(());
        }
        _ => return Err(Error::InvalidInput(format!("unknown tile action: {action}"))),
    };

    // Exit fullscreen first if needed
    if win.is_fullscreen().unwrap_or(false) {
        win.set_fullscreen(false)
            .map_err(|e| Error::Internal(e.to_string()))?;
    }

    win.set_position(PhysicalPosition::new(new_x, new_y))
        .map_err(|e| Error::Internal(e.to_string()))?;
    win.set_size(PhysicalSize::new(new_w, new_h))
        .map_err(|e| Error::Internal(e.to_string()))?;

    Ok(())
}
