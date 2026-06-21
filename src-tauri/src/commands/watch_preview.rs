//! Commands for fs_watch, html_preview, and lid_guard features.

use crate::{html_preview, Error, Result};
use serde_json::Value as JsonValue;
use tauri::State;

use crate::AppState;

// ── FsWatch commands ──

/// Start watching a directory recursively for file changes.
#[tauri::command]
pub fn fs_watch_start(path: String, state: State<'_, AppState>) -> Result<()> {
    state.fs_watcher.start(&path)
}

/// Stop watching a directory.
#[tauri::command]
pub fn fs_watch_stop(path: String, state: State<'_, AppState>) -> Result<()> {
    state.fs_watcher.stop(&path)
}

/// Stop all file watchers.
#[tauri::command]
pub fn fs_watch_stop_all(state: State<'_, AppState>) -> Result<()> {
    state.fs_watcher.stop_all()
}

/// Get list of currently watched paths.
#[tauri::command]
pub fn fs_watch_list(state: State<'_, AppState>) -> Result<Vec<String>> {
    state.fs_watcher.watched_paths()
}

// ── HtmlPreview commands ──

/// Prepare an HTML file for sandboxed preview.
/// Rewrites local paths to use /fs/ proxy, returns safe HTML.
#[tauri::command]
pub fn html_preview_prepare(html_path: String, state: State<'_, AppState>) -> Result<JsonValue> {
    let port = state
        .http_port
        .lock()
        .map_err(|e| Error::Internal(e.to_string()))?;
    let result = html_preview::prepare_html_preview(&html_path, *port)?;
    serde_json::to_value(result).map_err(|e| Error::Internal(e.to_string()))
}

// ── LidGuard commands ──

/// Set lid intent: disable sleep while terminals are active.
#[tauri::command]
pub fn lid_guard_set(on: bool, state: State<'_, AppState>) -> Result<()> {
    state.lid_guard.set_lid_intent(on)
}

/// Get current lid guard status.
#[tauri::command]
pub fn lid_guard_status(state: State<'_, AppState>) -> Result<JsonValue> {
    Ok(serde_json::json!({
        "sleepDisabled": state.lid_guard.is_sleep_disabled(),
        "terminalCount": state.lid_guard.terminal_count(),
    }))
}
