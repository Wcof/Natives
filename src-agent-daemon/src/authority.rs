//! Unified Execution Authority — the only production entry for run lifecycle.
//!
//! Two adapters:
//! - [`EmbeddedAuthority`]: in-process `RunManager` (tests / dev diagnosis only).
//! - [`UdsAuthority`]: independent Agent Daemon over UDS (production).
//!
//! UI, Tauri commands, scheduler, and subagent orchestration must call this
//! trait (or the thin façade in `src-tauri/daemon_authority.rs`) and must not
//! call Provider / Stream / legacy agent_loop directly.

use assistant_protocol::v2::{
    CancelRunRequest, ContinueRunRequest, CreateRunRequest, ReplayRunRequest, ResumeRunRequest,
    RetryRunRequest, RunEventV2, RunV2, StartRunRequest, SubscribeRunRequest,
};
use async_trait::async_trait;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;

use crate::client::{
    client_protocol_version, resolve_run_authority_mode, DaemonClient, DaemonClientError,
    RunAuthorityMode,
};
use crate::run_manager::{global_run_manager, RunManager};

/// Errors from the execution authority layer.
#[derive(Debug, thiserror::Error)]
pub enum AuthorityError {
    #[error("{0}")]
    Message(String),
    #[error("uds: {0}")]
    Uds(#[from] DaemonClientError),
    #[error("production mode forbids silent embedded fallback: {0}")]
    ProductionFallbackForbidden(String),
}

impl From<String> for AuthorityError {
    fn from(s: String) -> Self {
        AuthorityError::Message(s)
    }
}

impl AuthorityError {
    pub fn message(&self) -> String {
        self.to_string()
    }
}

/// Single execution interface for all run lifecycle operations.
#[async_trait]
pub trait ExecutionAuthority: Send + Sync {
    async fn request(&self, method: &str, params: Value) -> Result<Value, AuthorityError>;
    async fn create_run(&self, req: CreateRunRequest) -> Result<RunV2, AuthorityError>;
    async fn start_run(&self, req: StartRunRequest) -> Result<RunV2, AuthorityError>;
    async fn cancel_run(&self, run_id: &str) -> Result<RunV2, AuthorityError>;
    async fn retry_run(&self, run_id: &str) -> Result<RunV2, AuthorityError>;
    async fn get_run(&self, run_id: &str) -> Result<Option<RunV2>, AuthorityError>;
    async fn list_runs(&self, conversation_id: Option<&str>) -> Result<Vec<RunV2>, AuthorityError>;
    async fn replay_events(
        &self,
        run_id: &str,
        after_sequence: u64,
    ) -> Result<Vec<RunEventV2>, AuthorityError>;
    async fn subscribe_events(
        &self,
        run_id: &str,
        after_sequence: u64,
    ) -> Result<Vec<RunEventV2>, AuthorityError>;
    async fn respond_permission(
        &self,
        request_id: &str,
        approved: bool,
        scope: Option<&str>,
    ) -> Result<Value, AuthorityError>;

    fn mode_label(&self) -> &'static str;
}

/// In-process authority — **tests and development diagnosis only**.
pub struct EmbeddedAuthority {
    ensure_broker: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl EmbeddedAuthority {
    pub fn new() -> Self {
        Self {
            ensure_broker: None,
        }
    }

    pub fn with_broker_install(ensure_broker: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self {
            ensure_broker: Some(ensure_broker),
        }
    }

    fn install_broker(&self) {
        if let Some(f) = &self.ensure_broker {
            f();
        }
    }
}

impl Default for EmbeddedAuthority {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ExecutionAuthority for EmbeddedAuthority {
    async fn request(&self, method: &str, params: Value) -> Result<Value, AuthorityError> {
        match method {
            "daemon.ping" => Ok(serde_json::json!({"pong": true})),
            "daemon.getCapabilities" => {
                Ok(serde_json::to_value(RunManager::capabilities()).unwrap_or_default())
            }
            m if m.starts_with("conversation.") => crate::conversation_store::request(m, params)
                .await
                .map_err(AuthorityError::Message),
            "run.create" => {
                let req: CreateRunRequest = serde_json::from_value(params)
                    .map_err(|e| AuthorityError::Message(e.to_string()))?;
                serde_json::to_value(self.create_run(req).await?)
                    .map_err(|e| AuthorityError::Message(e.to_string()))
            }
            "run.start" => {
                let req: StartRunRequest = serde_json::from_value(params)
                    .map_err(|e| AuthorityError::Message(e.to_string()))?;
                serde_json::to_value(self.start_run(req).await?)
                    .map_err(|e| AuthorityError::Message(e.to_string()))
            }
            "run.cancel" => {
                let run_id = params
                    .get("run_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| AuthorityError::Message("run_id required".into()))?;
                serde_json::to_value(self.cancel_run(run_id).await?)
                    .map_err(|e| AuthorityError::Message(e.to_string()))
            }
            "run.retry" => {
                let run_id = params
                    .get("run_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| AuthorityError::Message("run_id required".into()))?;
                serde_json::to_value(self.retry_run(run_id).await?)
                    .map_err(|e| AuthorityError::Message(e.to_string()))
            }
            "run.continue" => {
                let req: ContinueRunRequest = serde_json::from_value(params)
                    .map_err(|e| AuthorityError::Message(e.to_string()))?;
                let new_run = global_run_manager()
                    .continue_run(req)
                    .map_err(AuthorityError::from)?;
                serde_json::to_value(RunManager::start_detached_global(StartRunRequest {
                    agent_profile_id: None,
                    capability_selection: None,
                    run_id: Some(new_run.id.clone()),
                    conversation_id: Some(new_run.conversation_id.clone()),
                    provider_id: Some(new_run.provider_id.clone()),
                    model_id: Some(new_run.model_id.clone()),
                    key_id: new_run.key_id.clone(),
                    content: None,
                    attachments: None,
                    trigger_message_id: None,
                    permission_profile: Some(new_run.permission_profile.clone()),
                    max_steps: Some(new_run.max_steps),
                    project_path: new_run.project_path.clone(),
                    idempotency_key: None,
                    effort: new_run.effort.clone(),
                    runtime_id: new_run.runtime_id.clone(),
                })?)
                .map_err(|e| AuthorityError::Message(e.to_string()))
            }
            "run.resume" => {
                let req: ResumeRunRequest = serde_json::from_value(params)
                    .map_err(|e| AuthorityError::Message(e.to_string()))?;
                let response = global_run_manager()
                    .resume_run(req)
                    .map_err(AuthorityError::from)?;
                let new_run_id = response.new_run_id.clone();
                if let Some(new_run_id) = new_run_id {
                    // SafeToContinue: start the fresh independent run.
                    if let Some(new_run) = global_run_manager().get_run(&new_run_id) {
                        RunManager::start_detached_global(StartRunRequest {
                            agent_profile_id: None,
                            capability_selection: None,
                            run_id: Some(new_run.id.clone()),
                            conversation_id: Some(new_run.conversation_id.clone()),
                            provider_id: Some(new_run.provider_id.clone()),
                            model_id: Some(new_run.model_id.clone()),
                            key_id: new_run.key_id.clone(),
                            content: None,
                            attachments: None,
                            trigger_message_id: None,
                            permission_profile: Some(new_run.permission_profile.clone()),
                            max_steps: Some(new_run.max_steps),
                            project_path: new_run.project_path.clone(),
                            idempotency_key: None,
                            effort: new_run.effort.clone(),
                            runtime_id: new_run.runtime_id.clone(),
                        })
                        .map_err(|e| AuthorityError::Message(e.to_string()))?;
                    }
                }
                serde_json::to_value(response).map_err(|e| AuthorityError::Message(e.to_string()))
            }
            "run.list" => {
                let conversation_id = params.get("conversation_id").and_then(Value::as_str);
                Ok(serde_json::json!({ "runs": self.list_runs(conversation_id).await? }))
            }
            "run.getEvents" | "run.replay" => {
                let run_id = params
                    .get("run_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| AuthorityError::Message("run_id required".into()))?;
                let after = params
                    .get("after_sequence")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                serde_json::to_value(self.replay_events(run_id, after).await?)
                    .map_err(|e| AuthorityError::Message(e.to_string()))
            }
            "run.subscribe" => {
                let run_id = params
                    .get("run_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| AuthorityError::Message("run_id required".into()))?;
                let after = params
                    .get("after_sequence")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let events = self.subscribe_events(run_id, after).await?;
                Ok(serde_json::json!({ "run_id": run_id, "events": events }))
            }
            _ => Err(AuthorityError::Message(format!(
                "{method} requires the UDS Agent Daemon"
            ))),
        }
    }

    async fn create_run(&self, req: CreateRunRequest) -> Result<RunV2, AuthorityError> {
        self.install_broker();
        global_run_manager().create_run(req).map_err(Into::into)
    }

    async fn start_run(&self, req: StartRunRequest) -> Result<RunV2, AuthorityError> {
        self.install_broker();
        RunManager::start_detached_global(req).map_err(Into::into)
    }

    async fn cancel_run(&self, run_id: &str) -> Result<RunV2, AuthorityError> {
        self.install_broker();
        global_run_manager()
            .cancel(CancelRunRequest {
                run_id: run_id.to_string(),
            })
            .await
            .map_err(Into::into)
    }

    async fn retry_run(&self, run_id: &str) -> Result<RunV2, AuthorityError> {
        self.install_broker();
        // Embedded: create new row then start exactly once.
        let new_run = global_run_manager()
            .retry(RetryRunRequest {
                run_id: run_id.to_string(),
            })
            .map_err(AuthorityError::from)?;
        RunManager::start_detached_global(StartRunRequest {
            agent_profile_id: None,
            capability_selection: None,
            run_id: Some(new_run.id.clone()),
            conversation_id: Some(new_run.conversation_id.clone()),
            provider_id: Some(new_run.provider_id.clone()),
            model_id: Some(new_run.model_id.clone()),
            key_id: new_run.key_id.clone(),
            content: None,
            attachments: None,
            trigger_message_id: None,
            permission_profile: Some(new_run.permission_profile.clone()),
            max_steps: Some(new_run.max_steps),
            project_path: new_run.project_path.clone(),
            idempotency_key: None,
            effort: None,
            runtime_id: None,
        })
        .map_err(Into::into)
    }

    async fn get_run(&self, run_id: &str) -> Result<Option<RunV2>, AuthorityError> {
        self.install_broker();
        Ok(global_run_manager().get_run(run_id))
    }

    async fn list_runs(&self, conversation_id: Option<&str>) -> Result<Vec<RunV2>, AuthorityError> {
        self.install_broker();
        Ok(global_run_manager().list_runs(conversation_id))
    }

    async fn replay_events(
        &self,
        run_id: &str,
        after_sequence: u64,
    ) -> Result<Vec<RunEventV2>, AuthorityError> {
        self.install_broker();
        global_run_manager()
            .replay_checked(ReplayRunRequest {
                run_id: run_id.to_string(),
                after_sequence,
            })
            .map_err(Into::into)
    }

    async fn subscribe_events(
        &self,
        run_id: &str,
        after_sequence: u64,
    ) -> Result<Vec<RunEventV2>, AuthorityError> {
        self.install_broker();
        // Poll batch (push subscribe is a later phase enhancement).
        let _ = SubscribeRunRequest {
            run_id: run_id.to_string(),
            after_sequence,
        };
        global_run_manager()
            .replay_checked(ReplayRunRequest {
                run_id: run_id.to_string(),
                after_sequence,
            })
            .map_err(Into::into)
    }

    async fn respond_permission(
        &self,
        request_id: &str,
        approved: bool,
        _scope: Option<&str>,
    ) -> Result<Value, AuthorityError> {
        self.install_broker();
        global_run_manager()
            .respond_permission_for_run(request_id, approved, None, None)
            .await
            .map(|_| serde_json::json!({ "ok": true }))
            .map_err(Into::into)
    }

    fn mode_label(&self) -> &'static str {
        "embedded"
    }
}

/// Production authority: independent daemon over UDS.
pub struct UdsAuthority {
    socket: PathBuf,
    bootstrap_token: String,
}

impl UdsAuthority {
    pub fn new(socket: PathBuf, bootstrap_token: String) -> Self {
        Self {
            socket,
            bootstrap_token,
        }
    }

    pub fn from_mode(socket: PathBuf, bootstrap_token: String) -> Result<Self, AuthorityError> {
        if bootstrap_token.is_empty() {
            return Err(AuthorityError::Message(
                "NATIVES_DAEMON_BOOTSTRAP required for UDS mode".into(),
            ));
        }
        Ok(Self::new(socket, bootstrap_token))
    }

    async fn call(&self, method: &str, params: Value) -> Result<Value, AuthorityError> {
        // Prefer live supervisor env after sidecar restart; fall back to construction-time token.
        let bootstrap = std::env::var("NATIVES_DAEMON_BOOTSTRAP")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| self.bootstrap_token.clone());
        let socket = std::env::var_os("NATIVES_DAEMON_SOCKET")
            .map(PathBuf::from)
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| self.socket.clone());
        let mut client =
            DaemonClient::connect(&socket, &bootstrap, client_protocol_version()).await?;
        client.call(method, params).await.map_err(Into::into)
    }
}

#[async_trait]
impl ExecutionAuthority for UdsAuthority {
    async fn request(&self, method: &str, params: Value) -> Result<Value, AuthorityError> {
        self.call(method, params).await
    }

    async fn create_run(&self, req: CreateRunRequest) -> Result<RunV2, AuthorityError> {
        let data = self
            .call("run.create", serde_json::to_value(&req).unwrap_or_default())
            .await?;
        serde_json::from_value(data).map_err(|e| AuthorityError::Message(e.to_string()))
    }

    async fn start_run(&self, req: StartRunRequest) -> Result<RunV2, AuthorityError> {
        let data = self
            .call("run.start", serde_json::to_value(&req).unwrap_or_default())
            .await?;
        serde_json::from_value(data).map_err(|e| AuthorityError::Message(e.to_string()))
    }

    async fn cancel_run(&self, run_id: &str) -> Result<RunV2, AuthorityError> {
        let data = self
            .call("run.cancel", serde_json::json!({ "run_id": run_id }))
            .await?;
        serde_json::from_value(data).map_err(|e| AuthorityError::Message(e.to_string()))
    }

    async fn retry_run(&self, run_id: &str) -> Result<RunV2, AuthorityError> {
        // Sidecar run.retry already detached-starts — do NOT start again.
        let data = self
            .call("run.retry", serde_json::json!({ "run_id": run_id }))
            .await?;
        serde_json::from_value(data).map_err(|e| AuthorityError::Message(e.to_string()))
    }

    async fn get_run(&self, run_id: &str) -> Result<Option<RunV2>, AuthorityError> {
        let data = self.call("run.list", serde_json::json!({})).await?;
        let runs = data.get("runs").cloned().unwrap_or(Value::Array(vec![]));
        let list: Vec<RunV2> = serde_json::from_value(runs).unwrap_or_default();
        Ok(list.into_iter().find(|r| r.id == run_id))
    }

    async fn list_runs(&self, conversation_id: Option<&str>) -> Result<Vec<RunV2>, AuthorityError> {
        let params = match conversation_id {
            Some(id) => serde_json::json!({ "conversation_id": id }),
            None => serde_json::json!({}),
        };
        let data = self.call("run.list", params).await?;
        let runs = data.get("runs").cloned().unwrap_or(Value::Array(vec![]));
        serde_json::from_value(runs).map_err(|e| AuthorityError::Message(e.to_string()))
    }

    async fn replay_events(
        &self,
        run_id: &str,
        after_sequence: u64,
    ) -> Result<Vec<RunEventV2>, AuthorityError> {
        let data = self
            .call(
                "run.replay",
                serde_json::json!({
                    "run_id": run_id,
                    "after_sequence": after_sequence,
                }),
            )
            .await?;
        let events = data.get("events").cloned().unwrap_or_else(|| data.clone());
        serde_json::from_value(events).map_err(|e| AuthorityError::Message(e.to_string()))
    }

    async fn subscribe_events(
        &self,
        run_id: &str,
        after_sequence: u64,
    ) -> Result<Vec<RunEventV2>, AuthorityError> {
        let data = self
            .call(
                "run.subscribe",
                serde_json::json!({
                    "run_id": run_id,
                    "after_sequence": after_sequence,
                }),
            )
            .await?;
        let events = data.get("events").cloned().unwrap_or_else(|| data.clone());
        serde_json::from_value(events).map_err(|e| AuthorityError::Message(e.to_string()))
    }

    async fn respond_permission(
        &self,
        request_id: &str,
        approved: bool,
        scope: Option<&str>,
    ) -> Result<Value, AuthorityError> {
        self.call(
            "permission.respond",
            serde_json::json!({
                "request_id": request_id,
                "approved": approved,
                "scope": scope,
            }),
        )
        .await
    }

    fn mode_label(&self) -> &'static str {
        "uds"
    }
}

/// Build the authority for the current process environment.
///
/// Production rules:
/// - `NATIVES_DAEMON_MODE=uds` (or sidecar/remote): UDS required; **no** embedded fallback.
/// - `embedded`: in-process (tests / explicit dev).
/// - `auto`: UDS when socket+bootstrap present, else embedded — **dev only**.
/// - unset non-test: prefer `uds` when socket+bootstrap present; otherwise **error** if
///   `NATIVES_REQUIRE_UDS=1`, else fall back to auto (transitional).
pub fn build_execution_authority() -> Result<Arc<dyn ExecutionAuthority>, AuthorityError> {
    match resolve_run_authority_mode() {
        RunAuthorityMode::Embedded => Ok(Arc::new(EmbeddedAuthority::new())),
        RunAuthorityMode::Uds {
            socket,
            bootstrap_token,
        } => {
            if bootstrap_token.is_empty() {
                // Explicit UDS mode must not silently become embedded.
                let mode = std::env::var("NATIVES_DAEMON_MODE").unwrap_or_default();
                if matches!(
                    mode.to_ascii_lowercase().as_str(),
                    "uds" | "sidecar" | "remote"
                ) {
                    return Err(AuthorityError::ProductionFallbackForbidden(
                        "NATIVES_DAEMON_BOOTSTRAP empty while NATIVES_DAEMON_MODE requires UDS"
                            .into(),
                    ));
                }
            }
            Ok(Arc::new(UdsAuthority::from_mode(socket, bootstrap_token)?))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn embedded_create_and_list() {
        let auth = EmbeddedAuthority::new();
        let run = auth
            .create_run(CreateRunRequest {
                capability_selection: None,
                conversation_id: "c-auth".into(),
                provider_id: "echo".into(),
                model_id: "echo".into(),
                key_id: None,
                agent_profile_id: None,
                permission_profile: Some("ask".into()),
                content: Some("hi".into()),
                attachments: None,
                max_steps: Some(3),
                parent_run_id: None,
                project_path: Some("/tmp".into()),
                idempotency_key: Some(format!("auth-{}", uuid::Uuid::new_v4())),
                effort: None,
                runtime_id: None,
            })
            .await
            .unwrap();
        assert_eq!(run.project_path.as_deref(), Some("/tmp"));
        let got = auth.get_run(&run.id).await.unwrap();
        assert!(got.is_some());
        assert_eq!(auth.mode_label(), "embedded");
    }
}
