//! runtime/mod.rs — Runtime 抽象层
//!
//! 顶层 `trait AgentRuntime`，用于 Claude CLI / Codex CLI 等独立 CLI runtime。
//! Native Assistant 生产执行入口已切换为 Protocol v2 Agent Daemon，不走这里。
//! `resolve_runtime` 按可用性自动分流。详见 CONTEXT.md「执行引擎」与
//! docs/architecture/EXECUTION-ENGINE-DESIGN.md P1。
//!
//! 曾有一个 `native` 子模块（AgentLoop / CapabilityRegistry / HookPipeline /
//! RuleEngine，约 5.4k 行）停在这里，从未注册进 `registry`，也没有任何外部调用
//! 方。它已被删除；其中值得保留的 Hook 概念（身份、来源、条件匹配）已吸收进
//! `harness-core`。
//!
//! 遗留待办：`AgentRuntime` 的执行面（`stream` / `interrupt` / `dispose`）目前
//! 无调用方——CLI runtime 只经 `registry::list_runtime_metadata` 用于能力展示。
//! 与 Harness 控制面设计中「Claude/Codex CLI 只读所有权视图」一致，清理排在
//! Phase 5。下面的 `dead_code` 豁免仅为此保留；原先的 blanket 豁免（含
//! `unused_imports`/`unused_variables`）已移除，它曾把整个 native 子树的死代码
//! 一并隐藏。
#![allow(dead_code)]

pub mod claude_cli;
pub mod cli_permission;
pub mod codex_cli;
pub mod registry;

use async_trait::async_trait;
use futures_util::stream::BoxStream;
use serde::Serialize;
use std::path::PathBuf;

/// 单次 stream 调用的入参（对应 CodePilot RuntimeStreamOptions）
pub struct RuntimeStreamOptions {
    pub run_id: String,
    pub session_id: String,
    pub prompt: String,
    pub model: String,
    pub provider_id: String,
    pub system_prompt: Option<String>,
    pub working_directory: Option<PathBuf>,
    /// runtime 内部 await 它感知中断信号。
    pub abort_receiver: tokio::sync::oneshot::Receiver<()>,
    /// runtime 专属透传字段（例如 CLI 的 sdk_session_id）。
    pub runtime_options: serde_json::Value,
}

/// SSE 下行事件载荷（对应 CodePilot RuntimeRunEvent 8+1 union）
///
/// 前端按 `type` 分发渲染，未知 type 走 `unknown_item` fallback。
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuntimeEvent {
    AssistantDelta {
        text: String,
    },
    ReasoningDelta {
        text: String,
    },
    ToolStarted {
        tool_name: String,
        tool_call_id: String,
        args: serde_json::Value,
    },
    PermissionRequested {
        tool_name: String,
        tool_call_id: String,
        reason: String,
        args: serde_json::Value,
    },
    ToolCompleted {
        tool_call_id: String,
        status: String,
        output: serde_json::Value,
    },
    ToolRejected {
        tool_name: String,
        tool_call_id: String,
        reason: String,
    },
    FileChanged {
        path: String,
        change_type: String,
    },
    UsageUpdated {
        input_tokens: u64,
        output_tokens: u64,
    },
    RunCompleted {
        reason: String,
    },
    RunFailed {
        error: String,
    },
    UnknownItem {
        raw: serde_json::Value,
    },
}

pub type EventStream = BoxStream<'static, RuntimeEvent>;

#[async_trait]
pub trait AgentRuntime: Send + Sync {
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn is_available(&self) -> bool;
    async fn stream(&self, options: RuntimeStreamOptions) -> crate::Result<EventStream>;
    fn interrupt(&self, session_id: &str);
    fn dispose(&self);
}
