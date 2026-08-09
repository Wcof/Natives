//! Allowlist / disabled / tool-pattern policy for the gated tool runtime.

use agent_core::ToolExecutionResult;
use serde_json::Value;

use super::gated::PermissionGatedTools;

/// Extract the server id from a namespaced `mcp__{server}__{tool}` name.
pub fn mcp_server_of_tool(name: &str) -> Option<&str> {
    let rest = name.strip_prefix("mcp__")?;
    let end = rest.find("__")?;
    Some(&rest[..end])
}

/// Compact human-readable pattern for a tool call (legacy audit trails).
pub fn tool_pattern(name: &str, input: &Value) -> String {
    if name == "run_terminal" {
        input
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    } else {
        String::new()
    }
}

impl PermissionGatedTools {
    pub(crate) fn tool_allowed(&self, name: &str) -> bool {
        // Server whitelist gate first (ADR-0016): with an active MCP selection
        // a namespaced tool outside the selected servers is invisible/denied,
        // regardless of allowlist wildcards.
        if let (Some(selected), Some(server)) =
            (&self.selected_mcp_servers, mcp_server_of_tool(name))
        {
            if !selected.contains(server) {
                return false;
            }
        }
        match &self.tool_allowlist {
            None => true,
            // Matching semantics (including the MCP surface) live in agent-core so
            // enforcement here and child-surface derivation cannot drift apart.
            Some(list) => agent_core::tool_list_allows(list, name),
        }
    }

    pub(crate) fn deny_not_allowlisted(name: &str) -> ToolExecutionResult {
        ToolExecutionResult {
            output: serde_json::json!({
                "error": format!("tool `{name}` not in subagent tool_allowlist"),
                "denied": true,
                "code": "tool_not_allowlisted",
            }),
            is_error: true,
            duration_ms: 0,
        }
    }
}
