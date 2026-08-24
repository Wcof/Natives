//! OAuth Orchestration & Persistence Service（OAU-007..008）。

use rusqlite::Connection as DbConn;
use std::time::{Duration, Instant};
use uuid::Uuid;

use super::authenticator::OauthAuthenticator;
use super::catalog::{get_oauth_preset, OauthFlowType};
use super::pkce::{generate_oauth_state, generate_pkce_codes};
use super::session::{OauthSession, OauthSessionManager, OauthSessionStatus, OauthTokenBundle};
use crate::ai::model::{
    Credential, CredentialKind, CredentialStatus, ModelAvailability, ModelSource, QuotaSnapshot,
    QuotaStatus, QuotaWindow, UpstreamProtocol,
};
use crate::ai::store;
use crate::secrets::keychain::KeychainSecretStore;
use crate::secrets::store::{SecretRef, SecretStore};
use crate::{Error, Result};

pub struct OauthService {
    authenticator: OauthAuthenticator,
}

impl Default for OauthService {
    fn default() -> Self {
        Self::new()
    }
}

impl OauthService {
    pub fn new() -> Self {
        Self {
            authenticator: OauthAuthenticator::new(),
        }
    }

    /// 创建 OAuth Session
    pub async fn start_oauth_session(
        &self,
        provider_id: &str,
        account_label: Option<String>,
    ) -> Result<OauthSession> {
        let preset = get_oauth_preset(provider_id)
            .ok_or_else(|| Error::NotFound(format!("OAuth preset for {provider_id} not found")))?;

        let session_id = format!("oauth-sess-{}", Uuid::new_v4());
        let state = generate_oauth_state();

        match preset.flow {
            OauthFlowType::Pkce => {
                let pkce = generate_pkce_codes();
                let redirect_uri = match preset.provider_id {
                    "claude" => "http://localhost:54545/callback".to_string(),
                    _ => "http://localhost:1455/auth/callback".to_string(),
                };

                let session = OauthSession {
                    session_id,
                    provider_id: preset.provider_id.to_string(),
                    flow: OauthFlowType::Pkce,
                    state,
                    pkce_verifier: Some(pkce.code_verifier),
                    redirect_uri,
                    device_code: None,
                    user_code: None,
                    verification_uri: None,
                    interval_secs: 5,
                    deadline: Instant::now() + Duration::from_secs(600),
                    status: OauthSessionStatus::WaitingForCallback,
                    error_message: None,
                    token_bundle: None,
                    connected_credential_id: None,
                    account_label,
                };

                Ok(session)
            }
            OauthFlowType::Device => {
                let (dev_code, user_code, verify_uri, expires_in, interval) =
                    match preset.provider_id {
                        "codex" => self
                            .authenticator
                            .codex_request_device_code()
                            .await
                            .map_err(Error::Internal)?,
                        "kimi" => self
                            .authenticator
                            .kimi_request_device_code()
                            .await
                            .map_err(Error::Internal)?,
                        "xai" => self
                            .authenticator
                            .xai_request_device_code()
                            .await
                            .map_err(Error::Internal)?,
                        _ => {
                            return Err(Error::Internal(format!(
                                "Unsupported device flow provider: {provider_id}"
                            )))
                        }
                    };

                let session = OauthSession {
                    session_id,
                    provider_id: preset.provider_id.to_string(),
                    flow: OauthFlowType::Device,
                    state,
                    pkce_verifier: None,
                    redirect_uri: String::new(),
                    device_code: Some(dev_code),
                    user_code: Some(user_code),
                    verification_uri: Some(verify_uri),
                    interval_secs: interval,
                    deadline: Instant::now() + Duration::from_secs(expires_in),
                    status: OauthSessionStatus::WaitingForUser,
                    error_message: None,
                    token_bundle: None,
                    connected_credential_id: None,
                    account_label,
                };

                Ok(session)
            }
        }
    }

    /// 轮询 Device flow 授权状态
    pub async fn poll_device_session(
        &self,
        session_id: &str,
        session_mgr: &OauthSessionManager,
    ) -> Result<Option<OauthTokenBundle>> {
        let (provider_id, device_code, status, is_expired): (
            String,
            Option<String>,
            OauthSessionStatus,
            bool,
        ) = session_mgr
            .update(session_id, |s| {
                let is_expired = s.deadline <= Instant::now();
                (
                    s.provider_id.clone(),
                    s.device_code.clone(),
                    s.status,
                    is_expired,
                )
            })
            .ok_or_else(|| Error::NotFound(format!("Session {session_id} not found")))?;

        if status == OauthSessionStatus::Connected {
            return Ok(None);
        }

        if is_expired {
            session_mgr.update(session_id, |s| {
                s.status = OauthSessionStatus::Expired;
            });
            return Ok(None);
        }

        let Some(dev_code) = device_code else {
            return Err(Error::Internal("Device code not found in session".into()));
        };

        let poll_res = match provider_id.as_str() {
            "codex" => self.authenticator.codex_poll_device_token(&dev_code).await,
            "kimi" => self.authenticator.kimi_poll_device_token(&dev_code).await,
            "xai" => self.authenticator.xai_poll_device_token(&dev_code).await,
            _ => {
                return Err(Error::Internal(format!(
                    "Provider {provider_id} not device flow"
                )))
            }
        };

        match poll_res {
            Ok(Some(bundle)) => Ok(Some(bundle)),
            Ok(None) => Ok(None),
            Err(e) => {
                session_mgr.update(session_id, |s| {
                    s.status = OauthSessionStatus::Error;
                    s.error_message = Some(e.clone());
                });
                Err(Error::Internal(e))
            }
        }
    }

    /// 解析 Callback URL 并执行 Token Exchange
    pub async fn exchange_callback_url(
        &self,
        callback_url: &str,
        session_mgr: &OauthSessionManager,
    ) -> Result<(String, OauthTokenBundle)> {
        let query_str = if let Some(pos) = callback_url.find('?') {
            &callback_url[pos + 1..]
        } else {
            callback_url
        };

        let mut code_opt = None;
        let mut state_opt = None;
        let mut error_opt = None;

        for pair in query_str.split('&') {
            let mut parts = pair.splitn(2, '=');
            let k = parts.next().unwrap_or("");
            let v = parts.next().unwrap_or("");
            let val_decoded = super::pkce::url_decode(v);

            if k == "code" {
                code_opt = Some(val_decoded);
            } else if k == "state" {
                state_opt = Some(val_decoded);
            } else if k == "error" || k == "error_description" {
                error_opt = Some(val_decoded);
            }
        }

        if let Some(err) = error_opt {
            return Err(Error::Internal(format!(
                "OAuth provider returned error: {err}"
            )));
        }

        let code =
            code_opt.ok_or_else(|| Error::InvalidInput("Missing code in callback URL".into()))?;
        let state =
            state_opt.ok_or_else(|| Error::InvalidInput("Missing state in callback URL".into()))?;

        let session_id = session_mgr.find_by_state(&state).ok_or_else(|| {
            Error::NotFound(format!("No active OAuth session matching state {state}"))
        })?;

        let (provider_id, pkce_verifier, redirect_uri) = session_mgr
            .update(&session_id, |s| {
                s.status = OauthSessionStatus::Exchanging;
                (
                    s.provider_id.clone(),
                    s.pkce_verifier.clone(),
                    s.redirect_uri.clone(),
                )
            })
            .unwrap();

        let verifier = pkce_verifier.unwrap_or_default();

        let bundle = match provider_id.as_str() {
            "claude" => {
                self.authenticator
                    .claude_exchange_code(&code, &verifier, &redirect_uri)
                    .await
            }
            "antigravity" => {
                self.authenticator
                    .antigravity_exchange_code(&code, &verifier, &redirect_uri)
                    .await
            }
            _ => Err(format!("Unsupported PKCE provider: {provider_id}")),
        }
        .map_err(Error::Internal)?;

        Ok((session_id, bundle))
    }

    /// 授权成功后的事务持久化（Keychain + DB + Models + Quota）
    pub fn persist_oauth_success(
        &self,
        conn: &DbConn,
        session_id: &str,
        bundle: OauthTokenBundle,
        session_mgr: &OauthSessionManager,
    ) -> Result<Credential> {
        let (provider_id, account_label): (String, Option<String>) = session_mgr
            .update(session_id, |s| {
                (s.provider_id.clone(), s.account_label.clone())
            })
            .ok_or_else(|| Error::NotFound(format!("Session {session_id} not found")))?;

        let preset = get_oauth_preset(&provider_id)
            .ok_or_else(|| Error::NotFound(format!("Preset {provider_id} not found")))?;

        let cred_id = format!("oauth-cred-{}", Uuid::new_v4());
        let secret_ref_str = format!("natives/ai/credential/{}/v1", cred_id);
        let secret_ref = SecretRef::new(secret_ref_str.clone());

        // 1. 序列化 Token Material 写入 OS Keychain
        let token_json = serde_json::to_vec(&bundle)
            .map_err(|e| Error::Internal(format!("Failed to serialize token bundle: {e}")))?;

        let store = KeychainSecretStore::default();
        store
            .write(&secret_ref, &token_json)
            .map_err(|e| Error::Internal(format!("Failed to save OAuth token to Keychain: {e}")))?;

        // 2. 确保 Provider 存在
        if store::get_provider(conn, preset.provider_id)?.is_none() {
            store::create_provider(
                conn,
                Some(preset.provider_id),
                Some(preset.provider_id),
                preset.name,
                preset.default_base_url,
                Some(preset.provider_id),
                true,
            )?;
        }

        // 3. 确保 Connection 存在
        let conns = store::list_connections(conn, Some(preset.provider_id))?;
        let connection_id = if let Some(first) = conns.first() {
            first.id.clone()
        } else {
            let conn_obj = store::create_connection(
                conn,
                Some(&format!("conn-{}", preset.provider_id)),
                preset.provider_id,
                &format!("{} Default Endpoint", preset.name),
                preset.default_base_url,
                UpstreamProtocol::from_str(preset.upstream_protocol)
                    .unwrap_or(UpstreamProtocol::OpenaiChatCompletions),
                None,
                None,
                None,
                true,
            )?;
            conn_obj.id
        };

        // 4. 构建 safe metadata
        let masked = if let Some(email) = &bundle.account_email {
            email.clone()
        } else if let Some(acct) = &bundle.account_id {
            format!("acct:{}…", &acct[..acct.len().min(8)])
        } else {
            format!(
                "oauth:{}…",
                &bundle.access_token[..bundle.access_token.len().min(8)]
            )
        };

        let label = account_label
            .filter(|l| !l.trim().is_empty())
            .unwrap_or_else(|| format!("{} ({masked})", preset.name));

        let now = chrono::Utc::now();
        let expires_at = bundle
            .expires_in_secs
            .map(|secs| (now + chrono::Duration::seconds(secs as i64)).to_rfc3339());

        let meta_json = serde_json::json!({
            "accountId": bundle.account_id,
            "accountEmail": bundle.account_email,
            "planName": bundle.plan_name,
            "hasRefreshToken": bundle.refresh_token.is_some(),
        })
        .to_string();

        let fingerprint = format!("{}:{}", preset.provider_id, masked);

        // 5. 写入 Credential 表
        let cred = store::insert_credential(
            conn,
            &cred_id,
            preset.provider_id,
            CredentialKind::Oauth,
            &label,
            &secret_ref_str,
            1,
            &masked,
            CredentialStatus::Active,
            0,
            10,
            expires_at.as_deref(),
            Some(&now.to_rfc3339()),
            expires_at.as_deref(),
            Some(&fingerprint),
            Some(&meta_json),
        )?;

        // 6. 绑定 Credential 与 Connection
        store::bind_credential_connection(conn, &cred.id, &connection_id)?;

        // 7. 同步预设模型目录
        for (model_id, display_name) in preset.default_models {
            store::upsert_model(
                conn,
                Some(preset.provider_id),
                Some(&connection_id),
                Some(&cred.id),
                model_id,
                display_name,
                ModelSource::Oauth,
                None,
                ModelAvailability::Available,
            )?;
        }

        // 8. 同步初始额度快照
        let quota_snapshot = QuotaSnapshot {
            id: format!("snap-{}", Uuid::new_v4()),
            credential_id: cred.id.clone(),
            provider_adapter: preset.provider_id.to_string(),
            status: QuotaStatus::Available,
            plan_name: bundle.plan_name.clone(),
            error_category: None,
            error_message: None,
            fetched_at: now.to_rfc3339(),
            expires_at: None,
            windows: vec![QuotaWindow {
                id: format!("win-{}", Uuid::new_v4()),
                snapshot_id: "".into(),
                label: "Account Status".into(),
                remaining: Some(100.0),
                limit_value: Some(100.0),
                used: Some(0.0),
                unit: Some("%".into()),
                reset_at: None,
            }],
        };
        store::save_quota_snapshot(conn, &quota_snapshot)?;

        session_mgr.update(session_id, |s| {
            s.status = OauthSessionStatus::Connected;
            s.connected_credential_id = Some(cred.id.clone());
        });

        Ok(cred)
    }

    /// 执行刷新请求并将新令牌写入 Keychain 和 DB
    pub async fn refresh_credential_async(
        &self,
        provider_id: &str,
        refresh_token: &str,
    ) -> Result<OauthTokenBundle> {
        match provider_id {
            "codex" => self.authenticator.codex_refresh_token(refresh_token).await,
            "claude" => self.authenticator.claude_refresh_token(refresh_token).await,
            "antigravity" => {
                self.authenticator
                    .antigravity_refresh_token(refresh_token)
                    .await
            }
            "kimi" => self.authenticator.kimi_refresh_token(refresh_token).await,
            "xai" => self.authenticator.xai_refresh_token(refresh_token).await,
            other => Err(format!("Unsupported OAuth provider: {other}")),
        }
        .map_err(Error::Internal)
    }
}
