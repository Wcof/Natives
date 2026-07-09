//! runtime/codex_cli.rs — Codex CLI Runtime
//!
//! 常驻 `codex app-server` 子进程，JSON-RPC over stdio 通信。

use super::{AgentRuntime, EventStream, RuntimeEvent, RuntimeStreamOptions};
use crate::Result;
use async_trait::async_trait;

pub struct CodexCliRuntime;

impl CodexCliRuntime {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl AgentRuntime for CodexCliRuntime {
    fn id(&self) -> &'static str {
        "codex_cli"
    }
    fn display_name(&self) -> &'static str {
        "Codex CLI"
    }
    fn is_available(&self) -> bool {
        // Codex CLI is intentionally disabled until app-server JSON-RPC is
        // implemented end-to-end. A detected binary is not enough to expose a
        // runtime as usable.
        false
    }

    async fn stream(&self, opts: RuntimeStreamOptions) -> Result<EventStream> {
        let _ = opts;

        let (tx, rx) = tokio::sync::mpsc::channel::<RuntimeEvent>(64);
        tokio::spawn(async move {
            let _ = tx.send(RuntimeEvent::RunFailed {
                error: "Codex CLI runtime is disabled in this version. Choose Native or Claude CLI.".into(),
            }).await;
        });
        Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }

    fn interrupt(&self, _session_id: &str) {}
    fn dispose(&self) {}
}

#[test]
fn test_codex_cli_id() {
    let rt = CodexCliRuntime::new();
    assert_eq!(rt.id(), "codex_cli");
}

#[test]
fn test_codex_cli_stays_unavailable_until_implemented() {
    let rt = CodexCliRuntime::new();
    assert!(!rt.is_available());
}
