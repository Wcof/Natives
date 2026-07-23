//! Claude CLI runtime bridge for session main path (REQ-T01).
//!
//! Spawns `claude -p --output-format stream-json`, maps stdout lines to
//! Protocol v2 [`RunEventKind`], and appends them to the shared EventSequencer.
//! Codex is intentionally not bridged here (red line: unavailable).
//!
//! ## Host-mediated permissions (deep path)
//!
//! Host `permission_profile` is mapped to Claude CLI `--permission-mode`:
//! - readonly  → dontAsk (+ tools limited via --allowedTools when possible)
//! - ask       → default session mode (omit flag; CLI interactive policy)
//! - full_access → acceptEdits
//!
//! When the CLI emits a control/permission request on stream-json stdout, we:
//! 1. emit `PermissionRequested` on the Host event bus
//! 2. wait on `ProductionRuntime` permission waiters
//! 3. write a control response line to CLI stdin (stream-json input)
//!
//! If the CLI version does not emit control requests, Host cards simply stay
//! idle while CLI-local policy applies — still honest, not fake-green.

use assistant_protocol::v2::RunEventKind;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::production::ProductionRuntime;

/// Resolve `claude` binary: PATH, then `~/.claude/local/claude`.
pub fn find_claude_binary() -> Option<PathBuf> {
    if which("claude") {
        return Some(PathBuf::from("claude"));
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    let fallback = PathBuf::from(home).join(".claude").join("local").join("claude");
    if fallback.exists() {
        Some(fallback)
    } else {
        None
    }
}

fn which(cmd: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|dir| {
                let p = dir.join(cmd);
                p.is_file()
            })
        })
        .unwrap_or(false)
}

/// True when a `claude` binary is discoverable.
pub fn claude_cli_available() -> bool {
    find_claude_binary().is_some()
}

/// Map Host permission_profile → Claude CLI `--permission-mode` value.
pub fn map_permission_mode(profile: &str) -> Option<&'static str> {
    match profile {
        "readonly" | "read_only" => Some("dontAsk"),
        "full_access" | "autonomous" | "full" => Some("acceptEdits"),
        // ask / default: leave CLI default (user can configure ~/.claude)
        _ => None,
    }
}

/// Parsed stdout line: normal events and/or a control request needing Host answer.
#[derive(Debug, Default)]
pub struct TranslatedLine {
    pub events: Vec<RunEventKind>,
    /// (request_id, tool_name, tool_call_id, input, reason)
    pub control: Option<ControlRequest>,
}

#[derive(Debug, Clone)]
pub struct ControlRequest {
    pub request_id: String,
    pub tool_name: String,
    pub tool_call_id: String,
    pub input: Value,
    pub reason: String,
}

/// Map one Claude CLI stream-json line → events + optional control request.
pub fn translate_cli_line(line: &str) -> TranslatedLine {
    let Ok(val) = serde_json::from_str::<Value>(line) else {
        return TranslatedLine::default();
    };
    let msg_type = val.get("type").and_then(|v| v.as_str()).unwrap_or("");

    // Control / permission request shapes (SDK + CLI variants)
    if matches!(
        msg_type,
        "control_request" | "can_use_tool" | "permission_request" | "tool_permission_request"
    ) || val.get("request").is_some() && msg_type == "control"
    {
        if let Some(ctrl) = parse_control_request(&val) {
            return TranslatedLine {
                events: vec![RunEventKind::PermissionRequested {
                    tool_call_id: ctrl.tool_call_id.clone(),
                    tool_name: ctrl.tool_name.clone(),
                    reason: ctrl.reason.clone(),
                    permission_id: ctrl.request_id.clone(),
                    input: ctrl.input.clone(),
                }],
                control: Some(ctrl),
            };
        }
    }

    // Nested control under system/subtype
    if msg_type == "system" {
        let subtype = val.get("subtype").and_then(|v| v.as_str()).unwrap_or("");
        if subtype.contains("permission") || subtype.contains("control") {
            if let Some(ctrl) = parse_control_request(&val) {
                return TranslatedLine {
                    events: vec![RunEventKind::PermissionRequested {
                        tool_call_id: ctrl.tool_call_id.clone(),
                        tool_name: ctrl.tool_name.clone(),
                        reason: ctrl.reason.clone(),
                        permission_id: ctrl.request_id.clone(),
                        input: ctrl.input.clone(),
                    }],
                    control: Some(ctrl),
                };
            }
        }
        return TranslatedLine::default();
    }

    let events = match msg_type {
        "assistant" => {
            let mut out = Vec::new();
            if let Some(content) = val
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_array())
            {
                for block in content {
                    let btype = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
                    match btype {
                        "text" => {
                            if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                if !text.is_empty() {
                                    out.push(RunEventKind::TextDelta {
                                        text: text.to_string(),
                                    });
                                }
                            }
                        }
                        "tool_use" => {
                            let id = block
                                .get("id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let name = block
                                .get("name")
                                .and_then(|v| v.as_str())
                                .unwrap_or("tool")
                                .to_string();
                            let input = block
                                .get("input")
                                .cloned()
                                .unwrap_or_else(|| json!({}));
                            out.push(RunEventKind::ToolCallRequested {
                                id: id.clone(),
                                name: name.clone(),
                                input,
                            });
                            out.push(RunEventKind::ToolCallStarted { id, name });
                        }
                        _ => {}
                    }
                }
            } else if let Some(text) = val
                .pointer("/message/content/0/text")
                .and_then(|t| t.as_str())
            {
                if !text.is_empty() {
                    out.push(RunEventKind::TextDelta {
                        text: text.to_string(),
                    });
                }
            }
            out
        }
        "content_block_delta" | "stream_event" => {
            let text = val
                .pointer("/delta/text")
                .or_else(|| val.pointer("/event/delta/text"))
                .and_then(|t| t.as_str())
                .unwrap_or("");
            if text.is_empty() {
                vec![]
            } else {
                vec![RunEventKind::TextDelta {
                    text: text.to_string(),
                }]
            }
        }
        "tool_use" => {
            let id = val
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let name = val
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("tool")
                .to_string();
            let input = val.get("input").cloned().unwrap_or_else(|| json!({}));
            vec![
                RunEventKind::ToolCallRequested {
                    id: id.clone(),
                    name: name.clone(),
                    input,
                },
                RunEventKind::ToolCallStarted { id, name },
            ]
        }
        "tool_result" | "user" => {
            let mut out = Vec::new();
            if let Some(content) = val
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_array())
            {
                for block in content {
                    if block.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
                        let id = block
                            .get("tool_use_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let is_error = block
                            .get("is_error")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        let output = block
                            .get("content")
                            .cloned()
                            .unwrap_or_else(|| json!(null));
                        out.push(RunEventKind::ToolCallCompleted {
                            id,
                            name: "tool".into(),
                            output,
                            is_error,
                            duration_ms: 0,
                        });
                    }
                }
            } else if msg_type == "tool_result" {
                let id = val
                    .get("tool_use_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let is_error = val
                    .get("is_error")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let output = val.get("content").cloned().unwrap_or_else(|| json!(null));
                out.push(RunEventKind::ToolCallCompleted {
                    id,
                    name: "tool".into(),
                    output,
                    is_error,
                    duration_ms: 0,
                });
            }
            out
        }
        "result" => {
            let is_error = val
                .get("is_error")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if is_error {
                let err = val
                    .get("result")
                    .and_then(|v| v.as_str())
                    .or_else(|| val.get("error").and_then(|v| v.as_str()))
                    .unwrap_or("claude cli error");
                vec![RunEventKind::Failed {
                    error: err.to_string(),
                    code: "CLI_ERROR".into(),
                }]
            } else {
                vec![RunEventKind::Completed {
                    reason: "done".into(),
                }]
            }
        }
        "error" => {
            let err = val
                .get("message")
                .or_else(|| val.get("error"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            vec![RunEventKind::Failed {
                error: err.to_string(),
                code: "CLI_ERROR".into(),
            }]
        }
        _ => vec![],
    };

    TranslatedLine {
        events,
        control: None,
    }
}

fn parse_control_request(val: &Value) -> Option<ControlRequest> {
    // Shapes:
    // { "type":"control_request", "request_id":"...", "request":{ "subtype":"can_use_tool", "tool_name":"...", "input":{} } }
    // { "type":"can_use_tool", "id":"...", "tool_name":"Bash", "input":{} }
    let request = val.get("request").unwrap_or(val);
    let request_id = val
        .get("request_id")
        .or_else(|| val.get("id"))
        .or_else(|| request.get("id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let tool_name = request
        .get("tool_name")
        .or_else(|| request.get("name"))
        .or_else(|| val.get("tool_name"))
        .and_then(|v| v.as_str())
        .unwrap_or("tool")
        .to_string();
    let tool_call_id = request
        .get("tool_use_id")
        .or_else(|| request.get("tool_call_id"))
        .or_else(|| val.get("tool_use_id"))
        .and_then(|v| v.as_str())
        .unwrap_or(request_id.as_str())
        .to_string();
    let input = request
        .get("input")
        .or_else(|| request.get("arguments"))
        .or_else(|| val.get("input"))
        .cloned()
        .unwrap_or_else(|| json!({}));
    let reason = request
        .get("description")
        .or_else(|| request.get("reason"))
        .or_else(|| val.get("reason"))
        .and_then(|v| v.as_str())
        .unwrap_or("CLI tool requires permission")
        .to_string();

    // Require tool-ish payload so we don't treat random system msgs as control
    let subtype = request
        .get("subtype")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let looks_like_tool = !tool_name.is_empty()
        && (subtype.contains("tool")
            || subtype.contains("permission")
            || val.get("type").and_then(|t| t.as_str()) == Some("can_use_tool")
            || val.get("type").and_then(|t| t.as_str()) == Some("control_request")
            || val.get("type").and_then(|t| t.as_str()) == Some("permission_request")
            || request.get("tool_name").is_some()
            || request.get("name").is_some());
    if !looks_like_tool {
        return None;
    }
    Some(ControlRequest {
        request_id,
        tool_name,
        tool_call_id,
        input,
        reason,
    })
}

/// Build stdin control response JSON line for Claude stream-json input.
pub fn control_response_line(request_id: &str, approved: bool) -> String {
    // Compatible shapes used by Claude Code control protocol variants.
    let body = json!({
        "type": "control_response",
        "response": {
            "subtype": "success",
            "request_id": request_id,
            "response": {
                "behavior": if approved { "allow" } else { "deny" },
                "updatedInput": null,
            }
        }
    });
    format!("{body}\n")
}

/// Wait Host permission.respond for `permission_id` (run-bound).
async fn wait_host_permission(
    runtime: &ProductionRuntime,
    run_id: &str,
    permission_id: &str,
    timeout: Duration,
) -> bool {
    let (tx, rx) = oneshot::channel();
    {
        let mut map = runtime.permission_waiters.lock().await;
        map.insert(permission_id.to_string(), (run_id.to_string(), "cli".into(), tx));
    }
    let cancel = runtime
        .execution
        .token(run_id)
        .await
        .or_else(|| {
            // Fall back to mirrored cli flag.
            None
        });
    let cancel = if let Some(c) = cancel {
        c
    } else {
        runtime
            .cli_cancel_flags
            .try_lock()
            .ok()
            .and_then(|g| g.get(run_id).cloned())
            .unwrap_or_else(CancellationToken::new)
    };
    tokio::select! {
        biased;
        _ = cancel.cancelled() => {
            let mut map = runtime.permission_waiters.lock().await;
            map.remove(permission_id);
            false
        }
        res = tokio::time::timeout(timeout, rx) => {
            match res {
                Ok(Ok((approved, _scope))) => approved,
                _ => {
                    let mut map = runtime.permission_waiters.lock().await;
                    map.remove(permission_id);
                    false
                }
            }
        }
    }
}

/// Run Claude CLI for one user turn; map events into `runtime.events` for `run_id`.
/// Returns terminal status string: completed | failed | interrupted.
pub async fn run_claude_cli_turn(
    runtime: &ProductionRuntime,
    run_id: &str,
    prompt: &str,
    model: &str,
    project_path: Option<&Path>,
    permission_profile: &str,
    cancel: CancellationToken,
) -> Result<String, String> {
    let bin = find_claude_binary().ok_or_else(|| {
        "runtime claude_cli unavailable (claude binary not found)".to_string()
    })?;

    runtime.events.append(run_id, RunEventKind::Preparing);
    runtime.events.append(run_id, RunEventKind::Started);

    let mut command = Command::new(&bin);
    command
        .arg("--print")
        .arg("--output-format")
        .arg("stream-json")
        .arg("--input-format")
        .arg("stream-json")
        .arg("--verbose")
        .arg("--model")
        .arg(model)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    if let Some(mode) = map_permission_mode(permission_profile) {
        command.arg("--permission-mode").arg(mode);
    }
    // readonly: limit to read-ish tools when CLI supports --allowedTools
    if matches!(permission_profile, "readonly" | "read_only") {
        command
            .arg("--allowedTools")
            .arg("Read,Glob,Grep,LS");
    }

    if let Some(cwd) = project_path {
        if cwd.is_dir() {
            command.current_dir(cwd);
        }
    }

    let mut child = command
        .spawn()
        .map_err(|e| format!("spawn claude cli failed: {e}"))?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "claude cli stdin missing".to_string())?;
    // Seed user message on stream-json input
    let user_line = json!({"type":"user","message":{"role":"user","content":prompt}});
    stdin
        .write_all(format!("{user_line}\n").as_bytes())
        .await
        .map_err(|e| format!("cli stdin write failed: {e}"))?;
    // Keep stdin open for control responses.

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "claude cli stdout missing".to_string())?;
    let stderr = child.stderr.take();

    if let Some(stderr) = stderr {
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if !line.trim().is_empty() {
                    eprintln!("[cli_runtime_bridge] stderr: {line}");
                }
            }
        });
    }

    let mut reader = BufReader::new(stdout).lines();
    let mut terminal: Option<String> = None;

    loop {
        if cancel.is_cancelled() {
            let _ = child.kill().await;
            runtime.events.append(
                run_id,
                RunEventKind::Interrupted {
                    reason: "cancelled".into(),
                },
            );
            return Ok("interrupted".into());
        }

        tokio::select! {
            line = reader.next_line() => {
                match line {
                    Ok(Some(line)) => {
                        let translated = translate_cli_line(&line);
                        for kind in translated.events {
                            let is_term = matches!(
                                kind,
                                RunEventKind::Completed { .. }
                                    | RunEventKind::Failed { .. }
                                    | RunEventKind::Interrupted { .. }
                                    | RunEventKind::Cancelled { .. }
                            );
                            let status_hint = match &kind {
                                RunEventKind::Completed { .. } => Some("completed"),
                                RunEventKind::Failed { .. } => Some("failed"),
                                RunEventKind::Interrupted { .. } => Some("interrupted"),
                                RunEventKind::Cancelled { .. } => Some("cancelled"),
                                _ => None,
                            };
                            runtime.events.append(run_id, kind);
                            if let Some(s) = status_hint {
                                terminal = Some(s.into());
                            }
                            if is_term {
                                break;
                            }
                        }
                        if let Some(ctrl) = translated.control {
                            // Host-mediated permission: wait UI respond, write CLI control response.
                            let approved = wait_host_permission(
                                runtime,
                                run_id,
                                &ctrl.request_id,
                                Duration::from_secs(300),
                            )
                            .await;
                            runtime.events.append(
                                run_id,
                                RunEventKind::PermissionResponded {
                                    permission_id: ctrl.request_id.clone(),
                                    approved,
                                    scope: "once".into(),
                                },
                            );
                            let resp = control_response_line(&ctrl.request_id, approved);
                            if let Err(e) = stdin.write_all(resp.as_bytes()).await {
                                eprintln!("[cli_runtime_bridge] control response write failed: {e}");
                            } else {
                                let _ = stdin.flush().await;
                            }
                        }
                        if terminal.is_some() {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        runtime.events.append(
                            run_id,
                            RunEventKind::Failed {
                                error: format!("cli stdout read error: {e}"),
                                code: "CLI_IO".into(),
                            },
                        );
                        return Ok("failed".into());
                    }
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(50)) => {}
        }
    }

    drop(stdin);
    let status = child
        .wait()
        .await
        .map_err(|e| format!("claude wait failed: {e}"))?;

    if terminal.is_none() {
        if status.success() {
            runtime.events.append(
                run_id,
                RunEventKind::Completed {
                    reason: "done".into(),
                },
            );
            terminal = Some("completed".into());
        } else {
            runtime.events.append(
                run_id,
                RunEventKind::Failed {
                    error: format!("claude exit {status}"),
                    code: "CLI_EXIT".into(),
                },
            );
            terminal = Some("failed".into());
        }
    }

    Ok(terminal.unwrap_or_else(|| "completed".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translate_content_block_delta() {
        let line = r#"{"type":"content_block_delta","delta":{"text":"Hello"}}"#;
        let t = translate_cli_line(line);
        assert!(matches!(
            &t.events[..],
            [RunEventKind::TextDelta { text }] if text == "Hello"
        ));
        assert!(t.control.is_none());
    }

    #[test]
    fn translate_result_completed() {
        let line = r#"{"type":"result","is_error":false}"#;
        let t = translate_cli_line(line);
        assert!(matches!(t.events[0], RunEventKind::Completed { .. }));
    }

    #[test]
    fn translate_control_request_emits_permission() {
        let line = r#"{"type":"control_request","request_id":"req-1","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"ls"}}}"#;
        let t = translate_cli_line(line);
        assert!(t.control.is_some());
        assert!(matches!(
            &t.events[..],
            [RunEventKind::PermissionRequested { permission_id, tool_name, .. }]
                if permission_id == "req-1" && tool_name == "Bash"
        ));
    }

    #[test]
    fn map_permission_mode_profiles() {
        assert_eq!(map_permission_mode("readonly"), Some("dontAsk"));
        assert_eq!(map_permission_mode("full_access"), Some("acceptEdits"));
        assert_eq!(map_permission_mode("ask"), None);
    }

    #[test]
    fn control_response_allow_shape() {
        let line = control_response_line("abc", true);
        assert!(line.contains("control_response"));
        assert!(line.contains("allow"));
        assert!(line.contains("abc"));
    }
}
