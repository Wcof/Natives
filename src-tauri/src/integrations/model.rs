//! AI Tool Integration 目标边界（ADR-0020 §4 / 05-MODULE-REMEDIATION-PLAN §6）
//!
//! Claude Code / Codex / Gemini CLI / OpenCode 属于这一层，**不是 AgentRuntime**。
//! 启动工具可借 Terminal / App Launch；本模块负责**安全接管本地配置**的
//! 七步流程：Detect / Inspect / Backup / Plan / Apply / Verify / Rollback。
//!
//! 每次配置写入必须 backup + atomic patch + verify + rollback（05 §6）。
//! 不引入 Agent Loop / Planner / Subagent 语义；旧 `runtime::{ClaudeCli,
//! CodexCli}Runtime` 的 AgentRuntime wrapper 在 Legacy Removal 阶段删除。

use serde::{Deserialize, Serialize};

/// 工具标识。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    ClaudeCode,
    Codex,
    GeminiCli,
    OpenCode,
}

impl ToolKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude_code",
            Self::Codex => "codex",
            Self::GeminiCli => "gemini_cli",
            Self::OpenCode => "opencode",
        }
    }
}

/// Detect：检测工具是否安装（可执行文件 / 版本）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DetectResult {
    pub installed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executable: Option<String>,
}

/// Inspect：检查当前配置（哪些字段存在 / 是否受管）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InspectResult {
    pub config_path: Option<String>,
    /// 配置是否已存在。
    pub exists: bool,
    /// 是否由 AiNative 接管（含 managed 标记）。
    pub managed: bool,
    /// 发现的敏感字段名（不投影值）。
    pub sensitive_keys: Vec<String>,
    /// 配置结构摘要（用于 Plan 步骤）。
    pub summary: String,
}

/// Backup：应用变更前的备份（时间戳文件）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BackupResult {
    pub backup_path: Option<String>,
    pub created: bool,
}

/// Plan：计算将写入的补丁（原子 patch 内容，不含明文 Secret）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlannedPatch {
    /// 目标配置文件路径。
    pub target: String,
    /// 补丁后 JSON（经审查的完整内容；Secret 字段只允许 `env_ref` 引用）。
    pub patch_json: String,
    /// 变更摘要（用户可读）。
    pub summary: String,
}

/// Verify：Apply 后验证（配置可解析 / 关键字段生效）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VerifyResult {
    pub ok: bool,
    pub checks: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Rollback：恢复备份。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RollbackResult {
    pub restored: bool,
    pub backup_path: Option<String>,
}
