//! Daemon capability advertisement for Protocol v2.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DaemonCapabilities {
    pub protocol_version: String,
    pub methods: Vec<String>,
    pub providers: Vec<String>,
    pub tools: bool,
    pub hooks: bool,
    pub subagents: bool,
    pub mcp: bool,
    pub extensions: bool,
    pub scheduler: bool,
    pub event_replay: bool,
    pub credential_broker: bool,
}

impl DaemonCapabilities {
    pub fn current() -> Self {
        Self {
            protocol_version: super::PROTOCOL_V2.to_string(),
            // Honest surface only — unimplemented catalogue entries stay in ALL_METHODS.
            methods: super::IMPLEMENTED_METHODS
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            providers: vec![
                "openai".into(),
                "openai_compatible".into(),
                "anthropic".into(),
                "gemini".into(),
                "deepseek".into(),
                "ollama".into(),
            ],
            tools: true,
            hooks: true,
            subagents: true, // engine/task path; list RPC still limited
            // MCP: registry + list RPC (stdio lifecycle still partial — not full transport).
            mcp: true,
            // Extension: list/enable/trust (install/host isolation still partial).
            extensions: true,
            // Scheduler: CRUD + persistence (cron runner still partial).
            scheduler: true,
            event_replay: true,
            credential_broker: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_advertise_v2_and_core_flags() {
        let caps = DaemonCapabilities::current();
        assert_eq!(caps.protocol_version, "2.0.0");
        assert!(caps.event_replay);
        assert!(caps.tools);
        assert!(caps.methods.iter().any(|m| m == "run.start"));
        assert!(
            caps.methods.iter().any(|m| m == "scheduler.create"),
            "scheduler.create is implemented and must be advertised"
        );
        assert!(
            caps.methods.iter().any(|m| m == "mcp.list"),
            "mcp.list is implemented and must be advertised"
        );
        assert!(caps.mcp, "mcp.list is implemented");
        assert!(!caps.methods.iter().any(|m| m == "mcp.auth.oauthStart"));
        assert!(!caps.methods.iter().any(|m| m == "mcp.auth.oauthCallback"));
        assert!(caps.scheduler, "scheduler CRUD is implemented");
        assert!(caps.extensions, "extension.list/enable implemented");
        assert!(
            caps.methods.iter().any(|m| m == "extension.enable"),
            "extension.enable must be advertised when implemented"
        );
        assert!(
            caps.methods.iter().any(|m| m == "skill.list"),
            "skill.list must be advertised"
        );
        assert!(
            caps.methods.iter().any(|m| m == "memory.search"),
            "memory.search must be advertised"
        );
        assert!(
            caps.methods.iter().any(|m| m == "artifact.open"),
            "artifact.open must be advertised when implemented"
        );
        let json = serde_json::to_string(&caps).unwrap();
        let back: DaemonCapabilities = serde_json::from_str(&json).unwrap();
        assert_eq!(back, caps);
    }

    #[test]
    fn implemented_methods_are_subset_of_catalogue() {
        for m in super::super::IMPLEMENTED_METHODS {
            assert!(
                super::super::is_known_method(m),
                "implemented method missing from ALL_METHODS: {m}"
            );
        }
    }
}
