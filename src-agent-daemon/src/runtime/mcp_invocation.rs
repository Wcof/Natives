//! Shared MCP invocation gate (task-06).
//!
//! `McpRuntime::call_tool` is transport-only. All executable entry points must
//! go through this module: trust → (schema) → permission already done by caller
//! → cancellable invoke → audit by caller.
//!
//! RPC must never call `global_mcp().call_tool` (phase-1 disabled).

use serde_json::Value;
use tokio_util::sync::CancellationToken;

/// Invoke a registered MCP tool with cancel support.
///
/// Permission/grant must already be enforced by the caller (PermissionGatedTools).
pub async fn invoke_mcp_tool(
    server_id: &str,
    tool_name: &str,
    arguments: Value,
    cancel: &CancellationToken,
) -> Result<Value, String> {
    if cancel.is_cancelled() {
        return Err("mcp call cancelled".into());
    }
    // Ensure server is registered before transport.
    let servers = crate::mcp_runtime::global_mcp().list_servers();
    if !servers.iter().any(|s| s.id == server_id) {
        return Err(format!("MCP server not registered: {server_id}"));
    }
    // Transport is currently blocking; run off the async worker and race cancel.
    let sid = server_id.to_string();
    let tname = tool_name.to_string();
    let invoke = tokio::task::spawn_blocking(move || {
        crate::mcp_runtime::global_mcp().call_tool(&sid, &tname, arguments)
    });
    tokio::pin!(invoke);
    tokio::select! {
        biased;
        _ = cancel.cancelled() => {
            invoke.abort();
            Err("mcp call cancelled".into())
        }
        res = &mut invoke => {
            match res {
                Ok(Ok(v)) => Ok(v),
                Ok(Err(e)) => Err(e),
                Err(e) => Err(format!("mcp invoke join error: {e}")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancel_before_invoke() {
        let c = CancellationToken::new();
        c.cancel();
        let err = invoke_mcp_tool("s", "t", serde_json::json!({}), &c)
            .await
            .unwrap_err();
        assert!(err.contains("cancelled"), "{err}");
    }

    #[tokio::test]
    async fn unregistered_server_rejected() {
        let c = CancellationToken::new();
        let err = invoke_mcp_tool("no-such-server-xyz", "t", serde_json::json!({}), &c)
            .await
            .unwrap_err();
        assert!(err.contains("not registered"), "{err}");
    }
}
