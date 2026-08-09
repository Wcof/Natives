//! Agent todo-list update tool.

use crate::{ToolCallContext, ToolError, ToolHandler, ToolOutput};

/// Simple in-memory todo write for the agent loop.
pub struct TodoWriteTool;
#[async_trait::async_trait]
impl ToolHandler for TodoWriteTool {
    async fn execute(
        &self,
        input: serde_json::Value,
        _context: &ToolCallContext,
    ) -> Result<ToolOutput, ToolError> {
        Ok(ToolOutput {
            result: serde_json::json!({ "ok": true, "todos": input.get("todos").cloned().unwrap_or(serde_json::json!([])) }),
            truncated: false,
            duration_ms: 0,
        })
    }
}
