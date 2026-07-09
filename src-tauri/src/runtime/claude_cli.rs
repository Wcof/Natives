//! runtime/claude_cli.rs — Claude CLI Runtime
//!
//! Rust 直接 spawn `claude` 二进制，stdin/stdout 管道通信。
//! 非 Node SDK，避免引入 Node 运行时依赖。

use super::{AgentRuntime, EventStream, RuntimeEvent, RuntimeStreamOptions};
use crate::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

pub struct ClaudeCliRuntime {
    binary_path: Option<std::path::PathBuf>,
}

impl ClaudeCliRuntime {
    pub fn new() -> Self {
        Self {
            binary_path: find_claude_binary(),
        }
    }
}

fn find_claude_binary() -> Option<std::path::PathBuf> {
    // 1. PATH 上的 `claude`
    if crate::wechat::driver::which("claude") {
        return Some(std::path::PathBuf::from("claude"));
    }
    // 2. ~/.claude/local/claude
    let home = dirs::home_dir()?;
    let fallback = home.join(".claude").join("local").join("claude");
    if fallback.exists() {
        Some(fallback)
    } else {
        None
    }
}

#[async_trait]
impl AgentRuntime for ClaudeCliRuntime {
    fn id(&self) -> &'static str {
        "claude_cli"
    }
    fn display_name(&self) -> &'static str {
        "Claude CLI"
    }
    fn is_available(&self) -> bool {
        self.binary_path.is_some()
    }

    async fn stream(&self, opts: RuntimeStreamOptions) -> Result<EventStream> {
        let bin = self
            .binary_path
            .as_ref()
            .ok_or_else(|| crate::Error::InvalidInput("Claude CLI binary not found".into()))?
            .clone();

        let (tx, rx) = tokio::sync::mpsc::channel::<RuntimeEvent>(64);

        tokio::spawn(async move {
            let mut cancel_rx = opts.abort_receiver;
            let mut command = Command::new(&bin);
            command
                .arg("--print")
                .arg("--output-format")
                .arg("stream-json")
                .arg("--input-format")
                .arg("stream-json")
                .arg("--model")
                .arg(&opts.model)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true);

            let mut child = match command.spawn() {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx
                        .send(RuntimeEvent::RunFailed {
                            error: crate::log_sanitizer::sanitize(&format!("spawn: {e}")),
                        })
                        .await;
                    return;
                }
            };

            if let Some(mut stdin) = child.stdin.take() {
                let msg = serde_json::json!({"type":"user","message":opts.prompt});
                let _ = stdin.write_all(format!("{msg}\n").as_bytes()).await;
                drop(stdin);
            }

            let stderr_task = child.stderr.take().map(|stderr| {
                tokio::spawn(async move {
                    let mut lines = BufReader::new(stderr).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        let sanitized = crate::log_sanitizer::sanitize(&line);
                        if !sanitized.trim().is_empty() {
                            eprintln!("[ClaudeCliRuntime] stderr: {sanitized}");
                        }
                    }
                })
            });

            let mut terminal_sent = false;
            if let Some(stdout) = child.stdout.take() {
                let reader = BufReader::new(stdout);
                let mut lines = reader.lines();
                loop {
                    tokio::select! {
                        _ = &mut cancel_rx => {
                            terminate_child(&mut child).await;
                            let _ = tx.send(RuntimeEvent::RunFailed {
                                error: "Claude CLI run cancelled".to_string(),
                            }).await;
                            terminal_sent = true;
                            break;
                        }
                        line = lines.next_line() => {
                            match line {
                                Ok(Some(line)) => {
                                    if let Some(ev) = translate_claude_cli_event(&line) {
                                        let is_terminal = matches!(
                                            ev,
                                            RuntimeEvent::RunCompleted { .. } | RuntimeEvent::RunFailed { .. }
                                        );
                                        let _ = tx.send(ev).await;
                                        if is_terminal {
                                            terminal_sent = true;
                                            break;
                                        }
                                    }
                                }
                                Ok(None) => break,
                                Err(e) => {
                                    let _ = tx.send(RuntimeEvent::RunFailed {
                                        error: crate::log_sanitizer::sanitize(&format!("stdout read failed: {e}")),
                                    }).await;
                                    terminal_sent = true;
                                    break;
                                }
                            }
                        }
                    }
                }
            } else {
                let _ = tx
                    .send(RuntimeEvent::RunFailed {
                        error: "Claude CLI stdout unavailable".to_string(),
                    })
                    .await;
                terminal_sent = true;
            }

            match child.wait().await {
                Ok(status) if status.success() => {
                    if !terminal_sent {
                        let _ = tx
                            .send(RuntimeEvent::RunCompleted {
                                reason: "done".to_string(),
                            })
                            .await;
                    }
                }
                Ok(status) => {
                    if !terminal_sent {
                        let _ = tx
                            .send(RuntimeEvent::RunFailed {
                                error: crate::log_sanitizer::sanitize(&format!(
                                    "Claude CLI exited with status {status}"
                                )),
                            })
                            .await;
                    }
                }
                Err(e) => {
                    if !terminal_sent {
                        let _ = tx
                            .send(RuntimeEvent::RunFailed {
                                error: crate::log_sanitizer::sanitize(&format!(
                                    "Claude CLI wait failed: {e}"
                                )),
                            })
                            .await;
                    }
                }
            }

            if let Some(task) = stderr_task {
                let _ = task.await;
            }
        });

        Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }

    fn interrupt(&self, _session_id: &str) {
        // CLI 进程由 OS signal 终止（SIGTERM）
    }
    fn dispose(&self) {}
}

async fn terminate_child(child: &mut tokio::process::Child) {
    if matches!(child.try_wait(), Ok(Some(_))) {
        return;
    }
    let _ = child.kill().await;
}

/// 翻译 Claude CLI stream-json 行为 RuntimeEvent
fn translate_claude_cli_event(line: &str) -> Option<RuntimeEvent> {
    let val: Value = serde_json::from_str(line).ok()?;
    let msg_type = val.get("type")?.as_str()?;
    match msg_type {
        "assistant" | "content_block_delta" => {
            let text = val
                .get("delta")
                .and_then(|d| d.get("text"))
                .and_then(|t| t.as_str())
                .unwrap_or("");
            Some(RuntimeEvent::AssistantDelta {
                text: text.to_string(),
            })
        }
        "tool_use" => Some(RuntimeEvent::ToolStarted {
            tool_name: val
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            tool_call_id: val
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            args: sanitize_json_value(val.get("input").cloned().unwrap_or(serde_json::json!({}))),
        }),
        "tool_result" => Some(RuntimeEvent::ToolCompleted {
            tool_call_id: val
                .get("tool_use_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            status: "success".to_string(),
            output: sanitize_json_value(
                val.get("content")
                    .cloned()
                    .unwrap_or(serde_json::json!(null)),
            ),
        }),
        "result" => Some(RuntimeEvent::RunCompleted {
            reason: "done".to_string(),
        }),
        "error" => Some(RuntimeEvent::RunFailed {
            error: crate::log_sanitizer::sanitize(
                val.get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown"),
            ),
        }),
        _ => Some(RuntimeEvent::UnknownItem {
            raw: sanitize_json_value(val),
        }),
    }
}

fn sanitize_json_value(value: Value) -> Value {
    match value {
        Value::String(value) => Value::String(crate::log_sanitizer::sanitize(&value)),
        Value::Array(values) => Value::Array(values.into_iter().map(sanitize_json_value).collect()),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| (key, sanitize_json_value(value)))
                .collect(),
        ),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_translate_known_events() {
        let delta = r#"{"type":"content_block_delta","delta":{"text":"Hello"}}"#;
        let ev = translate_claude_cli_event(delta).unwrap();
        assert!(matches!(ev, RuntimeEvent::AssistantDelta { ref text } if text == "Hello"));

        let result = r#"{"type":"result"}"#;
        let ev = translate_claude_cli_event(result).unwrap();
        assert!(matches!(ev, RuntimeEvent::RunCompleted { .. }));
    }

    #[test]
    fn test_translate_unknown_falls_back() {
        let unknown = r#"{"type":"custom_event","data":42}"#;
        let ev = translate_claude_cli_event(unknown).unwrap();
        assert!(matches!(ev, RuntimeEvent::UnknownItem { .. }));
    }

    #[test]
    fn test_translate_error_sanitizes_key() {
        let raw_key = "sk-testClaudeCliSecret1234567890";
        let line = format!(r#"{{"type":"error","message":"failed with {raw_key}"}}"#);
        let ev = translate_claude_cli_event(&line).unwrap();
        match ev {
            RuntimeEvent::RunFailed { error } => {
                assert!(!error.contains(raw_key));
                assert!(error.contains("sk-***"));
            }
            other => panic!("expected RunFailed, got {other:?}"),
        }
    }

    #[test]
    fn test_tool_payloads_are_sanitized() {
        let raw_key = "sk-toolSecret1234567890";
        let line = format!(
            r#"{{"type":"tool_use","id":"call-1","name":"shell","input":{{"authorization":"Bearer {raw_key}","nested":["{raw_key}"]}}}}"#
        );
        let ev = translate_claude_cli_event(&line).unwrap();
        match ev {
            RuntimeEvent::ToolStarted { args, .. } => {
                let rendered = args.to_string();
                assert!(!rendered.contains(raw_key));
                assert!(rendered.contains("Bearer ***"));
                assert!(rendered.contains("sk-***"));
            }
            other => panic!("expected ToolStarted, got {other:?}"),
        }
    }

    #[test]
    fn test_translate_unknown_sanitizes_raw_payload() {
        let raw_key = "sk-unknownSecret1234567890";
        let line = format!(
            r#"{{"type":"custom_event","headers":{{"authorization":"Bearer {raw_key}"}},"token":"{raw_key}"}}"#
        );
        let ev = translate_claude_cli_event(&line).unwrap();
        match ev {
            RuntimeEvent::UnknownItem { raw } => {
                let rendered = raw.to_string();
                assert!(!rendered.contains(raw_key));
                assert!(rendered.contains("Bearer ***"));
                assert!(rendered.contains("sk-***"));
            }
            other => panic!("expected UnknownItem, got {other:?}"),
        }
    }
}
