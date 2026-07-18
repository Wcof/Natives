//! Additional built-in tools required by the Native engine plan.

use crate::{PermissionClass, PathScope, SideEffect, Tool, ToolError, ToolHandler, ToolOutput};
use std::sync::Arc;

/// Apply a unified-diff style patch (simplified: path + content replacement).
pub struct ApplyPatchTool;
#[async_trait::async_trait]
impl ToolHandler for ApplyPatchTool {
    async fn execute(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        let path = input.get("path").and_then(|v| v.as_str()).ok_or_else(|| ToolError {
            code: "invalid_input".into(),
            message: "Missing path".into(),
            retryable: false,
        })?;
        if path.contains("..") {
            return Err(ToolError {
                code: "path_escape".into(),
                message: "Path traversal rejected".into(),
                retryable: false,
            });
        }
        let content = input
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError {
                code: "invalid_input".into(),
                message: "Missing content".into(),
                retryable: false,
            })?;
        tokio::fs::write(path, content).await.map_err(|e| ToolError {
            code: "write_error".into(),
            message: e.to_string(),
            retryable: true,
        })?;
        Ok(ToolOutput {
            result: serde_json::json!({ "path": path, "bytes": content.len() }),
            truncated: false,
            duration_ms: 0,
        })
    }
}

/// File-backed session/workspace memory under NATIVES_RUNTIME_DIR/memory
/// (shared path convention with Agent Daemon memory_store).
pub struct MemoryTool;
#[async_trait::async_trait]
impl ToolHandler for MemoryTool {
    async fn execute(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        let op = input.get("op").and_then(|v| v.as_str()).unwrap_or("get");
        let start = std::time::Instant::now();
        match op {
            "put" | "add" => {
                let text = input
                    .get("text")
                    .or_else(|| input.get("value"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let key = input
                    .get("key")
                    .and_then(|v| v.as_str())
                    .unwrap_or("note")
                    .to_string();
                if text.trim().is_empty() {
                    return Err(ToolError {
                        code: "invalid_input".into(),
                        message: "text required for memory put".into(),
                        retryable: false,
                    });
                }
                let id = memory_file_put(&key, &text)?;
                Ok(ToolOutput {
                    result: serde_json::json!({ "ok": true, "id": id, "key": key }),
                    truncated: false,
                    duration_ms: start.elapsed().as_millis() as u64,
                })
            }
            "search" => {
                let query = input
                    .get("query")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let matches = memory_file_search(&query, 20);
                Ok(ToolOutput {
                    result: serde_json::json!({ "matches": matches, "query": query }),
                    truncated: false,
                    duration_ms: start.elapsed().as_millis() as u64,
                })
            }
            _ => {
                let key = input.get("key").and_then(|v| v.as_str()).unwrap_or("");
                let value = memory_file_get(key);
                Ok(ToolOutput {
                    result: serde_json::json!({ "value": value, "key": key }),
                    truncated: false,
                    duration_ms: start.elapsed().as_millis() as u64,
                })
            }
        }
    }
}

fn memory_dir() -> std::path::PathBuf {
    std::env::var("NATIVES_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(|h| std::path::PathBuf::from(h).join(".natives").join("runtime"))
                .unwrap_or_else(|| std::env::temp_dir().join("natives-runtime"))
        })
        .join("memory")
}

fn memory_file_put(key: &str, text: &str) -> Result<String, ToolError> {
    let dir = memory_dir();
    std::fs::create_dir_all(&dir).map_err(|e| ToolError {
        code: "io".into(),
        message: e.to_string(),
        retryable: true,
    })?;
    let id = uuid::Uuid::new_v4().to_string();
    let path = dir.join("entries.jsonl");
    let line = serde_json::json!({
        "id": id,
        "key": key,
        "text": text,
        "created_at": chrono::Utc::now().to_rfc3339(),
    });
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| ToolError {
            code: "io".into(),
            message: e.to_string(),
            retryable: true,
        })?;
    writeln!(f, "{line}").map_err(|e| ToolError {
        code: "io".into(),
        message: e.to_string(),
        retryable: true,
    })?;
    Ok(id)
}

fn memory_file_search(query: &str, limit: usize) -> Vec<serde_json::Value> {
    let path = memory_dir().join("entries.jsonl");
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let q = query.to_ascii_lowercase();
    let terms: Vec<&str> = q.split_whitespace().filter(|t| !t.is_empty()).collect();
    let mut hits = Vec::new();
    for line in raw.lines().rev() {
        if line.trim().is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let text = v
            .get("text")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let key = v
            .get("key")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let hay = format!("{key} {text}");
        let score = if terms.is_empty() {
            0.1
        } else {
            terms.iter().filter(|t| hay.contains(*t)).count() as f64
        };
        if score > 0.0 {
            hits.push(serde_json::json!({ "entry": v, "score": score }));
        }
        if hits.len() >= limit {
            break;
        }
    }
    hits
}

fn memory_file_get(key: &str) -> Option<serde_json::Value> {
    if key.is_empty() {
        return None;
    }
    let path = memory_dir().join("entries.jsonl");
    let Ok(raw) = std::fs::read_to_string(path) else {
        return None;
    };
    for line in raw.lines().rev() {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
            if v.get("key").and_then(|k| k.as_str()) == Some(key) {
                return Some(v);
            }
        }
    }
    None
}

#[cfg(test)]
mod memory_tool_tests {
    use super::*;

    #[tokio::test]
    async fn put_and_search_round_trip() {
        let dir = std::env::temp_dir().join(format!("gw-mem-{}", uuid::Uuid::new_v4()));
        std::env::set_var("NATIVES_RUNTIME_DIR", &dir);
        let tool = MemoryTool;
        let put = tool
            .execute(serde_json::json!({
                "op": "put",
                "key": "deploy",
                "text": "deploy token rotated weekly"
            }))
            .await
            .unwrap();
        assert_eq!(put.result["ok"], true);
        let search = tool
            .execute(serde_json::json!({
                "op": "search",
                "query": "deploy token"
            }))
            .await
            .unwrap();
        let matches = search.result["matches"].as_array().unwrap();
        assert!(!matches.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
        std::env::remove_var("NATIVES_RUNTIME_DIR");
    }
}

/// Task tools are orchestrated by Agent Daemon `PermissionGatedTools` (real child
/// runs with independent provider/key/model). These gateway registrations only
/// expose schema for the model; bare execute without orchestrator is an error.
pub struct TaskTool;
#[async_trait::async_trait]
impl ToolHandler for TaskTool {
    async fn execute(&self, _input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        Err(ToolError {
            code: "orchestrator_required".into(),
            message: "task requires Agent Daemon PermissionGatedTools (real SubAgent child run)"
                .into(),
            retryable: false,
        })
    }
}

pub struct TaskOutputTool;
#[async_trait::async_trait]
impl ToolHandler for TaskOutputTool {
    async fn execute(&self, _input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        Err(ToolError {
            code: "orchestrator_required".into(),
            message: "task_output requires Agent Daemon PermissionGatedTools".into(),
            retryable: false,
        })
    }
}

pub struct KillTaskTool;
#[async_trait::async_trait]
impl ToolHandler for KillTaskTool {
    async fn execute(&self, _input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        Err(ToolError {
            code: "orchestrator_required".into(),
            message: "kill_task requires Agent Daemon PermissionGatedTools".into(),
            retryable: false,
        })
    }
}

pub struct NotificationTool;
#[async_trait::async_trait]
impl ToolHandler for NotificationTool {
    async fn execute(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        Ok(ToolOutput {
            result: serde_json::json!({
                "delivered": true,
                "message": input.get("message").and_then(|v| v.as_str()).unwrap_or("")
            }),
            truncated: false,
            duration_ms: 0,
        })
    }
}

pub struct SkillTool;
#[async_trait::async_trait]
impl ToolHandler for SkillTool {
    async fn execute(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        Ok(ToolOutput {
            result: serde_json::json!({
                "skill": input.get("name"),
                "loaded": false,
                "note": "Skill loader resolves from .claude/skills and .grok/skills"
            }),
            truncated: false,
            duration_ms: 0,
        })
    }
}

pub struct McpCallTool;
#[async_trait::async_trait]
impl ToolHandler for McpCallTool {
    async fn execute(&self, input: serde_json::Value) -> Result<ToolOutput, ToolError> {
        Ok(ToolOutput {
            result: serde_json::json!({
                "server": input.get("server"),
                "tool": input.get("tool"),
                "ok": false,
                "error": "MCP session not connected"
            }),
            truncated: false,
            duration_ms: 0,
        })
    }
}

pub fn extra_builtin_tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "apply_patch",
            description: "Write full file content as a patch application",
            schema: serde_json::json!({"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"]}),
            side_effect: SideEffect::Write,
            permission_class: PermissionClass::ProjectWrite,
            path_scope: PathScope::Any,
            timeout_ms: 15_000,
            output_limit: 1_048_576,
            cancellable: true,
            handler: Arc::new(ApplyPatchTool),
        },
        Tool {
            name: "memory_search",
            description: "Search session memory",
            schema: serde_json::json!({"type":"object","properties":{"query":{"type":"string"},"op":{"type":"string"}},"required":["query"]}),
            side_effect: SideEffect::ReadOnly,
            permission_class: PermissionClass::AlwaysAllowed,
            path_scope: PathScope::None,
            timeout_ms: 5_000,
            output_limit: 64_000,
            cancellable: true,
            handler: Arc::new(MemoryTool),
        },
        Tool {
            name: "memory_get",
            description: "Get a memory value by key",
            schema: serde_json::json!({"type":"object","properties":{"key":{"type":"string"},"op":{"type":"string"}},"required":["key"]}),
            side_effect: SideEffect::ReadOnly,
            permission_class: PermissionClass::AlwaysAllowed,
            path_scope: PathScope::None,
            timeout_ms: 5_000,
            output_limit: 64_000,
            cancellable: true,
            handler: Arc::new(MemoryTool),
        },
        Tool {
            name: "task",
            description: "Spawn a subagent task with independent identity",
            schema: serde_json::json!({
                "type":"object",
                "properties":{
                    "prompt":{"type":"string","description":"Task prompt for the subagent"},
                    "task":{"type":"string","description":"Alias for prompt"},
                    "provider_id":{"type":"string","description":"Provider id for the child run"},
                    "key_id":{"type":"string","description":"Credential key id (required for independent identity)"},
                    "model_id":{"type":"string","description":"Model id for the child run"},
                    "permission_profile":{"type":"string","description":"ask | full_access"}
                },
                "required":["prompt"]
            }),
            side_effect: SideEffect::Process,
            permission_class: PermissionClass::ProjectWrite,
            path_scope: PathScope::None,
            timeout_ms: 60_000,
            output_limit: 64_000,
            cancellable: true,
            handler: Arc::new(TaskTool),
        },
        Tool {
            name: "task_output",
            description: "Read subagent task output",
            schema: serde_json::json!({"type":"object","properties":{"task_id":{"type":"string"}},"required":["task_id"]}),
            side_effect: SideEffect::ReadOnly,
            permission_class: PermissionClass::AlwaysAllowed,
            path_scope: PathScope::None,
            timeout_ms: 10_000,
            output_limit: 256_000,
            cancellable: true,
            handler: Arc::new(TaskOutputTool),
        },
        Tool {
            name: "kill_task",
            description: "Cancel a subagent task",
            schema: serde_json::json!({"type":"object","properties":{"task_id":{"type":"string"}},"required":["task_id"]}),
            side_effect: SideEffect::Process,
            permission_class: PermissionClass::ProjectWrite,
            path_scope: PathScope::None,
            timeout_ms: 5_000,
            output_limit: 8_000,
            cancellable: true,
            handler: Arc::new(KillTaskTool),
        },
        Tool {
            name: "skill",
            description: "Load a skill by name",
            schema: serde_json::json!({"type":"object","properties":{"name":{"type":"string"}},"required":["name"]}),
            side_effect: SideEffect::ReadOnly,
            permission_class: PermissionClass::AlwaysAllowed,
            path_scope: PathScope::None,
            timeout_ms: 5_000,
            output_limit: 64_000,
            cancellable: true,
            handler: Arc::new(SkillTool),
        },
        Tool {
            name: "mcp_call",
            description: "Call an MCP tool on a registered server",
            schema: serde_json::json!({"type":"object","properties":{"server":{"type":"string"},"tool":{"type":"string"},"arguments":{"type":"object"}},"required":["server","tool"]}),
            side_effect: SideEffect::Network,
            permission_class: PermissionClass::ExternalWrite,
            path_scope: PathScope::None,
            timeout_ms: 30_000,
            output_limit: 256_000,
            cancellable: true,
            handler: Arc::new(McpCallTool),
        },
        Tool {
            name: "notification",
            description: "Emit a user-visible notification",
            schema: serde_json::json!({"type":"object","properties":{"message":{"type":"string"}},"required":["message"]}),
            side_effect: SideEffect::Write,
            permission_class: PermissionClass::AlwaysAllowed,
            path_scope: PathScope::None,
            timeout_ms: 2_000,
            output_limit: 4_000,
            cancellable: true,
            handler: Arc::new(NotificationTool),
        },
    ]
}
