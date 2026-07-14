// ── Daemon Module ──
// Agent Daemon sidecar — currently a placeholder.
// This module is declared for future use but contains only stubs.
// The existing data.rs and rpc_server.rs contain incomplete prototypes
// that depend on modules not yet ported. They are preserved as-is
// but not compiled (no `pub mod` re-export).

use crate::Result;

/// Placeholder struct for the agent daemon.
pub struct AgentDaemon;

impl AgentDaemon {
    pub fn new() -> Self {
        Self
    }
}
