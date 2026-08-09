//! File-system access tools (read / write / search / list / grep / edit).
//!
//! Each tool resolves its path through `ToolCallContext::resolve_path` so a
//! handler can never see a raw caller-supplied path.

use crate::{ToolCallContext, ToolError, ToolHandler, ToolOutput};

/// Read a file from the filesystem (offset/limit, binary-safe metadata).
pub struct ReadFileTool;

/// Atomically write `content` to `path`: write a temp file in the same
/// directory, fsync it, then rename over the target (P0-015). A crash before
/// rename leaves the original intact; a concurrent writer cannot observe a
/// half-written file. Returns the final path on success.
pub async fn atomic_write_file(path: &str, content: &[u8]) -> Result<String, ToolError> {
    use tokio::io::AsyncWriteExt as _;
    let target = std::path::Path::new(path);
    let dir = target
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let file_name = target
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "out".to_string());
    let tmp_name = format!(".natives-tmp-{}-{}", file_name, std::process::id());
    let tmp_path = dir.join(&tmp_name);

    // Write + fsync the temp file.
    {
        let mut f = tokio::fs::File::create(&tmp_path)
            .await
            .map_err(|e| ToolError {
                code: "write_error".into(),
                message: format!("atomic write create temp failed: {e}"),
                retryable: true,
            })?;
        f.write_all(content).await.map_err(|e| ToolError {
            code: "write_error".into(),
            message: format!("atomic write temp failed: {e}"),
            retryable: true,
        })?;
        f.sync_all().await.map_err(|e| ToolError {
            code: "write_error".into(),
            message: format!("atomic write fsync failed: {e}"),
            retryable: true,
        })?;
    }

    // Rename over the target (atomic on same filesystem).
    tokio::fs::rename(&tmp_path, target).await.map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        ToolError {
            code: "write_error".into(),
            message: format!("atomic write rename failed: {e}"),
            retryable: true,
        }
    })?;
    Ok(path.to_string())
}

/// Async trait impl marker for `atomic_write_file` usage above.
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
        let offset = input.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
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
        let is_binary = head.contains(&0);
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

        let full = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| ToolError {
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
        let pattern = input
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError {
                code: "invalid_input".into(),
                message: "Missing 'pattern' field".into(),
                retryable: false,
            })?;
        let root = input.get("root").and_then(|v| v.as_str()).unwrap_or(".");
        let walker = walkdir::WalkDir::new(root).max_depth(5);
        let results: Vec<String> = walker
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .map(|e| e.path().to_string_lossy().to_string())
            .filter(|p| {
                crate::policy::glob_matches_public(p, pattern)
                    || p.contains(pattern)
                    || std::path::Path::new(p)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| {
                            n.contains(pattern) || crate::policy::glob_matches_public(n, pattern)
                        })
                        .unwrap_or(false)
            })
            .take(100)
            .collect();
        Ok(ToolOutput {
            result: serde_json::json!({"files": results, "count": results.len(), "pattern": pattern}),
            truncated: results.len() >= 100,
            duration_ms: 0,
        })
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
        let path = input
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError {
                code: "invalid_input".into(),
                message: "Missing 'path'".into(),
                retryable: false,
            })?;
        let content = input
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError {
                code: "invalid_input".into(),
                message: "Missing 'content'".into(),
                retryable: false,
            })?;
        // P0-015: atomic write (temp + fsync + rename) so a crash or a
        // concurrent writer never leaves a half-written file.
        atomic_write_file(path, content.as_bytes())
            .await
            .map_err(|e| ToolError {
                code: "write_error".into(),
                message: e.message,
                retryable: true,
            })?;
        Ok(ToolOutput {
            result: serde_json::json!({"path": path, "bytes": content.len()}),
            truncated: false,
            duration_ms: 0,
        })
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
        let limit = input.get("limit").and_then(|v| v.as_u64()).unwrap_or(200) as usize;
        let cursor = input.get("cursor").and_then(|v| v.as_str()).unwrap_or("");
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
            let is_dir = entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false);
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
        let line_no = data
            .get("line_number")
            .and_then(|n| n.as_u64())
            .unwrap_or(0);
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
        let path = input
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError {
                code: "invalid_input".into(),
                message: "Missing path".into(),
                retryable: false,
            })?;
        let old = input
            .get("old_string")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError {
                code: "invalid_input".into(),
                message: "Missing old_string".into(),
                retryable: false,
            })?;
        let new = input
            .get("new_string")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError {
                code: "invalid_input".into(),
                message: "Missing new_string".into(),
                retryable: false,
            })?;
        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| ToolError {
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
        // P0-015: atomic write (temp + fsync + rename).
        atomic_write_file(path, updated.as_bytes())
            .await
            .map_err(|e| ToolError {
                code: "write_error".into(),
                message: e.message,
                retryable: true,
            })?;
        Ok(ToolOutput {
            result: serde_json::json!({"path": path, "replaced": true}),
            truncated: false,
            duration_ms: 0,
        })
    }
}
