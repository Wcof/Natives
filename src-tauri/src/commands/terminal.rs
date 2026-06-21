use crate::{env_manager, Error, Result};
use crate::terminal_recorder;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::State;

use crate::AppState;

#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
pub struct TerminalData {
    pub session_id: String,
    pub data: String,
}

#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
pub struct TerminalExit {
    pub session_id: String,
    pub exit_code: i32,
}

/// CWD 查询结果，包含来源信息
#[derive(Debug, Serialize, Deserialize)]
pub struct CwdResult {
    pub cwd: String,
    pub source: String, // "osc7" | "lsof" | "home"
}

/// 前台进程查询结果
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcResult {
    pub process_name: String,
    pub pid: u32,
}

#[tauri::command]
pub fn terminal_create(
    app: tauri::AppHandle,
    profile_id: Option<String>,
    cols: Option<u16>,
    rows: Option<u16>,
    state: State<'_, AppState>,
) -> Result<String> {
    // Resolve environment overrides from profile
    let env_overrides = resolve_env_overrides(profile_id.clone(), &state)?;
    let (session_id, _pid) = state
        .terminal_manager
        .create_session(app, profile_id.as_deref(), env_overrides, cols, rows)?;
    Ok(session_id)
}

/// Resolve environment variables from the default (or specified) profile.
/// Returns `None` if no profile is configured (terminal will inherit parent env).
fn resolve_env_overrides(
    profile_id: Option<String>,
    state: &State<'_, AppState>,
) -> Result<Option<HashMap<String, String>>> {
    let pool_conn = state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;

    // Determine which profile to use
    let target_id: Option<i64> = if let Some(pid) = profile_id {
        pid.parse().ok()
    } else {
        // Use default profile
        env_manager::get_default_profile(conn)?
            .map(|p| p.id)
    };

    let pid = match target_id {
        Some(id) => id,
        None => return Ok(None),
    };

    // Get encryption key and inject env variables
    let encryption_key = env_manager::get_encryption_key(conn)?;
    let mut env = HashMap::new();
    env_manager::inject_env(conn, pid, &encryption_key, &mut env)?;
    Ok(Some(env))
}

#[tauri::command]
pub fn terminal_write(session_id: String, data: String, state: State<'_, AppState>) -> Result<()> {
    state.terminal_manager.write(&session_id, &data)
}

#[tauri::command]
pub fn terminal_resize(
    session_id: String,
    cols: u16,
    rows: u16,
    state: State<'_, AppState>,
) -> Result<()> {
    state.terminal_manager.resize(&session_id, cols, rows)
}

#[tauri::command]
pub fn terminal_kill(session_id: String, state: State<'_, AppState>) -> Result<()> {
    state.terminal_manager.kill(&session_id)
}

/// 获取 session CWD，返回 cwd + source 信息
#[tauri::command]
pub fn terminal_cwd(session_id: String, state: State<'_, AppState>) -> Result<CwdResult> {
    let (cwd, source) = state.terminal_manager.cwd(&session_id)?;
    Ok(CwdResult {
        cwd,
        source: source.to_string(),
    })
}

/// 获取前台进程信息
#[tauri::command]
pub fn terminal_proc(session_id: String, state: State<'_, AppState>) -> Result<ProcResult> {
    let (process_name, pid) = state.terminal_manager.proc(&session_id)?;
    Ok(ProcResult { process_name, pid })
}

/// 获取 session 状态快照
#[tauri::command]
pub fn terminal_session_state(session_id: String, state: State<'_, AppState>) -> Result<crate::terminal::SessionStateSnapshot> {
    state.terminal_manager.get_session_state(&session_id)
}

/// 获取所有 session 列表
#[tauri::command]
pub fn terminal_list_sessions(state: State<'_, AppState>) -> Result<Vec<crate::terminal::SessionStateSnapshot>> {
    state.terminal_manager.list_sessions()
}

// ── Ghostty render state (feature gate ghostty-vt) ──

/// RenderState payload returned by terminal_render_state
#[cfg(feature = "ghostty-vt")]
#[derive(Debug, Serialize, Deserialize)]
pub struct RenderState {
    pub session_id: String,
    pub cursor_x: u16,
    pub cursor_y: u16,
    pub title: Option<String>,
    pub pwd: Option<String>,
    pub cols: u16,
    pub rows: u16,
}

/// Query the current render state from Ghostty VT engine.
/// Returns the latest cursor position, title, and working directory.
#[cfg(feature = "ghostty-vt")]
#[tauri::command]
pub fn terminal_render_state(session_id: String, state: State<'_, AppState>) -> Result<RenderState> {
    let sessions = state
        .terminal_manager
        .get_session(&session_id)?;

    let session = sessions
        .get(&session_id)
        .ok_or_else(|| Error::NotFound(format!("session not found: {session_id}")))?;

    if let Some(ref ghostty) = session.ghostty {
        let guard = ghostty
            .lock()
            .map_err(|e| Error::Internal(format!("ghostty lock: {e}")))?;
        Ok(RenderState {
            session_id: session_id.clone(),
            cursor_x: guard.cursor_x(),
            cursor_y: guard.cursor_y(),
            title: guard.title(),
            pwd: guard.pwd(),
            cols: guard.cols(),
            rows: guard.rows(),
        })
    } else {
        Err(Error::Internal("ghostty-vt not available for this session".into()))
    }
}

// ── Terminal recording commands (asciinema v2 .cast) ──

#[tauri::command]
pub fn terminal_record_start(
    session_id: String,
    cols: u16,
    rows: u16,
    state: State<'_, AppState>,
) -> Result<()> {
    state.terminal_recorder.start(&session_id, cols, rows)
}

#[tauri::command]
pub fn terminal_record_stop(session_id: String, state: State<'_, AppState>) -> Result<()> {
    state.terminal_recorder.stop(&session_id)
}

#[tauri::command]
pub fn terminal_record_list(state: State<'_, AppState>) -> Result<Vec<terminal_recorder::RecordingMeta>> {
    state.terminal_recorder.list()
}

#[tauri::command]
pub fn terminal_record_play(id: String, state: State<'_, AppState>) -> Result<Vec<u8>> {
    state.terminal_recorder.read_cast(&id)
}

/// Export a recording to MP4 / GIF / WebM.
/// Falls back to .cast on conversion failure.
#[tauri::command]
pub fn terminal_record_export(
    id: String,
    format: String,
    state: State<'_, AppState>,
) -> Result<serde_json::Value> {
    state.terminal_recorder.export_recording(&id, &format)
}

/// Prune old recordings (keep 60 most recent + 800MB total size).
#[tauri::command]
pub fn terminal_record_prune(state: State<'_, AppState>) -> Result<()> {
    state.terminal_recorder.prune()
}

// ──────────────────────────────────────────────
// Builtin tool commands (extensible tool registry)
// ──────────────────────────────────────────────

/// Detect whether an external tool driver is installed.
#[tauri::command]
pub fn builtin_tool_detect(driver: String) -> Result<bool> {
    use crate::terminal::detect_binary;
    match driver.as_str() {
        "ghostty" => Ok(detect_binary(
            "ghostty",
            &["/Applications/Ghostty.app/Contents/MacOS/ghostty"],
        )),
        "vscode" => Ok(detect_binary("code", &[])),
        "chromium" => Ok(detect_binary(
            "chromium",
            &[
                "/Applications/Chromium.app/Contents/MacOS/Chromium",
                "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            ],
        )),
        // "native" is always available (uses built-in PTY)
        "native" => Ok(true),
        _ => Ok(false),
    }
}

/// Launch an external tool driver as a separate process.
#[tauri::command]
pub fn builtin_tool_launch(driver: String, state: State<'_, AppState>) -> Result<()> {
    use crate::terminal::launch_binary;
    match driver.as_str() {
        "ghostty" => state.ghostty_manager.launch(None),
        "vscode" => launch_binary("code", &[]),
        "chromium" => launch_binary(
            "chromium",
            &[
                "/Applications/Chromium.app/Contents/MacOS/Chromium",
                "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            ],
        ),
        _ => Err(Error::InvalidInput(format!("unknown driver: {driver}"))),
    }
}

/// List all builtin tools and their enabled/driver state from DB.
#[tauri::command]
pub fn builtin_tool_list(state: State<'_, AppState>) -> Result<Vec<serde_json::Value>> {
    let pool_conn = state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;
    crate::db::list_builtin_tools(conn)
}

/// Update a builtin tool's enabled/driver state.
#[tauri::command]
pub fn builtin_tool_update(
    id: String,
    enabled: bool,
    driver: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<()> {
    let pool_conn = state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;
    crate::db::update_builtin_tool(conn, &id, enabled, &driver)?;
    drop(pool_conn);
    crate::emit_db_state_changed(
        &app_handle,
        "builtin_tool",
        serde_json::json!({ "id": id, "enabled": enabled, "driver": driver }),
    );
    Ok(())
}

/// Seed a builtin tool row (called from frontend when registry has new tools).
#[tauri::command]
pub fn builtin_tool_seed(
    id: String,
    driver: String,
    state: State<'_, AppState>,
) -> Result<()> {
    let pool_conn = state.db.get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;
    crate::db::seed_builtin_tool(conn, &id, &driver)
}

// ── Ghostty-specific commands ──

/// 检测 Ghostty VT 引擎是否可用（编译期 feature gate）
#[tauri::command]
pub fn ghostty_vt_available() -> Result<bool> {
    #[cfg(feature = "ghostty-vt")]
    { Ok(true) }
    #[cfg(not(feature = "ghostty-vt"))]
    { Ok(false) }
}

#[tauri::command]
pub fn builtin_tool_ghostty_is_running(state: State<'_, AppState>) -> Result<bool> {
    Ok(state.ghostty_manager.is_running())
}

#[tauri::command]
pub fn builtin_tool_ghostty_focus(state: State<'_, AppState>) -> Result<()> {
    state.ghostty_manager.focus()
}

#[tauri::command]
pub fn builtin_tool_ghostty_launch(
    config_path: Option<String>,
    state: State<'_, AppState>,
) -> Result<()> {
    let path = config_path.as_ref().map(std::path::Path::new);
    state.ghostty_manager.launch(path)
}
