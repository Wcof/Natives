//! Built-in tool implementations for the capability gateway.

use crate::{Tool, SideEffect, PermissionClass, PathScope, ToolHandler, ToolOutput, ToolError};
use std::sync::Arc;

/// Read a file from the filesystem.
pub struct ReadFileTool;
#[async_trait::async_trait]
impl ToolHandler for ReadFileTool {
    async fn execute(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        let path = input.get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError {
                code: "invalid_input".into(), message: "Missing 'path' field".into(), retryable: false,
            })?;
        let content = tokio::fs::read_to_string(path).await
            .map_err(|e| ToolError {
                code: "read_error".into(), message: e.to_string(), retryable: true,
            })?;
        Ok(ToolOutput { result: serde_json::json!({"content": content, "path": path}), truncated: false, duration_ms: 0 })
    }
}

/// Search for files using a glob pattern.
pub struct SearchFilesTool;
#[async_trait::async_trait]
impl ToolHandler for SearchFilesTool {
    async fn execute(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        let pattern = input.get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError {
                code: "invalid_input".into(), message: "Missing 'pattern' field".into(), retryable: false,
            })?;
        let root = input.get("root").and_then(|v| v.as_str()).unwrap_or(".");
        let walker = walkdir::WalkDir::new(root).max_depth(5);
        let results: Vec<String> = walker
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .map(|e| e.path().to_string_lossy().to_string())
            .take(100)
            .collect();
        Ok(ToolOutput { result: serde_json::json!({"files": results, "count": results.len()}), truncated: results.len() >= 100, duration_ms: 0 })
    }
}

/// Write content to a file.
pub struct WriteFileTool;
#[async_trait::async_trait]
impl ToolHandler for WriteFileTool {
    async fn execute(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        let path = input.get("path").and_then(|v| v.as_str())
            .ok_or_else(|| ToolError { code: "invalid_input".into(), message: "Missing 'path'".into(), retryable: false })?;
        let content = input.get("content").and_then(|v| v.as_str())
            .ok_or_else(|| ToolError { code: "invalid_input".into(), message: "Missing 'content'".into(), retryable: false })?;
        tokio::fs::write(path, content).await
            .map_err(|e| ToolError { code: "write_error".into(), message: e.to_string(), retryable: true })?;
        Ok(ToolOutput { result: serde_json::json!({"path": path, "bytes": content.len()}), truncated: false, duration_ms: 0 })
    }
}

/// List directory contents.
pub struct ListDirTool;
#[async_trait::async_trait]
impl ToolHandler for ListDirTool {
    async fn execute(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        let path = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        let mut entries = tokio::fs::read_dir(path).await
            .map_err(|e| ToolError { code: "read_error".into(), message: e.to_string(), retryable: true })?;
        let mut items = Vec::new();
        while let Some(entry) = entries.next_entry().await.map_err(|e| ToolError { code: "read_error".into(), message: e.to_string(), retryable: true })? {
            items.push(serde_json::json!({
                "name": entry.file_name().to_string_lossy(),
                "is_dir": entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false),
            }));
        }
        Ok(ToolOutput { result: serde_json::json!({"path": path, "entries": items, "count": items.len()}), truncated: false, duration_ms: 0 })
    }
}

/// Return all built-in tool definitions.
pub fn builtin_tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "read_file",
            description: "Read the contents of a file",
            schema: serde_json::json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}),
            side_effect: SideEffect::ReadOnly,
            permission_class: PermissionClass::ProjectRead,
            path_scope: PathScope::Any,
            timeout_ms: 10000,
            output_limit: 1_048_576,
            cancellable: true,
            handler: Arc::new(ReadFileTool),
        },
        Tool {
            name: "search_files",
            description: "Search for files matching a pattern",
            schema: serde_json::json!({"type":"object","properties":{"pattern":{"type":"string"},"root":{"type":"string"}},"required":["pattern"]}),
            side_effect: SideEffect::ReadOnly,
            permission_class: PermissionClass::ProjectRead,
            path_scope: PathScope::Any,
            timeout_ms: 30000,
            output_limit: 1_048_576,
            cancellable: true,
            handler: Arc::new(SearchFilesTool),
        },
        Tool {
            name: "write_file",
            description: "Write content to a file",
            schema: serde_json::json!({"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"]}),
            side_effect: SideEffect::Write,
            permission_class: PermissionClass::ProjectWrite,
            path_scope: PathScope::Any,
            timeout_ms: 10000,
            output_limit: 1_048_576,
            cancellable: true,
            handler: Arc::new(WriteFileTool),
        },
        Tool {
            name: "list_directory",
            description: "List contents of a directory",
            schema: serde_json::json!({"type":"object","properties":{"path":{"type":"string"}},"required":[]}),
            side_effect: SideEffect::ReadOnly,
            permission_class: PermissionClass::ProjectRead,
            path_scope: PathScope::Any,
            timeout_ms: 10000,
            output_limit: 1_048_576,
            cancellable: true,
            handler: Arc::new(ListDirTool),
        },
    ]
}