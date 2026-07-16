//! runtime/mod.rs — Runtime 抽象层
//!
//! 顶层 `trait AgentRuntime`，三个实现（Claude CLI / Codex CLI / Native），
//! `resolve_runtime` 按可用性自动分流。详见 CONTEXT.md「执行引擎」与
//! docs/architecture/EXECUTION-ENGINE-DESIGN.md P1。

pub mod registry;
pub mod native_runtime;
pub mod native;
pub mod claude_cli;
pub mod codex_cli;
pub mod cli_permission;

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
    /// runtime 内部 await 它感知中断信号（由外部 cancel_stream IPC 持有 Sender 发信号）
    pub abort_receiver: tokio::sync::oneshot::Receiver<()>,
    /// runtime 专属透传字段（CLI 的 sdk_session_id、Native 的 files 等）
    pub runtime_options: serde_json::Value,
}

/// SSE 下行事件载荷（对应 CodePilot RuntimeRunEvent 8+1 union）
///
/// 前端按 `type` 分发渲染，未知 type 走 `unknown_item` fallback。
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuntimeEvent {
    AssistantDelta { text: String },
    ReasoningDelta { text: String },
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
    RunCompleted { reason: String },
    RunFailed { error: String },
    UnknownItem { raw: serde_json::Value },
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
