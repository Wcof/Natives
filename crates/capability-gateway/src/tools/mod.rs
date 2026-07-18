//! Built-in tool implementations for the capability gateway.

mod extra;

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
            .filter(|p| crate::policy::glob_matches_public(p, pattern)
                || p.contains(pattern)
                || std::path::Path::new(p)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.contains(pattern) || crate::policy::glob_matches_public(n, pattern))
                    .unwrap_or(false))
            .take(100)
            .collect();
        Ok(ToolOutput { result: serde_json::json!({"files": results, "count": results.len(), "pattern": pattern}), truncated: results.len() >= 100, duration_ms: 0 })
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

/// Grep for a pattern under a root directory (bounded).
pub struct GrepTool;
#[async_trait::async_trait]
impl ToolHandler for GrepTool {
    async fn execute(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        let pattern = input
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError {
                code: "invalid_input".into(),
                message: "Missing 'pattern'".into(),
                retryable: false,
            })?;
        let root = input.get("root").and_then(|v| v.as_str()).unwrap_or(".");
        let re = regex::Regex::new(pattern).map_err(|e| ToolError {
            code: "invalid_regex".into(),
            message: e.to_string(),
            retryable: false,
        })?;
        let mut matches = Vec::new();
        for entry in walkdir::WalkDir::new(root).max_depth(6).into_iter().filter_map(|e| e.ok()) {
            if !entry.file_type().is_file() {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                for (idx, line) in content.lines().enumerate() {
                    if re.is_match(line) {
                        matches.push(serde_json::json!({
                            "path": entry.path().to_string_lossy(),
                            "line": idx + 1,
                            "text": line.chars().take(240).collect::<String>(),
                        }));
                        if matches.len() >= 50 {
                            break;
                        }
                    }
                }
            }
            if matches.len() >= 50 {
                break;
            }
        }
        Ok(ToolOutput {
            result: serde_json::json!({"matches": matches, "count": matches.len()}),
            truncated: matches.len() >= 50,
            duration_ms: 0,
        })
    }
}

/// Apply a simple string replace edit.
pub struct EditFileTool;
#[async_trait::async_trait]
impl ToolHandler for EditFileTool {
    async fn execute(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        let path = input.get("path").and_then(|v| v.as_str()).ok_or_else(|| ToolError {
            code: "invalid_input".into(),
            message: "Missing path".into(),
            retryable: false,
        })?;
        let old = input.get("old_string").and_then(|v| v.as_str()).ok_or_else(|| ToolError {
            code: "invalid_input".into(),
            message: "Missing old_string".into(),
            retryable: false,
        })?;
        let new = input.get("new_string").and_then(|v| v.as_str()).ok_or_else(|| ToolError {
            code: "invalid_input".into(),
            message: "Missing new_string".into(),
            retryable: false,
        })?;
        let content = tokio::fs::read_to_string(path).await.map_err(|e| ToolError {
            code: "read_error".into(),
            message: e.to_string(),
            retryable: true,
        })?;
        if !content.contains(old) {
            return Err(ToolError {
                code: "not_found".into(),
                message: "old_string not found".into(),
                retryable: false,
            });
        }
        let updated = content.replacen(old, new, 1);
        tokio::fs::write(path, &updated).await.map_err(|e| ToolError {
            code: "write_error".into(),
            message: e.to_string(),
            retryable: true,
        })?;
        Ok(ToolOutput {
            result: serde_json::json!({"path": path, "replaced": true}),
            truncated: false,
            duration_ms: 0,
        })
    }
}

/// Run a terminal command (argv-safe: command + args array only).
pub struct RunTerminalTool;
#[async_trait::async_trait]
impl ToolHandler for RunTerminalTool {
    async fn execute(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        let command = input.get("command").and_then(|v| v.as_str()).ok_or_else(|| ToolError {
            code: "invalid_input".into(),
            message: "Missing command".into(),
            retryable: false,
        })?;
        // Reject shell metacharacters — callers must pass simple command name.
        if command.contains(['|', ';', '&', '`', '$', '\n', '>', '<']) {
            return Err(ToolError {
                code: "shell_injection".into(),
                message: "Shell metacharacters rejected; pass argv-safe command only".into(),
                retryable: false,
            });
        }
        let args: Vec<String> = input
            .get("args")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        // Prefer explicit cwd; callers (PermissionGatedTools) inject project_root.
        let cwd = input.get("cwd").and_then(|v| v.as_str());
        if let Some(cwd) = cwd {
            crate::policy::check_path_traversal(cwd)?;
        }
        let mut cmd = tokio::process::Command::new(command);
        cmd.args(&args).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
        if let Some(cwd) = cwd {
            cmd.current_dir(cwd);
        } else {
            // No cwd and no project injection → refuse open-ended shell.
            return Err(ToolError {
                code: "cwd_required".into(),
                message: "run_terminal requires cwd within project scope".into(),
                retryable: false,
            });
        }
        let output = tokio::time::timeout(std::time::Duration::from_secs(30), cmd.output())
            .await
            .map_err(|_| ToolError {
                code: "timeout".into(),
                message: "command timed out".into(),
                retryable: true,
            })?
            .map_err(|e| ToolError {
                code: "spawn_error".into(),
                message: e.to_string(),
                retryable: true,
            })?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let mut combined = stdout.to_string();
        if !stderr.is_empty() {
            combined.push_str("\n");
            combined.push_str(&stderr);
        }
        let truncated = combined.len() > 64_000;
        if truncated {
            combined.truncate(64_000);
        }
        Ok(ToolOutput {
            result: serde_json::json!({
                "exit_code": output.status.code(),
                "output": combined,
                "truncated": truncated,
            }),
            truncated,
            duration_ms: 0,
        })
    }
}

/// Fetch a URL (basic SSRF blocklist).
pub struct WebFetchTool;
#[async_trait::async_trait]
impl ToolHandler for WebFetchTool {
    async fn execute(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        let url = input.get("url").and_then(|v| v.as_str()).ok_or_else(|| ToolError {
            code: "invalid_input".into(),
            message: "Missing url".into(),
            retryable: false,
        })?;
        if url.contains("127.0.0.1") || url.contains("localhost") || url.contains("169.254.169.254") {
            return Err(ToolError {
                code: "ssrf".into(),
                message: "Blocked local/metadata URL".into(),
                retryable: false,
            });
        }
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .build()
            .map_err(|e| ToolError {
                code: "http_client".into(),
                message: e.to_string(),
                retryable: true,
            })?;
        let resp = client.get(url).send().await.map_err(|e| ToolError {
            code: "network".into(),
            message: e.to_string(),
            retryable: true,
        })?;
        let status = resp.status().as_u16();
        let mut body = resp.text().await.unwrap_or_default();
        let truncated = body.len() > 64_000;
        if truncated {
            body.truncate(64_000);
        }
        Ok(ToolOutput {
            result: serde_json::json!({ "status": status, "body": body, "truncated": truncated }),
            truncated,
            duration_ms: 0,
        })
    }
}

/// Simple in-memory todo write for the agent loop.
pub struct TodoWriteTool;
#[async_trait::async_trait]
impl ToolHandler for TodoWriteTool {
    async fn execute(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        Ok(ToolOutput {
            result: serde_json::json!({ "ok": true, "todos": input.get("todos").cloned().unwrap_or(serde_json::json!([])) }),
            truncated: false,
            duration_ms: 0,
        })
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
            name: "list_dir",
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
        Tool {
            name: "grep",
            description: "Search file contents with a regex",
            schema: serde_json::json!({"type":"object","properties":{"pattern":{"type":"string"},"root":{"type":"string"}},"required":["pattern"]}),
            side_effect: SideEffect::ReadOnly,
            permission_class: PermissionClass::ProjectRead,
            path_scope: PathScope::Any,
            timeout_ms: 30000,
            output_limit: 1_048_576,
            cancellable: true,
            handler: Arc::new(GrepTool),
        },
        Tool {
            name: "edit_file",
            description: "Replace the first occurrence of old_string with new_string in a file",
            schema: serde_json::json!({"type":"object","properties":{"path":{"type":"string"},"old_string":{"type":"string"},"new_string":{"type":"string"}},"required":["path","old_string","new_string"]}),
            side_effect: SideEffect::Write,
            permission_class: PermissionClass::ProjectWrite,
            path_scope: PathScope::Any,
            timeout_ms: 10000,
            output_limit: 1_048_576,
            cancellable: true,
            handler: Arc::new(EditFileTool),
        },
        Tool {
            name: "run_terminal",
            description: "Run a command with argv (no shell)",
            schema: serde_json::json!({"type":"object","properties":{"command":{"type":"string"},"args":{"type":"array","items":{"type":"string"}},"cwd":{"type":"string"}},"required":["command"]}),
            side_effect: SideEffect::Process,
            permission_class: PermissionClass::DestructiveCommand,
            path_scope: PathScope::Any,
            timeout_ms: 30000,
            output_limit: 64_000,
            cancellable: true,
            handler: Arc::new(RunTerminalTool),
        },
        Tool {
            name: "web_fetch",
            description: "Fetch a public HTTP(S) URL",
            schema: serde_json::json!({"type":"object","properties":{"url":{"type":"string"}},"required":["url"]}),
            side_effect: SideEffect::Network,
            permission_class: PermissionClass::ExternalWrite,
            path_scope: PathScope::None,
            timeout_ms: 20000,
            output_limit: 64_000,
            cancellable: true,
            handler: Arc::new(WebFetchTool),
        },
        Tool {
            name: "todo_write",
            description: "Update the agent todo list",
            schema: serde_json::json!({"type":"object","properties":{"todos":{"type":"array"}},"required":["todos"]}),
            side_effect: SideEffect::Write,
            permission_class: PermissionClass::AlwaysAllowed,
            path_scope: PathScope::None,
            timeout_ms: 5000,
            output_limit: 16_000,
            cancellable: true,
            handler: Arc::new(TodoWriteTool),
        },
    ]
    .into_iter()
    .chain(extra::extra_builtin_tools())
    .collect()
}