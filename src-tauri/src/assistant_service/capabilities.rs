//! Runtime capability honesty for settings / RuntimePanel.
use crate::runtime::AgentRuntime;

use super::{success_response, RpcResponse};

pub(crate) async fn handle_host_get_capabilities() -> RpcResponse {
    use assistant_protocol::v2::{DaemonCapabilities, RuntimeAvailability, RuntimeCapability};

    let mut runtimes = vec![RuntimeCapability {
        id: "native".into(),
        display_name: "Native Daemon".into(),
        status: RuntimeAvailability::Executable,
        reason: None,
        methods: vec![],
    }];

    let meta = crate::runtime::registry::list_runtime_metadata().await;
    let mut seen = std::collections::HashSet::new();
    for m in &meta {
        seen.insert(m.id.clone());
        let (status, reason) = if m.id == "codex_cli" {
            (
                RuntimeAvailability::Unavailable,
                Some("app-server not implemented".into()),
            )
        } else if m.available {
            (RuntimeAvailability::Executable, None)
        } else {
            (
                RuntimeAvailability::Unavailable,
                Some(format!("{} binary not found", m.display_name)),
            )
        };
        runtimes.push(RuntimeCapability {
            id: m.id.clone(),
            display_name: m.display_name.clone(),
            status,
            reason,
            methods: vec![],
        });
    }
    if !seen.contains("claude_cli") {
        let claude = crate::runtime::claude_cli::ClaudeCliRuntime::new();
        runtimes.push(RuntimeCapability {
            id: "claude_cli".into(),
            display_name: "Claude CLI".into(),
            status: if claude.is_available() {
                RuntimeAvailability::Executable
            } else {
                RuntimeAvailability::Unavailable
            },
            reason: if claude.is_available() {
                None
            } else {
                Some("claude binary not found".into())
            },
            methods: vec![],
        });
    }
    if !seen.contains("codex_cli") {
        runtimes.push(RuntimeCapability {
            id: "codex_cli".into(),
            display_name: "Codex CLI".into(),
            status: RuntimeAvailability::Unavailable,
            reason: Some("app-server not implemented".into()),
            methods: vec![],
        });
    }

    let caps = DaemonCapabilities::host_mediated(runtimes);
    success_response(serde_json::to_value(caps).unwrap_or_default())
}

// ─── Provider catalog (host-owned) ───
