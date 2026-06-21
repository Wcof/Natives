use crate::{db, emit_db_state_changed, Error, Result};
use tauri::State;

use crate::AppState;

const THEME_KEY: &str = "settings:theme";
const DEFAULT_THEME: &str = "terminal-volt";

#[tauri::command]
pub fn get_theme(state: State<'_, AppState>) -> Result<String> {
    let pool_conn = state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;
    Ok(db::get_setting(conn, THEME_KEY)?
        .unwrap_or_else(|| DEFAULT_THEME.to_string()))
}

#[tauri::command]
pub fn set_theme(theme: String, app_handle: tauri::AppHandle, state: State<'_, AppState>) -> Result<()> {
    let pool_conn = state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;
    db::set_setting(conn, THEME_KEY, &theme)?;
    emit_db_state_changed(&app_handle, "theme", serde_json::json!({ "theme": theme }));

    // ── 同步 Ghostty 主题配置（下次启动生效） ──
    let _ = crate::ghostty_config::write_config(&theme);

    Ok(())
}

/// 对外暴露的 Ghostty 主题同步命令（可由前端手动触发）
#[tauri::command]
pub fn builtin_tool_ghostty_sync_theme(state: State<'_, AppState>) -> Result<String> {
    let pool_conn = state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;
    let theme = db::get_setting(conn, THEME_KEY)?
        .unwrap_or_else(|| DEFAULT_THEME.to_string());
    let path = crate::ghostty_config::write_config(&theme)?;
    Ok(path.to_string_lossy().to_string())
}
