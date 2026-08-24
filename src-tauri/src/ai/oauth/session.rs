//! OAuth Session 内存状态机（OAU-001 / plan3 02-target-architecture §2）。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use super::catalog::OauthFlowType;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OauthSessionStatus {
    Starting,
    WaitingForUser,
    WaitingForCallback,
    Exchanging,
    Connected,
    Expired,
    Cancelled,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OauthTokenBundle {
    pub access_token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_type: Option<String>,
    pub expires_in_secs: Option<u64>,
    pub account_id: Option<String>,
    pub account_email: Option<String>,
    pub project_id: Option<String>,
    pub plan_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OauthSessionInfo {
    pub session_id: String,
    pub provider_id: String,
    pub flow: OauthFlowType,
    pub status: OauthSessionStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authorize_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verification_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_in_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connected_credential_id: Option<String>,
}

pub struct OauthSession {
    pub session_id: String,
    pub provider_id: String,
    pub flow: OauthFlowType,
    pub state: String,
    pub pkce_verifier: Option<String>,
    pub redirect_uri: String,
    pub device_code: Option<String>,
    pub user_code: Option<String>,
    pub verification_uri: Option<String>,
    pub interval_secs: u64,
    pub deadline: Instant,
    pub status: OauthSessionStatus,
    pub error_message: Option<String>,
    pub token_bundle: Option<OauthTokenBundle>,
    pub connected_credential_id: Option<String>,
    pub account_label: Option<String>,
}

impl OauthSession {
    pub fn to_info(&self) -> OauthSessionInfo {
        let remaining = self
            .deadline
            .saturating_duration_since(Instant::now())
            .as_secs();
        OauthSessionInfo {
            session_id: self.session_id.clone(),
            provider_id: self.provider_id.clone(),
            flow: self.flow,
            status: if remaining == 0 && self.status != OauthSessionStatus::Connected {
                OauthSessionStatus::Expired
            } else {
                self.status
            },
            authorize_url: None,
            user_code: self.user_code.clone(),
            verification_uri: self.verification_uri.clone(),
            expires_in_secs: Some(remaining),
            interval_secs: Some(self.interval_secs),
            error_message: self.error_message.clone(),
            connected_credential_id: self.connected_credential_id.clone(),
        }
    }
}

pub struct OauthSessionManager {
    sessions: Mutex<HashMap<String, OauthSession>>,
}

impl Default for OauthSessionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl OauthSessionManager {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }

    pub fn insert(&self, session: OauthSession) {
        let mut map = self.sessions.lock().unwrap();
        let now = Instant::now();
        map.retain(|_, s| s.deadline > now || s.status == OauthSessionStatus::Connected);
        map.insert(session.session_id.clone(), session);
    }

    pub fn get_info(&self, session_id: &str) -> Option<OauthSessionInfo> {
        let map = self.sessions.lock().unwrap();
        map.get(session_id).map(|s| s.to_info())
    }

    pub fn find_by_state(&self, state: &str) -> Option<String> {
        let map = self.sessions.lock().unwrap();
        map.values()
            .find(|s| s.state == state && s.deadline > Instant::now())
            .map(|s| s.session_id.clone())
    }

    pub fn update<F, R>(&self, session_id: &str, f: F) -> Option<R>
    where
        F: FnOnce(&mut OauthSession) -> R,
    {
        let mut map = self.sessions.lock().unwrap();
        let session = map.get_mut(session_id)?;
        Some(f(session))
    }

    pub fn cancel(&self, session_id: &str) -> bool {
        let mut map = self.sessions.lock().unwrap();
        if let Some(s) = map.get_mut(session_id) {
            s.status = OauthSessionStatus::Cancelled;
            true
        } else {
            false
        }
    }
}
