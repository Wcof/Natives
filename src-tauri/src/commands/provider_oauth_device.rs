//! OAuth device-code flow (RFC 8628), Host side (ADR-0019 P8).
//!
//! Codex and Kimi authorize via the device-code dance instead of a browser
//! PKCE redirect. `provider_oauth_device_start` registers a pending device code
//! and returns the `user_code` + `verification_uri` for the user to open;
//! `provider_oauth_device_poll` polls the provider and, once the user approves,
//! exchanges for tokens and persists the account (envelope-encrypted).
//!
//! The renderer only ever sees an opaque `session_id` + `user_code`; the
//! `device_code`/`device_auth_id` never leaves the Host.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::{AppState, Error, Result};

use super::mcp_oauth::sanitize_reqwest_error;
use super::provider_oauth_preset::{
    ensure_oauth_provider, oauth_preset, persist_oauth_account, OauthFlowKind,
};

const TOKEN_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const DEVICE_CODE_DEFAULT_EXPIRES_IN: u64 = 900;
const POLLING_SAFETY_MARGIN_SECS: u64 = 3;

// ── Data types ──

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderOauthDeviceStartInput {
    pub provider_id: String,
    pub platform: Option<String>,
    pub client_id: Option<String>,
    pub account_name: Option<String>,
    pub identity: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderOauthDeviceStartResult {
    pub ok: bool,
    pub session_id: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: Option<String>,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderOauthDevicePollInput {
    pub session_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderOauthDevicePollResult {
    pub status: String,
    pub account_id: Option<String>,
    pub state: Option<String>,
    pub has_refresh: Option<bool>,
    pub expires_in: Option<u64>,
    pub interval: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeviceFlowVariant {
    Codex,
    Kimi,
}

#[derive(Clone)]
struct PendingDeviceSession {
    provider_id: String,
    platform: String,
    client_id: String,
    token_url: String,
    device_poll_url: String,
    redirect_uri: String,
    variant: DeviceFlowVariant,
    device_code: String,
    user_code: String,
    expires_at_ms: i64,
    account_name: Option<String>,
    identity: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CodexDeviceStart {
    device_auth_id: String,
    user_code: String,
    #[serde(default)]
    interval: Option<serde_json::Value>,
    #[serde(default)]
    expires_in: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct CodexPollSuccess {
    authorization_code: String,
    code_verifier: String,
}

#[derive(Debug, Deserialize)]
struct KimiDeviceStart {
    device_code: String,
    user_code: String,
    verification_uri: String,
    #[serde(default)]
    verification_uri_complete: Option<String>,
    #[serde(default)]
    interval: Option<serde_json::Value>,
    #[serde(default)]
    expires_in: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct DeviceTokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
}

enum CodexPollOutcome {
    Pending,
    Expired,
    Granted {
        authorization_code: String,
        code_verifier: String,
    },
}

enum KimiPollOutcome {
    Pending,
    Expired,
    Granted(DeviceTokenResponse),
}

// ── Session store (memory-only, mirroring html_preview session pattern) ──

fn sessions() -> &'static Mutex<HashMap<String, PendingDeviceSession>> {
    static DEVICE_SESSIONS: OnceLock<Mutex<HashMap<String, PendingDeviceSession>>> =
        OnceLock::new();
    DEVICE_SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn prune_expired(map: &mut HashMap<String, PendingDeviceSession>) {
    let now = now_ms();
    map.retain(|_, s| s.expires_at_ms > now);
}

// ── Commands ──

#[tauri::command]
pub async fn provider_oauth_device_start(
    input: ProviderOauthDeviceStartInput,
) -> Result<ProviderOauthDeviceStartResult> {
    let preset = oauth_preset(&input.provider_id);
    let Some(preset) = preset else {
        return Err(Error::InvalidInput("unknown OAuth provider".into()));
    };
    if preset.flow != OauthFlowKind::Device {
        return Err(Error::InvalidInput(
            "provider does not use the device flow".into(),
        ));
    }
    if !preset.has_client_id() {
        return Err(Error::InvalidInput(
            "OAuth client id is not configured for this provider".into(),
        ));
    }
    let provider_id = preset.provider_id.to_string();
    let platform = input
        .platform
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_else(|| preset.platform.to_string());
    let client_id = input
        .client_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(preset.client_id)
        .to_string();
    let variant = if preset.provider_id == "codex" {
        DeviceFlowVariant::Codex
    } else {
        DeviceFlowVariant::Kimi
    };

    let client = reqwest::Client::builder()
        .timeout(TOKEN_REQUEST_TIMEOUT)
        .build()
        .map_err(|e| Error::Internal(format!("failed to build HTTP client: {e}")))?;

    let (device_code, user_code, verification_uri, verification_uri_complete, expires_in, interval) =
        match variant {
            DeviceFlowVariant::Codex => {
                let start =
                    codex_device_start(&client, preset.device_authorize_url, &client_id).await?;
                (
                    start.device_auth_id,
                    start.user_code,
                    preset.verification_uri.to_string(),
                    None,
                    start.expires_in.unwrap_or(DEVICE_CODE_DEFAULT_EXPIRES_IN),
                    parse_interval(start.interval.as_ref()),
                )
            }
            DeviceFlowVariant::Kimi => {
                let start =
                    kimi_device_start(&client, preset.device_authorize_url, &client_id).await?;
                (
                    start.device_code,
                    start.user_code,
                    start.verification_uri,
                    start.verification_uri_complete,
                    start.expires_in.unwrap_or(DEVICE_CODE_DEFAULT_EXPIRES_IN),
                    parse_interval(start.interval.as_ref()),
                )
            }
        };

    let session_id = uuid::Uuid::new_v4().to_string();
    let expires_at_ms = now_ms() + (expires_in as i64) * 1000;
    let session = PendingDeviceSession {
        provider_id,
        platform,
        client_id,
        token_url: preset.token_url.to_string(),
        device_poll_url: preset.device_poll_url.to_string(),
        redirect_uri: preset.redirect_uri_override.unwrap_or("").to_string(),
        variant,
        device_code,
        user_code: user_code.clone(),
        expires_at_ms,
        account_name: input.account_name,
        identity: input.identity,
    };
    {
        let mut map = sessions()
            .lock()
            .map_err(|e| Error::Internal(format!("session lock poisoned: {e}")))?;
        prune_expired(&mut map);
        map.insert(session_id.clone(), session);
    }

    Ok(ProviderOauthDeviceStartResult {
        ok: true,
        session_id,
        user_code,
        verification_uri,
        verification_uri_complete,
        expires_in,
        interval,
    })
}

#[tauri::command]
pub async fn provider_oauth_device_poll(
    input: ProviderOauthDevicePollInput,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<ProviderOauthDevicePollResult> {
    let session = {
        let map = sessions()
            .lock()
            .map_err(|e| Error::Internal(format!("session lock poisoned: {e}")))?;
        map.get(&input.session_id).cloned()
    };
    let Some(session) = session else {
        return Err(Error::NotFound("oauth session not found".into()));
    };
    if session.expires_at_ms <= now_ms() {
        remove_session(&input.session_id);
        return Ok(ProviderOauthDevicePollResult {
            status: "expired".into(),
            account_id: None,
            state: None,
            has_refresh: None,
            expires_in: None,
            interval: None,
        });
    }

    let client = reqwest::Client::builder()
        .timeout(TOKEN_REQUEST_TIMEOUT)
        .build()
        .map_err(|e| Error::Internal(format!("failed to build HTTP client: {e}")))?;

    let tokens = match session.variant {
        DeviceFlowVariant::Codex => {
            let outcome = codex_device_poll(
                &client,
                &session.device_poll_url,
                &session.device_code,
                &session.user_code,
            )
            .await?;
            match outcome {
                CodexPollOutcome::Pending => {
                    return Ok(pending_result());
                }
                CodexPollOutcome::Expired => {
                    remove_session(&input.session_id);
                    return Ok(expired_result());
                }
                CodexPollOutcome::Granted {
                    authorization_code,
                    code_verifier,
                } => {
                    codex_exchange_tokens(
                        &client,
                        &session.token_url,
                        &session.redirect_uri,
                        &session.client_id,
                        &authorization_code,
                        &code_verifier,
                    )
                    .await?
                }
            }
        }
        DeviceFlowVariant::Kimi => {
            let outcome = kimi_device_poll(
                &client,
                &session.token_url,
                &session.client_id,
                &session.device_code,
            )
            .await?;
            match outcome {
                KimiPollOutcome::Pending => {
                    return Ok(pending_result());
                }
                KimiPollOutcome::Expired => {
                    remove_session(&input.session_id);
                    return Ok(expired_result());
                }
                KimiPollOutcome::Granted(tokens) => tokens,
            }
        }
    };

    let expires_at = tokens
        .expires_in
        .filter(|s| *s > 0)
        .map(|s| (chrono::Utc::now() + chrono::Duration::seconds(s)).to_rfc3339());
    let refresh = tokens
        .refresh_token
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty());
    let identity = session
        .identity
        .clone()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| extract_identity(&tokens))
        .unwrap_or_else(|| tokens.access_token.clone());
    let credentials = serde_json::json!({
        "access_token": tokens.access_token,
        "refresh_token": refresh,
        "id_token": tokens.id_token,
        "token_url": session.token_url,
        "client_id": session.client_id,
    });

    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    ensure_oauth_provider(&conn, &session.provider_id)?;
    let persisted = persist_oauth_account(
        &conn,
        &app,
        &session.provider_id,
        &session.platform,
        &identity,
        session.account_name.as_deref(),
        &credentials,
        expires_at,
    )?;
    drop(conn);
    remove_session(&input.session_id);

    Ok(ProviderOauthDevicePollResult {
        status: "connected".into(),
        account_id: Some(persisted.account_id),
        state: Some(if persisted.has_refresh {
            "connected".into()
        } else {
            "reauth_required".into()
        }),
        has_refresh: Some(persisted.has_refresh),
        expires_in: None,
        interval: None,
    })
}

fn remove_session(session_id: &str) {
    if let Ok(mut map) = sessions().lock() {
        map.remove(session_id);
    }
}

fn pending_result() -> ProviderOauthDevicePollResult {
    ProviderOauthDevicePollResult {
        status: "pending".into(),
        account_id: None,
        state: None,
        has_refresh: None,
        expires_in: None,
        interval: None,
    }
}

fn expired_result() -> ProviderOauthDevicePollResult {
    ProviderOauthDevicePollResult {
        status: "expired".into(),
        account_id: None,
        state: None,
        has_refresh: None,
        expires_in: None,
        interval: None,
    }
}

// ── HTTP helpers ──

async fn codex_device_start(
    client: &reqwest::Client,
    usercode_url: &str,
    client_id: &str,
) -> Result<CodexDeviceStart> {
    let response = client
        .post(usercode_url)
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({ "client_id": client_id }))
        .send()
        .await
        .map_err(|e| {
            Error::Internal(format!(
                "device code request failed: {}",
                sanitize_reqwest_error(&e)
            ))
        })?;
    if !response.status().is_success() {
        return Err(Error::Internal(format!(
            "device code endpoint returned HTTP {}",
            response.status()
        )));
    }
    response
        .json()
        .await
        .map_err(|_| Error::Internal("device code endpoint returned invalid JSON".into()))
}

async fn codex_device_poll(
    client: &reqwest::Client,
    poll_url: &str,
    device_auth_id: &str,
    user_code: &str,
) -> Result<CodexPollOutcome> {
    let response = client
        .post(poll_url)
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "device_auth_id": device_auth_id,
            "user_code": user_code,
        }))
        .send()
        .await
        .map_err(|e| {
            Error::Internal(format!(
                "device poll failed: {}",
                sanitize_reqwest_error(&e)
            ))
        })?;
    let status = response.status();
    if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::NOT_FOUND {
        return Ok(CodexPollOutcome::Pending);
    }
    if status == reqwest::StatusCode::GONE {
        return Ok(CodexPollOutcome::Expired);
    }
    if !status.is_success() {
        return Err(Error::Internal(format!(
            "device poll returned HTTP {status}"
        )));
    }
    let success: CodexPollSuccess = response
        .json()
        .await
        .map_err(|_| Error::Internal("device poll returned invalid JSON".into()))?;
    Ok(CodexPollOutcome::Granted {
        authorization_code: success.authorization_code,
        code_verifier: success.code_verifier,
    })
}

async fn codex_exchange_tokens(
    client: &reqwest::Client,
    token_url: &str,
    redirect_uri: &str,
    client_id: &str,
    code: &str,
    code_verifier: &str,
) -> Result<DeviceTokenResponse> {
    let response = client
        .post(token_url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", client_id),
            ("code_verifier", code_verifier),
        ])
        .send()
        .await
        .map_err(|e| {
            Error::Internal(format!(
                "token exchange failed: {}",
                sanitize_reqwest_error(&e)
            ))
        })?;
    if !response.status().is_success() {
        // Body deliberately dropped: it may echo the authorization code.
        return Err(Error::Internal(format!(
            "token endpoint returned HTTP {}",
            response.status()
        )));
    }
    response
        .json()
        .await
        .map_err(|_| Error::Internal("token endpoint returned invalid JSON".into()))
}

async fn kimi_device_start(
    client: &reqwest::Client,
    device_authorize_url: &str,
    client_id: &str,
) -> Result<KimiDeviceStart> {
    let response = client
        .post(device_authorize_url)
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({ "client_id": client_id }))
        .send()
        .await
        .map_err(|e| {
            Error::Internal(format!(
                "device code request failed: {}",
                sanitize_reqwest_error(&e)
            ))
        })?;
    if !response.status().is_success() {
        return Err(Error::Internal(format!(
            "device code endpoint returned HTTP {}",
            response.status()
        )));
    }
    response
        .json()
        .await
        .map_err(|_| Error::Internal("device code endpoint returned invalid JSON".into()))
}

async fn kimi_device_poll(
    client: &reqwest::Client,
    token_url: &str,
    client_id: &str,
    device_code: &str,
) -> Result<KimiPollOutcome> {
    let response = client
        .post(token_url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("device_code", device_code),
            ("client_id", client_id),
        ])
        .send()
        .await
        .map_err(|e| {
            Error::Internal(format!(
                "device poll failed: {}",
                sanitize_reqwest_error(&e)
            ))
        })?;
    let status = response.status();
    if status.is_success() {
        let tokens: DeviceTokenResponse = response
            .json()
            .await
            .map_err(|_| Error::Internal("device poll returned invalid JSON".into()))?;
        return Ok(KimiPollOutcome::Granted(tokens));
    }
    let body = response.text().await.unwrap_or_default();
    let error = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_owned))
        .unwrap_or_default();
    match error.as_str() {
        "authorization_pending" | "slow_down" => Ok(KimiPollOutcome::Pending),
        "expired_token" | "access_denied" => Ok(KimiPollOutcome::Expired),
        _ => Err(Error::Internal(format!(
            "device poll returned HTTP {status}"
        ))),
    }
}

// ── Pure helpers ──

fn parse_interval(value: Option<&serde_json::Value>) -> u64 {
    let raw = match value {
        Some(serde_json::Value::Number(n)) => n.as_u64().unwrap_or(5),
        Some(serde_json::Value::String(s)) => s.parse::<u64>().unwrap_or(5),
        _ => 5,
    };
    raw.max(1) + POLLING_SAFETY_MARGIN_SECS
}

fn jwt_claim(token: &str, key: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let decoded = URL_SAFE_NO_PAD.decode(payload).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
    value.get(key)?.as_str().map(str::to_owned)
}

fn extract_identity(tokens: &DeviceTokenResponse) -> Option<String> {
    let id_token = tokens.id_token.as_deref();
    for token in [id_token, Some(tokens.access_token.as_str())]
        .into_iter()
        .flatten()
    {
        if let Some(id) = jwt_claim(token, "chatgpt_account_id") {
            return Some(id);
        }
        if let Some(email) = jwt_claim(token, "email") {
            return Some(email);
        }
    }
    None
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_interval_handles_number_string_default_and_min() {
        assert_eq!(
            parse_interval(Some(&serde_json::Value::Number(5.into()))),
            5 + POLLING_SAFETY_MARGIN_SECS
        );
        assert_eq!(
            parse_interval(Some(&serde_json::Value::String("10".into()))),
            10 + POLLING_SAFETY_MARGIN_SECS
        );
        assert_eq!(parse_interval(None), 5 + POLLING_SAFETY_MARGIN_SECS);
        assert_eq!(
            parse_interval(Some(&serde_json::Value::Number(0.into()))),
            1 + POLLING_SAFETY_MARGIN_SECS
        );
    }

    #[test]
    fn extract_identity_reads_email_from_id_token() {
        let header = URL_SAFE_NO_PAD.encode("{\"alg\":\"none\"}");
        let payload = URL_SAFE_NO_PAD.encode("{\"email\":\"u@example.com\"}");
        let id_token = format!("{header}.{payload}.sig");
        let tokens = DeviceTokenResponse {
            access_token: "at".into(),
            refresh_token: None,
            id_token: Some(id_token),
            expires_in: None,
        };
        assert_eq!(extract_identity(&tokens).as_deref(), Some("u@example.com"));
    }
}
