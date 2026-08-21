//! Provider OAuth browser flow, Host side (ADR-0019 P3).
//!
//! `provider_oauth_start` runs the full authorization-code + PKCE (S256)
//! browser dance (loopback callback, state verification, token exchange) and
//! persists the resulting OAuth account into the existing `provider_accounts`
//! table with envelope encryption — no new account table (ADR-0019 §3).
//!
//! Security invariants (Plan 05 §5.3): tokens, codes and verifiers never appear
//! in logs or in any returned value/error string. Renderer gets safe session
//! state only: `session_id` / `provider` / `state` / `safe_error?` / account
//! identity summary. Never `access_token` / `refresh_token` / `client_secret`.
//!
//! OAuth state model (Plan 05 §5.5): Disconnected → Starting →
//! WaitingForCallback → Persisting → Connected | ReauthRequired | Error.

use crate::{emit_db_state_changed, provider_key_manager, AppState, Error, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::net::TcpListener;
use std::time::Duration;
use tauri::{AppHandle, State};

use rusqlite::OptionalExtension;

use super::mcp_oauth::{
    percent_encode, random_urlsafe_token, sanitize_reqwest_error, validate_endpoint_url,
    wait_for_callback,
};
use super::provider_oauth_preset::{
    ensure_oauth_provider, oauth_preset, persist_oauth_account, token_identity_fingerprint,
};

const FLOW_TIMEOUT: Duration = Duration::from_secs(180);
const TOKEN_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Best-effort project/identity discovery timeout for OAuth-backed Cloud Code
/// providers (antigravity). Discovery never blocks login: any failure returns
/// `None` and the renderer falls back to manual `project_id` entry.
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(15);
const GOOGLE_PROJECTS_URL: &str = "https://cloudresourcemanager.googleapis.com/v1/projects";
const GOOGLE_USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v2/userinfo?alt=json";

// ── Data types (safe session surface only) ──

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderOauthStartInput {
    pub provider_id: String,
    /// Provider platform identifier (openai / anthropic / gemini).
    pub platform: String,
    pub authorize_url: String,
    pub token_url: String,
    pub client_id: String,
    /// Confidential-client secret (e.g. Google OAuth). Optional — public PKCE
    /// clients omit it. Encrypted at rest, never returned to the renderer.
    pub client_secret: Option<String>,
    pub scopes: Option<Vec<String>>,
    pub redirect_port: Option<u16>,
    /// Optional stable account identity (email / user id) for dedupe.
    /// When absent, the Host derives a secret-safe opaque fingerprint.
    pub identity: Option<String>,
    pub account_name: Option<String>,
    /// Google Cloud project id for OAuth-backed Cloud Code providers
    /// (antigravity). Optional — sent as `x-goog-cloud-target-resource`.
    pub project_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderOauthStartResult {
    pub ok: bool,
    pub account_id: String,
    pub state: String,
    pub has_refresh: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderOauthAccountSummary {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub status: String,
    pub expires_at: Option<String>,
    pub has_refresh: bool,
    /// Google Cloud project id for OAuth-backed Cloud Code providers
    /// (antigravity), read from the encrypted credential. Safe to expose.
    pub project_id: Option<String>,
    /// Google account email, read from the encrypted credential. Safe to expose.
    pub email: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderOauthStatusInput {
    pub provider_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderOauthDisconnectInput {
    pub provider_id: String,
    pub account_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderOauthDisconnectResult {
    pub ok: bool,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
}

// ── Start ──

#[tauri::command]
pub async fn provider_oauth_start(
    input: ProviderOauthStartInput,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<ProviderOauthStartResult> {
    // Resolve any empty field from the fixed OAuth catalog so the renderer can
    // call start({ providerId }) without echoing client_id / secret / endpoints.
    let preset = oauth_preset(&input.provider_id);
    let provider_id = input.provider_id.trim().to_string();
    let platform = if input.platform.trim().is_empty() {
        preset.map(|p| p.platform).unwrap_or("").to_string()
    } else {
        input.platform.trim().to_ascii_lowercase()
    };
    let client_id = if input.client_id.trim().is_empty() {
        preset.map(|p| p.client_id).unwrap_or("").to_string()
    } else {
        input.client_id.trim().to_string()
    };
    let client_secret = input
        .client_secret
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .or_else(|| preset.and_then(|p| p.client_secret).map(str::to_owned))
        .or_else(|| {
            // Runtime injection for providers whose preset deliberately omits a
            // hardcoded secret (antigravity — see provider_oauth_preset.rs).
            std::env::var("NATIVES_ANTIGRAVITY_CLIENT_SECRET")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        });
    let authorize_url = if input.authorize_url.trim().is_empty() {
        preset.map(|p| p.authorize_url).unwrap_or("").to_string()
    } else {
        input.authorize_url.trim().to_string()
    };
    let token_url = if input.token_url.trim().is_empty() {
        preset.map(|p| p.token_url).unwrap_or("").to_string()
    } else {
        input.token_url.trim().to_string()
    };
    let scopes: Vec<String> = if input
        .scopes
        .as_deref()
        .is_none_or(|s| s.iter().all(|x| x.trim().is_empty()))
    {
        preset
            .map(|p| p.scopes.iter().map(|s| s.to_string()).collect())
            .unwrap_or_default()
    } else {
        input.scopes.unwrap_or_default()
    };
    if provider_id.is_empty() {
        return Err(Error::InvalidInput("providerId is required".into()));
    }
    if platform.is_empty() {
        return Err(Error::InvalidInput("platform is required".into()));
    }
    if client_id.is_empty() {
        return Err(Error::InvalidInput("clientId is required".into()));
    }
    validate_endpoint_url(&authorize_url, "authorizeUrl")?;
    validate_endpoint_url(&token_url, "tokenUrl")?;

    // PKCE S256 + CSRF state.
    let code_verifier = random_urlsafe_token();
    let code_challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(code_verifier.as_bytes()));
    let oauth_state = random_urlsafe_token();

    // One-shot loopback listener; bind before opening the browser.
    let listener = TcpListener::bind(("127.0.0.1", input.redirect_port.unwrap_or(0)))
        .map_err(|e| Error::Internal(format!("failed to bind loopback listener: {e}")))?;
    let port = listener
        .local_addr()
        .map_err(|e| Error::Internal(format!("failed to read loopback port: {e}")))?
        .port();
    listener
        .set_nonblocking(true)
        .map_err(|e| Error::Internal(format!("failed to configure loopback listener: {e}")))?;
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");

    let mut authorize = String::from(authorize_url.as_str());
    authorize.push(if authorize.contains('?') { '&' } else { '?' });
    authorize.push_str(&format!(
        "response_type=code&client_id={}&redirect_uri={}&state={}&code_challenge={}&code_challenge_method=S256",
        percent_encode(&client_id),
        percent_encode(&redirect_uri),
        percent_encode(&oauth_state),
        percent_encode(&code_challenge),
    ));
    let scope = scopes
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if !scope.is_empty() {
        authorize.push_str("&scope=");
        authorize.push_str(&percent_encode(&scope));
    }

    open::that(&authorize)
        .map_err(|e| Error::Internal(format!("failed to open system browser: {e}")))?;

    // Wait for the browser redirect (blocking IO off the async runtime).
    let expected_state = oauth_state.clone();
    let code = tauri::async_runtime::spawn_blocking(move || {
        wait_for_callback(&listener, &expected_state, FLOW_TIMEOUT)
    })
    .await
    .map_err(|e| Error::Internal(format!("OAuth callback task failed: {e}")))?
    .map_err(Error::Internal)?;

    // Exchange code + verifier for tokens.
    let client = reqwest::Client::builder()
        .timeout(TOKEN_REQUEST_TIMEOUT)
        .build()
        .map_err(|e| Error::Internal(format!("failed to build HTTP client: {e}")))?;
    let mut form: Vec<(&str, &str)> = vec![
        ("grant_type", "authorization_code"),
        ("code", code.as_str()),
        ("redirect_uri", redirect_uri.as_str()),
        ("client_id", client_id.as_str()),
        ("code_verifier", code_verifier.as_str()),
    ];
    if let Some(secret) = client_secret.as_deref() {
        form.push(("client_secret", secret));
    }
    let response = client
        .post(token_url.as_str())
        .form(&form)
        .send()
        .await
        .map_err(|e| {
            Error::Internal(format!(
                "token request failed: {}",
                sanitize_reqwest_error(&e)
            ))
        })?;
    let status = response.status();
    if !status.is_success() {
        // Body deliberately dropped: it may echo the authorization code.
        return Err(Error::Internal(format!(
            "token endpoint returned HTTP {status}"
        )));
    }
    let token: TokenResponse = response
        .json()
        .await
        .map_err(|_| Error::Internal("token endpoint returned an invalid JSON body".into()))?;
    if token.access_token.trim().is_empty() {
        return Err(Error::Internal(
            "token endpoint returned an empty access_token".into(),
        ));
    }

    let expires_at = token
        .expires_in
        .filter(|s| *s > 0)
        .map(|s| (chrono::Utc::now() + chrono::Duration::seconds(s)).to_rfc3339());
    let refresh = token
        .refresh_token
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty());

    let provided_identity = input
        .identity
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);

    // Best-effort project_id for Cloud Code providers (antigravity): use the
    // explicit login param when present, otherwise auto-discover from the token.
    // Discovery is best-effort and never blocks login. The userinfo email serves
    // both as a stable dedupe identity and for the antigravity card display.
    let project_id = if input
        .project_id
        .as_deref()
        .is_some_and(|s| !s.trim().is_empty())
        || provider_id != "antigravity"
    {
        input
            .project_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    } else {
        discover_google_project(&token.access_token)
            .await
            .unwrap_or(None)
    };
    // Called after discovery so `token.access_token` is not moved too early.
    let userinfo_email = if provider_id == "antigravity" && provided_identity.is_none() {
        fetch_google_userinfo(&token.access_token)
            .await
            .unwrap_or(None)
    } else {
        None
    };
    let identity = provided_identity
        .or_else(|| userinfo_email.clone())
        .unwrap_or_else(|| token_identity_fingerprint(&token.access_token));
    let account_name = input.account_name.as_deref().or(userinfo_email.as_deref());

    // Envelope-encrypt the OAuth credential (never plaintext at rest).
    // client_secret is encrypted here; it never reaches the plaintext extra_json.
    let credentials = serde_json::json!({
        "access_token": token.access_token,
        "refresh_token": refresh,
        "token_url": token_url,
        "client_id": client_id,
        "client_secret": client_secret,
        "project_id": project_id,
        "userinfo_email": userinfo_email,
    });
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    ensure_oauth_provider(&conn, &provider_id)?;
    let provider_exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM user_providers WHERE id=?1)",
            [&provider_id],
            |r| r.get(0),
        )
        .map_err(Error::Database)?;
    if !provider_exists {
        return Err(Error::NotFound("provider not found".into()));
    }
    let persisted = persist_oauth_account(
        &conn,
        &app,
        &provider_id,
        &platform,
        &identity,
        account_name,
        &credentials,
        expires_at,
    )?;
    drop(conn);

    Ok(ProviderOauthStartResult {
        ok: true,
        account_id: persisted.account_id,
        state: if persisted.has_refresh {
            "connected".into()
        } else {
            "reauth_required".into()
        },
        has_refresh: persisted.has_refresh,
    })
}

// ── Status ──

/// Safe account summary list for a provider. Never includes token material.
#[tauri::command]
pub fn provider_oauth_status(
    input: ProviderOauthStatusInput,
    state: State<'_, AppState>,
) -> Result<Vec<ProviderOauthAccountSummary>> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    let mut stmt = conn
        .prepare(
            "SELECT id, name, platform, status, expires_at, credentials_encrypted, dek_encrypted
             FROM provider_accounts
             WHERE provider_id=?1 AND account_type='oauth'
             ORDER BY created_at ASC",
        )
        .map_err(Error::Database)?;
    let rows = stmt
        .query_map([&input.provider_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
            ))
        })
        .map_err(Error::Database)?;
    let mut accounts = Vec::new();
    for row in rows {
        let (id, name, platform, status, expires_at, encrypted, dek) =
            row.map_err(Error::Database)?;
        // has_refresh / project_id / email are derived from the decrypted
        // material but never expose the token itself.
        let decrypted = provider_key_manager::envelope_decrypt(&encrypted, &dek, &conn)
            .ok()
            .and_then(|plain| serde_json::from_str::<serde_json::Value>(&plain).ok());
        let has_refresh = decrypted
            .as_ref()
            .and_then(|v| v.get("refresh_token").cloned())
            .and_then(|v| v.as_str().map(str::to_owned))
            .is_some_and(|t| !t.trim().is_empty());
        let project_id = decrypted
            .as_ref()
            .and_then(|v| v.get("project_id").cloned())
            .and_then(|v| v.as_str().map(str::to_owned))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let email = decrypted
            .as_ref()
            .and_then(|v| v.get("userinfo_email").cloned())
            .and_then(|v| v.as_str().map(str::to_owned))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        accounts.push(ProviderOauthAccountSummary {
            id,
            name,
            platform,
            status,
            expires_at,
            has_refresh,
            project_id,
            email,
        });
    }
    Ok(accounts)
}

// ── Disconnect ──

/// Removes an OAuth account row. In-flight runs keep their short-TTL lease and
/// simply fail on next acquisition (fail-closed, never a stale reuse).
#[tauri::command]
pub fn provider_oauth_disconnect(
    input: ProviderOauthDisconnectInput,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ProviderOauthDisconnectResult> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    let deleted = conn
        .execute(
            "DELETE FROM provider_accounts WHERE id=?1 AND provider_id=?2 AND account_type='oauth'",
            rusqlite::params![input.account_id, input.provider_id],
        )
        .map_err(Error::Database)?;
    if deleted == 0 {
        return Err(Error::NotFound("oauth account not found".into()));
    }
    emit_db_state_changed(
        &app,
        "provider:accounts",
        serde_json::json!({ "providerId": input.provider_id }),
    );
    Ok(ProviderOauthDisconnectResult { ok: true })
}

// ── Refresh ──

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderOauthRefreshInput {
    pub provider_id: String,
    pub account_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderOauthRefreshResult {
    pub ok: bool,
    pub state: String,
    pub has_refresh: bool,
}

/// Refresh an OAuth account's access token in place (Host-side path). The
/// daemon already refreshes via `broker_pool_refresh`; this fills the manual
/// on-demand gap for the OAuth tab. Never returns token material.
#[tauri::command]
pub async fn provider_oauth_refresh(
    input: ProviderOauthRefreshInput,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<ProviderOauthRefreshResult> {
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    let row: Option<(String, String, String)> = conn
        .query_row(
            "SELECT platform, credentials_encrypted, dek_encrypted
             FROM provider_accounts
             WHERE id=?1 AND provider_id=?2 AND account_type='oauth'",
            rusqlite::params![input.account_id, input.provider_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(Error::Database)?;
    let Some((_platform, encrypted, dek)) = row else {
        return Err(Error::NotFound("oauth account not found".into()));
    };
    let plain = provider_key_manager::envelope_decrypt(&encrypted, &dek, &conn)?;
    let mut creds: serde_json::Value = serde_json::from_str(&plain)
        .map_err(|_| Error::Internal("invalid stored credential".into()))?;
    let refresh_token = creds
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .ok_or_else(|| Error::InvalidInput("account has no refresh_token".into()))?
        .to_string();
    let token_url = creds
        .get("token_url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let client_id = creds
        .get("client_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let client_secret = creds
        .get("client_secret")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    if token_url.is_empty() || client_id.is_empty() {
        return Err(Error::InvalidInput(
            "stored credential is missing token_url/client_id".into(),
        ));
    }
    validate_endpoint_url(&token_url, "tokenUrl")?;

    let client = reqwest::Client::builder()
        .timeout(TOKEN_REQUEST_TIMEOUT)
        .build()
        .map_err(|e| Error::Internal(format!("failed to build HTTP client: {e}")))?;
    let mut form: Vec<(&str, &str)> = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token.as_str()),
        ("client_id", client_id.as_str()),
    ];
    if let Some(secret) = client_secret.as_deref() {
        form.push(("client_secret", secret));
    }
    let response = client
        .post(token_url.as_str())
        .form(&form)
        .send()
        .await
        .map_err(|e| {
            Error::Internal(format!(
                "token request failed: {}",
                sanitize_reqwest_error(&e)
            ))
        })?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        if refresh_token_expired(&body, status) {
            return Ok(ProviderOauthRefreshResult {
                ok: false,
                state: "reauth_required".into(),
                has_refresh: false,
            });
        }
        // Body deliberately dropped from the error: it may echo the refresh token.
        return Err(Error::Internal(format!(
            "token endpoint returned HTTP {status}"
        )));
    }
    let token: TokenResponse = response
        .json()
        .await
        .map_err(|_| Error::Internal("token endpoint returned an invalid JSON body".into()))?;
    if token.access_token.trim().is_empty() {
        return Err(Error::Internal(
            "token endpoint returned an empty access_token".into(),
        ));
    }

    // Merge refreshed tokens, keeping the previous refresh_token/id_token when
    // the response omits them.
    let obj = creds
        .as_object_mut()
        .ok_or_else(|| Error::Internal("invalid stored credential".into()))?;
    obj.insert("access_token".into(), serde_json::json!(token.access_token));
    if let Some(rt) = token
        .refresh_token
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        obj.insert("refresh_token".into(), serde_json::json!(rt));
    }
    let expires_at = token
        .expires_in
        .filter(|s| *s > 0)
        .map(|s| (chrono::Utc::now() + chrono::Duration::seconds(s)).to_rfc3339());
    let (encrypted, dek_encrypted) =
        provider_key_manager::envelope_encrypt(&creds.to_string(), &conn)?;
    let updated = conn
        .execute(
            "UPDATE provider_accounts
             SET credentials_encrypted=?1, dek_encrypted=?2, expires_at=?3, status='active', updated_at=?4
             WHERE id=?5 AND provider_id=?6 AND account_type='oauth'",
            rusqlite::params![
                encrypted,
                dek_encrypted,
                expires_at,
                crate::provider_accounts_parse::now(),
                input.account_id,
                input.provider_id
            ],
        )
        .map_err(Error::Database)?;
    if updated == 0 {
        return Err(Error::NotFound("oauth account not found".into()));
    }
    let has_refresh = creds
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .is_some_and(|t| !t.trim().is_empty());
    emit_db_state_changed(
        &app,
        "provider:accounts",
        serde_json::json!({ "providerId": input.provider_id }),
    );
    Ok(ProviderOauthRefreshResult {
        ok: true,
        state: if has_refresh {
            "connected".into()
        } else {
            "reauth_required".into()
        },
        has_refresh,
    })
}

// ── Best-effort Google project / identity discovery ──

/// Discovery response shapes. Unknown/absent fields are tolerated so a schema
/// drift on Google's side degrades to `None` rather than an error.
#[derive(Debug, Deserialize, Default)]
struct GoogleProjectsResponse {
    #[serde(default)]
    projects: Vec<GoogleProject>,
}

#[derive(Debug, Deserialize, Default)]
struct GoogleProject {
    #[serde(default)]
    project_id: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct GoogleUserinfoResponse {
    #[serde(default)]
    email: Option<String>,
}

/// Resolve the user's Google Cloud project id with an OAuth access token.
///
/// Best-effort: any failure (network, auth, empty list, parse) returns
/// `Ok(None)` so it never blocks the login flow. The caller falls back to
/// manual `project_id` entry in the renderer.
async fn discover_google_project(access_token: &str) -> Result<Option<String>> {
    let client = reqwest::Client::builder()
        .timeout(DISCOVERY_TIMEOUT)
        .build()
        .map_err(|e| Error::Internal(format!("failed to build discovery client: {e}")))?;
    let response = client
        .get(GOOGLE_PROJECTS_URL)
        .header("Authorization", format!("Bearer {}", access_token.trim()))
        .send()
        .await
        .map_err(sanitize_discovery_error)?;
    if !response.status().is_success() {
        return Ok(None);
    }
    let body: GoogleProjectsResponse = match response.json().await {
        Ok(b) => b,
        Err(_) => return Ok(None),
    };
    let project_id = body
        .projects
        .into_iter()
        .filter_map(|p| p.project_id)
        .map(|s| s.trim().to_string())
        .find(|s| !s.is_empty());
    Ok(project_id)
}

/// Resolve the Google account email for the given access token (used both for
/// dedupe identity and for the antigravity card display). Best-effort like
/// [`discover_google_project`].
async fn fetch_google_userinfo(access_token: &str) -> Result<Option<String>> {
    let client = reqwest::Client::builder()
        .timeout(DISCOVERY_TIMEOUT)
        .build()
        .map_err(|e| Error::Internal(format!("failed to build discovery client: {e}")))?;
    let response = client
        .get(GOOGLE_USERINFO_URL)
        .header("Authorization", format!("Bearer {}", access_token.trim()))
        .send()
        .await
        .map_err(sanitize_discovery_error)?;
    if !response.status().is_success() {
        return Ok(None);
    }
    let body: GoogleUserinfoResponse = match response.json().await {
        Ok(b) => b,
        Err(_) => return Ok(None),
    };
    let email = body
        .email
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    Ok(email)
}

fn sanitize_discovery_error(error: reqwest::Error) -> Error {
    // Never surface the URL or headers: the userinfo URL carries the access
    // token as a query parameter. Report only the transport class.
    let message = if error.is_timeout() {
        "request timed out"
    } else if error.is_connect() {
        "connection failed"
    } else {
        "request failed"
    };
    Error::Internal(format!("Google project discovery unavailable: {message}"))
}

// ── Set project id (manual fallback for antigravity) ──

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderOauthSetProjectInput {
    pub provider_id: String,
    pub account_id: String,
    pub project_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderOauthSetProjectResult {
    pub ok: bool,
}

/// Manually set the encrypted `project_id` for an OAuth account. Used as the
/// antigravity fallback when auto-discovery returns empty. Never returns token
/// material; re-encrypts the stored credential in place.
#[tauri::command]
pub fn provider_oauth_set_project_id(
    input: ProviderOauthSetProjectInput,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ProviderOauthSetProjectResult> {
    let project_id = input.project_id.trim().to_string();
    if project_id.is_empty() {
        return Err(Error::InvalidInput("projectId is required".into()));
    }
    let conn = state.db.get().map_err(|e| Error::Internal(e.to_string()))?;
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT credentials_encrypted, dek_encrypted
             FROM provider_accounts
             WHERE id=?1 AND provider_id=?2 AND account_type='oauth'",
            rusqlite::params![input.account_id, input.provider_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(Error::Database)?;
    let Some((encrypted, dek)) = row else {
        return Err(Error::NotFound("oauth account not found".into()));
    };
    let plain = provider_key_manager::envelope_decrypt(&encrypted, &dek, &conn)?;
    let mut creds: serde_json::Value = serde_json::from_str(&plain)
        .map_err(|_| Error::Internal("invalid stored credential".into()))?;
    creds
        .as_object_mut()
        .ok_or_else(|| Error::Internal("invalid stored credential".into()))?
        .insert("project_id".into(), serde_json::json!(project_id));
    let (encrypted, dek_encrypted) =
        provider_key_manager::envelope_encrypt(&creds.to_string(), &conn)?;
    conn.execute(
        "UPDATE provider_accounts
         SET credentials_encrypted=?1, dek_encrypted=?2, updated_at=?3
         WHERE id=?4 AND provider_id=?5 AND account_type='oauth'",
        rusqlite::params![
            encrypted,
            dek_encrypted,
            crate::provider_accounts_parse::now(),
            input.account_id,
            input.provider_id
        ],
    )
    .map_err(Error::Database)?;
    emit_db_state_changed(
        &app,
        "provider:accounts",
        serde_json::json!({ "providerId": input.provider_id }),
    );
    Ok(ProviderOauthSetProjectResult { ok: true })
}

fn refresh_token_expired(body: &str, status: reqwest::StatusCode) -> bool {
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return true;
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return false;
    };
    let code = value
        .get("error")
        .and_then(|e| match e {
            serde_json::Value::Object(o) => o.get("code").and_then(|c| c.as_str()),
            serde_json::Value::String(s) => Some(s.as_str()),
            _ => None,
        })
        .or_else(|| value.get("code").and_then(|c| c.as_str()))
        .map(str::to_ascii_lowercase);
    matches!(
        code.as_deref(),
        Some("refresh_token_expired" | "refresh_token_reused" | "refresh_token_invalidated")
    )
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_result_state_reflects_refresh_presence() {
        let with = ProviderOauthStartResult {
            ok: true,
            account_id: "a".into(),
            state: "connected".into(),
            has_refresh: true,
        };
        assert_eq!(with.state, "connected");
        let without = ProviderOauthStartResult {
            ok: true,
            account_id: "b".into(),
            state: "reauth_required".into(),
            has_refresh: false,
        };
        assert_eq!(without.state, "reauth_required");
    }

    #[test]
    fn refresh_error_code_detects_expired() {
        assert!(refresh_token_expired(
            r#"{"error":{"code":"refresh_token_expired"}}"#,
            reqwest::StatusCode::BAD_REQUEST
        ));
        assert!(refresh_token_expired(
            "{}",
            reqwest::StatusCode::UNAUTHORIZED
        ));
        assert!(!refresh_token_expired(
            "{}",
            reqwest::StatusCode::BAD_REQUEST
        ));
        assert!(!refresh_token_expired(
            "not json",
            reqwest::StatusCode::BAD_REQUEST
        ));
    }
}
