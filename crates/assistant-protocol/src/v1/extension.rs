use serde::{Deserialize, Serialize};

/// An extension (plugin, MCP server, skill, hook, command).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Extension {
    pub id: String,
    pub name: String,
    pub version: String,
    pub kind: ExtensionKind,
    pub enabled: bool,
    pub description: Option<String>,
    pub manifest: serde_json::Value,
    pub permissions: Vec<String>,
    pub health: ExtensionHealth,
}

/// Extension kind.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionKind {
    Plugin,
    McpServer,
    Skill,
    Hook,
    Command,
}

/// Extension health status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionHealth {
    Healthy,
    Degraded(String),
    Offline,
    Error(String),
}

/// An MCP server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub id: String,
    pub name: String,
    pub transport: McpTransport,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub env: Vec<String>,
    pub url: Option<String>,
    pub enabled: bool,
}

/// MCP transport type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum McpTransport {
    Stdio,
    HttpSse,
}

/// A skill manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: Option<String>,
    pub prompt: String,
    pub parameters: Option<serde_json::Value>,
}

// `HookRegistration`, `HookPoint`, and `HookFailStrategy` lived here as a third,
// never-referenced Hook model with its own event naming (`BeforeRun`,
// `BeforeToolCall`, …). They were removed once `harness-core` absorbed the two
// concepts they alone carried — a stable Hook identity and an explicit failure
// policy — into `HookDefinition` and `FailurePolicy`. The single Hook event
// enum is now `harness_core::hooks::HookEvent`.