//! Shell command execution tool backed by the process supervisor.

use crate::{ProcessSupervisor, ToolCallContext, ToolError, ToolHandler, ToolOutput};
use std::time::Duration;

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
        let (program, args, display) =
            if let Some(arr) = input.get("args").and_then(|v| v.as_array()) {
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

        let cwd = crate::process_supervisor::resolve_cwd(&context.project_root, cwd_input)
            .map_err(|e| ToolError {
                code: "cwd_invalid".into(),
                message: e,
                retryable: false,
            })?;

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
        use crate::ProcessState;

        // Keep the handler's live output path independent from the final tool
        // result. The supervisor owns child lifetime; this task only drains
        // already-buffered chunks and exits when the process reaches a terminal
        // state or the caller drops the channel.
        let progress_task = context.progress.as_ref().map(|tx| {
            let tx = tx.clone();
            let dropped = context.progress_dropped_bytes.clone();
            let task_id = task_id.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(20)).await;
                for _ in 0..12_000 {
                    for chunk in supervisor.drain_output(&task_id).await {
                        let chunk = crate::ToolProgressChunk {
                            stream: chunk.stream.to_string(),
                            text: chunk.text,
                        };
                        // H03: the live-output channel is bounded. Overflow is
                        // dropped and counted — never buffered unboundedly and
                        // never allowed to stall the shell.
                        match tx.try_send(chunk) {
                            Ok(()) => {}
                            Err(tokio::sync::mpsc::error::TrySendError::Full(full)) => {
                                dropped.fetch_add(
                                    full.text.len() as u64,
                                    std::sync::atomic::Ordering::Relaxed,
                                );
                            }
                            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => return,
                        }
                    }
                    match supervisor.poll(&task_id).await {
                        Ok(snapshot)
                            if matches!(
                                snapshot.state,
                                ProcessState::Completed
                                    | ProcessState::Failed
                                    | ProcessState::Cancelled
                            ) =>
                        {
                            break
                        }
                        Ok(_) | Err(_) => {
                            tokio::time::sleep(Duration::from_millis(50)).await;
                        }
                    }
                }
            })
        });

        // Race spawn against run cancel; if cancelled mid-foreground, force kill.
        let spawn_fut = supervisor.spawn(spec);
        let snap = tokio::select! {
            biased;
            _ = context.cancel.cancelled() => {
                if let Some(task) = &progress_task { task.abort(); }
                let _ = supervisor.cancel(&task_id).await;
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

#[allow(clippy::too_many_arguments)] // terminal output fields are fixed
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
