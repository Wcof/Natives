//! Daemon capability advertisement for Protocol v2.
//!
//! Honesty rules:
//! - `methods` only lists truly callable RPCs.
//! - Per-runtime status is executable | unavailable | undetermined.
//! - Codex stays unavailable until app-server is ready.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeAvailability {
    Executable,
    Unavailable,
    Undetermined,
}

impl RuntimeAvailability {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Executable => "executable",
            Self::Unavailable => "unavailable",
            Self::Undetermined => "undetermined",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeCapability {
    pub id: String,
    pub display_name: String,
    pub status: RuntimeAvailability,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub methods: Vec<String>,
}

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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runtimes: Vec<RuntimeCapability>,
}

impl DaemonCapabilities {
    pub fn current() -> Self {
        Self::from_method_lists(
            super::IMPLEMENTED_METHODS,
            vec![RuntimeCapability {
                id: "native".into(),
                display_name: "Native Daemon".into(),
                status: RuntimeAvailability::Executable,
                reason: None,
                methods: Vec::new(),
            }],
        )
    }

    /// Host-mediated surface: union daemon + host methods, with honest CLI rows.
    pub fn host_mediated(runtimes: Vec<RuntimeCapability>) -> Self {
        let mut methods: Vec<String> = super::IMPLEMENTED_METHODS
            .iter()
            .chain(super::HOST_IMPLEMENTED_METHODS.iter())
            .map(|s| (*s).to_string())
            .collect();
        methods.sort();
        methods.dedup();
        let mut caps = Self::from_method_lists(&[], runtimes);
        let base = Self::current();
        caps.methods = methods;
        caps.tools = base.tools;
        caps.hooks = base.hooks;
        caps.subagents = base.subagents;
        caps.mcp = base.mcp;
        caps.extensions = base.extensions;
        caps.scheduler = base.scheduler;
        caps.event_replay = base.event_replay;
        caps.credential_broker = base.credential_broker;
        caps.providers = base.providers;
        caps
    }

    fn from_method_lists(methods: &[&str], runtimes: Vec<RuntimeCapability>) -> Self {
        Self {
            protocol_version: super::PROTOCOL_V2.to_string(),
            methods: methods.iter().map(|s| (*s).to_string()).collect(),
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
            subagents: true,
            mcp: true,
            extensions: true,
            scheduler: false,
            event_replay: true,
            credential_broker: true,
            runtimes,
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
        assert!(caps.hooks);
        assert!(caps.subagents);
        assert!(caps.mcp);
        assert!(!caps.scheduler);
        assert!(caps.extensions);
        assert!(caps.methods.iter().any(|m| m == "run.start"));
        assert!(caps.methods.iter().any(|m| m == "extension.list"));
        assert!(!caps.methods.iter().any(|m| m.starts_with("scheduler.")));
        assert!(!caps.methods.iter().any(|m| m == "extension.enable"));
        assert!(caps.runtimes.iter().any(|r| r.id == "native"));
        assert!(!caps.methods.iter().any(|m| m == "mcp.auth.oauthStart"));
    }

    #[test]
    fn host_mediated_includes_host_methods_and_codex_unavailable() {
        let caps = DaemonCapabilities::host_mediated(vec![
            RuntimeCapability {
                id: "native".into(),
                display_name: "Native".into(),
                status: RuntimeAvailability::Executable,
                reason: None,
                methods: vec![],
            },
            RuntimeCapability {
                id: "codex_cli".into(),
                display_name: "Codex CLI".into(),
                status: RuntimeAvailability::Unavailable,
                reason: Some("app-server not implemented".into()),
                methods: vec![],
            },
        ]);
        assert!(caps.methods.iter().any(|m| m == "promptQueue.enqueue"));
        let codex = caps.runtimes.iter().find(|r| r.id == "codex_cli").unwrap();
        assert_eq!(codex.status, RuntimeAvailability::Unavailable);
    }
}
