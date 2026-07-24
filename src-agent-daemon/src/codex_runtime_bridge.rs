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

/// Fail-closed Codex turn. Returns Err only — RunManager commits Failed status/lifecycle.
pub async fn run_codex_cli_turn(
    _runtime: &ProductionRuntime,
    _run_id: &str,
    _prompt: &str,
    _model: &str,
    _project_path: Option<&Path>,
    _permission_profile: &str,
    cancel: CancellationToken,
) -> Result<String, String> {
    let _ = cancel.is_cancelled();
    let msg = "runtime codex_cli is unavailable (app-server not implemented)".to_string();
    // Do not append Preparing/Started/Failed lifecycle events — RunManager is sole committer.
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
        let before = rt.events.replay_after("run-x", 0).len();
        let err = run_codex_cli_turn(&rt, "run-x", "hi", "m", None, "ask", cancel)
            .await
            .unwrap_err();
        assert!(err.contains("unavailable"));
        // Bridge must not write any events; RunManager commits Failed.
        let after = rt.events.replay_after("run-x", 0).len();
        assert_eq!(
            before, after,
            "codex bridge must not append any events to the sequencer"
        );
    }
}
