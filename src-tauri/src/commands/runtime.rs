//! commands/runtime.rs — Runtime 抽象层 IPC 命令
//!
//! 暴露 runtime 元信息给前端设置页 + 工作台降级横幅。
//!
//! `runtime_set_capability_enabled` 已退役（MIG-002）：能力启用/禁用由
//! Execution Engine Settings V2（`executionEngine.saveSettings` →
//! `execution_engine_save_settings`）唯一权威，不再提供旧写入口。

use crate::runtime::registry;
use crate::runtime::AgentRuntime;
use crate::Result;

use crate::runtime::claude_cli::ClaudeCliRuntime;
use crate::runtime::codex_cli::CodexCliRuntime;

/// 列出所有已注册 runtime 的元信息（id / display_name / available）
#[tauri::command]
pub async fn runtime_list_available() -> Result<Vec<registry::RuntimeMetadata>> {
    Ok(registry::list_runtime_metadata().await)
}

/// 主动触发 CLI 二进制检测（设置页「检测」按钮调用）
/// 返回 { claude_cli: bool, codex_cli: bool }
#[tauri::command]
pub async fn runtime_detect_cli() -> Result<serde_json::Value> {
    let claude_runtime = ClaudeCliRuntime::new();
    let codex_runtime = CodexCliRuntime::new();
    Ok(serde_json::json!({
        "claude_cli": claude_runtime.is_available(),
        "codex_cli": codex_runtime.is_available(),
    }))
}
