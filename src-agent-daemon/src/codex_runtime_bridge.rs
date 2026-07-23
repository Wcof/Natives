//! Codex CLI runtime bridge (REQ-T02).
//!
//! **Red line**: Codex remains unavailable until a real app-server JSON-RPC
//! transport is implemented end-to-end. This module exists so the daemon main
//! path has a single place to plug Codex in later, while every entry point
//! today is **fail-closed**.
//!
//! Do not advertise Codex as executable based on binary detection alone.

use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;

use assistant_protocol::v2::RunEventKind;

use crate::production::ProductionRuntime;

/// Product red line — always false until app-server is implemented.
pub fn codex_cli_available() -> bool {
    false
}

pub fn find_codex_binary() -> Option<PathBuf> {
    // Intentionally unused for availability: detecting a binary must not enable the runtime.
    if which("codex") {
        return Some(PathBuf::from("codex"));
    }
    None
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

/// Fail-closed Codex turn. Emits a single Failed event and returns Err.
pub async fn run_codex_cli_turn(
    runtime: &ProductionRuntime,
    run_id: &str,
    _prompt: &str,
    _model: &str,
    _project_path: Option<&Path>,
    _permission_profile: &str,
    cancel: CancellationToken,
) -> Result<String, String> {
    let _ = cancel.is_cancelled();
    let msg = "runtime codex_cli is unavailable (app-server not implemented)".to_string();
    runtime.events.append(run_id, RunEventKind::Preparing);
    runtime.events.append(run_id, RunEventKind::Started);
    runtime.events.append(
        run_id,
        RunEventKind::Failed {
            error: msg.clone(),
            code: "CODEX_UNAVAILABLE".into(),
        },
    );
    Err(msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_always_unavailable() {
        assert!(!codex_cli_available());
    }

    #[tokio::test]
    async fn run_codex_turn_fails_closed() {
        let rt = ProductionRuntime::new();
        let cancel = CancellationToken::new();
        let err = run_codex_cli_turn(&rt, "run-x", "hi", "m", None, "ask", cancel)
            .await
            .unwrap_err();
        assert!(err.contains("unavailable"));
        let evs = rt.events.replay_after("run-x", 0);
        assert!(evs
            .iter()
            .any(|e| matches!(e.payload, RunEventKind::Failed { .. })));
    }
}
