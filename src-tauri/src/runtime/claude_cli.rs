//! runtime/claude_cli.rs — Claude CLI Runtime
//!
//! Rust 直接 spawn `claude` 二进制，stdin/stdout 管道通信。
//! 非 Node SDK，避免引入 Node 运行时依赖。

use super::{AgentRuntime, EventStream, RuntimeEvent, RuntimeStreamOptions};
use crate::Result;
use async_trait::async_trait;
use tokio::process::Command;

pub struct ClaudeCliRuntime {
    binary_path: Option<std::path::PathBuf>,
}

impl ClaudeCliRuntime {
    pub fn new() -> Self {
        Self { binary_path: find_claude_binary() }
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
    if fallback.exists() { Some(fallback) } else { None }
}

#[async_trait]
impl AgentRuntime for ClaudeCliRuntime {
    fn id(&self) -> &'static str { "claude_cli" }
    fn display_name(&self) -> &'static str { "Claude CLI" }
    fn is_available(&self) -> bool { self.binary_path.is_some() }

    async fn stream(&self, opts: RuntimeStreamOptions) -> Result<EventStream> {
        let bin = self.binary_path.as_ref()
            .ok_or_else(|| crate::Error::InvalidInput("Claude CLI binary not found".into()))?
            .clone();

        let (tx, rx) = tokio::sync::mpsc::channel::<RuntimeEvent>(64);

        tokio::spawn(async move {
            let mut child = match Command::new(&bin)
                .arg("--print")
                .arg("--output-format").arg("stream-json")
                .arg("--input-format").arg("stream-json")
                .arg("--model").arg(&opts.model)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
            {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send(RuntimeEvent::RunFailed { error: format!("spawn: {e}") }).await;
                    return;
                }
            };

            // stdin 喂 prompt
            if let Some(mut stdin) = child.stdin.take() {
                use tokio::io::AsyncWriteExt;
                let msg = serde_json::json!({"type":"user","message":opts.prompt});
                let _ = stdin.write_all(format!("{msg}\n").as_bytes()).await;
                drop(stdin);
            }

            // stdout 读 stream-json
            if let Some(stdout) = child.stdout.take() {
                use tokio::io::{AsyncBufReadExt, BufReader};
                let reader = BufReader::new(stdout);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if let Some(ev) = translate_claude_cli_event(&line) {
                        let is_terminal = matches!(ev, RuntimeEvent::RunCompleted { .. } | RuntimeEvent::RunFailed { .. });
                        let _ = tx.send(ev).await;
                        if is_terminal { break; }
                    }
                }
            }

            let _ = child.wait().await;
        });

        Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }

    fn interrupt(&self, _session_id: &str) {
        // CLI 进程由 OS signal 终止（SIGTERM）
    }
    fn dispose(&self) {}
}

/// 翻译 Claude CLI stream-json 行为 RuntimeEvent
fn translate_claude_cli_event(line: &str) -> Option<RuntimeEvent> {
    let val: serde_json::Value = serde_json::from_str(line).ok()?;
    let msg_type = val.get("type")?.as_str()?;
    match msg_type {
        "assistant" | "content_block_delta" => {
            let text = val.get("delta").and_then(|d| d.get("text")).and_then(|t| t.as_str()).unwrap_or("");
            Some(RuntimeEvent::AssistantDelta { text: text.to_string() })
        }
        "tool_use" => Some(RuntimeEvent::ToolStarted {
            tool_name: val.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            tool_call_id: val.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            args: val.get("input").cloned().unwrap_or(serde_json::json!({})),
        }),
        "tool_result" => Some(RuntimeEvent::ToolCompleted {
            tool_call_id: val.get("tool_use_id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            status: "success".to_string(),
            output: val.get("content").cloned().unwrap_or(serde_json::json!(null)),
        }),
        "result" => Some(RuntimeEvent::RunCompleted { reason: "done".to_string() }),
        "error" => Some(RuntimeEvent::RunFailed {
            error: val.get("message").and_then(|v| v.as_str()).unwrap_or("unknown").to_string(),
        }),
        _ => Some(RuntimeEvent::UnknownItem { raw: val }),
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
}
