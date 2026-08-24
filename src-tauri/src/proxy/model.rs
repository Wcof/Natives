//! Local Proxy 领域模型与契约（ADR-0020 §4 / plan3 03-data-security-contracts §1）。
//!
//! 核心领域实体：
//! - `ProxySettings`: 本地代理配置（单例）
//! - `ProxyRuntimeStatus`: 代理服务运行时状态（Stopped, Starting, Running, Restarting, Stopping, Failed）
//! - `Route`: 本地模型别名路由
//! - `RouteTarget`: 路由目标（Connection + upstream model + Credential Selector）
//! - `CredentialSelector` / `PoolPolicy`: 凭证池选择策略
//! - `ProxyUsageRecord`: 代理调用用量审计

use serde::{Deserialize, Serialize};

/// 凭证选择策略
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CredentialSelector {
    /// 显式指定单 Credential ID
    Credential { id: String },
    /// 凭证池（按策略自动选择）
    Pool { policy: PoolPolicy },
}

/// 凭证池调度策略
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PoolPolicy {
    /// 优先级优先，同优先级轮转
    PriorityRoundRobin,
    /// 纯轮转
    RoundRobin,
    /// 最少并发在途
    LeastInflight,
}

impl PoolPolicy {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PriorityRoundRobin => "priority_round_robin",
            Self::RoundRobin => "round_robin",
            Self::LeastInflight => "least_inflight",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "round_robin" => Self::RoundRobin,
            "least_inflight" => Self::LeastInflight,
            _ => Self::PriorityRoundRobin,
        }
    }
}

/// 路由目标 —— `proxy_route_targets` 表行。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RouteTarget {
    pub id: String,
    pub route_id: String,
    pub position: u32,
    pub connection_id: String,
    pub model_id: String,
    pub credential_selector: CredentialSelector,
    pub priority: u32,
    pub enabled: bool,
}

/// 本地模型路由 —— `proxy_routes` 表行。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Route {
    pub id: String,
    pub local_model: String,
    pub enabled: bool,
    pub strategy: String,
    pub targets: Vec<RouteTarget>,
    pub created_at: String,
    pub updated_at: String,
}

/// 端口配置模式
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PortMode {
    Dynamic,
    Fixed,
}

impl PortMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Dynamic => "dynamic",
            Self::Fixed => "fixed",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "fixed" => Self::Fixed,
            _ => Self::Dynamic,
        }
    }
}

/// 本地 Proxy 持久化配置 —— `proxy_settings` 表行。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProxySettings {
    pub id: String,
    pub enabled_intent: bool,
    pub bind_host: String,
    pub port_mode: PortMode,
    pub configured_port: u16,
    pub effective_port: u16,
    pub access_secret_ref: String,
    pub grace_timeout_ms: u64,
    pub max_concurrency: u32,
    pub max_request_body_bytes: usize,
    pub updated_at: String,
}

impl Default for ProxySettings {
    fn default() -> Self {
        Self {
            id: "default".to_string(),
            enabled_intent: false,
            bind_host: "127.0.0.1".to_string(),
            port_mode: PortMode::Dynamic,
            configured_port: 15721,
            effective_port: 0,
            access_secret_ref: "natives/proxy/local-access/v1".to_string(),
            grace_timeout_ms: 5000,
            max_concurrency: 64,
            max_request_body_bytes: 32 * 1024 * 1024,
            updated_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}

/// 运行时状态枚举
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProxyRuntimeStatus {
    Stopped,
    Starting,
    Running,
    Restarting,
    Stopping,
    Failed,
}

impl ProxyRuntimeStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Stopped => "stopped",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Restarting => "restarting",
            Self::Stopping => "stopping",
            Self::Failed => "failed",
        }
    }
}

/// 供前端/外部查询的 Proxy 实时状态 DTO
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProxyStatusDTO {
    pub running: bool,
    pub status: ProxyRuntimeStatus,
    pub host: String,
    pub port: u16,
    pub effective_port: u16,
    pub started_at: Option<String>,
    pub uptime_seconds: u64,
    pub active_requests: usize,
    pub route_count: usize,
    pub engine: String,
    pub last_error: Option<String>,
    pub protocol_endpoints: Vec<ProxyEndpointInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProxyEndpointInfo {
    pub protocol: String,
    pub path: String,
    pub url: String,
}

/// 路由/凭据运行态健康与冷却信息
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CredentialHealthInfo {
    pub credential_id: String,
    pub status: String,
    pub in_flight: usize,
    pub total_calls: u64,
    pub success_calls: u64,
    pub failed_calls: u64,
    pub cooling_until: Option<String>,
    pub last_error: Option<String>,
}

/// Proxy 用量记录 —— `proxy_usage_records` 表行。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProxyUsageRecord {
    pub id: String,
    pub route_id: Option<String>,
    pub connection_id: Option<String>,
    pub credential_id: Option<String>,
    pub inbound_protocol: String,
    pub upstream_protocol: String,
    pub local_model: String,
    pub upstream_model: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub reasoning_tokens: Option<u64>,
    pub cached_tokens: Option<u64>,
    pub latency_ms: u64,
    pub status: String,
    pub error_code: Option<String>,
    pub created_at: String,
}

// 兼容旧引用
pub type ProxyStatus = ProxyStatusDTO;
pub type RouteDefinition = Route;
