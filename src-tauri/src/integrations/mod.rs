//! AI Tool Integration 目标域（ADR-0020 §4 / 05-MODULE-REMEDIATION-PLAN §6）
//!
//! Claude Code / Codex / Gemini CLI / OpenCode 归入本层，**不是 AgentRuntime**。
//! 统一七步配置流程：Detect / Inspect / Backup / Plan / Apply / Verify /
//! Rollback；每次写入必须 backup + atomic patch + verify，失败可回滚。
//! Secret 字段只允许 `secret_ref`（Keychain）引用，绝不落盘明文。

pub mod adapter;
pub mod claude_code;
pub mod codex;
pub mod common;
pub mod gemini_cli;
pub mod model;
pub mod opencode;

pub use adapter::{ApplyRequest, EnvRef, IntegrationError, ToolIntegration};
pub use claude_code::ClaudeCodeAdapter;
pub use codex::CodexAdapter;
pub use gemini_cli::GeminiCliAdapter;
pub use model::{
    BackupResult, DetectResult, InspectResult, PlannedPatch, RollbackResult, ToolKind, VerifyResult,
};
pub use opencode::OpenCodeAdapter;

pub fn get_integration(kind: ToolKind) -> Box<dyn ToolIntegration> {
    match kind {
        ToolKind::ClaudeCode => Box::new(ClaudeCodeAdapter),
        ToolKind::Codex => Box::new(CodexAdapter),
        ToolKind::GeminiCli => Box::new(GeminiCliAdapter),
        ToolKind::OpenCode => Box::new(OpenCodeAdapter),
    }
}
