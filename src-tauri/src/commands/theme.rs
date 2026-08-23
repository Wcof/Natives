use crate::{db, emit_db_state_changed, Error, Result};
use tauri::State;

use crate::AppState;

const THEME_KEY: &str = "settings:theme";
/// V-002: the contract theme vocabulary is exactly `dark` | `light`.
const DEFAULT_THEME: &str = "dark";

/// Normalize any theme string to the two-value contract (`dark` | `light`).
/// Legacy aliases (`terminal-volt` → `dark`, `frosted-jasmine` → `light`) and
/// unknown values are mapped to the canonical vocabulary (V-002/V-004).
fn normalize_theme(theme: &str) -> &'static str {
    match theme {
        "light" | "frosted-jasmine" => "light",
        _ => "dark",
    }
}

#[tauri::command]
pub fn get_theme(state: State<'_, AppState>) -> Result<String> {
    let pool_conn = state
        .db
        .get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &pool_conn;
    let stored = db::get_setting(conn, THEME_KEY)?.unwrap_or_else(|| DEFAULT_THEME.to_string());
    // V-002: never return a legacy alias or unknown value across IPC.
    Ok(normalize_theme(&stored).to_string())
}

#[tauri::command]
pub fn set_theme(
    theme: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<()> {
    // V-002/V-004: only the canonical value is persisted and forwarded.
    let normalized = normalize_theme(&theme);
    let pool_conn = state
        .db
        .get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &pool_conn;
    db::set_setting(conn, THEME_KEY, normalized)?;
    emit_db_state_changed(
        &app_handle,
        "theme",
        serde_json::json!({ "theme": normalized }),
    );

    // ── 同步 Ghostty 主题配置（下次启动生效） ──
    let _ = crate::ghostty_config::write_config(normalized);

    Ok(())
}

/// 对外暴露的 Ghostty 主题同步命令（可由前端手动触发）
#[tauri::command]
pub fn builtin_tool_ghostty_sync_theme(state: State<'_, AppState>) -> Result<String> {
    let pool_conn = state
        .db
        .get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &pool_conn;
    let stored = db::get_setting(conn, THEME_KEY)?.unwrap_or_else(|| DEFAULT_THEME.to_string());
    let normalized = normalize_theme(&stored);
    let path = crate::ghostty_config::write_config(normalized)?;
    Ok(path.to_string_lossy().to_string())
}
