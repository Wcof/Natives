use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Provider type identifier.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderType {
    Openai,
    Anthropic,
    Gemini,
    Deepseek,
    OpenaiCompatible,
    Ollama,
}

/// A provider configuration (vendor + endpoint + key references).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub id: String,
    pub provider_type: ProviderType,
    pub display_name: String,
    pub api_base_url: String,
    pub organization_id: Option<String>,
    pub project_id: Option<String>,
    pub proxy_url: Option<String>,
    pub timeout_secs: Option<u64>,
    pub default_model: Option<String>,
    pub health_status: ProviderHealthStatus,
    pub last_test_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Provider health status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderHealthStatus {
    Unknown,
    Verified,
    Unverified,
    Error(String),
}

/// A provider API key (encrypted; only metadata is returned to the frontend).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderKey {
    pub id: String,
    pub provider_id: String,
    /// Masked key (e.g. "sk-...AbCd").
    pub masked_key: String,
    /// Key label for display.
    pub label: Option<String>,
    pub is_active: bool,
    pub last_test_at: Option<DateTime<Utc>>,
    pub last_test_ok: Option<bool>,
    pub created_at: DateTime<Utc>,
}

/// Input for adding a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateProviderInput {
    pub provider_type: ProviderType,
    pub display_name: String,
    pub api_base_url: String,
    pub organization_id: Option<String>,
    pub project_id: Option<String>,
    pub proxy_url: Option<String>,
    pub timeout_secs: Option<u64>,
    pub default_model: Option<String>,
}

/// Input for adding a provider key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddProviderKeyInput {
    pub provider_id: String,
    pub api_key: String,
    pub label: Option<String>,
    pub is_active: bool,
}

/// Result of a provider connection test.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderTestResult {
    pub success: bool,
    pub category: TestResultCategory,
    pub latency_ms: Option<u64>,
    pub message: String,
    pub models_count: Option<u32>,
}

/// Category of test result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TestResultCategory {
    Ok,
    AuthError,
    NetworkError,
    RateLimited,
    ModelNotFound,
    ProtocolIncompatible,
    Timeout,
    Unknown,
}

/// Model capability discovery result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredModel {
    pub id: String,
    pub display_name: Option<String>,
    pub capabilities: ModelCapabilities,
    pub context_window: u64,
    pub max_output: u64,
    pub source: ModelSource,
    pub discovered_at: DateTime<Utc>,
}

/// Model capability flags.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCapabilities {
    pub streaming: bool,
    pub image_input: bool,
    pub file_input: bool,
    pub reasoning: bool,
    pub tool_calling: bool,
    pub structured_output: bool,
    pub function_calling: bool,
    pub system_prompt: bool,
}

/// Source of model information.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelSource {
    /// Live discovery from provider API.
    ApiDiscovery,
    /// Cached from last successful discovery.
    Cache,
    /// Versioned official preset.
    Preset,
    /// User manually added.
    Manual,
}