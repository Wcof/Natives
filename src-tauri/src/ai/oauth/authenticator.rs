//! OAuth Authenticators（OAU-001..006）。
//!
//! 包含 Codex, Claude, Antigravity, Kimi, xAI 的 Token 交换、刷新、模型和真实额度获取。

use reqwest::header::AUTHORIZATION;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

use super::catalog::{PRESET_ANTIGRAVITY, PRESET_CLAUDE, PRESET_CODEX, PRESET_KIMI, PRESET_XAI};
use super::session::OauthTokenBundle;

const HTTP_TIMEOUT: Duration = Duration::from_secs(15);

// ── JWT Payload Parser ──

#[derive(Debug, Deserialize)]
struct JwtClaims {
    email: Option<String>,
    sub: Option<String>,
    #[serde(rename = "https://api.openai.com/auth")]
    openai_auth: Option<OpenAiAuthCustomClaim>,
}

#[derive(Debug, Deserialize)]
struct OpenAiAuthCustomClaim {
    #[allow(dead_code)]
    user_id: Option<String>,
    account_id: Option<String>,
    email: Option<String>,
    plan_name: Option<String>,
}

fn parse_jwt_claims(jwt: &str) -> Option<JwtClaims> {
    let parts: Vec<&str> = jwt.split('.').collect();
    if parts.len() < 2 {
        return None;
    }
    let payload_b64 = parts[1];
    let decoded = base64::Engine::decode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        payload_b64,
    )
    .or_else(|_| base64::Engine::decode(&base64::engine::general_purpose::STANDARD, payload_b64))
    .ok()?;

    serde_json::from_slice::<JwtClaims>(&decoded).ok()
}

// ── Singleflight Refresh Lock ──

pub struct RefreshLockManager {
    locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

impl Default for RefreshLockManager {
    fn default() -> Self {
        Self::new()
    }
}

impl RefreshLockManager {
    pub fn new() -> Self {
        Self {
            locks: Mutex::new(HashMap::new()),
        }
    }

    pub async fn get_lock(&self, refresh_token: &str) -> Arc<Mutex<()>> {
        let mut map = self.locks.lock().await;
        map.entry(refresh_token.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }
}

// ── Authenticator Implementation ──

pub struct OauthAuthenticator {
    client: reqwest::Client,
    refresh_locks: Arc<RefreshLockManager>,
}

impl Default for OauthAuthenticator {
    fn default() -> Self {
        Self::new()
    }
}

impl OauthAuthenticator {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            client,
            refresh_locks: Arc::new(RefreshLockManager::new()),
        }
    }

    // ── 1. Codex ──

    pub async fn codex_request_device_code(
        &self,
    ) -> Result<(String, String, String, u64, u64), String> {
        let params = [
            ("client_id", PRESET_CODEX.client_id),
            ("scope", "openid profile email offline_access"),
        ];

        let resp = self
            .client
            .post(PRESET_CODEX.device_authorize_url)
            .form(&params)
            .send()
            .await
            .map_err(|e| format!("Codex device code request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Codex device code error ({status}): {body}"));
        }

        #[derive(Deserialize)]
        struct DeviceResp {
            device_code: String,
            user_code: String,
            verification_uri: Option<String>,
            expires_in: Option<u64>,
            interval: Option<u64>,
        }

        let data: DeviceResp = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse Codex device response: {e}"))?;

        Ok((
            data.device_code,
            data.user_code,
            data.verification_uri
                .unwrap_or_else(|| PRESET_CODEX.verification_uri.to_string()),
            data.expires_in.unwrap_or(900),
            data.interval.unwrap_or(5),
        ))
    }

    pub async fn codex_poll_device_token(
        &self,
        device_code: &str,
    ) -> Result<Option<OauthTokenBundle>, String> {
        let params = [
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("client_id", PRESET_CODEX.client_id),
            ("device_code", device_code),
        ];

        let resp = self
            .client
            .post(PRESET_CODEX.device_poll_url)
            .form(&params)
            .send()
            .await
            .map_err(|e| format!("Codex poll request failed: {e}"))?;

        let status = resp.status();
        if status.is_success() {
            #[derive(Deserialize)]
            struct TokenResp {
                access_token: String,
                refresh_token: Option<String>,
                id_token: Option<String>,
                expires_in: Option<u64>,
                token_type: Option<String>,
            }

            let data: TokenResp = resp
                .json()
                .await
                .map_err(|e| format!("Failed to parse token response: {e}"))?;

            let mut email = None;
            let mut account_id = None;
            let mut plan_name = None;

            if let Some(id_tok) = &data.id_token {
                if let Some(claims) = parse_jwt_claims(id_tok) {
                    email = claims.email;
                    account_id = claims.sub;
                    if let Some(custom) = claims.openai_auth {
                        if custom.email.is_some() {
                            email = custom.email;
                        }
                        if custom.account_id.is_some() {
                            account_id = custom.account_id;
                        }
                        plan_name = custom.plan_name;
                    }
                }
            }

            Ok(Some(OauthTokenBundle {
                access_token: data.access_token,
                refresh_token: data.refresh_token,
                id_token: data.id_token,
                token_type: data.token_type,
                expires_in_secs: data.expires_in,
                account_id,
                account_email: email,
                project_id: None,
                plan_name,
            }))
        } else {
            let body = resp.text().await.unwrap_or_default();
            if body.contains("authorization_pending") || body.contains("slow_down") {
                Ok(None)
            } else {
                Err(format!("Codex authorization failed: {body}"))
            }
        }
    }

    pub async fn codex_refresh_token(
        &self,
        refresh_token: &str,
    ) -> Result<OauthTokenBundle, String> {
        let lock = self.refresh_locks.get_lock(refresh_token).await;
        let _guard = lock.lock().await;

        let params = [
            ("client_id", PRESET_CODEX.client_id),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("scope", "openid profile email"),
        ];

        let resp = self
            .client
            .post(PRESET_CODEX.token_url)
            .form(&params)
            .send()
            .await
            .map_err(|e| format!("Codex refresh request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Codex refresh failed ({status}): {body}"));
        }

        #[derive(Deserialize)]
        struct RefreshResp {
            access_token: String,
            refresh_token: Option<String>,
            id_token: Option<String>,
            expires_in: Option<u64>,
            token_type: Option<String>,
        }

        let data: RefreshResp = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse Codex refresh response: {e}"))?;

        let mut email = None;
        let mut account_id = None;
        let mut plan_name = None;

        if let Some(id_tok) = &data.id_token {
            if let Some(claims) = parse_jwt_claims(id_tok) {
                email = claims.email;
                account_id = claims.sub;
                if let Some(custom) = claims.openai_auth {
                    if custom.email.is_some() {
                        email = custom.email;
                    }
                    if custom.account_id.is_some() {
                        account_id = custom.account_id;
                    }
                    plan_name = custom.plan_name;
                }
            }
        }

        Ok(OauthTokenBundle {
            access_token: data.access_token,
            refresh_token: data
                .refresh_token
                .or_else(|| Some(refresh_token.to_string())),
            id_token: data.id_token,
            token_type: data.token_type,
            expires_in_secs: data.expires_in,
            account_id,
            account_email: email,
            project_id: None,
            plan_name,
        })
    }

    // ── 2. Claude ──

    pub async fn claude_exchange_code(
        &self,
        code: &str,
        code_verifier: &str,
        redirect_uri: &str,
    ) -> Result<OauthTokenBundle, String> {
        let params = [
            ("grant_type", "authorization_code"),
            ("client_id", PRESET_CLAUDE.client_id),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("code_verifier", code_verifier),
        ];

        let resp = self
            .client
            .post(PRESET_CLAUDE.token_url)
            .form(&params)
            .send()
            .await
            .map_err(|e| format!("Claude token exchange failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Claude token exchange failed ({status}): {body}"));
        }

        #[derive(Deserialize)]
        struct ClaudeTokenResp {
            access_token: String,
            refresh_token: Option<String>,
            expires_in: Option<u64>,
            token_type: Option<String>,
        }

        let data: ClaudeTokenResp = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse Claude token response: {e}"))?;

        let mut email = None;
        let mut account_id = None;
        let mut plan_name = None;

        if let Ok(prof_resp) = self
            .client
            .get("https://api.anthropic.com/api/oauth/profile")
            .header(AUTHORIZATION, format!("Bearer {}", data.access_token))
            .send()
            .await
        {
            if prof_resp.status().is_success() {
                if let Ok(v) = prof_resp.json::<serde_json::Value>().await {
                    email = v.get("email").and_then(|s| s.as_str()).map(str::to_string);
                    account_id = v.get("id").and_then(|s| s.as_str()).map(str::to_string);
                    plan_name = v.get("plan").and_then(|s| s.as_str()).map(str::to_string);
                }
            }
        }

        Ok(OauthTokenBundle {
            access_token: data.access_token,
            refresh_token: data.refresh_token,
            id_token: None,
            token_type: data.token_type,
            expires_in_secs: data.expires_in,
            account_id,
            account_email: email,
            project_id: None,
            plan_name,
        })
    }

    pub async fn claude_refresh_token(
        &self,
        refresh_token: &str,
    ) -> Result<OauthTokenBundle, String> {
        let lock = self.refresh_locks.get_lock(refresh_token).await;
        let _guard = lock.lock().await;

        let params = [
            ("client_id", PRESET_CLAUDE.client_id),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ];

        let resp = self
            .client
            .post(PRESET_CLAUDE.token_url)
            .form(&params)
            .send()
            .await
            .map_err(|e| format!("Claude refresh request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Claude refresh failed ({status}): {body}"));
        }

        #[derive(Deserialize)]
        struct RefreshResp {
            access_token: String,
            refresh_token: Option<String>,
            expires_in: Option<u64>,
            token_type: Option<String>,
        }

        let data: RefreshResp = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse Claude refresh response: {e}"))?;

        Ok(OauthTokenBundle {
            access_token: data.access_token,
            refresh_token: data
                .refresh_token
                .or_else(|| Some(refresh_token.to_string())),
            id_token: None,
            token_type: data.token_type,
            expires_in_secs: data.expires_in,
            account_id: None,
            account_email: None,
            project_id: None,
            plan_name: None,
        })
    }

    // ── 3. Antigravity (Google PKCE) ──

    pub async fn antigravity_exchange_code(
        &self,
        code: &str,
        code_verifier: &str,
        redirect_uri: &str,
    ) -> Result<OauthTokenBundle, String> {
        let params = [
            ("grant_type", "authorization_code"),
            ("client_id", PRESET_ANTIGRAVITY.client_id),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("code_verifier", code_verifier),
        ];

        let resp = self
            .client
            .post(PRESET_ANTIGRAVITY.token_url)
            .form(&params)
            .send()
            .await
            .map_err(|e| format!("Antigravity token exchange failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!(
                "Antigravity token exchange failed ({status}): {body}"
            ));
        }

        #[derive(Deserialize)]
        struct GoogleTokenResp {
            access_token: String,
            refresh_token: Option<String>,
            id_token: Option<String>,
            expires_in: Option<u64>,
            token_type: Option<String>,
        }

        let data: GoogleTokenResp = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse Google token response: {e}"))?;

        let mut email = None;
        let mut account_id = None;
        if let Some(id_tok) = &data.id_token {
            if let Some(claims) = parse_jwt_claims(id_tok) {
                email = claims.email;
                account_id = claims.sub;
            }
        }

        Ok(OauthTokenBundle {
            access_token: data.access_token,
            refresh_token: data.refresh_token,
            id_token: data.id_token,
            token_type: data.token_type,
            expires_in_secs: data.expires_in,
            account_id,
            account_email: email,
            project_id: None,
            plan_name: Some("Google Cloud Code".into()),
        })
    }

    pub async fn antigravity_refresh_token(
        &self,
        refresh_token: &str,
    ) -> Result<OauthTokenBundle, String> {
        let lock = self.refresh_locks.get_lock(refresh_token).await;
        let _guard = lock.lock().await;

        let params = [
            ("client_id", PRESET_ANTIGRAVITY.client_id),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ];

        let resp = self
            .client
            .post(PRESET_ANTIGRAVITY.token_url)
            .form(&params)
            .send()
            .await
            .map_err(|e| format!("Google refresh request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Google refresh failed ({status}): {body}"));
        }

        #[derive(Deserialize)]
        struct RefreshResp {
            access_token: String,
            id_token: Option<String>,
            expires_in: Option<u64>,
            token_type: Option<String>,
        }

        let data: RefreshResp = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse Google refresh response: {e}"))?;

        Ok(OauthTokenBundle {
            access_token: data.access_token,
            refresh_token: Some(refresh_token.to_string()),
            id_token: data.id_token,
            token_type: data.token_type,
            expires_in_secs: data.expires_in,
            account_id: None,
            account_email: None,
            project_id: None,
            plan_name: Some("Google Cloud Code".into()),
        })
    }

    // ── 4. Kimi ──

    pub async fn kimi_request_device_code(
        &self,
    ) -> Result<(String, String, String, u64, u64), String> {
        let params = [("client_id", PRESET_KIMI.client_id)];

        let resp = self
            .client
            .post(PRESET_KIMI.device_authorize_url)
            .form(&params)
            .send()
            .await
            .map_err(|e| format!("Kimi device code request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Kimi device code error ({status}): {body}"));
        }

        #[derive(Deserialize)]
        struct DeviceResp {
            device_code: String,
            user_code: String,
            verification_uri: Option<String>,
            expires_in: Option<u64>,
            interval: Option<u64>,
        }

        let data: DeviceResp = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse Kimi device response: {e}"))?;

        Ok((
            data.device_code,
            data.user_code,
            data.verification_uri
                .unwrap_or_else(|| PRESET_KIMI.verification_uri.to_string()),
            data.expires_in.unwrap_or(900),
            data.interval.unwrap_or(5),
        ))
    }

    pub async fn kimi_poll_device_token(
        &self,
        device_code: &str,
    ) -> Result<Option<OauthTokenBundle>, String> {
        let params = [
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("client_id", PRESET_KIMI.client_id),
            ("device_code", device_code),
        ];

        let resp = self
            .client
            .post(PRESET_KIMI.device_poll_url)
            .form(&params)
            .send()
            .await
            .map_err(|e| format!("Kimi poll request failed: {e}"))?;

        let status = resp.status();
        if status.is_success() {
            #[derive(Deserialize)]
            struct TokenResp {
                access_token: String,
                refresh_token: Option<String>,
                expires_in: Option<u64>,
                token_type: Option<String>,
            }

            let data: TokenResp = resp
                .json()
                .await
                .map_err(|e| format!("Failed to parse Kimi token response: {e}"))?;

            Ok(Some(OauthTokenBundle {
                access_token: data.access_token,
                refresh_token: data.refresh_token,
                id_token: None,
                token_type: data.token_type,
                expires_in_secs: data.expires_in,
                account_id: None,
                account_email: None,
                project_id: None,
                plan_name: Some("Kimi Coding".into()),
            }))
        } else {
            let body = resp.text().await.unwrap_or_default();
            if body.contains("authorization_pending") || body.contains("slow_down") {
                Ok(None)
            } else {
                Err(format!("Kimi authorization failed: {body}"))
            }
        }
    }

    pub async fn kimi_refresh_token(
        &self,
        refresh_token: &str,
    ) -> Result<OauthTokenBundle, String> {
        let lock = self.refresh_locks.get_lock(refresh_token).await;
        let _guard = lock.lock().await;

        let params = [
            ("client_id", PRESET_KIMI.client_id),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ];

        let resp = self
            .client
            .post(PRESET_KIMI.token_url)
            .form(&params)
            .send()
            .await
            .map_err(|e| format!("Kimi refresh request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Kimi refresh failed ({status}): {body}"));
        }

        #[derive(Deserialize)]
        struct RefreshResp {
            access_token: String,
            refresh_token: Option<String>,
            expires_in: Option<u64>,
            token_type: Option<String>,
        }

        let data: RefreshResp = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse Kimi refresh response: {e}"))?;

        Ok(OauthTokenBundle {
            access_token: data.access_token,
            refresh_token: data
                .refresh_token
                .or_else(|| Some(refresh_token.to_string())),
            id_token: None,
            token_type: data.token_type,
            expires_in_secs: data.expires_in,
            account_id: None,
            account_email: None,
            project_id: None,
            plan_name: Some("Kimi Coding".into()),
        })
    }

    // ── 5. xAI / Grok ──

    pub async fn xai_request_device_code(
        &self,
    ) -> Result<(String, String, String, u64, u64), String> {
        let params = [
            ("client_id", PRESET_XAI.client_id),
            (
                "scope",
                "openid profile email offline_access grok-cli:access api:access",
            ),
        ];

        let resp = self
            .client
            .post(PRESET_XAI.device_authorize_url)
            .form(&params)
            .send()
            .await
            .map_err(|e| format!("xAI device code request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("xAI device code error ({status}): {body}"));
        }

        #[derive(Deserialize)]
        struct DeviceResp {
            device_code: String,
            user_code: String,
            verification_uri: Option<String>,
            expires_in: Option<u64>,
            interval: Option<u64>,
        }

        let data: DeviceResp = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse xAI device response: {e}"))?;

        Ok((
            data.device_code,
            data.user_code,
            data.verification_uri
                .unwrap_or_else(|| PRESET_XAI.verification_uri.to_string()),
            data.expires_in.unwrap_or(900),
            data.interval.unwrap_or(5),
        ))
    }

    pub async fn xai_poll_device_token(
        &self,
        device_code: &str,
    ) -> Result<Option<OauthTokenBundle>, String> {
        let params = [
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("client_id", PRESET_XAI.client_id),
            ("device_code", device_code),
        ];

        let resp = self
            .client
            .post(PRESET_XAI.device_poll_url)
            .form(&params)
            .send()
            .await
            .map_err(|e| format!("xAI poll request failed: {e}"))?;

        let status = resp.status();
        if status.is_success() {
            #[derive(Deserialize)]
            struct TokenResp {
                access_token: String,
                refresh_token: Option<String>,
                id_token: Option<String>,
                expires_in: Option<u64>,
                token_type: Option<String>,
            }

            let data: TokenResp = resp
                .json()
                .await
                .map_err(|e| format!("Failed to parse xAI token response: {e}"))?;

            let mut email = None;
            let mut account_id = None;
            if let Some(id_tok) = &data.id_token {
                if let Some(claims) = parse_jwt_claims(id_tok) {
                    email = claims.email;
                    account_id = claims.sub;
                }
            }

            Ok(Some(OauthTokenBundle {
                access_token: data.access_token,
                refresh_token: data.refresh_token,
                id_token: data.id_token,
                token_type: data.token_type,
                expires_in_secs: data.expires_in,
                account_id,
                account_email: email,
                project_id: None,
                plan_name: Some("xAI Grok".into()),
            }))
        } else {
            let body = resp.text().await.unwrap_or_default();
            if body.contains("authorization_pending") || body.contains("slow_down") {
                Ok(None)
            } else {
                Err(format!("xAI authorization failed: {body}"))
            }
        }
    }

    pub async fn xai_refresh_token(&self, refresh_token: &str) -> Result<OauthTokenBundle, String> {
        let lock = self.refresh_locks.get_lock(refresh_token).await;
        let _guard = lock.lock().await;

        let params = [
            ("client_id", PRESET_XAI.client_id),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ];

        let resp = self
            .client
            .post(PRESET_XAI.token_url)
            .form(&params)
            .send()
            .await
            .map_err(|e| format!("xAI refresh request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("xAI refresh failed ({status}): {body}"));
        }

        #[derive(Deserialize)]
        struct RefreshResp {
            access_token: String,
            refresh_token: Option<String>,
            id_token: Option<String>,
            expires_in: Option<u64>,
            token_type: Option<String>,
        }

        let data: RefreshResp = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse xAI refresh response: {e}"))?;

        Ok(OauthTokenBundle {
            access_token: data.access_token,
            refresh_token: data
                .refresh_token
                .or_else(|| Some(refresh_token.to_string())),
            id_token: data.id_token,
            token_type: data.token_type,
            expires_in_secs: data.expires_in,
            account_id: None,
            account_email: None,
            project_id: None,
            plan_name: Some("xAI Grok".into()),
        })
    }
}
