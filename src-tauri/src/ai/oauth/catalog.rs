//! OAuth Provider Catalog & Constants（OAU-001..006 / plan3 01-reference-audit §3）。
//!
//! 覆盖五大 OAuth 供应商：Codex, Claude, Antigravity, Kimi, xAI。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OauthFlowType {
    Pkce,
    Device,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OauthProviderPreset {
    pub provider_id: &'static str,
    pub name: &'static str,
    pub platform: &'static str,
    pub flow: OauthFlowType,
    pub authorize_url: &'static str,
    pub token_url: &'static str,
    pub client_id: &'static str,
    pub scopes: &'static [&'static str],
    pub device_authorize_url: &'static str,
    pub device_poll_url: &'static str,
    pub verification_uri: &'static str,
    pub default_base_url: &'static str,
    pub upstream_protocol: &'static str,
    pub default_models: &'static [(&'static str, &'static str)],
}

pub static PRESET_CODEX: OauthProviderPreset = OauthProviderPreset {
    provider_id: "codex",
    name: "Codex / OpenAI",
    platform: "openai",
    flow: OauthFlowType::Device,
    authorize_url: "https://auth.openai.com/oauth/authorize",
    token_url: "https://auth.openai.com/oauth/token",
    client_id: "app_EMoamEEZ73f0CkXaXp7hrann",
    scopes: &["openid", "profile", "email", "offline_access"],
    device_authorize_url: "https://auth.openai.com/api/accounts/deviceauth/usercode",
    device_poll_url: "https://auth.openai.com/api/accounts/deviceauth/token",
    verification_uri: "https://auth.openai.com/codex/device",
    default_base_url: "https://api.openai.com/v1",
    upstream_protocol: "openai_responses",
    default_models: &[
        ("gpt-5-codex", "GPT-5 Codex"),
        ("o3-mini", "o3-mini"),
        ("o1", "o1"),
        ("gpt-4o", "GPT-4o"),
        ("gpt-4o-mini", "GPT-4o mini"),
    ],
};

pub static PRESET_CLAUDE: OauthProviderPreset = OauthProviderPreset {
    provider_id: "claude",
    name: "Claude / Anthropic",
    platform: "anthropic",
    flow: OauthFlowType::Pkce,
    authorize_url: "https://claude.ai/oauth/authorize",
    token_url: "https://platform.claude.com/v1/oauth/token",
    client_id: "9d1c250a-e61b-44d9-88ed-5944d1962f5e",
    scopes: &[
        "user:profile",
        "user:inference",
        "user:sessions:claude_code",
        "user:mcp_servers",
        "user:file_upload",
    ],
    device_authorize_url: "",
    device_poll_url: "",
    verification_uri: "",
    default_base_url: "https://api.anthropic.com",
    upstream_protocol: "anthropic_messages",
    default_models: &[
        ("claude-3-7-sonnet-20250219", "Claude 3.7 Sonnet"),
        ("claude-3-5-sonnet-20241022", "Claude 3.5 Sonnet"),
        ("claude-3-5-haiku-20241022", "Claude 3.5 Haiku"),
    ],
};

pub static PRESET_ANTIGRAVITY: OauthProviderPreset = OauthProviderPreset {
    provider_id: "antigravity",
    name: "Antigravity / Google",
    platform: "gemini",
    flow: OauthFlowType::Pkce,
    authorize_url: "https://accounts.google.com/o/oauth2/v2/auth",
    token_url: "https://oauth2.googleapis.com/token",
    client_id: "",
    scopes: &[
        "https://www.googleapis.com/auth/cclog",
        "https://www.googleapis.com/auth/cloud-platform",
        "https://www.googleapis.com/auth/experimentsandconfigs",
        "https://www.googleapis.com/auth/userinfo.profile",
    ],
    device_authorize_url: "",
    device_poll_url: "",
    verification_uri: "",
    default_base_url: "https://cloudcode-pa.googleapis.com",
    upstream_protocol: "openai_chat_completions",
    default_models: &[
        ("gemini-2.5-pro", "Gemini 2.5 Pro"),
        ("gemini-2.5-flash", "Gemini 2.5 Flash"),
        ("gemini-2.0-flash", "Gemini 2.0 Flash"),
    ],
};

pub static PRESET_KIMI: OauthProviderPreset = OauthProviderPreset {
    provider_id: "kimi",
    name: "Kimi / Moonshot",
    platform: "openai",
    flow: OauthFlowType::Device,
    authorize_url: "",
    token_url: "https://auth.kimi.com/api/oauth/token",
    client_id: "17e5f671-d194-4dfb-9706-5516cb48c098",
    scopes: &[],
    device_authorize_url: "https://auth.kimi.com/api/oauth/device_authorization",
    device_poll_url: "https://auth.kimi.com/api/oauth/token",
    verification_uri: "https://auth.kimi.com/device",
    default_base_url: "https://api.kimi.com/coding/v1",
    upstream_protocol: "openai_chat_completions",
    default_models: &[
        ("kimi-k2", "Kimi K2"),
        ("moonshot-v1-auto", "Moonshot V1 Auto"),
    ],
};

pub static PRESET_XAI: OauthProviderPreset = OauthProviderPreset {
    provider_id: "xai",
    name: "xAI / Grok",
    platform: "openai",
    flow: OauthFlowType::Device,
    authorize_url: "https://auth.x.ai/oauth/authorize",
    token_url: "https://auth.x.ai/oauth/token",
    client_id: "b1a00492-073a-47ea-816f-4c329264a828",
    scopes: &[
        "openid",
        "profile",
        "email",
        "offline_access",
        "grok-cli:access",
        "api:access",
    ],
    device_authorize_url: "https://auth.x.ai/api/oauth/device_authorization",
    device_poll_url: "https://auth.x.ai/oauth/token",
    verification_uri: "https://auth.x.ai/device",
    default_base_url: "https://api.x.ai/v1",
    upstream_protocol: "openai_chat_completions",
    default_models: &[
        ("grok-3", "Grok 3"),
        ("grok-3-mini", "Grok 3 Mini"),
        ("grok-2", "Grok 2"),
    ],
};

pub static OAUTH_PRESETS: &[&OauthProviderPreset] = &[
    &PRESET_CODEX,
    &PRESET_CLAUDE,
    &PRESET_ANTIGRAVITY,
    &PRESET_KIMI,
    &PRESET_XAI,
];

pub fn get_oauth_preset(provider_id: &str) -> Option<&'static OauthProviderPreset> {
    match provider_id.trim().to_ascii_lowercase().as_str() {
        "codex" | "openai" => Some(&PRESET_CODEX),
        "claude" | "anthropic" => Some(&PRESET_CLAUDE),
        "antigravity" | "anti-gravity" | "google" => Some(&PRESET_ANTIGRAVITY),
        "kimi" | "moonshot" => Some(&PRESET_KIMI),
        "xai" | "grok" => Some(&PRESET_XAI),
        _ => None,
    }
}
