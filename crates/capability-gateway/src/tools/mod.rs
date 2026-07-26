//! Built-in tool implementations for the capability gateway.

mod apply_patch_parser;
pub mod creative_draft;
mod extra;
pub mod plan;
mod ssrf;
pub mod web_search;

pub use apply_patch_parser::{parse_patch_input, PatchOp};
pub use creative_draft::{creative_draft_tools, CREATIVE_DRAFT_TOOL_NAMES};
pub use plan::plan_mode_tools;
pub use ssrf::validate_fetch_url;
pub use web_search::{web_search_tool, SearchBackend, SearchProvider};

use crate::{
    Tool, SideEffect, PermissionClass, PathScope, ToolHandler, ToolOutput, ToolError, ToolCallContext,
    ProcessSupervisor,
};
use std::sync::Arc;

/// Read a file from the filesystem (offset/limit, binary-safe metadata).
pub struct ReadFileTool;
#[async_trait::async_trait]
impl ToolHandler for ReadFileTool {
    async fn execute(
        &self,
        input: serde_json::Value,
        context: &ToolCallContext,
    ) -> Result<ToolOutput, ToolError> {
        let path_str = input
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError {
                code: "invalid_input".into(),
                message: "Missing 'path' field".into(),
                retryable: false,
            })?;
        let path = context.resolve_path(path_str)?;
        let offset = input
            .get("offset")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;
        let limit = input
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize);
        let max_chars = input
            .get("max_chars")
            .and_then(|v| v.as_u64())
            .unwrap_or(200_000) as usize;

        let meta = tokio::fs::metadata(&path).await.map_err(|e| ToolError {
            code: "read_error".into(),
            message: e.to_string(),
            retryable: true,
        })?;
        let size = meta.len();

        // Sample head for binary detection
        let mut file = tokio::fs::File::open(&path).await.map_err(|e| ToolError {
            code: "read_error".into(),
            message: e.to_string(),
            retryable: true,
        })?;
        use tokio::io::AsyncReadExt;
        let mut head = vec![0u8; 8192.min(size as usize)];
        let n = file.read(&mut head).await.map_err(|e| ToolError {
            code: "read_error".into(),
            message: e.to_string(),
            retryable: true,
        })?;
        head.truncate(n);
        let is_binary = head.iter().any(|&b| b == 0);
        if is_binary {
            return Ok(ToolOutput {
                result: serde_json::json!({
                    "path": path.to_string_lossy(),
                    "binary": true,
                    "size": size,
                    "content": null,
                    "message": "binary file; content omitted",
                }),
                truncated: false,
                duration_ms: 0,
            });
        }

        let full = tokio::fs::read_to_string(&path).await.map_err(|e| ToolError {
            code: "read_error".into(),
            message: e.to_string(),
            retryable: true,
        })?;
        let lines: Vec<&str> = full.lines().collect();
        let total_lines = lines.len();
        let start = offset.min(total_lines);
        let end = limit
            .map(|l| (start + l).min(total_lines))
            .unwrap_or(total_lines);
        let mut slice = lines[start..end].join("\n");
        let mut truncated = end < total_lines;
        if slice.len() > max_chars {
            slice.truncate(max_chars);
            truncated = true;
        }
        Ok(ToolOutput {
            result: serde_json::json!({
                "path": path.to_string_lossy(),
                "binary": false,
                "size": size,
                "total_lines": total_lines,
                "offset": start,
                "limit": end.saturating_sub(start),
                "content": slice,
                "truncated": truncated,
            }),
            truncated,
            duration_ms: 0,
        })
    }
}

/// Search for files using a glob pattern.
pub struct SearchFilesTool;
#[async_trait::async_trait]
impl ToolHandler for SearchFilesTool {
    async fn execute(
        &self,
        input: serde_json::Value,
        _context: &ToolCallContext,
    ) -> Result<ToolOutput, ToolError> {
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
    async fn execute(
        &self,
        input: serde_json::Value,
        _context: &ToolCallContext,
    ) -> Result<ToolOutput, ToolError> {
        let path = input.get("path").and_then(|v| v.as_str())
            .ok_or_else(|| ToolError { code: "invalid_input".into(), message: "Missing 'path'".into(), retryable: false })?;
        let content = input.get("content").and_then(|v| v.as_str())
            .ok_or_else(|| ToolError { code: "invalid_input".into(), message: "Missing 'content'".into(), retryable: false })?;
        tokio::fs::write(path, content).await
            .map_err(|e| ToolError { code: "write_error".into(), message: e.to_string(), retryable: true })?;
        Ok(ToolOutput { result: serde_json::json!({"path": path, "bytes": content.len()}), truncated: false, duration_ms: 0 })
    }
}

/// List directory contents (stable sort, cap + continuation).
pub struct ListDirTool;
#[async_trait::async_trait]
impl ToolHandler for ListDirTool {
    async fn execute(
        &self,
        input: serde_json::Value,
        _context: &ToolCallContext,
    ) -> Result<ToolOutput, ToolError> {
        let path = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        let limit = input
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(200) as usize;
        let cursor = input
            .get("cursor")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let mut entries = tokio::fs::read_dir(path).await.map_err(|e| ToolError {
            code: "read_error".into(),
            message: e.to_string(),
            retryable: true,
        })?;
        let mut items = Vec::new();
        while let Some(entry) = entries.next_entry().await.map_err(|e| ToolError {
            code: "read_error".into(),
            message: e.to_string(),
            retryable: true,
        })? {
            let name = entry.file_name().to_string_lossy().to_string();
            let is_dir = entry
                .file_type()
                .await
                .map(|t| t.is_dir())
                .unwrap_or(false);
            items.push((name, is_dir));
        }
        items.sort_by(|a, b| a.0.cmp(&b.0));
        let start = if cursor.is_empty() {
            0
        } else {
            items
                .iter()
                .position(|(n, _)| n.as_str() > cursor)
                .unwrap_or(items.len())
        };
        let end = (start + limit).min(items.len());
        let page: Vec<serde_json::Value> = items[start..end]
            .iter()
            .map(|(name, is_dir)| serde_json::json!({"name": name, "is_dir": is_dir}))
            .collect();
        let next_cursor = if end < items.len() {
            Some(items[end - 1].0.clone())
        } else {
            None
        };
        Ok(ToolOutput {
            result: serde_json::json!({
                "path": path,
                "entries": page,
                "count": page.len(),
                "total": items.len(),
                "next_cursor": next_cursor,
                "truncated": next_cursor.is_some(),
            }),
            truncated: next_cursor.is_some(),
            duration_ms: 0,
        })
    }
}

/// Grep for a pattern under a root directory.
/// Prefers bundled/system `rg --json` when available; falls back to walkdir+regex.
pub struct GrepTool;
#[async_trait::async_trait]
impl ToolHandler for GrepTool {
    async fn execute(
        &self,
        input: serde_json::Value,
        _context: &ToolCallContext,
    ) -> Result<ToolOutput, ToolError> {
        let pattern = input
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError {
                code: "invalid_input".into(),
                message: "Missing 'pattern'".into(),
                retryable: false,
            })?;
        let root = input.get("root").and_then(|v| v.as_str()).unwrap_or(".");
        let glob = input.get("glob").and_then(|v| v.as_str());
        if let Some(result) = try_ripgrep_json(pattern, root, glob).await {
            return result;
        }
        // Fallback: pure Rust
        let re = regex::Regex::new(pattern).map_err(|e| ToolError {
            code: "invalid_regex".into(),
            message: e.to_string(),
            retryable: false,
        })?;
        let mut matches = Vec::new();
        for entry in walkdir::WalkDir::new(root)
            .max_depth(8)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                for (idx, line) in content.lines().enumerate() {
                    if re.is_match(line) {
                        matches.push(serde_json::json!({
                            "path": entry.path().to_string_lossy(),
                            "line": idx + 1,
                            "column": 1,
                            "text": line.chars().take(240).collect::<String>(),
                        }));
                        if matches.len() >= 200 {
                            break;
                        }
                    }
                }
            }
            if matches.len() >= 200 {
                break;
            }
        }
        Ok(ToolOutput {
            result: serde_json::json!({
                "matches": matches,
                "count": matches.len(),
                "engine": "regex_fallback",
                "truncated": matches.len() >= 200,
            }),
            truncated: matches.len() >= 200,
            duration_ms: 0,
        })
    }
}

async fn try_ripgrep_json(
    pattern: &str,
    root: &str,
    glob: Option<&str>,
) -> Option<Result<ToolOutput, ToolError>> {
    let mut cmd = tokio::process::Command::new("rg");
    cmd.arg("--json")
        .arg("--line-number")
        .arg("--no-heading")
        .arg("--color=never")
        .arg("--max-count")
        .arg("200")
        .arg("--")
        .arg(pattern)
        .arg(root);
    if let Some(g) = glob {
        cmd.arg("--glob").arg(g);
    }
    let output = cmd.output().await.ok()?;
    // rg exits 1 when no matches — still success for us
    if !output.status.success() && output.status.code() != Some(1) {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut matches = Vec::new();
    for line in stdout.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if v.get("type").and_then(|t| t.as_str()) != Some("match") {
            continue;
        }
        let data = v.get("data")?;
        let path = data
            .pointer("/path/text")
            .and_then(|p| p.as_str())
            .unwrap_or("");
        let line_no = data.get("line_number").and_then(|n| n.as_u64()).unwrap_or(0);
        let text = data
            .pointer("/lines/text")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .trim_end_matches('\n')
            .chars()
            .take(240)
            .collect::<String>();
        let column = data
            .pointer("/submatches/0/start")
            .and_then(|c| c.as_u64())
            .unwrap_or(0)
            + 1;
        matches.push(serde_json::json!({
            "path": path,
            "line": line_no,
            "column": column,
            "text": text,
        }));
        if matches.len() >= 200 {
            break;
        }
    }
    Some(Ok(ToolOutput {
        result: serde_json::json!({
            "matches": matches,
            "count": matches.len(),
            "engine": "ripgrep",
            "truncated": matches.len() >= 200,
        }),
        truncated: matches.len() >= 200,
        duration_ms: 0,
    }))
}

/// Apply a simple string replace edit.
pub struct EditFileTool;
#[async_trait::async_trait]
impl ToolHandler for EditFileTool {
    async fn execute(
        &self,
        input: serde_json::Value,
        _context: &ToolCallContext,
    ) -> Result<ToolOutput, ToolError> {
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

/// Run a terminal command via [`LocalProcessSupervisor`] (no orphan `mem::forget`).
///
/// Preferred schema (Phase 1):
/// `{ "command": "string", "cwd": "relative?", "timeout_ms": 300000, "background": false, "description": "..." }`
///
/// Also accepts legacy argv form: `{ "command", "args": [], "cwd" }`.
pub struct RunTerminalTool;
#[async_trait::async_trait]
impl ToolHandler for RunTerminalTool {
    async fn execute(
        &self,
        input: serde_json::Value,
        context: &ToolCallContext,
    ) -> Result<ToolOutput, ToolError> {
        if context.cancel.is_cancelled() {
            return Err(ToolError {
                code: "cancelled".into(),
                message: "run cancelled before terminal spawn".into(),
                retryable: false,
            });
        }
        let command = input
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError {
                code: "invalid_input".into(),
                message: "Missing command".into(),
                retryable: false,
            })?;
        let timeout_ms = input
            .get("timeout_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(300_000)
            .clamp(1_000, 600_000);
        let background = input
            .get("background")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let description = input
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let cwd_input = input.get("cwd").and_then(|v| v.as_str());
        if let Some(cwd) = cwd_input {
            crate::policy::check_path_traversal(cwd)?;
        } else {
            return Err(ToolError {
                code: "cwd_required".into(),
                message: "run_terminal requires cwd within project scope".into(),
                retryable: false,
            });
        }

        // Legacy argv-only path when `args` is present.
        let (program, args, display) = if let Some(arr) = input.get("args").and_then(|v| v.as_array())
        {
            if command.contains(['|', ';', '&', '`', '$', '\n', '>', '<']) {
                return Err(ToolError {
                    code: "shell_injection".into(),
                    message: "Shell metacharacters rejected for argv mode".into(),
                    retryable: false,
                });
            }
            let args: Vec<String> = arr
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect();
            let display = format!("{command} {}", args.join(" "));
            (command.to_string(), args, display)
        } else {
            // Shell form — real user command for permission cards.
            let (shell, mut prefix) = crate::process_supervisor::platform_shell_program();
            prefix.push(command.to_string());
            let display = command.to_string();
            let program = shell;
            let args = prefix;
            (program, args, display)
        };

        let cwd = crate::process_supervisor::resolve_cwd(&context.project_root, cwd_input).map_err(
            |e| ToolError {
                code: "cwd_invalid".into(),
                message: e,
                retryable: false,
            },
        )?;

        let task_id = uuid::Uuid::new_v4().to_string();
        let supervisor = crate::global_process_supervisor();
        let started = std::time::Instant::now();
        let spec = crate::ProcessSpec {
            run_id: context.run_id.clone(),
            task_id: task_id.clone(),
            display_command: display.clone(),
            program,
            args,
            cwd,
            timeout_ms,
            background,
        };

        // Race spawn against run cancel; if cancelled mid-foreground, force kill.
        let spawn_fut = supervisor.spawn(spec);
        let snap = tokio::select! {
            biased;
            _ = context.cancel.cancelled() => {
                return Err(ToolError {
                    code: "cancelled".into(),
                    message: "run cancelled during terminal spawn".into(),
                    retryable: false,
                });
            }
            res = spawn_fut => res.map_err(|e| ToolError {
                code: "spawn_error".into(),
                message: e,
                retryable: true,
            })?,
        };

        use crate::ProcessState;
        match snap.state {
            ProcessState::Completed | ProcessState::Failed => Ok(terminal_result(
                display,
                &description,
                snap.exit_code,
                &snap.stdout_tail,
                &snap.stderr_tail,
                snap.truncated,
                snap.background,
                started.elapsed().as_millis() as u64,
            )),
            ProcessState::Cancelled => Err(ToolError {
                code: "cancelled".into(),
                message: "terminal process cancelled".into(),
                retryable: false,
            }),
            ProcessState::Background | ProcessState::Running => {
                // Supervised background — no mem::forget; cancel tree can kill via task_id.
                Ok(ToolOutput {
                    result: serde_json::json!({
                        "display_command": display,
                        "description": description,
                        "exit_code": snap.exit_code,
                        "output": format!("{}{}", snap.stdout_tail, snap.stderr_tail),
                        "stdout": snap.stdout_tail,
                        "stderr": snap.stderr_tail,
                        "background": true,
                        "auto_backgrounded": !background,
                        "task_id": task_id,
                        "truncated": snap.truncated,
                        "message": if background {
                            "process running under process supervisor"
                        } else {
                            "foreground budget exceeded; process continued under supervisor"
                        },
                    }),
                    truncated: snap.truncated,
                    duration_ms: started.elapsed().as_millis() as u64,
                })
            }
        }
    }
}

fn terminal_result(
    display: String,
    description: &str,
    exit_code: Option<i32>,
    stdout: &str,
    stderr: &str,
    background: bool,
    auto_bg: bool,
    duration_ms: u64,
) -> ToolOutput {
    let mut combined = stdout.to_string();
    if !stderr.is_empty() {
        if !combined.is_empty() {
            combined.push('\n');
        }
        combined.push_str(stderr);
    }
    let mut truncated = false;
    if combined.len() > 64_000 {
        combined.truncate(64_000);
        truncated = true;
    }
    ToolOutput {
        result: serde_json::json!({
            "display_command": display,
            "description": description,
            "exit_code": exit_code,
            "output": combined,
            "stdout": stdout.chars().take(32_000).collect::<String>(),
            "stderr": stderr.chars().take(16_000).collect::<String>(),
            "background": background,
            "auto_backgrounded": auto_bg,
            "truncated": truncated,
        }),
        truncated,
        duration_ms,
    }
}

/// Fetch a URL with DNS/private IP SSRF guards and redirect re-check.
pub struct WebFetchTool;
#[async_trait::async_trait]
impl ToolHandler for WebFetchTool {
    async fn execute(
        &self,
        input: serde_json::Value,
        _context: &ToolCallContext,
    ) -> Result<ToolOutput, ToolError> {
        let url = input
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError {
                code: "invalid_input".into(),
                message: "Missing url".into(),
                retryable: false,
            })?;
        ssrf::validate_fetch_url(url)?;
        let max_bytes = input
            .get("max_bytes")
            .and_then(|v| v.as_u64())
            .unwrap_or(64_000)
            .min(512_000) as usize;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                let next = attempt.url().as_str();
                if ssrf::validate_fetch_url(next).is_err() {
                    attempt.error(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "redirect blocked by SSRF policy",
                    ))
                } else if attempt.previous().len() > 5 {
                    attempt.stop()
                } else {
                    attempt.follow()
                }
            }))
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
        // Re-validate final URL after redirects.
        ssrf::validate_fetch_url(resp.url().as_str())?;
        let status = resp.status().as_u16();
        let bytes = resp.bytes().await.map_err(|e| ToolError {
            code: "network".into(),
            message: e.to_string(),
            retryable: true,
        })?;
        let truncated = bytes.len() > max_bytes;
        let slice = if truncated {
            &bytes[..max_bytes]
        } else {
            &bytes
        };
        let body = String::from_utf8_lossy(slice).into_owned();
        Ok(ToolOutput {
            result: serde_json::json!({
                "status": status,
                "body": body,
                "truncated": truncated,
                "bytes": slice.len(),
            }),
            truncated,
            duration_ms: 0,
        })
    }
}

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

/// Return all built-in tool definitions.
pub fn builtin_tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "read_file",
            description: "Read a text file (supports offset/limit; binary returns metadata only)",
            schema: serde_json::json!({
                "type":"object",
                "properties":{
                    "path":{"type":"string"},
                    "offset":{"type":"integer","minimum":0},
                    "limit":{"type":"integer","minimum":1},
                    "max_chars":{"type":"integer","minimum":1}
                },
                "required":["path"]
            }),
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
            description: "Write content to a file (legacy; prefer apply_patch for new runs)",
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
            description: "List directory entries (stable sort, cursor pagination)",
            schema: serde_json::json!({"type":"object","properties":{"path":{"type":"string"},"limit":{"type":"integer"},"cursor":{"type":"string"}},"required":[]}),
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
            description: "Search file contents (ripgrep JSON when available)",
            schema: serde_json::json!({"type":"object","properties":{"pattern":{"type":"string"},"root":{"type":"string"},"glob":{"type":"string"}},"required":["pattern"]}),
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
            description: "Replace the first occurrence of old_string with new_string (legacy; prefer apply_patch)",
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
            description: "Run a shell command in the project (15s foreground budget then auto-background)",
            schema: serde_json::json!({
                "type":"object",
                "properties":{
                    "command":{"type":"string"},
                    "args":{"type":"array","items":{"type":"string"}},
                    "cwd":{"type":"string"},
                    "timeout_ms":{"type":"integer"},
                    "background":{"type":"boolean"},
                    "description":{"type":"string"}
                },
                "required":["command"]
            }),
            side_effect: SideEffect::Process,
            permission_class: PermissionClass::DestructiveCommand,
            path_scope: PathScope::Any,
            timeout_ms: 300_000,
            output_limit: 64_000,
            cancellable: true,
            handler: Arc::new(RunTerminalTool),
        },
        Tool {
            name: "web_fetch",
            description: "Fetch a public HTTP(S) URL (SSRF-safe DNS checks)",
            schema: serde_json::json!({"type":"object","properties":{"url":{"type":"string"},"max_bytes":{"type":"integer"}},"required":["url"]}),
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
    .chain(plan::plan_mode_tools())
    // `web_search` is registered only when a real backend is configured. An
    // unconfigured install must not advertise a search tool it cannot honour —
    // the model would spend a turn discovering that, and the only alternative
    // (returning something) would be fabricated data.
    .chain(web_search::is_configured().then(web_search::web_search_tool))
    .collect()
}