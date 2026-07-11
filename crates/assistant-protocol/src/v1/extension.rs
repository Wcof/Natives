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

/// A hook registration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookRegistration {
    pub id: String,
    pub hook_point: HookPoint,
    pub priority: i32,
    pub handler: String,
    pub timeout_ms: u64,
    pub fail_strategy: HookFailStrategy,
}

/// Hook points in the agent lifecycle.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HookPoint {
    BeforeRun,
    AfterRun,
    BeforePrompt,
    AfterPrompt,
    BeforeToolCall,
    AfterToolCall,
    BeforePermission,
    AfterPermission,
    OnCompletion,
    OnError,
}

/// What happens when a hook fails.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HookFailStrategy {
    /// Fail the entire run.
    Fail,
    /// Skip this hook and continue.
    Skip,
    /// Use a default value and continue.
    Default,
}