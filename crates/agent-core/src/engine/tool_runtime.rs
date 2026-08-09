//! Tool execution contract for the engine loop.
//!
//! Extracted from `engine_core.rs` so the loop keeps only its orchestration.
//! This module owns the tool-side types (`ToolSchema`, `ToolExecutionResult`,
//! `ToolProgressUpdate`, ...) and the [`EngineToolRuntime`] seam the daemon
//! implements with capability-gateway.

use serde_json::{json, Value};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Tool execution seam used by the engine.
#[async_trait::async_trait]
#[allow(clippy::too_many_arguments)] // public trait: signature is frozen for implementors
pub trait EngineToolRuntime: Send + Sync {
    async fn list_tool_schemas(&self) -> Vec<ToolSchema>;

    /// Capability metadata is supplied by the Gateway. Unknown tools are
    /// intentionally sequential so the core fails closed.
    async fn list_tool_capabilities(&self) -> Vec<ToolCapability> {
        Vec::new()
    }
    async fn execute_tool(
        &self,
        name: &str,
        input: Value,
        cancel: &CancellationToken,
    ) -> ToolExecutionResult;

    /// Execute with a stable Core ToolCall id so gateway output and ledger
    /// records cannot drift to a second per-handler UUID.
    async fn execute_tool_with_call_id(
        &self,
        name: &str,
        input: Value,
        cancel: &CancellationToken,
        _call_id: Option<&str>,
    ) -> ToolExecutionResult {
        self.execute_tool_with_call_id_and_progress(
            name,
            input,
            cancel,
            _call_id,
            None,
            None,
            Arc::new(NoopToolProgressSink),
        )
        .await
    }

    /// Optional call-identity-aware execution hook for runtimes that can emit
    /// live handler progress. The default keeps lightweight runtimes compatible.
    async fn execute_tool_with_call_id_and_progress(
        &self,
        name: &str,
        input: Value,
        cancel: &CancellationToken,
        _call_id: Option<&str>,
        _turn_id: Option<&str>,
        _message_id: Option<&str>,
        _progress: Arc<dyn ToolProgressSink>,
    ) -> ToolExecutionResult {
        self.execute_tool(name, input, cancel).await
    }

    async fn execute_tool_with_progress(
        &self,
        name: &str,
        input: Value,
        cancel: &CancellationToken,
        _progress: &dyn ToolProgressSink,
    ) -> ToolExecutionResult {
        self.execute_tool(name, input, cancel).await
    }

    /// Progress variant carrying Core's stable ToolCall identity.  The older
    /// method remains as a compatibility hook for lightweight runtimes.
    async fn execute_tool_with_progress_for_call(
        &self,
        call_id: &str,
        _turn_id: Option<&str>,
        _message_id: Option<&str>,
        name: &str,
        input: Value,
        cancel: &CancellationToken,
        progress: Arc<dyn ToolProgressSink>,
    ) -> ToolExecutionResult {
        let _ = call_id;
        self.execute_tool_with_progress(name, input, cancel, progress.as_ref())
            .await
    }

    /// Optional batch entry for same-turn `task` tool calls.
    ///
    /// Default falls back to sequential `execute_tool("task", ...)`.
    /// Native Runtime overrides this to emit **one** `subagent_assignment`
    /// interaction for the whole batch, then start children by `call_id`.
    async fn execute_task_batch(
        &self,
        tasks: Vec<(String, Value)>,
        cancel: &CancellationToken,
    ) -> Vec<ToolExecutionResult> {
        let mut out = Vec::with_capacity(tasks.len());
        for (_call_id, input) in tasks {
            if cancel.is_cancelled() {
                out.push(ToolExecutionResult {
                    output: json!({"error": "cancelled"}),
                    is_error: true,
                    duration_ms: 0,
                });
                continue;
            }
            out.push(self.execute_tool("task", input, cancel).await);
        }
        out
    }

    /// Progress-aware batch seam for subagent tools. The default preserves
    /// compatibility with lightweight runtimes; production runtimes can keep
    /// the sink alive while child Runs emit their own events.
    async fn execute_task_batch_with_progress(
        &self,
        tasks: Vec<(String, Value)>,
        cancel: &CancellationToken,
        _progress: Arc<dyn ToolProgressSink>,
        _turn_id: Option<&str>,
        _message_id: Option<&str>,
    ) -> Vec<ToolExecutionResult> {
        self.execute_task_batch(tasks, cancel).await
    }

    /// Called when a handler returned but the authoritative completion fact
    /// could not be persisted. Production runtimes record this as `uncertain`
    /// so resume code cannot replay an unknown side effect. Returns Err when
    /// the uncertain fact itself could not be persisted — the caller must then
    /// fail the run with a `recovery_blocked` terminal so a later resume can
    /// never assume a side effect it cannot prove.
    async fn mark_tool_call_uncertain(
        &self,
        _call_id: &str,
        _name: &str,
        _turn_id: Option<&str>,
        _input: &Value,
    ) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolExecutionMode {
    ParallelSafe,
    Sequential,
    Exclusive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCapability {
    pub name: String,
    pub schema: Value,
    pub execution_mode: ToolExecutionMode,
    pub side_effect: ToolSideEffect,
    pub conflict_key: Option<String>,
}

/// Provider-neutral safety classification advertised by the Gateway.
/// Core never infers this from a tool name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolSideEffect {
    ReadOnly,
    Write,
    Destructive,
    Network,
    Process,
}

#[derive(Debug, Clone)]
pub struct ToolExecutionResult {
    pub output: Value,
    pub is_error: bool,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolProgressUpdate {
    pub run_id: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub stream: String,
    pub text: String,
    pub final_update: bool,
    pub turn_id: Option<String>,
    pub message_id: Option<String>,
    pub progress_sequence: u64,
}

#[async_trait::async_trait]
pub trait ToolProgressSink: Send + Sync {
    async fn publish(&self, update: ToolProgressUpdate);

    async fn mark_tool_call_settled(&self, _tool_call_id: &str) {}
}

#[derive(Debug, Default)]
pub struct NoopToolProgressSink;

#[async_trait::async_trait]
impl ToolProgressSink for NoopToolProgressSink {
    async fn publish(&self, _update: ToolProgressUpdate) {}
}
