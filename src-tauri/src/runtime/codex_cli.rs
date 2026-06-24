//! runtime/codex_cli.rs — Codex CLI Runtime
//!
//! 常驻 `codex app-server` 子进程，JSON-RPC over stdio 通信。

use super::{AgentRuntime, EventStream, RuntimeEvent, RuntimeStreamOptions};
use crate::Result;
use async_trait::async_trait;

pub struct CodexCliRuntime {
    binary_path: Option<std::path::PathBuf>,
}

impl CodexCliRuntime {
    pub fn new() -> Self {
        Self { binary_path: find_codex_binary() }
    }
}

fn find_codex_binary() -> Option<std::path::PathBuf> {
    if crate::wechat::driver::which("codex") {
        return Some(std::path::PathBuf::from("codex"));
    }
    None
}

#[async_trait]
impl AgentRuntime for CodexCliRuntime {
    fn id(&self) -> &'static str { "codex_cli" }
    fn display_name(&self) -> &'static str { "Codex CLI" }
    fn is_available(&self) -> bool { self.binary_path.is_some() }

    async fn stream(&self, opts: RuntimeStreamOptions) -> Result<EventStream> {
        let _bin = self.binary_path.as_ref()
            .ok_or_else(|| crate::Error::InvalidInput("Codex CLI binary not found".into()))?
            .clone();

        let (tx, rx) = tokio::sync::mpsc::channel::<RuntimeEvent>(64);

        // MVP：启动 app-server + JSON-RPC 通信
        // 完整实现在后续迭代补齐（当前 placeholder 返回 RunFailed）
        tokio::spawn(async move {
            let _ = tx.send(RuntimeEvent::RunFailed {
                error: "Codex CLI runtime not yet fully implemented (placeholder)".into(),
            }).await;
        });

        // 消除未使用警告
        let _ = opts;
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
