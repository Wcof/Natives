//! commands/runtime.rs — Runtime 抽象层 IPC 命令
//!
//! 暴露 runtime 元信息给前端设置页 + 工作台降级横幅。

use crate::runtime::registry;
use crate::Result;
use crate::{db, emit_db_state_changed, Error};
use tauri::State;

use crate::commands::executor_settings::{ExecutorSettings, EXECUTOR_KEY};
use crate::runtime::AgentRuntime;
use crate::AppState;

/// 列出所有已注册 runtime 的元信息（id / display_name / available）
#[tauri::command]
pub async fn runtime_list_available() -> Result<Vec<registry::RuntimeMetadata>> {
    Ok(registry::list_runtime_metadata().await)
}

/// 主动触发 CLI 二进制检测（设置页「检测」按钮调用）
/// 返回 { claude_cli: bool, codex_cli: bool }
#[tauri::command]
pub async fn runtime_detect_cli() -> Result<serde_json::Value> {
    let claude_runtime = crate::runtime::claude_cli::ClaudeCliRuntime::new();
    let codex_runtime = crate::runtime::codex_cli::CodexCliRuntime::new();
    Ok(serde_json::json!({
        "claude_cli": claude_runtime.is_available(),
        "codex_cli": codex_runtime.is_available(),
    }))
}

#[tauri::command]
pub fn runtime_set_capability_enabled(
    name: String,
    enabled: bool,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<()> {
    let pool_conn = state
        .db
        .get()
        .map_err(|e| Error::Internal(format!("failed to get DB connection: {e}")))?;
    let conn: &rusqlite::Connection = &*pool_conn;

    let mut settings: ExecutorSettings = db::get_setting(conn, EXECUTOR_KEY)?
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| ExecutorSettings {
            enabled_tools: crate::executor_catalog::default_enabled_tools(),
            max_self_heal: 3,
            max_steps: None,
        });

    settings.enabled_tools.insert(name.clone(), enabled);
    let json = serde_json::to_string(&settings).map_err(|e| Error::Internal(e.to_string()))?;
    db::set_setting(conn, EXECUTOR_KEY, &json)?;
    emit_db_state_changed(
        &app_handle,
        "runtime",
        serde_json::json!({ "capability": name, "enabled": enabled }),
    );
    Ok(())
}
