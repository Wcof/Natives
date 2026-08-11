//! Gated-execution helpers for `PermissionGatedTools`.
//!
//! Inherent helpers backing the `EngineToolRuntime` impl that must stay in a
//! single block (Rust forbids splitting one trait impl across files, E0119):
//! the A4 read-only fast path and the uncertain side-effect ledger hook.
//! Split out of `gated_execute` so each file stays under 1000 lines (W10).

use super::*;

impl PermissionGatedTools {
    /// A4 — ReadOnly Fast Path.
    ///
    /// Classification source is the Gateway's SideEffect, never the legacy
    /// name-default `external`. Genuinely read-only tools (read_file /
    /// list_dir / grep / search / memory reads) skip checkpoint before/after,
    /// the side-effect ledger, and the conflict lease entirely. ToolCallStarted
    /// and ToolCallCompleted remain durable facts, so the read-only invocation
    /// is still observable/replayable.
    pub(crate) async fn execute_readonly_fast_path(
        &self,
        name: &str,
        input: Value,
        tool_context: &capability_gateway::ToolCallContext,
        stream_tool_call_id: &str,
    ) -> ToolExecutionResult {
        let started = Instant::now();
        match self
            .gateway
            .execute(name, input.clone(), tool_context)
            .await
        {
            Ok(out) => {
                let mut output = out.result;
                attach_tool_output_artifact(&self.parent_run_id, stream_tool_call_id, &mut output);
                let is_error = output.get("error_code").and_then(Value::as_str).is_some()
                    || output.get("error").is_some();
                ToolExecutionResult {
                    output,
                    is_error,
                    duration_ms: out.duration_ms.max(started.elapsed().as_millis() as u64),
                }
            }
            Err(error) => ToolExecutionResult {
                output: serde_json::json!({ "error": error, "code": "readonly_tool_failed" }),
                is_error: true,
                duration_ms: started.elapsed().as_millis() as u64,
            },
        }
    }

    /// Called when a handler returned but the authoritative completion fact
    /// could not be persisted: record an `uncertain` side-effect marker so
    /// resume code cannot replay an unknown side effect.
    pub(crate) async fn mark_tool_call_uncertain_impl(
        &self,
        call_id: &str,
        name: &str,
        turn_id: Option<&str>,
        input: &Value,
    ) -> Result<(), String> {
        crate::side_effect_ledger::record_tool_effect_state(
            &self.parent_run_id,
            call_id,
            name,
            crate::side_effect_ledger::category_for_tool(name),
            "uncertain",
            false,
            turn_id,
            input,
        )
    }
}
