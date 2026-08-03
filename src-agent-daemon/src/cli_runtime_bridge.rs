//! Claude CLI runtime bridge for session main path (REQ-T01).
//!
//! Spawns `claude -p --output-format stream-json`, maps stdout lines to
//! Protocol v2 domain events, and returns a terminal status string for
//! RunManager (sole lifecycle committer). Never writes Completed/Failed/
//! Cancelled/Interrupted lifecycle events. Codex is not bridged here.
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
//! ## Fail-closed tool policy (task-05)
//!
//! Until we can prove every tool call is host-mediated via control protocol,
//! Claude CLI runs in **text-only** mode by default:
//! - only Read/Glob/Grep/LS allowed (readonly surface)
//! - any tool_use without a matching Host control request fails the turn with
//!   `CLI_TOOL_BRIDGE_UNAVAILABLE` (no side-effect execution path claimed)
//! - never rely on user `~/.claude` defaults for write/exec tools
//!
//! Set `NATIVES_CLI_CONTROL_PROVEN=1` only when fixtures/integration prove the
//! control loop end-to-end; otherwise remain fail-closed.

use assistant_protocol::v2::RunEventKind;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::production::ProductionRuntime;

/// Resolve `claude` binary: PATH, then `~/.claude/local/claude`.
pub fn find_claude_binary() -> Option<PathBuf> {
    if which("claude") {
        return Some(PathBuf::from("claude"));
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    let fallback = PathBuf::from(home)
        .join(".claude")
        .join("local")
        .join("claude");
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
///
/// Fail-closed: never use `acceptEdits` unless control protocol is proven —
/// that mode would allow CLI-local writes without Host mediation.
pub fn map_permission_mode(profile: &str) -> Option<&'static str> {
    if !cli_control_proven() {
        // Text-only / host-mediated transitional: dontAsk + restricted tools.
        return Some("dontAsk");
    }
    match profile {
        "readonly" | "read_only" => Some("dontAsk"),
        "full_access" | "autonomous" | "full" => Some("acceptEdits"),
        // ask / default: leave CLI default only when control is proven
        _ => None,
    }
}

/// True only when operator/fixture has proven Host control mediation.
pub fn cli_control_proven() -> bool {
    std::env::var("NATIVES_CLI_CONTROL_PROVEN")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
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
                            let input = block.get("input").cloned().unwrap_or_else(|| json!({}));
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
                        let output = block.get("content").cloned().unwrap_or_else(|| json!(null));
                        out.push(RunEventKind::ToolCallCompleted {
                            id,
                            name: "tool".into(),
                            output,
                            is_error,
                            duration_ms: 0,
                            result_message_id: None,
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
                    result_message_id: None,
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
    runtime
        .insert_permission_waiter(permission_id, run_id, "cli", tx)
        .await;
    let cancel = runtime
        .execution
        .token(run_id)
        .await
        .unwrap_or_else(CancellationToken::new);
    tokio::select! {
        biased;
        _ = cancel.cancelled() => {
            let _ = runtime.remove_permission_waiter(permission_id).await;
            false
        }
        res = tokio::time::timeout(timeout, rx) => {
            match res {
                Ok(Ok((approved, _scope))) => approved,
                _ => {
                    let _ = runtime.remove_permission_waiter(permission_id).await;
                    false
                }
            }
        }
    }
}

/// Deletes the run-scoped `--mcp-config` file on every exit path (normal,
/// cancel, spawn failure) — it may contain resolved secret references.
struct CliMcpConfigGuard {
    path: std::path::PathBuf,
}

impl Drop for CliMcpConfigGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Project the selected connector configs into the standard `mcpServers`
/// format for `--mcp-config`. `secret:<id>` env references resolve through the
/// Host broker; the file is 0600 and removed when the turn ends.
fn write_cli_mcp_config(run_id: &str, server_ids: &[String]) -> Result<CliMcpConfigGuard, String> {
    let configs = crate::capability::mcp::enabled_runtime_configs()?;
    let mut servers = serde_json::Map::new();
    for id in server_ids {
        let Some(config) = configs.iter().find(|c| &c.id == id) else {
            return Err(format!("mcp server config missing: {id}"));
        };
        let raw_env = crate::capability::mcp::env_for_server(id)?;
        let mut env = serde_json::Map::new();
        for (key, value) in raw_env {
            let resolved = match value.strip_prefix("secret:") {
                Some(secret_id) => crate::natives_db_broker::read_capability_secret(secret_id)
                    .map_err(|_| {
                        format!("secret reference for env '{key}' of '{id}' could not be resolved")
                    })?,
                None => value,
            };
            env.insert(key, json!(resolved));
        }
        let entry = match config.transport {
            agent_core::mcp::McpTransport::Stdio => {
                let mut e = serde_json::Map::new();
                e.insert(
                    "command".into(),
                    json!(config.command.clone().unwrap_or_default()),
                );
                if let Some(args) = &config.args {
                    e.insert("args".into(), json!(args));
                }
                if !env.is_empty() {
                    e.insert("env".into(), serde_json::Value::Object(env));
                }
                e
            }
            agent_core::mcp::McpTransport::Http | agent_core::mcp::McpTransport::Sse => {
                let mut e = serde_json::Map::new();
                e.insert(
                    "type".into(),
                    json!(
                        if matches!(config.transport, agent_core::mcp::McpTransport::Sse) {
                            "sse"
                        } else {
                            "http"
                        }
                    ),
                );
                e.insert("url".into(), json!(config.url.clone().unwrap_or_default()));
                if let Some(headers) = &config.headers {
                    if !headers.is_empty() {
                        e.insert("headers".into(), json!(headers));
                    }
                }
                e
            }
        };
        servers.insert(id.clone(), serde_json::Value::Object(entry));
    }
    let dir = std::env::var("NATIVES_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            std::path::PathBuf::from(home)
                .join(".natives")
                .join("runtime")
        })
        .join("cli-mcp");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{run_id}.json"));
    let body =
        serde_json::to_string(&json!({ "mcpServers": servers })).map_err(|e| e.to_string())?;
    std::fs::write(&path, body).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(CliMcpConfigGuard { path })
}

/// Run Claude CLI for one user turn; map events into `runtime.events` for `run_id`.
/// Returns terminal status string: completed | failed | interrupted.
///
/// `capability` is the resolved ADR-0016 snapshot. This path is an independent
/// execution backend: capabilities are injected through CLI flags and executed
/// by the Claude CLI's own harness — approvals do NOT pass through the native
/// capability gateway (run row `runtime_id` + capability matrix say so).
pub async fn run_claude_cli_turn(
    runtime: &ProductionRuntime,
    run_id: &str,
    prompt: &str,
    model: &str,
    project_path: Option<&Path>,
    permission_profile: &str,
    capability: &crate::capability_resolution::ResolvedCapabilitySnapshot,
    cancel: CancellationToken,
) -> Result<String, String> {
    let bin = find_claude_binary()
        .ok_or_else(|| "runtime claude_cli unavailable (claude binary not found)".to_string())?;

    // Lifecycle Preparing/Started is committed by RunManager, not here.

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
    // Fail-closed text-only surface until control protocol is proven (task-05).
    // readonly always uses the same restricted allowlist.
    if !cli_control_proven() || matches!(permission_profile, "readonly" | "read_only") {
        command.arg("--allowedTools").arg("Read,Glob,Grep,LS");
    }

    // Capability injection (ADR-0016): persona + skills + roster via
    // --append-system-prompt; team members via --agents; connectors via a
    // run-scoped --mcp-config file (never argv — headers/env may hold secrets).
    let mut extra_system = String::new();
    if let Some(persona) = capability
        .profile
        .as_ref()
        .and_then(|p| p.system_prompt.as_deref())
        .filter(|s| !s.trim().is_empty())
    {
        extra_system.push_str(persona);
    }
    if let Some(skills) = capability.skill_prompt.as_deref().filter(|s| !s.is_empty()) {
        if !extra_system.is_empty() {
            extra_system.push_str("\n\n");
        }
        extra_system.push_str(skills);
    }
    if let Some(roster) = capability
        .extra_system_prompt
        .as_deref()
        .filter(|s| !s.is_empty())
    {
        if !extra_system.is_empty() {
            extra_system.push_str("\n\n");
        }
        extra_system.push_str(roster);
    }
    if !extra_system.is_empty() {
        command.arg("--append-system-prompt").arg(&extra_system);
    }
    if let Some(team) = &capability.team {
        let mut agents = serde_json::Map::new();
        for member in &team.members {
            let profile =
                crate::capability_resolution::load_profile(&member.expert_id, project_path)
                    .ok_or_else(|| {
                        format!("team member profile not loadable: {}", member.expert_id)
                    })?;
            let description = if member.role_hint.is_empty() {
                member.description.clone()
            } else {
                member.role_hint.clone()
            };
            agents.insert(
                member.expert_id.clone(),
                json!({
                    "description": description,
                    "prompt": profile.system_prompt.unwrap_or_default(),
                }),
            );
        }
        command
            .arg("--agents")
            .arg(serde_json::to_string(&agents).map_err(|e| e.to_string())?);
    }
    let _mcp_config_guard = if capability.mcp_servers.is_empty() {
        None
    } else {
        let guard = write_cli_mcp_config(run_id, &capability.mcp_servers)?;
        command
            .arg("--mcp-config")
            .arg(&guard.path)
            // strict: the CLI must not additionally load project .mcp.json and
            // escape the selection contract.
            .arg("--strict-mcp-config");
        Some(guard)
    };

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
    // Track control mediation; tool_use without control is fail-closed.
    let mut control_seen = false;
    let mut pending_control_tools: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    loop {
        if cancel.is_cancelled() {
            let _ = child.kill().await;
            // Status only — RunManager commits Interrupted/Cancelled.
            return Ok("interrupted".into());
        }

        tokio::select! {
            line = reader.next_line() => {
                match line {
                    Ok(Some(line)) => {
                        let translated = translate_cli_line(&line);
                        if translated.control.is_some() {
                            control_seen = true;
                            if let Some(ref c) = translated.control {
                                pending_control_tools.insert(c.tool_name.clone());
                            }
                        }
                        for kind in translated.events {
                            // Fail-closed: tool_use without proven control mediation.
                            if !cli_control_proven() {
                                if let RunEventKind::ToolCallRequested { name, .. } = &kind {
                                    let readonly = matches!(
                                        name.as_str(),
                                        "Read" | "Glob" | "Grep" | "LS" | "read_file" | "list_dir" | "grep" | "glob"
                                    );
                                    if !readonly && !control_seen {
                                        let _ = child.kill().await;
                                        return Err(
                                            "CLI_TOOL_BRIDGE_UNAVAILABLE: tool execution requires host control mediation; text-only CLI adapter is active".into()
                                        );
                                    }
                                }
                            }
                            let status_hint = match &kind {
                                RunEventKind::Completed { .. } => Some("completed"),
                                RunEventKind::Failed { .. } => Some("failed"),
                                RunEventKind::Interrupted { .. } => Some("interrupted"),
                                RunEventKind::Cancelled { .. } => Some("cancelled"),
                                _ => None,
                            };
                            if let Some(s) = status_hint {
                                // Never append terminal lifecycle events from CLI bridge.
                                terminal = Some(s.into());
                                break;
                            }
                            // Domain events only (text/tool/permission).
                            runtime.events.append(run_id, kind);
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
                            let _ = pending_control_tools;
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
                        return Err(format!("cli stdout read error: {e}"));
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
        // Status only — RunManager commits the single terminal lifecycle event.
        if status.success() {
            terminal = Some("completed".into());
        } else {
            terminal = Some("failed".into());
        }
    }

    Ok(terminal.unwrap_or_else(|| "completed".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

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
    fn map_permission_mode_fail_closed_without_proven_control() {
        // Default: no NATIVES_CLI_CONTROL_PROVEN → always dontAsk (text-only).
        let _lock = env_lock();
        let _guard = EnvVarGuard::remove("NATIVES_CLI_CONTROL_PROVEN");
        assert_eq!(map_permission_mode("readonly"), Some("dontAsk"));
        assert_eq!(map_permission_mode("full_access"), Some("dontAsk"));
        assert_eq!(map_permission_mode("ask"), Some("dontAsk"));
        assert!(!cli_control_proven());
    }

    #[test]
    fn map_permission_mode_proven_allows_profile_modes() {
        let _lock = env_lock();
        let _guard = EnvVarGuard::set("NATIVES_CLI_CONTROL_PROVEN", "1");
        assert_eq!(map_permission_mode("readonly"), Some("dontAsk"));
        assert_eq!(map_permission_mode("full_access"), Some("acceptEdits"));
        assert_eq!(map_permission_mode("ask"), None);
    }

    /// Process-global env isolation for parallel cargo tests.
    struct EnvVarGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }
    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, previous }
        }
        fn remove(key: &'static str) -> Self {
            let previous = std::env::var_os(key);
            std::env::remove_var(key);
            Self { key, previous }
        }
    }
    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }

    #[test]
    fn tool_use_without_control_is_detected() {
        let line = r#"{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"rm -rf /"}}"#;
        let t = translate_cli_line(line);
        assert!(matches!(
            &t.events[..],
            [RunEventKind::ToolCallRequested { name, .. }, RunEventKind::ToolCallStarted { .. }]
                if name == "Bash"
        ));
        assert!(t.control.is_none());
    }

    #[test]
    fn control_response_allow_shape() {
        let line = control_response_line("abc", true);
        assert!(line.contains("control_response"));
        assert!(line.contains("allow"));
        assert!(line.contains("abc"));
    }

    #[test]
    fn cli_mcp_config_file_is_private_and_cleaned_up() {
        let _guard = crate::storage::DataStore::env_test_lock();
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("cli-mcp-test.db");
        let art = dir.path().join("artifacts");
        crate::storage::set_test_db_override(Some(db.clone()), Some(art));
        let _warm = crate::storage::DataStore::new(&db, &dir.path().join("artifacts")).unwrap();
        std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());

        crate::capability::mcp::create(&serde_json::json!({
            "id": "docs",
            "name": "Docs",
            "transport": "stdio",
            "command": "npx",
            "args": ["-y", "docs-mcp"],
            "env": { "LOG_LEVEL": "info" },
            "trusted": true,
        }))
        .unwrap();

        let path = {
            let guard = write_cli_mcp_config("run-1", &["docs".to_string()]).unwrap();
            let body = std::fs::read_to_string(&guard.path).unwrap();
            assert!(body.contains("mcpServers"));
            assert!(body.contains("docs-mcp"));
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = std::fs::metadata(&guard.path).unwrap().permissions().mode();
                assert_eq!(mode & 0o777, 0o600, "config must be private");
            }
            guard.path.clone()
        };
        // Guard drop removes the file (cancel / failure paths share this).
        assert!(!path.exists(), "mcp-config must be deleted on drop");

        std::env::remove_var("NATIVES_RUNTIME_DIR");
        crate::storage::set_test_db_override(None, None);
    }
}
