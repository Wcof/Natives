//! runtime/native/hook_pipeline.rs — 完整 Hook 管道（代码级复刻 Claude Code 5 点 Hook 模型）
//!
//! 精确复刻 Claude Code 的 Hook 协议：
//!
//! ## Hook 点（5 个）
//!   SessionStart — 会话启动时触发（一次性）
//!   UserPromptSubmit — 用户提交 prompt 时触发
//!   PreToolUse — 工具执行前（可拦截/修改参数）
//!   PostToolUse — 工具执行后（可拦截/修改结果）
//!   Stop — Agent 停止前
//!
//! ## hooks.json 格式（精确复刻）
//!   ```json
//!   { "hooks": {
//!     "PreToolUse": [{
//!       "hooks": [{
//!         "type": "command",
//!         "command": "python3 ${CLAUDE_PLUGIN_ROOT}/hooks/script.py",
//!         "timeout": 10
//!       }],
//!       "matcher": "Edit|Write|MultiEdit",
//!     }]
//!   }}
//!   ```
//!
//! ## 输入/输出协议
//!   输入 (stdin JSON): { tool_name, tool_input, hook_event_name, session_id, cwd }
//!   输出 (stdout JSON): { systemMessage, hookSpecificOutput, metrics, decision }

//!
//! Residual catalog/compat after Protocol v2 cutover (execution retired).
#![allow(dead_code)]
use std::path::Path;
use std::time::{Duration, Instant};
use std::fmt;
use super::capability::{CapabilityRequest, CapabilityResult, CapabilityStatus};

// ══════════════════════════════════════════════════════════════════════════════
// Hook 点枚举 — 精确匹配 Claude Code 的 5 个 Hook 点
// ══════════════════════════════════════════════════════════════════════════════

/// Hook 点类型（精确匹配 hooks.json 中的 key 名）
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum HookPoint {
    /// 会话启动
    SessionStart,
    /// 用户提交 prompt
    UserPromptSubmit,
    /// 工具执行前
    PreToolUse,
    /// 工具执行后
    PostToolUse,
    /// Agent 停止
    Stop,
    /// 会话结束
    SessionEnd,
    /// 子代理停止
    SubagentStop,
    /// 上下文压缩前
    PreCompact,
    /// 通知事件
    Notification,
}

impl HookPoint {
    /// 返回 hooks.json 中使用的字符串名称
    pub fn as_str(&self) -> &'static str {
        match self {
            HookPoint::SessionStart => "SessionStart",
            HookPoint::UserPromptSubmit => "UserPromptSubmit",
            HookPoint::PreToolUse => "PreToolUse",
            HookPoint::PostToolUse => "PostToolUse",
            HookPoint::Stop => "Stop",
            HookPoint::SessionEnd => "SessionEnd",
            HookPoint::SubagentStop => "SubagentStop",
            HookPoint::PreCompact => "PreCompact",
            HookPoint::Notification => "Notification",
        }
    }

    /// 从 hooks.json key 解析
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "SessionStart" => Some(HookPoint::SessionStart),
            "UserPromptSubmit" => Some(HookPoint::UserPromptSubmit),
            "PreToolUse" => Some(HookPoint::PreToolUse),
            "PostToolUse" => Some(HookPoint::PostToolUse),
            "Stop" => Some(HookPoint::Stop),
            "SessionEnd" => Some(HookPoint::SessionEnd),
            "SubagentStop" => Some(HookPoint::SubagentStop),
            "PreCompact" => Some(HookPoint::PreCompact),
            "Notification" => Some(HookPoint::Notification),
            _ => None,
        }
    }
}

impl fmt::Display for HookPoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Hook 条目 — 匹配 hooks.json 中单个 hook entry
// ══════════════════════════════════════════════════════════════════════════════

/// 单个 Hook Entry（精确匹配 hooks.json 的 hook entry 字段）
#[derive(Clone, Debug)]
pub struct HookEntry {
    /// 类型：目前支持 "command"
    pub r#type: String,
    /// 执行的命令（含 ${CLAUDE_PLUGIN_ROOT} 等变量）
    pub command: String,
    /// 超时秒数（默认 10）
    pub timeout_secs: u64,
    /// 条件表达式，如 "Bash(git commit:*)"
    pub if_condition: Option<String>,
    /// 是否异步唤醒（后台执行）
    pub async_rewake: bool,
    /// 唤醒消息
    pub rewake_message: Option<String>,
    /// 唤醒摘要
    pub rewake_summary: Option<String>,
}

impl HookEntry {
    /// 解析 hooks.json 中的 hook entry
    pub fn from_json(value: &serde_json::Value) -> Option<Self> {
        let obj = value.as_object()?;
        let r#type = obj.get("type")?.as_str()?.to_string();
        let command = obj.get("command")?.as_str()?.to_string();
        let timeout_secs = obj.get("timeout").and_then(|v| v.as_u64()).unwrap_or(10);
        let if_condition = obj.get("if").and_then(|v| v.as_str()).map(String::from);
        let async_rewake = obj.get("asyncRewake").and_then(|v| v.as_bool()).unwrap_or(false);
        let rewake_message = obj.get("rewakeMessage").and_then(|v| v.as_str()).map(String::from);
        let rewake_summary = obj.get("rewakeSummary").and_then(|v| v.as_str()).map(String::from);

        Some(HookEntry {
            r#type,
            command,
            timeout_secs,
            if_condition,
            async_rewake,
            rewake_message,
            rewake_summary,
        })
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Hook 组 — 匹配 hooks.json 中每个 hook point 的数组元素
// ══════════════════════════════════════════════════════════════════════════════

/// Hook 组（包含多个 hook entry，共享 matcher）
#[derive(Clone, Debug)]
pub struct HookGroup {
    /// hook 条目列表
    pub hooks: Vec<HookEntry>,
    /// 工具匹配模式（如 "Edit|Write|MultiEdit"）
    pub matcher: Option<String>,
}

impl HookGroup {
    pub fn from_json(value: &serde_json::Value) -> Option<Self> {
        let obj = value.as_object().or_else(|| {
            // 兼容直接数组格式
            None
        })?;

        let matcher = obj.get("matcher").and_then(|v| v.as_str()).map(String::from);
        let hooks: Vec<HookEntry> = obj.get("hooks")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(HookEntry::from_json).collect())
            .unwrap_or_default();

        if hooks.is_empty() { return None; }

        Some(HookGroup { hooks, matcher })
    }

    /// 检查是否匹配给定的工具名
    pub fn matches_tool(&self, tool_name: &str) -> bool {
        match &self.matcher {
            Some(pattern) => tool_matches_pattern(tool_name, pattern),
            None => true,
        }
    }

    /// 检查 if_condition 是否匹配（如 "Bash(git commit:*)"）
    pub fn matches_if(&self, tool_name: &str, tool_input: &serde_json::Value) -> Vec<bool> {
        self.hooks.iter().map(|entry| {
            match &entry.if_condition {
                Some(condition) => evaluate_if_condition(condition, tool_name, tool_input),
                None => true,
            }
        }).collect()
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// 条件匹配工具函数
// ══════════════════════════════════════════════════════════════════════════════

/// 工具名匹配模式（支持 | 分隔的多种工具名）
fn tool_matches_pattern(tool_name: &str, pattern: &str) -> bool {
    for pat in pattern.split('|') {
        let pat = pat.trim();
        if pat == "*" || pat == tool_name {
            return true;
        }
        // 支持 glob 通配符
        if let Some(suffix) = pat.strip_suffix('*') {
            if tool_name.starts_with(suffix) {
                return true;
            }
        }
        if let Some(prefix) = pat.strip_prefix('*') {
            if tool_name.ends_with(prefix) {
                return true;
            }
        }
    }
    false
}

/// 评估 if_condition，如 "Bash(git commit:*)" 匹配 Bash 工具且命令以 git commit 开头
fn evaluate_if_condition(condition: &str, tool_name: &str, tool_input: &serde_json::Value) -> bool {
    // 格式: "ToolName(pattern)"
    if let Some(paren_start) = condition.find('(') {
        let cond_tool = &condition[..paren_start];
        if cond_tool != tool_name {
            return false;
        }
        let inner = &condition[paren_start + 1..];
        let pattern = inner.trim_end_matches(')');

        // 从 tool_input 中提取命令文本
        let command = tool_input.get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let file_path = tool_input.get("file_path")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let content = tool_input.get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        // 对命令进行 glob 匹配
        let text = command.to_string() + &file_path + content;
        if let Some(suffix) = pattern.strip_suffix('*') {
            return text.starts_with(suffix);
        }
        if let Some(prefix) = pattern.strip_prefix('*') {
            return text.ends_with(prefix);
        }
        return pattern == &text || pattern == command;
    }

    // 纯工具名匹配
    condition == tool_name
}

// ══════════════════════════════════════════════════════════════════════════════
// 环境变量替换
// ══════════════════════════════════════════════════════════════════════════════

/// 替换命令中的环境变量 (${VAR_NAME})
fn resolve_env_vars(cmd: &str, plugin_root: Option<&Path>, project_dir: Option<&Path>) -> String {
    let mut result = cmd.to_string();

    // ${CLAUDE_PLUGIN_ROOT}
    if let Some(root) = plugin_root {
        result = result.replace("${CLAUDE_PLUGIN_ROOT}", root.to_str().unwrap_or(""));
        result = result.replace("${CLAUDE_PLUGIN_DIR}", root.to_str().unwrap_or(""));
    }

    // ${CLAUDE_PROJECT_DIR}
    if let Some(dir) = project_dir {
        result = result.replace("${CLAUDE_PROJECT_DIR}", dir.to_str().unwrap_or(""));
    }

    // 通用环境变量
    for (key, val) in std::env::vars() {
        let var = format!("${{{}}}", key);
        result = result.replace(&var, &val);
    }
    // 也支持 $VAR 格式（不带花括号）
    for (key, val) in std::env::vars() {
        let var = format!("${}", key);
        result = result.replace(&var, &val);
    }

    result
}

// ══════════════════════════════════════════════════════════════════════════════
// 外部脚本 Hook 执行器
// ══════════════════════════════════════════════════════════════════════════════

/// 外部脚本 Hook — 执行外部进程通过 JSON stdin/stdout 通信
///
/// 精确匹配 Claude Code 的 hook 协议：
///   输入 (stdin JSON):
///     { tool_name, tool_input, hook_event_name, session_id, cwd }
///   输出 (stdout JSON):
///     { systemMessage, hookSpecificOutput, metrics, decision }
///   退出码 0: 成功
///   非 0: 放行并记录警告
pub struct ScriptHook {
    entry: HookEntry,
    plugin_root: Option<std::path::PathBuf>,
    project_dir: Option<std::path::PathBuf>,
}

impl ScriptHook {
    pub fn new(
        entry: HookEntry,
        plugin_root: Option<std::path::PathBuf>,
        project_dir: Option<std::path::PathBuf>,
    ) -> Self {
        Self { entry, plugin_root, project_dir }
    }

    /// 获取 hook 名称（用于日志）
    pub fn name(&self) -> String {
        format!("script:{}", self.entry.command.chars().take(60).collect::<String>())
    }

    /// 是否匹配工具（基于 matcher）
    pub fn matches_tool(&self, _tool_name: &str) -> bool {
        // 单个 entry 没有 matcher 字段，matcher 在 HookGroup 层面检查
        true
    }

    /// 是否匹配 if_condition
    pub fn matches_if(&self, tool_name: &str, tool_input: &serde_json::Value) -> bool {
        match &self.entry.if_condition {
            Some(condition) => evaluate_if_condition(condition, tool_name, tool_input),
            None => true,
        }
    }

    /// 获取是否异步唤醒
    pub fn is_async_rewake(&self) -> bool {
        self.entry.async_rewake
    }

    /// 执行外部脚本，返回解析后的 JSON 响应
    pub fn execute(
        &self,
        tool_name: &str,
        tool_input: &serde_json::Value,
        hook_event: &HookPoint,
        session_id: &str,
        cwd: Option<&str>,
    ) -> HookResult {
        let start = Instant::now();

        // 构建输入 JSON（精确匹配 Claude Code 的 hook 输入协议）
        let input = serde_json::json!({
            "tool_name": tool_name,
            "tool_input": tool_input,
            "hook_event_name": hook_event.as_str(),
            "session_id": session_id,
            "cwd": cwd,
        });

        // 解析命令（替换环境变量）
        let resolved_cmd = resolve_env_vars(
            &self.entry.command,
            self.plugin_root.as_deref(),
            self.project_dir.as_deref(),
        );

        // 分割命令
        let parts: Vec<String> = parse_command_argv(&resolved_cmd);
        if parts.is_empty() {
            return HookResult::error("Empty command after resolving env vars".to_string());
        }

        let program = &parts[0];
        let args: Vec<&str> = parts[1..].iter().map(|s| s.as_str()).collect();

        let mut cmd = std::process::Command::new(program);
        cmd.args(&args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        // 注入环境变量
        cmd.env("CLAUDE_HOOK_POINT", hook_event.as_str());
        cmd.env("CLAUDE_SESSION_ID", session_id);

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                return HookResult::error(format!("Failed to spawn hook script: {}", e));
            }
        };

        // 写 stdin
        let input_bytes = match serde_json::to_vec(&input) {
            Ok(b) => b,
            Err(e) => return HookResult::error(format!("JSON serialization error: {e}")),
        };

        use std::io::Write;
        if let Some(mut stdin) = child.stdin.take() {
            if let Err(e) = stdin.write_all(&input_bytes) {
                return HookResult::error(format!("Failed to write stdin: {e}"));
            }
        }

        // 等待完成（带超时）
        let output = match wait_with_timeout(&mut child, Duration::from_secs(self.entry.timeout_secs)) {
            Ok(output) => output,
            Err(e) => return HookResult::error(e),
        };

        let duration_ms = start.elapsed().as_millis() as u64;

        // 检查退出码
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.trim().is_empty() {
                eprintln!("[ScriptHook] '{}' exited with {}: {}",
                    self.entry.command, output.status, crate::log_sanitizer::sanitize(&stderr.trim()));
            }
            // 非0退出码 → 放行，返回空结果
            return HookResult {
                system_message: None,
                hook_specific_output: None,
                metrics: None,
                decision: None,
                duration_ms,
            };
        }

        // 解析 stdout（可能为空）
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if stdout.is_empty() {
            return HookResult {
                system_message: None,
                hook_specific_output: None,
                metrics: None,
                decision: None,
                duration_ms,
            };
        }

        // 尝试解析 JSON 输出
        let parsed: serde_json::Value = match serde_json::from_str(&stdout) {
            Ok(v) => v,
            Err(_) => {
                // 非 JSON 输出 → 放行
                return HookResult {
                    system_message: None,
                    hook_specific_output: None,
                    metrics: None,
                    decision: None,
                    duration_ms,
                };
            }
        };

        // 提取 HookResult 字段（精确匹配 Claude Code 输出协议）
        HookResult::from_json(&parsed, duration_ms)
    }
}

/// 带超时的子进程等待
fn wait_with_timeout(child: &mut std::process::Child, timeout: Duration) -> std::result::Result<std::process::Output, String> {
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // 收集输出
                let output = child.stdout.take()
                    .and_then(|mut s| {
                        let mut buf = Vec::new();
                        std::io::Read::read_to_end(&mut s, &mut buf).ok().map(|_| buf)
                    })
                    .unwrap_or_default();
                let stderr = child.stderr.take()
                    .and_then(|mut s| {
                        let mut buf = Vec::new();
                        std::io::Read::read_to_end(&mut s, &mut buf).ok().map(|_| buf)
                    })
                    .unwrap_or_default();
                return Ok(std::process::Output {
                    status,
                    stdout: output,
                    stderr,
                });
            }
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    return Err(format!("Hook timed out after {}s", timeout.as_secs()));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(format!("Hook process error: {e}")),
        }
    }
}

/// 简易 shell 命令分割（处理引号）
fn parse_command_argv(cmd: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    let mut in_double_quote = false;

    for ch in cmd.chars() {
        match ch {
            '\'' if !in_double_quote => {
                in_quote = !in_quote;
            }
            '"' if !in_quote => {
                in_double_quote = !in_double_quote;
            }
            ' ' if !in_quote && !in_double_quote => {
                if !current.is_empty() {
                    parts.push(current.clone());
                    current.clear();
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

#[cfg(test)]
fn shell_words_split(cmd: &str) -> Vec<String> {
    parse_command_argv(cmd)
}

// ══════════════════════════════════════════════════════════════════════════════
// Hook 结果 — 精确匹配 Claude Code 的 hook stdout 协议
// ══════════════════════════════════════════════════════════════════════════════

/// Hook 执行结果（精确匹配 Claude Code stdout JSON 协议）
#[derive(Clone, Debug, Default)]
pub struct HookResult {
    /// 系统消息（将注入到对话中）
    pub system_message: Option<String>,
    /// Hook 特定输出（如权限决策）
    pub hook_specific_output: Option<serde_json::Value>,
    /// 埋点数据
    pub metrics: Option<serde_json::Value>,
    /// 决策（如 "block"）
    pub decision: Option<String>,
    /// 执行耗时
    pub duration_ms: u64,
}

impl HookResult {
    /// 从解析后的 JSON 构建
    pub fn from_json(value: &serde_json::Value, duration_ms: u64) -> Self {
        HookResult {
            system_message: value.get("systemMessage").and_then(|v| v.as_str()).map(String::from),
            hook_specific_output: value.get("hookSpecificOutput").cloned(),
            metrics: value.get("metrics").cloned(),
            decision: value.get("decision").and_then(|v| v.as_str()).map(String::from),
            duration_ms,
        }
    }

    /// 创建错误结果
    pub fn error(msg: String) -> Self {
        HookResult {
            system_message: Some(format!("Hook error: {msg}")),
            hook_specific_output: None,
            metrics: None,
            decision: None,
            duration_ms: 0,
        }
    }

    /// 是否被阻止
    pub fn is_blocked(&self) -> bool {
        // Claude Code 的 blocking 逻辑：
        // - PreToolUse/PostToolUse: hookSpecificOutput.permissionDecision == "deny"
        // - Stop: decision == "block"
        if let Some(output) = &self.hook_specific_output {
            if output.get("permissionDecision").and_then(|v| v.as_str()) == Some("deny") {
                return true;
            }
        }
        self.decision.as_deref() == Some("block")
    }

    /// 是否异步唤醒
    pub fn is_async_rewake(&self) -> bool {
        false // 由 hooks.json 的 asyncRewake 字段决定，不在执行结果中
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// BeforeTool / AfterTool Hook 类型（与 Capability 系统交互）
// ══════════════════════════════════════════════════════════════════════════════

/// 工具执行前 hook 的返回
pub enum BeforeHookAction {
    /// 允许执行，可附带修改后的参数
    Allow(serde_json::Value),
    /// 拒绝执行，返回拒绝原因
    Reject(String),
}

/// 工具执行后 hook 的返回
pub enum AfterHookAction {
    /// 继续（默认）
    Continue,
    /// 触发熔断
    CircuitBreak(String),
    /// 修改结果
    Modify(Box<CapabilityResult>),
}

/// Stop hook 返回
pub enum StopAction {
    /// 允许停止
    Allow,
    /// 阻止停止，附带原因
    Block(String),
    /// 警告但不阻止
    Warn(String),
}

/// 用户 prompt 提交 hook 返回
pub enum PromptAction {
    /// 允许提交，可附带修改后的 prompt
    Allow(String),
    /// 拦截提交
    Block(String),
}

// ══════════════════════════════════════════════════════════════════════════════
// 内置 Hook 实现
// ══════════════════════════════════════════════════════════════════════════════

/// 计时 Hook — 记录每个工具的执行耗时
pub struct TimingHook;

impl TimingHook {
    pub fn new() -> Self { Self }
}

impl ToolHook for TimingHook {
    fn name(&self) -> &'static str { "timing" }

    fn hook_points(&self) -> Vec<HookPoint> {
        vec![HookPoint::PreToolUse, HookPoint::PostToolUse]
    }

    fn on_before_tool(&self, _request: &CapabilityRequest) -> BeforeHookAction {
        BeforeHookAction::Allow(serde_json::Value::Null)
    }

    fn on_after_tool(&self, request: &CapabilityRequest, result: &CapabilityResult) -> AfterHookAction {
        println!("[Hook:timing] '{}' completed in {}ms with status {:?}",
            request.name, result.duration_ms, result.status);
        AfterHookAction::Continue
    }
}

/// 安全审批 Hook — 对 requires_approval 能力的二次确认
pub struct ApprovalHook;

impl ToolHook for ApprovalHook {
    fn name(&self) -> &'static str { "approval" }
    fn on_before_tool(&self, _request: &CapabilityRequest) -> BeforeHookAction {
        BeforeHookAction::Allow(serde_json::Value::Null)
    }
}

/// 审计日志 Hook
pub struct AuditHook;

impl ToolHook for AuditHook {
    fn name(&self) -> &'static str { "audit" }

    fn hook_points(&self) -> Vec<HookPoint> {
        vec![HookPoint::PreToolUse, HookPoint::PostToolUse]
    }

    fn on_before_tool(&self, request: &CapabilityRequest) -> BeforeHookAction {
        println!("[Hook:audit] → {} (call_id={})", request.name, request.call_id);
        BeforeHookAction::Allow(serde_json::Value::Null)
    }

    fn on_after_tool(&self, request: &CapabilityRequest, result: &CapabilityResult) -> AfterHookAction {
        println!("[Hook:audit] ← {} (status={:?}, {}ms)",
            request.name, result.status, result.duration_ms);
        AfterHookAction::Continue
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// ToolHook trait — 每个 hook 实现此 trait
// ══════════════════════════════════════════════════════════════════════════════

/// Hook trait（每个 hook 可注册到多个 HookPoint）
pub trait ToolHook: Send + Sync {
    fn name(&self) -> &'static str;

    /// 注册到哪些 HookPoint（默认 PreToolUse）
    fn hook_points(&self) -> Vec<HookPoint> {
        vec![HookPoint::PreToolUse]
    }

    // ── 工具执行前 ──
    fn on_before_tool(&self, _request: &CapabilityRequest) -> BeforeHookAction {
        BeforeHookAction::Allow(serde_json::Value::Null)
    }

    // ── 工具执行后 ──
    fn on_after_tool(&self, _request: &CapabilityRequest, _result: &CapabilityResult) -> AfterHookAction {
        AfterHookAction::Continue
    }

    // ── Stop ──
    fn on_stop(&self, _reason: &str, _transcript: &[serde_json::Value]) -> StopAction {
        StopAction::Allow
    }

    // ── 用户提交 prompt ──
    fn on_prompt_submit(&self, _prompt: &str, _context: &serde_json::Value) -> PromptAction {
        PromptAction::Allow(_prompt.to_string())
    }

    // ── 会话启动 ──
    fn on_session_start(&self, _session_id: &str, _context: &serde_json::Value) -> Vec<HookResult> {
        vec![]
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Hook Pipeline — 按注册顺序依次执行 hooks
// ══════════════════════════════════════════════════════════════════════════════

/// Hook 管道 — 管理所有注册的 hooks 并按 HookPoint 调度执行
pub struct HookPipeline {
    /// 全部注册的 hooks（保持注册顺序）
    internal_hooks: Vec<Box<dyn ToolHook>>,
    /// 外部脚本 hooks（从插件加载）
    script_hooks: Vec<(HookPoint, ScriptHook)>,
    /// 插件根目录（用于环境变量替换）
    plugin_root: Option<std::path::PathBuf>,
    /// 项目目录
    project_dir: Option<std::path::PathBuf>,
}

impl HookPipeline {
    pub fn new() -> Self {
        Self {
            internal_hooks: Vec::new(),
            script_hooks: Vec::new(),
            plugin_root: None,
            project_dir: None,
        }
    }

    /// 设置插件根目录
    pub fn set_plugin_root(&mut self, path: Option<std::path::PathBuf>) {
        self.plugin_root = path;
    }

    /// 设置项目目录
    pub fn set_project_dir(&mut self, path: Option<std::path::PathBuf>) {
        self.project_dir = path;
    }

    /// 注册一个内部 hook
    pub fn register(&mut self, hook: Box<dyn ToolHook>) {
        self.internal_hooks.push(hook);
    }

    /// 注册一个外部脚本 hook（来自插件）
    pub fn register_script_hook(&mut self, point: HookPoint, entry: HookEntry) {
        let hook = ScriptHook::new(
            entry,
            self.plugin_root.clone(),
            self.project_dir.clone(),
        );
        self.script_hooks.push((point, hook));
    }

    /// 注册默认 hooks
    pub fn register_defaults(&mut self) {
        self.register(Box::new(TimingHook::new()));
        self.register(Box::new(ApprovalHook));
        self.register(Box::new(AuditHook));
    }

    /// 获取注册到指定 HookPoint 的内部 hooks
    fn internal_hooks_for(&self, point: &HookPoint) -> Vec<&Box<dyn ToolHook>> {
        self.internal_hooks.iter()
            .filter(|h| h.hook_points().contains(point))
            .collect()
    }

    /// 获取注册到指定 HookPoint 的外部脚本 hooks
    fn script_hooks_for(&self, point: &HookPoint) -> Vec<&ScriptHook> {
        self.script_hooks.iter()
            .filter(|(p, _)| p == point)
            .map(|(_, h)| h)
            .collect()
    }

    // ── 会话启动 (SessionStart) ──

    /// 执行 SessionStart hooks
    pub fn on_session_start(&self, session_id: &str, context: &serde_json::Value) -> Vec<HookResult> {
        let mut results = vec![];

        // 内部 hooks
        for hook in self.internal_hooks_for(&HookPoint::SessionStart) {
            results.extend(hook.on_session_start(session_id, context));
        }

        // 外部脚本 hooks
        for script in self.script_hooks_for(&HookPoint::SessionStart) {
            let result = script.execute("", &serde_json::json!({}), &HookPoint::SessionStart, session_id, None);
            results.push(result);
        }

        results
    }

    // ── 用户提交 prompt (UserPromptSubmit) ──

    /// 执行 UserPromptSubmit hooks
    pub fn on_prompt_submit(&self, prompt: &str, context: &serde_json::Value) -> PromptAction {
        let mut current_prompt = prompt.to_string();

        // 内部 hooks
        for hook in self.internal_hooks_for(&HookPoint::UserPromptSubmit) {
            match hook.on_prompt_submit(&current_prompt, context) {
                PromptAction::Allow(modified) => current_prompt = modified,
                PromptAction::Block(msg) => return PromptAction::Block(msg),
            }
        }

        // 外部脚本 hooks
        let session_id = context.get("session_id").and_then(|v| v.as_str()).unwrap_or("");
        for script in self.script_hooks_for(&HookPoint::UserPromptSubmit) {
            let tool_input = serde_json::json!({ "prompt": current_prompt });
            let result = script.execute(
                "",
                &tool_input,
                &HookPoint::UserPromptSubmit,
                session_id,
                context.get("working_directory").and_then(|v| v.as_str()),
            );
            if result.is_blocked() {
                let msg = result.system_message.unwrap_or_else(|| "Blocked by hook".to_string());
                return PromptAction::Block(msg);
            }
            // 非阻塞的消息当作警告
            if let Some(_msg) = &result.system_message {
                eprintln!("[HookPipeline] UserPromptSubmit warning: {}", crate::log_sanitizer::sanitize(_msg));
            }
        }

        PromptAction::Allow(current_prompt)
    }

    // ── 工具执行前 (PreToolUse) ──

    /// 执行 before_tool 链
    ///
    /// 返回 Ok(modified_args) 或 Err(rejection_reason)
    /// 精确匹配 Claude Code 的 PreToolUse 处理逻辑：
    ///   1. 内部 hooks 顺序评估
    ///   2. 外部脚本 hooks 顺序评估（含 matcher + if 匹配）
    ///   3. 任一 hook 拒绝则中断
    ///   4. hookSpecificOutput.permissionDecision == "deny" 表示拒绝
    pub fn before_tool(&self, request: &CapabilityRequest, session_id: &str) -> std::result::Result<serde_json::Value, String> {
        let mut current_args = request.arguments.clone();

        // 内部 hooks
        for hook in self.internal_hooks_for(&HookPoint::PreToolUse) {
            match hook.on_before_tool(request) {
                BeforeHookAction::Allow(modified) => {
                    if !modified.is_null() {
                        current_args = modified;
                    }
                }
                BeforeHookAction::Reject(reason) => {
                    return Err(format!("[{}] {}", hook.name(), reason));
                }
            }
        }

        // 外部脚本 hooks（含 matcher + if 匹配）
        let tool_input = serde_json::json!({ "arguments": request.arguments });
        for (_, script) in &self.script_hooks {
            if !script.matches_tool(&request.name) {
                continue;
            }
            if !script.matches_if(&request.name, &tool_input) {
                continue;
            }

            let result = script.execute(
                &request.name,
                &serde_json::json!({ "arguments": request.arguments }),
                &HookPoint::PreToolUse,
                session_id,
                request.working_dir.as_ref().and_then(|p| p.to_str()),
            );

            // 检查是否被拒绝
            if result.is_blocked() {
                let reason = result.system_message
                    .unwrap_or_else(|| "Hook rejected without message".to_string());
                return Err(reason);
            }

            // 检查 systemMessage 警告（不影响执行）
            if let Some(msg) = &result.system_message {
                eprintln!("[HookPipeline] PreToolUse warning: {}", crate::log_sanitizer::sanitize(msg));
            }
        }

        Ok(current_args)
    }

    // ── 工具执行后 (PostToolUse) ──

    /// 执行 after_tool 链
    ///
    /// 精确匹配 Claude Code 的 PostToolUse 处理逻辑：
    ///   1. 内部 hooks 顺序评估
    ///   2. 外部脚本 hooks 顺序评估（含 matcher + if 匹配）
    ///   3. CircuitBreak 触发熔断
    pub fn after_tool(&self, request: &CapabilityRequest, result: &mut CapabilityResult, session_id: &str) -> Option<String> {
        // 内部 hooks
        for hook in self.internal_hooks_for(&HookPoint::PostToolUse) {
            match hook.on_after_tool(request, result) {
                AfterHookAction::Continue => {}
                AfterHookAction::CircuitBreak(reason) => {
                    result.status = CapabilityStatus::CircuitBroken;
                    result.output = serde_json::json!({
                        "error": "circuit_broken",
                        "message": reason
                    });
                    return Some(reason);
                }
                AfterHookAction::Modify(modified) => {
                    *result = *modified;
                }
            }
        }

        // 外部脚本 hooks
        let result_json = serde_json::json!({
            "status": format!("{:?}", result.status),
            "output": result.output,
            "duration_ms": result.duration_ms,
        });
        let tool_input = serde_json::json!({
            "arguments": request.arguments,
            "result": result_json,
        });

        for (_, script) in &self.script_hooks {
            if !script.matches_tool(&request.name) {
                continue;
            }
            if !script.matches_if(&request.name, &tool_input) {
                continue;
            }

            let hook_result = script.execute(
                &request.name,
                &tool_input,
                &HookPoint::PostToolUse,
                session_id,
                request.working_dir.as_ref().and_then(|p| p.to_str()),
            );

            if hook_result.is_blocked() {
                let msg = hook_result.system_message.unwrap_or_default();
                result.status = CapabilityStatus::CircuitBroken;
                result.output = serde_json::json!({
                    "error": "post_hook_rejected",
                    "message": msg.clone(),
                });
                return Some(msg);
            }
        }

        None
    }

    // ── Agent 停止 (Stop) ──

    /// 执行 Stop hooks
    pub fn on_stop(&self, reason: &str, transcript: &[serde_json::Value], session_id: &str) -> StopAction {
        // 内部 hooks
        for hook in self.internal_hooks_for(&HookPoint::Stop) {
            match hook.on_stop(reason, transcript) {
                StopAction::Allow => continue,
                StopAction::Block(msg) => return StopAction::Block(msg),
                StopAction::Warn(msg) => {
                    eprintln!("[HookPipeline] Hook '{}' warns on stop: {}", hook.name(), crate::log_sanitizer::sanitize(&msg));
                }
            }
        }

        // 外部脚本 hooks
        let tool_input = serde_json::json!({
            "reason": reason,
            "transcript": transcript,
        });
        for (_, script) in &self.script_hooks {
            let result = script.execute(
                "",
                &tool_input,
                &HookPoint::Stop,
                session_id,
                None,
            );

            if result.is_blocked() {
                let msg = result.system_message.unwrap_or_else(|| "Stopped by hook".to_string());
                return StopAction::Block(msg);
            }
            if let Some(msg) = &result.system_message {
                return StopAction::Warn(msg.clone());
            }
            if result.is_async_rewake() {
                // 后台任务已触发
            }
        }

        StopAction::Allow
    }

    /// 列出所有已注册的 hook 信息
    pub fn list_hooks(&self) -> Vec<HookInfo> {
        let mut infos = vec![];
        for hook in &self.internal_hooks {
            let points = hook.hook_points();
            infos.push(HookInfo {
                name: hook.name().to_string(),
                hook_type: "internal".to_string(),
                points: points.into_iter().map(|p| p.as_str().to_string()).collect(),
            });
        }
        for (point, hook) in &self.script_hooks {
            infos.push(HookInfo {
                name: hook.name(),
                hook_type: "script".to_string(),
                points: vec![point.as_str().to_string()],
            });
        }
        infos
    }
}

/// Hook 信息（用于调试和展示）
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookInfo {
    pub name: String,
    pub hook_type: String,
    pub points: Vec<String>,
}

// ══════════════════════════════════════════════════════════════════════════════
// 测试
// ══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_hook_point_from_str_roundtrip() {
        for (s, expected) in &[
            ("SessionStart", HookPoint::SessionStart),
            ("UserPromptSubmit", HookPoint::UserPromptSubmit),
            ("PreToolUse", HookPoint::PreToolUse),
            ("PostToolUse", HookPoint::PostToolUse),
            ("Stop", HookPoint::Stop),
        ] {
            let parsed = HookPoint::from_str(s).unwrap();
            assert_eq!(parsed, *expected);
            assert_eq!(parsed.as_str(), *s);
        }
    }

    #[test]
    fn test_tool_matches_pattern() {
        assert!(tool_matches_pattern("Edit", "Edit|Write|MultiEdit"));
        assert!(tool_matches_pattern("Write", "Edit|Write|MultiEdit"));
        assert!(!tool_matches_pattern("Bash", "Edit|Write|MultiEdit"));
        assert!(tool_matches_pattern("Bash", "Bash"));
        assert!(tool_matches_pattern("anything", "*"));
    }

    #[test]
    fn test_evaluate_if_condition() {
        let tool_input = json!({"command": "git commit -m 'fix'"});
        assert!(evaluate_if_condition("Bash(git commit*)", "Bash", &tool_input));
        assert!(!evaluate_if_condition("Bash(git push*)", "Bash", &tool_input));
        assert!(!evaluate_if_condition("Bash(git commit*)", "Write", &tool_input));
    }

    #[test]
    fn test_env_var_resolution() {
        let result = resolve_env_vars(
            "python3 ${CLAUDE_PLUGIN_ROOT}/hook.py",
            Some(Path::new("/plugins/my-hook")),
            None,
        );
        assert_eq!(result, "python3 /plugins/my-hook/hook.py");
    }

    #[test]
    fn test_hook_entry_from_json() {
        let json = json!({
            "type": "command",
            "command": "python3 test.py",
            "timeout": 30,
            "if": "Bash(git commit:*)",
            "asyncRewake": true,
            "rewakeMessage": "Reviewing...",
            "rewakeSummary": "Found issues"
        });
        let entry = HookEntry::from_json(&json).unwrap();
        assert_eq!(entry.r#type, "command");
        assert_eq!(entry.command, "python3 test.py");
        assert_eq!(entry.timeout_secs, 30);
        assert_eq!(entry.if_condition.as_deref(), Some("Bash(git commit:*)"));
        assert!(entry.async_rewake);
    }

    #[test]
    fn test_extended_hook_points_parse() {
        for name in ["SessionEnd", "SubagentStop", "PreCompact", "Notification"] {
            let point = HookPoint::from_str(name).expect("extended hook point parses");
            assert_eq!(point.as_str(), name);
        }
    }

    #[test]
    fn test_hook_entry_from_json_minimal() {
        let json = json!({
            "type": "command",
            "command": "echo hello"
        });
        let entry = HookEntry::from_json(&json).unwrap();
        assert_eq!(entry.timeout_secs, 10); // default
        assert!(!entry.async_rewake);
        assert!(entry.if_condition.is_none());
    }

    #[test]
    fn test_hook_result_from_json() {
        let json = json!({
            "systemMessage": "Warning!",
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "deny"
            },
            "metrics": {"pattern_hits": 3}
        });
        let result = HookResult::from_json(&json, 42);
        assert_eq!(result.system_message.as_deref(), Some("Warning!"));
        assert!(result.is_blocked());
        assert_eq!(result.duration_ms, 42);
    }

    #[test]
    fn test_hook_result_block_decision() {
        let json = json!({
            "decision": "block",
            "systemMessage": "Blocked by rule"
        });
        let result = HookResult::from_json(&json, 0);
        assert!(result.is_blocked());
    }

    #[test]
    fn test_hook_pipeline_before_tool_no_hooks() {
        let pipeline = HookPipeline::new();
        let req = CapabilityRequest {
            call_id: "t1".into(),
            name: "read_file".into(),
            arguments: json!({"path": "/test"}),
            working_dir: None,
        };
        let result = pipeline.before_tool(&req, "session-1");
        assert!(result.is_ok());
    }

    #[test]
    fn test_hook_group_from_json() {
        let json = json!({
            "hooks": [{
                "type": "command",
                "command": "python3 test.py"
            }],
            "matcher": "Edit|Write"
        });
        let group = HookGroup::from_json(&json).unwrap();
        assert_eq!(group.matcher.as_deref(), Some("Edit|Write"));
        assert_eq!(group.hooks.len(), 1);
    }

    #[test]
    fn test_hook_group_matches_tool() {
        let json = json!({
            "hooks": [{"type": "command", "command": "test.py"}],
            "matcher": "Edit|Write|MultiEdit"
        });
        let group = HookGroup::from_json(&json).unwrap();
        assert!(group.matches_tool("Edit"));
        assert!(group.matches_tool("Write"));
        assert!(!group.matches_tool("Bash"));
    }

    #[test]
    fn test_shell_words_split() {
        let parts = shell_words_split("python3 '/path/to/script.py' --arg value");
        assert_eq!(parts.len(), 4);
        assert_eq!(parts[1], "/path/to/script.py");
    }

    #[test]
    fn test_before_tool_rejection() {
        let pipeline = HookPipeline::new();
        // 没有注册任何 hook，应该允许
        let req = CapabilityRequest {
            call_id: "t1".into(),
            name: "write_file".into(),
            arguments: json!({"path": "/tmp/x", "content": "test"}),
            working_dir: None,
        };
        let result = pipeline.before_tool(&req, "session-1");
        assert!(result.is_ok(), "No hooks → should allow");
    }

    #[test]
    fn test_stop_hook_allow_by_default() {
        let pipeline = HookPipeline::new();
        let action = pipeline.on_stop("done", &[], "session-1");
        assert!(matches!(action, StopAction::Allow));
    }

    #[test]
    fn test_prompt_submit_allow_by_default() {
        let pipeline = HookPipeline::new();
        let action = pipeline.on_prompt_submit("hello", &json!({}));
        assert!(matches!(action, PromptAction::Allow(_)));
    }

    #[test]
    fn test_list_hooks_after_register() {
        let mut pipeline = HookPipeline::new();
        pipeline.register(Box::new(TimingHook::new()));
        pipeline.register(Box::new(ApprovalHook));
        let hooks = pipeline.list_hooks();
        assert!(hooks.iter().any(|h| h.name == "timing"));
        assert!(hooks.iter().any(|h| h.name == "approval"));
    }
}
