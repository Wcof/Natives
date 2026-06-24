//! commands/runtime.rs — Runtime 抽象层 IPC 命令
//!
//! 暴露 runtime 元信息给前端设置页 + 工作台降级横幅。

use crate::runtime::registry;
use crate::Result;

/// 列出所有已注册 runtime 的元信息（id / display_name / available）
#[tauri::command]
pub async fn runtime_list_available() -> Result<Vec<registry::RuntimeMetadata>> {
    Ok(registry::list_runtime_metadata().await)
}

/// 主动触发 CLI 二进制检测（设置页「检测」按钮调用）
/// 返回 { claude_cli: bool, codex_cli: bool }
#[tauri::command]
pub async fn runtime_detect_cli() -> Result<serde_json::Value> {
    let detected = crate::wechat::driver::detect_agents();
    Ok(serde_json::json!({
        "claude_cli": detected["claude"].as_bool().unwrap_or(false),
        "codex_cli": detected["codex"].as_bool().unwrap_or(false),
    }))
}
