//! AI Resources 领域模型与契约（ADR-0020 §4 / plan3 03-data-security-contracts §1）。
//!
//! 核心领域实体：
//! - `Provider`: 厂商/品牌身份（如 OpenAI, Anthropic, DeepSeek, xAI），不按认证方式分类
//! - `Connection`: 真实 upstream 端点配置（Base URL, upstream protocol, 非敏感 headers, 代理 URL），不包含 Secret
//! - `Credential`: 凭据（API Key 或 OAuth 账号），包含 safe metadata + opaque `secret_ref`（持久化在 OS Keychain）
//! - `Model`: 模型目录条目（来源包含 OAuth 专属目录、Connection 真实 /models 发现、用户手工添加）
//! - `QuotaSnapshot` / `QuotaWindow`: 上游真实额度窗口快照，无可靠来源时标记 unknown

use serde::{Deserialize, Serialize};

/// 供应商 / 厂商身份 —— `ai_providers` 表行。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Provider {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preset_key: Option<String>,
    pub name: String,
    pub website_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_key: Option<String>,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// 上游协议类型
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UpstreamProtocol {
    OpenaiChatCompletions,
    OpenaiResponses,
    AnthropicMessages,
}

impl UpstreamProtocol {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::OpenaiChatCompletions => "openai_chat_completions",
            Self::OpenaiResponses => "openai_responses",
            Self::AnthropicMessages => "anthropic_messages",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "openai_chat_completions" | "chat_completions" | "openai" => {
                Some(Self::OpenaiChatCompletions)
            }
            "openai_responses" | "responses" => Some(Self::OpenaiResponses),
            "anthropic_messages" | "messages" | "anthropic" => Some(Self::AnthropicMessages),
            _ => None,
        }
    }
}

/// Connection 健康检查状态
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionHealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
    Unknown,
}

impl ConnectionHealthStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Degraded => "degraded",
            Self::Unhealthy => "unhealthy",
            Self::Unknown => "unknown",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "healthy" => Self::Healthy,
            "degraded" => Self::Degraded,
            "unhealthy" => Self::Unhealthy,
            _ => Self::Unknown,
        }
    }
}

/// 真实 upstream 连接端点 —— `ai_connections` 表行。**绝不保存 Secret**。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    pub id: String,
    pub provider_id: String,
    pub name: String,
    pub base_url: String,
    pub upstream_protocol: UpstreamProtocol,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub models_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers_json: Option<String>,
    pub enabled: bool,
    pub health_status: ConnectionHealthStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_checked_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// 凭据类型
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CredentialKind {
    ApiKey,
    Oauth,
}

impl CredentialKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ApiKey => "api_key",
            Self::Oauth => "oauth",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "api_key" | "apiKey" => Some(Self::ApiKey),
            "oauth" => Some(Self::Oauth),
            _ => None,
        }
    }
}

/// 凭据状态
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CredentialStatus {
    Active,
    Refreshing,
    Cooling,
    ReauthRequired,
    Invalid,
    Disabled,
}

impl CredentialStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Refreshing => "refreshing",
            Self::Cooling => "cooling",
            Self::ReauthRequired => "reauth_required",
            Self::Invalid => "invalid",
            Self::Disabled => "disabled",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "active" => Self::Active,
            "refreshing" => Self::Refreshing,
            "cooling" => Self::Cooling,
            "reauth_required" => Self::ReauthRequired,
            "invalid" => Self::Invalid,
            "disabled" => Self::Disabled,
            _ => Self::Invalid,
        }
    }
}

/// 凭据 —— `ai_credentials` 表行。
///
/// 包含 API Key 与 OAuth 账号的统一元数据，Secret 在 OS Keychain。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Credential {
    pub id: String,
    pub provider_id: String,
    pub kind: CredentialKind,
    pub label: String,
    /// Keychain opaque reference
    pub secret_ref: String,
    pub secret_revision: u32,
    pub masked_identity: String,
    pub status: CredentialStatus,
    pub priority: u32,
    pub concurrency_limit: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_refreshed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_refresh_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity_fingerprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_json: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// 模型来源
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelSource {
    Discovered,
    Oauth,
    Manual,
}

impl ModelSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Discovered => "discovered",
            Self::Oauth => "oauth",
            Self::Manual => "manual",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "discovered" => Self::Discovered,
            "oauth" => Self::Oauth,
            _ => Self::Manual,
        }
    }
}

/// 模型可用性
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelAvailability {
    Available,
    Unavailable,
    Unknown,
}

impl ModelAvailability {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Unavailable => "unavailable",
            Self::Unknown => "unknown",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "available" => Self::Available,
            "unavailable" => Self::Unavailable,
            _ => Self::Unknown,
        }
    }
}

/// 模型目录条目 —— `ai_models` 表行。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_credential_id: Option<String>,
    pub model_id: String,
    pub display_name: String,
    pub source: ModelSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities_json: Option<String>,
    pub availability: ModelAvailability,
    pub discovered_at: String,
    pub last_seen_at: String,
}

/// 额度快照状态
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QuotaStatus {
    Available,
    Unknown,
    Stale,
    Error,
}

impl QuotaStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Unknown => "unknown",
            Self::Stale => "stale",
            Self::Error => "error",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "available" => Self::Available,
            "stale" => Self::Stale,
            "error" => Self::Error,
            _ => Self::Unknown,
        }
    }
}

/// 额度窗口条目 —— `ai_quota_windows` 表行。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindow {
    pub id: String,
    pub snapshot_id: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remaining: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reset_at: Option<String>,
}

/// 额度快照 —— `ai_quota_snapshots` 表行。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QuotaSnapshot {
    pub id: String,
    pub credential_id: String,
    pub provider_adapter: String,
    pub status: QuotaStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    pub fetched_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    pub windows: Vec<QuotaWindow>,
}

/// AI Resources 总体计数/摘要
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AiResourcesSummary {
    pub provider_count: usize,
    pub connection_count: usize,
    pub credential_count: usize,
    pub available_model_count: usize,
}

/// 删除资源时的关联影响
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeleteImpact {
    pub connection_count: usize,
    pub credential_count: usize,
    pub model_count: usize,
    pub affected_route_count: usize,
}
