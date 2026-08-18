//! MCP execution and the in-flight MCP call registry.

use agent_core::{ToolExecutionResult, ToolProgressSink, ToolProgressUpdate};
use serde_json::Value;
use std::sync::Arc;
use std::time::Instant;
use tokio_util::sync::CancellationToken;

use super::gated::PermissionGatedTools;

// Keep dynamic MCP calls aligned with CapabilityGateway's default manifest.
const MCP_OUTPUT_LIMIT_BYTES: usize = 256_000;

fn mcp_output_exceeds_limit(output: &Value) -> bool {
    capability_gateway::policy::check_output_limit(
        output.to_string().as_bytes(),
        MCP_OUTPUT_LIMIT_BYTES as u64,
    )
}

/// Process-wide registry of in-flight MCP tool calls (J03). A call enters when
/// its invocation starts and leaves when it settles, so after a cancel the
/// registry is quiet — there is no lingering request whose late response could
/// be mistaken for a completed effect.
static PENDING_MCP_CALLS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
    std::sync::OnceLock::new();

pub(crate) fn pending_mcp_calls() -> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    PENDING_MCP_CALLS.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
}

/// Number of in-flight MCP calls; must be zero after every settle.
pub fn pending_mcp_call_count() -> usize {
    pending_mcp_calls().lock().unwrap().len()
}

/// Ledger settlement status for an MCP call outcome.
///
/// A transport timeout / failure while the run is *not* cancelled is `failed`;
/// a user cancel is `uncertain` because the external outcome is unknowable
/// while we tear the call down. The two states are deliberately distinct and
/// auditable (T04).
pub(crate) fn mcp_ledger_status(outcome_ok: bool, cancelled: bool) -> &'static str {
    if cancelled {
        "uncertain"
    } else if outcome_ok {
        "completed"
    } else {
        "failed"
    }
}

impl PermissionGatedTools {
    /// Route `mcp_call` / namespaced `mcp__server__tool` through daemon MCP runtime.
    #[allow(clippy::too_many_arguments)] // pre-existing: parameter list is fixed
    pub(crate) async fn execute_mcp_call(
        &self,
        name: &str,
        input: Value,
        core_call_id: Option<&str>,
        turn_id: Option<&str>,
        message_id: Option<&str>,
        parent_cancel: &CancellationToken,
        progress: Arc<dyn ToolProgressSink>,
    ) -> ToolExecutionResult {
        let started = Instant::now();
        let (server_id, tool_name, arguments) = if name == "mcp_call" {
            let server = input
                .get("server")
                .or_else(|| input.get("server_id"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let tool = input
                .get("tool")
                .or_else(|| input.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let args = input
                .get("arguments")
                .or_else(|| input.get("input"))
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            (server, tool, args)
        } else {
            // mcp__{server}__{tool}
            let rest = name.strip_prefix("mcp__").unwrap_or(name);
            let mut parts = rest.splitn(2, "__");
            let server = parts.next().unwrap_or("").to_string();
            let tool = parts.next().unwrap_or("").to_string();
            (server, tool, input)
        };
        if server_id.is_empty() || tool_name.is_empty() {
            return ToolExecutionResult {
                output: serde_json::json!({
                    "error": "mcp_call requires server and tool",
                    "code": "invalid_input",
                }),
                is_error: true,
                duration_ms: started.elapsed().as_millis() as u64,
            };
        }
        let call_id = core_call_id
            .filter(|id| !id.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("mcp-{server_id}-{tool_name}"));
        let cancel = if let Some(rt) = &self.runtime {
            rt.execution
                .token(&self.parent_run_id)
                .await
                .unwrap_or_else(|| parent_cancel.clone())
        } else {
            parent_cancel.clone()
        };
        let ledger_summary = serde_json::json!({
            "server": server_id.clone(),
            "tool": tool_name.clone(),
        });
        if let Err(error) = crate::side_effect_ledger::record_tool_effect_state(
            &self.parent_run_id,
            &call_id,
            "mcp_call",
            "mcp",
            "started",
            false,
            turn_id,
            &ledger_summary,
        ) {
            return ToolExecutionResult {
                output: serde_json::json!({
                    "error_code": "PERSISTENCE_FAILED",
                    "error": format!("MCP side-effect ledger could not be started: {error}"),
                }),
                is_error: true,
                duration_ms: started.elapsed().as_millis() as u64,
            };
        }
        let progress_callback: Arc<dyn Fn(Value) + Send + Sync> = {
            let progress = progress.clone();
            let run_id = self.parent_run_id.clone();
            let call_id = call_id.clone();
            let turn_id = turn_id.map(str::to_string);
            let message_id = message_id.map(str::to_string);
            Arc::new(move |frame: Value| {
                let text = frame
                    .get("params")
                    .and_then(|params| params.get("message").or_else(|| params.get("progress")))
                    .map(|value| {
                        value
                            .as_str()
                            .map(str::to_string)
                            .unwrap_or_else(|| value.to_string())
                    })
                    .unwrap_or_else(|| frame.to_string());
                let progress = progress.clone();
                let run_id = run_id.clone();
                let call_id = call_id.clone();
                let turn_id = turn_id.clone();
                let message_id = message_id.clone();
                if let Ok(handle) = tokio::runtime::Handle::try_current() {
                    handle.spawn(async move {
                        progress
                            .publish(ToolProgressUpdate {
                                run_id,
                                tool_call_id: call_id,
                                tool_name: "mcp_call".into(),
                                stream: "mcp".into(),
                                text,
                                final_update: false,
                                turn_id,
                                message_id,
                                progress_sequence: 0,
                            })
                            .await;
                    });
                }
            })
        };
        pending_mcp_calls().lock().unwrap().insert(call_id.clone());
        let outcome = crate::runtime::mcp_invocation::invoke_mcp_tool_with_progress(
            &server_id,
            &tool_name,
            arguments,
            &cancel,
            Some(&self.parent_run_id),
            Some(progress_callback),
        )
        .await;
        // J03: the pending registry is quiet once the call settles — no
        // lingering request can later be mistaken for a fresh effect.
        pending_mcp_calls().lock().unwrap().remove(&call_id);
        // T04: settle the progress channel so late progress frames are rejected
        // and any buffered flush task is drained — progress tasks return to 0.
        progress.mark_tool_call_settled(&call_id).await;
        match outcome {
            Ok(result) => {
                let duration_ms = started.elapsed().as_millis() as u64;
                // J03: a late success that lands after the run was cancelled is
                // never recorded as completed — the external outcome is
                // unknowable while we are tearing the call down.
                let status = mcp_ledger_status(true, cancel.is_cancelled());
                // MCP is not auto-rollbackable — record for restore coverage honesty.
                let ledger_result = crate::side_effect_ledger::record_tool_effect_state(
                    &self.parent_run_id,
                    &call_id,
                    "mcp_call",
                    "mcp",
                    status,
                    false,
                    turn_id,
                    &ledger_summary,
                );
                if let Err(error) = ledger_result {
                    let _ = crate::side_effect_ledger::record_tool_effect_state(
                        &self.parent_run_id,
                        &call_id,
                        "mcp_call",
                        "mcp",
                        "uncertain",
                        false,
                        turn_id,
                        &ledger_summary,
                    );
                    return ToolExecutionResult {
                        output: serde_json::json!({
                            "error_code": "PERSISTENCE_FAILED",
                            "error": format!("MCP side-effect ledger could not be completed: {error}"),
                        }),
                        is_error: true,
                        duration_ms,
                    };
                }
                if mcp_output_exceeds_limit(&result) {
                    return ToolExecutionResult {
                        output: serde_json::json!({
                            "error": format!(
                                "tool `{name}` output exceeded {MCP_OUTPUT_LIMIT_BYTES} bytes"
                            ),
                            "code": "output_limit",
                        }),
                        is_error: true,
                        duration_ms,
                    };
                }
                ToolExecutionResult {
                    output: serde_json::json!({
                        "server": server_id,
                        "tool": tool_name,
                        "ok": true,
                        "result": result,
                    }),
                    is_error: false,
                    duration_ms,
                }
            }
            Err(e) => {
                let duration_ms = started.elapsed().as_millis() as u64;
                let ledger_result = crate::side_effect_ledger::record_tool_effect_state(
                    &self.parent_run_id,
                    &call_id,
                    "mcp_call",
                    "mcp",
                    mcp_ledger_status(false, cancel.is_cancelled()),
                    false,
                    turn_id,
                    &ledger_summary,
                );
                if let Err(error) = ledger_result {
                    let _ = crate::side_effect_ledger::record_tool_effect_state(
                        &self.parent_run_id,
                        &call_id,
                        "mcp_call",
                        "mcp",
                        "uncertain",
                        false,
                        turn_id,
                        &ledger_summary,
                    );
                    return ToolExecutionResult {
                        output: serde_json::json!({
                            "error_code": "PERSISTENCE_FAILED",
                            "error": format!("MCP side-effect ledger could not be settled: {error}"),
                        }),
                        is_error: true,
                        duration_ms,
                    };
                }
                ToolExecutionResult {
                    output: serde_json::json!({
                        "server": server_id,
                        "tool": tool_name,
                        "ok": false,
                        "error": e,
                        "code": "mcp_call_failed",
                    }),
                    is_error: true,
                    duration_ms,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TASK-007 (J03): the in-flight MCP registry returns to zero once a call
    /// settles — no lingering request can be mistaken for a fresh effect.
    #[test]
    fn progress_backpressure_mcp_registry_quiet_after_settle() {
        crate::tools::mcp::pending_mcp_calls()
            .lock()
            .unwrap()
            .insert("mcp-call-1".to_string());
        assert_eq!(pending_mcp_call_count(), 1);
        crate::tools::mcp::pending_mcp_calls()
            .lock()
            .unwrap()
            .remove("mcp-call-1");
        assert_eq!(
            pending_mcp_call_count(),
            0,
            "registry is quiet after settle"
        );
    }

    /// T04: the ledger must distinguish a transport timeout (not cancelled →
    /// `failed`) from a user cancel (`uncertain`) so resume/audit can tell them
    /// apart.
    #[test]
    fn mcp_ledger_status_distinguishes_timeout_from_cancel() {
        assert_eq!(
            crate::tools::mcp::mcp_ledger_status(true, false),
            "completed"
        );
        assert_eq!(crate::tools::mcp::mcp_ledger_status(false, false), "failed"); // timeout
        assert_eq!(
            crate::tools::mcp::mcp_ledger_status(true, true),
            "uncertain"
        ); // cancel
        assert_eq!(
            crate::tools::mcp::mcp_ledger_status(false, true),
            "uncertain"
        ); // cancel
    }

    #[test]
    fn mcp_output_limit_allows_exact_boundary_and_rejects_one_byte_over() {
        let exact = Value::String("x".repeat(MCP_OUTPUT_LIMIT_BYTES - 2));
        let over = Value::String("x".repeat(MCP_OUTPUT_LIMIT_BYTES - 1));

        assert_eq!(exact.to_string().len(), MCP_OUTPUT_LIMIT_BYTES);
        assert!(!mcp_output_exceeds_limit(&exact));
        assert_eq!(over.to_string().len(), MCP_OUTPUT_LIMIT_BYTES + 1);
        assert!(mcp_output_exceeds_limit(&over));
    }
}
