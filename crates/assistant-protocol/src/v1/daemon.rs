use serde::{Deserialize, Serialize};

/// Daemon status information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub version: String,
    pub protocol_version: String,
    pub uptime_secs: u64,
    pub pid: u64,
    pub active_runs: u32,
    pub active_extensions: u32,
    pub provider_count: u32,
    pub memory_usage_mb: u64,
    pub health: DaemonHealth,
}

/// Daemon health status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DaemonHealth {
    Healthy,
    Degraded(Vec<String>),
    Unhealthy(String),
}

/// Daemon capabilities declaration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonCapabilities {
    pub protocol_version: String,
    pub features: Vec<String>,
    pub max_concurrent_runs: u32,
    pub max_concurrent_sub_agents: u32,
    pub supported_providers: Vec<String>,
    pub has_plugin_host: bool,
    pub has_mcp_support: bool,
}

/// Handshake request (initial bootstrap).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeRequest {
    pub client_version: String,
    pub client_id: String,
    pub bootstrap_token: String,
}

/// Handshake response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeResponse {
    pub session_token: String,
    pub daemon_version: String,
    pub protocol_version: String,
    pub accepted: bool,
    pub upgrade_required: Option<String>,
}

/// RPC request envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRequest {
    pub protocol_version: String,
    pub request_id: String,
    pub client_id: String,
    pub session_token: String,
    pub method: String,
    pub params: serde_json::Value,
}

/// RPC response envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcResponse {
    pub protocol_version: String,
    pub request_id: String,
    pub success: bool,
    pub data: Option<serde_json::Value>,
    pub error: Option<serde_json::Value>,
}

/// Stream subscription request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscribeRequest {
    pub run_id: String,
    pub last_sequence: Option<u64>,
}

/// Stream event envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamEvent {
    pub run_id: String,
    pub sequence: u64,
    pub event: serde_json::Value,
}