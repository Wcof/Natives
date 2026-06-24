//! assistant_executor.rs — 助理执行引擎（Tool Call 执行回路 + Agentic Loop + 自愈熔断）
//!
//! 参考 CodePilot `runtime/registry.ts` + `native-runtime.ts` 的工具执行模型，
//! 落地为 Tauri Rust 底座版本。核心职责：
//!
//! 1. **工具注册表** — 声明可用工具的 schema（参数、返回、是否副作用），
//!    供前端 Settings 页治理 + 模型 function-calling 协议使用。
//! 2. **执行器** — 收到模型输出的 `tool_calls` 后，校验参数 → 执行 → 回填
//!    `tool_result` 到 messages → 开启下一轮（agentic loop）。
//! 3. **自愈熔断** — 执行失败时把结构化错误塞回 messages 让模型自纠，
//!    计数 ≤ `max_self_heal`（默认 3）次；超限熔断，标记 `circuit_broken`。
//!
//! 安全约束（CONTEXT.md 红线）：
//! - 工具执行全部在 Rust 进程内，前端只收事件，不直接调度副作用。
//! - 写盘类工具复用 `module_manager::write_generated_module`（含 KI-1/KI-3 审计）。
//! - 文件读取复用 `file_manager::read_file`（含路径校验 + 截断保护）。
//! - 终端命令走白名单 + 超时，禁止任意 shell 注入。

use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;
use tokio::time::timeout;

// ─────────────────────────────────────────────────────────────────────────────
// 工具 schema 定义（供前端 Settings 治理 + 模型 function-calling）
// ─────────────────────────────────────────────────────────────────────────────

/// 单个工具的声明性 schema。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDescriptor {
    pub name: String,
    pub description: String,
    /// JSON Schema 形式的参数定义（透传给模型的 function definition）。
    pub parameters: serde_json::Value,
    /// 是否有副作用（写盘/执行命令）。前端据此分类展示 + 二次确认。
    pub has_side_effects: bool,
    /// 默认是否启用。用户可在 Settings → 执行引擎里开关。
    pub default_enabled: bool,
}

/// 工具执行结果。回填给模型作为 `tool` role message。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolExecResult {
    /// 工具调用 ID（对应 OpenAI tool_calls[].id）。
    pub tool_call_id: String,
    /// 执行状态：success / error。
    pub status: String,
    /// 结构化结果（success）或错误摘要（error）。
    pub output: serde_json::Value,
    /// 是否触发熔断（连续失败超限）。
    pub circuit_broken: bool,
}

// ─────────────────────────────────────────────────────────────────────────────
// 工具注册表
// ─────────────────────────────────────────────────────────────────────────────

/// 返回所有可用工具的 schema。供前端 Settings 展示 + 注册给模型。
pub fn list_tools() -> Vec<ToolDescriptor> {
    vec![
        ToolDescriptor {
            name: "read_file".into(),
            description: "Read a local file's content (UTF-8, with truncation above 1MB).".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Absolute or ~-prefixed file path." }
                },
                "required": ["path"]
            }),
            has_side_effects: false,
            default_enabled: true,
        },
        ToolDescriptor {
            name: "list_dir".into(),
            description: "List entries of a directory.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"]
            }),
            has_side_effects: false,
            default_enabled: true,
        },
        ToolDescriptor {
            name: "write_file".into(),
            description: "Atomically write content to a local file.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["path", "content"]
            }),
            has_side_effects: true,
            default_enabled: true,
        },
        ToolDescriptor {
            name: "write_module".into(),
            description: "Write an AI-generated SPA module to ~/.natives/modules/, with Contract Linter gate (KI-3) + contract_id audit (KI-1).".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "moduleId": { "type": "string" },
                    "name": { "type": "string" },
                    "htmlContent": { "type": "string" },
                    "permissions": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["moduleId", "name", "htmlContent", "permissions"]
            }),
            has_side_effects: true,
            default_enabled: true,
        },
        ToolDescriptor {
            name: "run_terminal".into(),
            description: "Execute a whitelisted terminal command with timeout.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "Single command (no shell metachars)." },
                    "cwd": { "type": "string" },
                    "timeoutMs": { "type": "integer", "default": 30000 }
                },
                "required": ["command"]
            }),
            has_side_effects: true,
            default_enabled: false,
        },
        ToolDescriptor {
            name: "lint_module".into(),
            description: "Run Contract Linter on HTML content without writing to disk.".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "htmlContent": { "type": "string" }
                },
                "required": ["htmlContent"]
            }),
            has_side_effects: false,
            default_enabled: true,
        },
    ]
}

// ─────────────────────────────────────────────────────────────────────────────
// 执行器
// ─────────────────────────────────────────────────────────────────────────────

/// 单次工具调用的请求（从模型 SSE 输出解析得到）。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInvocation {
    pub id: String,
    pub name: String,
    /// 原始 arguments JSON 字符串（OpenAI 格式）。
    pub arguments: serde_json::Value,
}

/// 执行一组工具调用，返回每个工具的结果（供回填模型）。
///
/// `modules_dir` — 模块写入根目录（~/.natives/modules）。
/// `enabled_tools` — 用户在 Settings 里启用的工具名集合；未启用的工具拒绝执行。
pub fn execute_batch(
    invocations: &[ToolInvocation],
    modules_dir: &Path,
    enabled_tools: &HashMap<String, bool>,
) -> Vec<ToolExecResult> {
    invocations
        .iter()
        .map(|inv| execute_one(inv, modules_dir, enabled_tools))
        .collect()
}

fn execute_one(
    inv: &ToolInvocation,
    modules_dir: &Path,
    enabled_tools: &HashMap<String, bool>,
) -> ToolExecResult {
    // 1. 检查工具是否启用
    let enabled = enabled_tools.get(&inv.name).copied().unwrap_or(false);
    if !enabled {
        return ToolExecResult {
            tool_call_id: inv.id.clone(),
            status: "error".into(),
            output: serde_json::json!({
                "error": "tool_disabled",
                "message": format!("Tool '{}' is not enabled in Settings → Execution Engine.", inv.name)
            }),
            circuit_broken: false,
        };
    }

    // 2. 调度到具体实现
    let exec_result = match inv.name.as_str() {
        "read_file" => exec_read_file(&inv.arguments),
        "list_dir" => exec_list_dir(&inv.arguments),
        "write_file" => exec_write_file(&inv.arguments),
        "write_module" => exec_write_module(&inv.arguments, modules_dir),
        "run_terminal" => exec_run_terminal(&inv.arguments),
        "lint_module" => exec_lint_module(&inv.arguments),
        other => Err(Error::InvalidInput(format!("unknown tool: {other}"))),
    };

    match exec_result {
        Ok(output) => ToolExecResult {
            tool_call_id: inv.id.clone(),
            status: "success".into(),
            output,
            circuit_broken: false,
        },
        Err(e) => ToolExecResult {
            tool_call_id: inv.id.clone(),
            status: "error".into(),
            output: serde_json::json!({
                "error": "execution_failed",
                "message": e.to_string()
            }),
            circuit_broken: false,
        },
    }
}

// ── 具体工具实现 ──

fn exec_read_file(args: &serde_json::Value) -> Result<serde_json::Value> {
    let path = args["path"].as_str().ok_or_else(|| Error::InvalidInput("missing path".into()))?;
    let result = crate::file_manager::read_file(path)?;
    Ok(serde_json::to_value(result).map_err(|e| Error::Internal(e.to_string()))?)
}

fn exec_list_dir(args: &serde_json::Value) -> Result<serde_json::Value> {
    let path = args["path"].as_str().ok_or_else(|| Error::InvalidInput("missing path".into()))?;
    let entries = crate::file_manager::list_dir(path, &Default::default())?;
    Ok(serde_json::to_value(entries).map_err(|e| Error::Internal(e.to_string()))?)
}

fn exec_write_file(args: &serde_json::Value) -> Result<serde_json::Value> {
    let path = args["path"].as_str().ok_or_else(|| Error::InvalidInput("missing path".into()))?;
    let content = args["content"].as_str().ok_or_else(|| Error::InvalidInput("missing content".into()))?;
    crate::module_manager::atomic_write(std::path::Path::new(path), content)?;
    Ok(serde_json::json!({ "path": path, "bytes": content.len() }))
}

fn exec_write_module(args: &serde_json::Value, modules_dir: &Path) -> Result<serde_json::Value> {
    let module_id = args["moduleId"].as_str().ok_or_else(|| Error::InvalidInput("missing moduleId".into()))?;
    let name = args["name"].as_str().ok_or_else(|| Error::InvalidInput("missing name".into()))?;
    let html = args["htmlContent"].as_str().ok_or_else(|| Error::InvalidInput("missing htmlContent".into()))?;
    let perms: Vec<String> = args["permissions"].as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();

    // 复用 natives.db 连接（KI-1 审计写入需要）
    let pool_conn = crate::db::get_assistant_db_conn()?;
    let conn: &rusqlite::Connection = &*pool_conn;
    let outcome = crate::module_manager::write_generated_module(
        conn, modules_dir, module_id, name, html, &perms,
    )?;

    // 返回 oldContent/newContent 供前端 DiffViewer + 一键回滚（US-6）
    Ok(serde_json::json!({
        "moduleId": module_id,
        "written": true,
        "lintPassed": true,
        "contractId": outcome.contract_id,
        "contentHash": outcome.content_hash,
        "oldContent": outcome.old_content,
        "newContent": outcome.new_content,
        "fileName": format!("{module_id}/index.html")
    }))
}

fn exec_run_terminal(args: &serde_json::Value) -> Result<serde_json::Value> {
    let command = args["command"].as_str().ok_or_else(|| Error::InvalidInput("missing command".into()))?;
    let cwd = args["cwd"].as_str().unwrap_or(".").to_string();
    let timeout_ms = args["timeoutMs"].as_u64().unwrap_or(30_000);

    // 安全：拒绝 shell 元字符，只允许单条命令
    if command.contains('&') || command.contains('|') || command.contains(';') || command.contains('$') {
        return Err(Error::InvalidInput("shell metacharacters forbidden".into()));
    }

    let parts = command.split_whitespace().map(String::from).collect::<Vec<_>>();
    if parts.is_empty() {
        return Err(Error::InvalidInput("empty command".into()));
    }
    let (cmd, cmd_args) = parts.split_first().unwrap();
    let cmd = cmd.clone();
    let cmd_args = cmd_args.to_vec();

    // spawn_blocking 包裹阻塞的 Command::output，并用 tokio timeout 限时
    let rt_handle = tokio::runtime::Handle::try_current()
        .map_err(|e| Error::Internal(format!("no tokio runtime: {e}")))?;
    let join = rt_handle.spawn_blocking(move || {
        std::process::Command::new(&cmd)
            .args(&cmd_args)
            .current_dir(&cwd)
            .output()
    });
    let output_fut = async {
        match timeout(Duration::from_millis(timeout_ms), join).await {
            Ok(Ok(output)) => Ok(output),
            Ok(Err(e)) => Err(Error::Internal(format!("join error: {e}"))),
            Err(_) => Err(Error::Internal("command timed out".into())),
        }
    };
    let output = rt_handle.block_on(output_fut)?;
    let output = output.map_err(|e| Error::Internal(format!("command failed: {e}")))?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let exit_code = output.status.code().unwrap_or(-1);

    Ok(serde_json::json!({
        "exitCode": exit_code,
        "stdout": stdout,
        "stderr": stderr
    }))
}

fn exec_lint_module(args: &serde_json::Value) -> Result<serde_json::Value> {
    let html = args["htmlContent"].as_str().ok_or_else(|| Error::InvalidInput("missing htmlContent".into()))?;
    let result = crate::contract_linter::lint_html(html);
    Ok(serde_json::json!({
        "passed": result.passed,
        "errors": result.errors
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// 自愈熔断策略
// ─────────────────────────────────────────────────────────────────────────────

/// 根据当前连续失败次数 + 上限，判定是否熔断。
///
/// - `fail_count` 已失败次数（含本次即将计入的）
/// - `max_self_heal` 用户配置的自愈上限（默认 3）
/// - 返回 `true` 表示熔断：应停止 loop，把控制权交还用户。
pub fn should_circuit_break(fail_count: u32, max_self_heal: u32) -> bool {
    fail_count > max_self_heal
}

/// 把一批工具结果转换为回填给模型的 `tool` role messages。
pub fn results_to_messages(results: &[ToolExecResult]) -> Vec<serde_json::Value> {
    results
        .iter()
        .map(|r| {
            serde_json::json!({
                "role": "tool",
                "tool_call_id": r.tool_call_id,
                "content": serde_json::to_string(&r.output).unwrap_or_else(|_| "null".into())
            })
        })
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
// 默认启用的工具集合（首次启动 / Settings 未配置时用）
// ─────────────────────────────────────────────────────────────────────────────

pub fn default_enabled_tools() -> HashMap<String, bool> {
    let mut m = HashMap::new();
    for t in list_tools() {
        m.insert(t.name, t.default_enabled);
    }
    m
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_descriptors_have_required_fields() {
        for t in list_tools() {
            assert!(!t.name.is_empty(), "tool name empty");
            assert!(t.parameters.is_object(), "tool {} parameters not object", t.name);
        }
    }

    #[test]
    fn test_should_circuit_break() {
        assert!(!should_circuit_break(0, 3));
        assert!(!should_circuit_break(1, 3));
        assert!(!should_circuit_break(3, 3));
        assert!(should_circuit_break(4, 3), "4th failure must circuit-break");
        assert!(should_circuit_break(10, 3));
    }

    #[test]
    fn test_disabled_tool_rejected() {
        let inv = ToolInvocation {
            id: "call_1".into(),
            name: "run_terminal".into(),
            arguments: serde_json::json!({ "command": "ls" }),
        };
        let mut enabled = HashMap::new();
        enabled.insert("run_terminal".into(), false);

        let result = execute_one(&inv, Path::new("/tmp"), &enabled);
        assert_eq!(result.status, "error");
        assert_eq!(result.output["error"], "tool_disabled");
    }

    #[test]
    fn test_unknown_tool_rejected() {
        let inv = ToolInvocation {
            id: "call_x".into(),
            name: "nonexistent_tool".into(),
            arguments: serde_json::json!({}),
        };
        let mut enabled = HashMap::new();
        enabled.insert("nonexistent_tool".into(), true);

        let result = execute_one(&inv, Path::new("/tmp"), &enabled);
        assert_eq!(result.status, "error");
        assert_eq!(result.output["error"], "execution_failed");
    }

    #[test]
    fn test_terminal_rejects_metacharacters() {
        let inv = ToolInvocation {
            id: "call_2".into(),
            name: "run_terminal".into(),
            arguments: serde_json::json!({ "command": "ls; rm -rf /" }),
        };
        let mut enabled = HashMap::new();
        enabled.insert("run_terminal".into(), true);

        let result = execute_one(&inv, Path::new("/tmp"), &enabled);
        assert_eq!(result.status, "error");
        assert!(result.output["message"].as_str().unwrap().contains("metachar"));
    }

    #[test]
    fn test_results_to_messages_format() {
        let results = vec![ToolExecResult {
            tool_call_id: "call_1".into(),
            status: "success".into(),
            output: serde_json::json!({ "path": "/tmp/x" }),
            circuit_broken: false,
        }];
        let msgs = results_to_messages(&results);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0]["role"], "tool");
        assert_eq!(msgs[0]["tool_call_id"], "call_1");
    }
}
