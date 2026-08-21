//! Fixed OAuth provider catalog + shared persistence for the Provider OAuth
//! flow (ADR-0019 P8).
//!
//! The catalog hardcodes the four OAuth providers shown in the "OAuth" tab of
//! Provider Settings (Codex / Claude / antigravity / Kimi) with their flow
//! kind, endpoints, client ids/secrets and scopes — extracted from the
//! `cpa-core` kernel (and cross-checked against cc-switch's Codex device flow).
//!
//! Security: `client_secret` is treated as a credential — it is only ever
//! written into the envelope-encrypted `credentials` JSON, never the plaintext
//! `extra_json` column, and never returned to the renderer.

use crate::provider_accounts_parse::{digest, now};
use crate::{emit_db_state_changed, provider_key_manager, Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OauthFlowKind {
    Pkce,
    Device,
}

/// The `user_providers` row materialized when an OAuth provider is auto-created.
#[derive(Debug, Clone)]
pub(crate) struct OauthProviderRow {
    pub preset_name: &'static str,
    pub api_protocol: &'static str,
    pub name: &'static str,
    pub website_url: &'static str,
    pub base_url: &'static str,
    pub default_model: &'static str,
}

#[derive(Debug, Clone)]
pub(crate) struct OauthPreset {
    pub provider_id: &'static str,
    pub platform: &'static str,
    pub flow: OauthFlowKind,
    pub authorize_url: &'static str,
    pub token_url: &'static str,
    pub client_id: &'static str,
    pub client_secret: Option<&'static str>,
    pub scopes: &'static [&'static str],
    pub device_authorize_url: &'static str,
    pub device_poll_url: &'static str,
    pub redirect_uri_override: Option<&'static str>,
    pub verification_uri: &'static str,
    pub provider_row: OauthProviderRow,
}

impl OauthPreset {
    /// True when the client id is still a placeholder (login not wired yet).
    pub fn has_client_id(&self) -> bool {
        !self.client_id.is_empty() && self.client_id != "TBD"
    }
}

const CODEX_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";

pub(crate) fn oauth_preset(provider_id: &str) -> Option<&'static OauthPreset> {
    match provider_id.trim().to_ascii_lowercase().as_str() {
        "codex" => Some(&PRESET_CODEX),
        "claude" | "anthropic" => Some(&PRESET_CLAUDE),
        "antigravity" | "anti-gravity" => Some(&PRESET_ANTIGRAVITY),
        "kimi" => Some(&PRESET_KIMI),
        _ => None,
    }
}

// ── Catalog ──

static PRESET_CODEX: OauthPreset = OauthPreset {
    provider_id: "codex",
    platform: "openai",
    flow: OauthFlowKind::Device,
    authorize_url: "",
    token_url: "https://auth.openai.com/oauth/token",
    client_id: CODEX_CLIENT_ID,
    client_secret: None,
    scopes: &["openid", "profile", "email"],
    device_authorize_url: "https://auth.openai.com/api/accounts/deviceauth/usercode",
    device_poll_url: "https://auth.openai.com/api/accounts/deviceauth/token",
    redirect_uri_override: Some("https://auth.openai.com/deviceauth/callback"),
    verification_uri: "https://auth.openai.com/codex/device",
    provider_row: OauthProviderRow {
        preset_name: "codex",
        api_protocol: "openai_responses",
        name: "Codex",
        website_url: "https://openai.com/codex",
        base_url: "https://api.openai.com/v1",
        default_model: "gpt-5-codex",
    },
};

static PRESET_CLAUDE: OauthPreset = OauthPreset {
    provider_id: "claude",
    platform: "anthropic",
    flow: OauthFlowKind::Pkce,
    authorize_url: "https://claude.ai/oauth/authorize",
    token_url: "https://platform.claude.com/v1/oauth/token",
    // TODO: extract from cpa-core (embedded in Go string table). Blocked until provided.
    client_id: "TBD",
    client_secret: None,
    scopes: &[],
    device_authorize_url: "",
    device_poll_url: "",
    redirect_uri_override: None,
    verification_uri: "",
    provider_row: OauthProviderRow {
        preset_name: "claude",
        api_protocol: "anthropic_messages",
        name: "Claude",
        website_url: "https://claude.ai",
        base_url: "https://api.anthropic.com",
        default_model: "claude-sonnet-4-5",
    },
};

static PRESET_ANTIGRAVITY: OauthPreset = OauthPreset {
    provider_id: "antigravity",
    platform: "gemini",
    flow: OauthFlowKind::Pkce,
    authorize_url: "https://accounts.google.com/o/oauth2/v2/auth",
    token_url: "https://oauth2.googleapis.com/token",
    client_id: "",
    // Confidential-client secret is deliberately NOT hardcoded here (it is a
    // real Google credential extracted from cpa-core; committing it trips
    // GitHub secret scanning). Inject it at runtime via the
    // `NATIVES_ANTIGRAVITY_CLIENT_SECRET` env var, or pass `clientSecret`
    // through `provider_oauth_start`. PKCE flow can also run secretless.
    client_secret: None,
    scopes: &[
        "https://www.googleapis.com/auth/cclog",
        "https://www.googleapis.com/auth/cloud-platform",
        "https://www.googleapis.com/auth/experimentsandconfigs",
        "https://www.googleapis.com/auth/userinfo.profile",
    ],
    device_authorize_url: "",
    device_poll_url: "",
    redirect_uri_override: None,
    verification_uri: "",
    provider_row: OauthProviderRow {
        preset_name: "antigravity",
        api_protocol: "gemini_generate_content",
        name: "Antigravity",
        website_url: "https://antigravity.google",
        // Google Cloud Code API host (from cpa-core). The request path is
        // `/v1internal:generateContent` — a dedicated adapter, not the public
        // `generativelanguage.googleapis.com` Gemini adapter.
        base_url: "https://cloudcode-pa.googleapis.com",
        default_model: "gemini-2.5-pro",
    },
};

static PRESET_KIMI: OauthPreset = OauthPreset {
    provider_id: "kimi",
    platform: "openai",
    flow: OauthFlowKind::Device,
    authorize_url: "",
    token_url: "https://auth.kimi.com/api/oauth/token",
    // TODO: extract from cpa-core (embedded in Go string table). Blocked until provided.
    client_id: "TBD",
    client_secret: None,
    scopes: &[],
    device_authorize_url: "https://auth.kimi.com/api/oauth/device_authorization",
    device_poll_url: "https://auth.kimi.com/api/oauth/token",
    redirect_uri_override: None,
    verification_uri: "",
    provider_row: OauthProviderRow {
        preset_name: "kimi",
        api_protocol: "openai_chat_completions",
        name: "Kimi",
        website_url: "https://www.kimi.com",
        base_url: "https://api.moonshot.cn/v1",
        default_model: "kimi-k2",
    },
};

// ── Shared helpers ──

/// Idempotently create the `user_providers` row for a known OAuth preset.
///
/// Unknown ids return `Ok(())` without inserting so callers keep their existing
/// pre-registration gate for custom (non-preset) providers.
pub(crate) fn ensure_oauth_provider(conn: &rusqlite::Connection, provider_id: &str) -> Result<()> {
    let Some(preset) = oauth_preset(provider_id) else {
        return Ok(());
    };
    let row = &preset.provider_row;
    let stamp = now();
    conn.execute(
        "INSERT OR IGNORE INTO user_providers
            (id, preset_name, api_protocol, name, website_url, base_url, default_model, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
        rusqlite::params![
            preset.provider_id,
            row.preset_name,
            row.api_protocol,
            row.name,
            row.website_url,
            row.base_url,
            row.default_model,
            stamp
        ],
    )
    .map_err(Error::Database)?;
    Ok(())
}

pub(crate) struct PersistedOauthAccount {
    pub account_id: String,
    pub has_refresh: bool,
}

/// Build a stable, secret-safe fallback identity when an OAuth provider does
/// not return an account id or email. The raw token must never leave the
/// encrypted credential payload.
pub(crate) fn token_identity_fingerprint(access_token: &str) -> String {
    format!("token-fingerprint:{}", digest(access_token))
}

fn default_account_name(provider_id: &str) -> String {
    format!("{} OAuth account", provider_id.trim())
}

/// Envelope-encrypt an OAuth credential and upsert it into `provider_accounts`,
/// deduping on `(provider_id, identity_fingerprint)`. Shared by the PKCE and
/// device flows so there is one persistence implementation (R-B3).
///
/// `client_secret` (if present in `credentials`) is encrypted here and never
/// reaches the plaintext `extra_json` column.
pub(crate) fn persist_oauth_account(
    conn: &rusqlite::Connection,
    app: &tauri::AppHandle,
    provider_id: &str,
    platform: &str,
    identity: &str,
    account_name: Option<&str>,
    credentials: &serde_json::Value,
    expires_at: Option<String>,
) -> Result<PersistedOauthAccount> {
    let fingerprint = digest(&format!("{platform}:oauth:{identity}"));
    let (credentials_encrypted, dek_encrypted) =
        provider_key_manager::envelope_encrypt(&credentials.to_string(), conn)?;
    let account_id = uuid::Uuid::new_v4().to_string();
    let stamp = now();
    let name = account_name
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| default_account_name(provider_id));
    let token_url = credentials
        .get("token_url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let client_id = credentials
        .get("client_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let extra = serde_json::json!({ "token_url": token_url, "client_id": client_id });
    conn.execute(
        "INSERT INTO provider_accounts
            (id, provider_id, name, platform, account_type, credentials_encrypted,
             dek_encrypted, extra_json, concurrency, priority, expires_at, status,
             identity_fingerprint, created_at, updated_at)
         VALUES (?1,?2,?3,?4,'oauth',?5,?6,?7,1,0,?8,'active',?9,?10,?10)
         ON CONFLICT(provider_id, identity_fingerprint) DO UPDATE SET
             name=excluded.name, credentials_encrypted=excluded.credentials_encrypted,
             dek_encrypted=excluded.dek_encrypted, extra_json=excluded.extra_json,
             expires_at=excluded.expires_at, status='active', updated_at=excluded.updated_at",
        rusqlite::params![
            account_id,
            provider_id,
            name,
            platform,
            credentials_encrypted,
            dek_encrypted,
            extra.to_string(),
            expires_at,
            fingerprint,
            stamp
        ],
    )
    .map_err(Error::Database)?;
    let has_refresh = credentials
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .is_some_and(|t| !t.trim().is_empty());
    emit_db_state_changed(
        app,
        "provider:accounts",
        serde_json::json!({ "providerId": provider_id }),
    );
    Ok(PersistedOauthAccount {
        account_id,
        has_refresh,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_resolves_all_four_ids() {
        for id in ["codex", "claude", "antigravity", "kimi"] {
            assert!(oauth_preset(id).is_some(), "{id} must resolve");
        }
    }

    #[test]
    fn catalog_unknown_id_is_none() {
        assert!(oauth_preset("custom").is_none());
        assert!(oauth_preset("openai").is_none());
    }

    #[test]
    fn codex_and_antigravity_have_client_ids() {
        assert!(oauth_preset("codex").unwrap().has_client_id());
        assert!(oauth_preset("antigravity").unwrap().has_client_id());
        // The antigravity confidential secret is injected at runtime (env var /
        // start input), never hardcoded — see PRESET_ANTIGRAVITY.
        assert!(oauth_preset("antigravity").unwrap().client_secret.is_none());
    }

    #[test]
    fn fingerprint_is_stable_for_same_identity() {
        let a = digest("openai:oauth:user@example.com");
        let b = digest("openai:oauth:user@example.com");
        assert_eq!(a, b);
        let c = digest("openai:oauth:other@example.com");
        assert_ne!(a, c);
    }

    #[test]
    fn token_fallback_identity_never_contains_the_token() {
        let token = "oauth-access-secret";
        let identity = token_identity_fingerprint(token);
        assert!(!identity.contains(token));
        assert!(identity.starts_with("token-fingerprint:"));
        assert_eq!(identity, token_identity_fingerprint(token));
    }

    #[test]
    fn default_account_name_does_not_reuse_identity() {
        let identity = "oauth-access-secret";
        let name = default_account_name("codex");
        assert!(!name.contains(identity));
        assert_eq!(name, "codex OAuth account");
    }
}
