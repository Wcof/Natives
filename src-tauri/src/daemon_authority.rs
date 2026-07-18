//! Run Authority façade for the Tauri host.
//!
//! Thin wrappers over [`natives_agent_daemon::ExecutionAuthority`]:
//! - **Embedded**: in-process `RunManager` + credential broker install (tests/dev).
//! - **UDS**: independent Agent Daemon; Tauri is a client only.
//!
//! Production: set `NATIVES_DAEMON_MODE=uds` / `NATIVES_REQUIRE_UDS=1`.
//! UDS failure must not silently become Embedded (see SidecarSupervisor).

use natives_agent_daemon::{
    resolve_run_authority_mode, AuthorityError, EmbeddedAuthority, ExecutionAuthority,
    RunAuthorityMode, UdsAuthority,
};
use std::sync::{Arc, OnceLock};
use tokio::sync::RwLock;

static AUTHORITY: OnceLock<RwLock<Option<Arc<dyn ExecutionAuthority>>>> = OnceLock::new();

fn authority_slot() -> &'static RwLock<Option<Arc<dyn ExecutionAuthority>>> {
    AUTHORITY.get_or_init(|| RwLock::new(None))
}

fn ensure_embedded_broker() {
    static BROKER_INSTALLED: std::sync::Once = std::sync::Once::new();
    BROKER_INSTALLED.call_once(|| {
        natives_agent_daemon::install_credential_broker(std::sync::Arc::new(
            |provider_id: &str, key_id: Option<&str>, run_id: &str| {
                crate::credential_broker::resolve_for_daemon(provider_id, key_id, run_id)
            },
        ));
    });
}

fn map_err(e: AuthorityError) -> String {
    e.message()
}

/// Build or return cached authority for the current process env.
pub async fn current_authority() -> Result<Arc<dyn ExecutionAuthority>, String> {
    {
        let guard = authority_slot().read().await;
        if let Some(a) = guard.as_ref() {
            return Ok(Arc::clone(a));
        }
    }
    let mut guard = authority_slot().write().await;
    if let Some(a) = guard.as_ref() {
        return Ok(Arc::clone(a));
    }
    let built = match resolve_run_authority_mode() {
        RunAuthorityMode::Embedded => {
            ensure_embedded_broker();
            Arc::new(EmbeddedAuthority::with_broker_install(Arc::new(
                ensure_embedded_broker,
            ))) as Arc<dyn ExecutionAuthority>
        }
        RunAuthorityMode::Uds {
            socket,
            bootstrap_token,
        } => {
            if bootstrap_token.is_empty() {
                return Err(
                    "NATIVES_DAEMON_BOOTSTRAP required for UDS mode (no embedded fallback)".into(),
                );
            }
            Arc::new(
                UdsAuthority::from_mode(socket, bootstrap_token).map_err(map_err)?,
            ) as Arc<dyn ExecutionAuthority>
        }
    };
    *guard = Some(Arc::clone(&built));
    Ok(built)
}

/// Drop cached client (e.g. after sidecar restart).
pub async fn reset_authority_cache() {
    let mut guard = authority_slot().write().await;
    *guard = None;
}

pub fn authority_mode_label() -> &'static str {
    match resolve_run_authority_mode() {
        RunAuthorityMode::Embedded => "embedded",
        RunAuthorityMode::Uds { .. } => "uds",
    }
}

pub async fn request(method: &str, params: serde_json::Value) -> Result<serde_json::Value, String> {
    current_authority()
        .await?
        .request(method, params)
        .await
        .map_err(map_err)
}

pub async fn create_run(
    req: assistant_protocol::v2::CreateRunRequest,
) -> Result<assistant_protocol::v2::RunV2, String> {
    current_authority()
        .await?
        .create_run(req)
        .await
        .map_err(map_err)
}

/// Non-blocking start.
pub async fn start_run(
    req: assistant_protocol::v2::StartRunRequest,
) -> Result<assistant_protocol::v2::RunV2, String> {
    current_authority()
        .await?
        .start_run(req)
        .await
        .map_err(map_err)
}

pub async fn cancel_run(run_id: &str) -> Result<assistant_protocol::v2::RunV2, String> {
    current_authority()
        .await?
        .cancel_run(run_id)
        .await
        .map_err(map_err)
}

pub async fn retry_run(run_id: &str) -> Result<assistant_protocol::v2::RunV2, String> {
    current_authority()
        .await?
        .retry_run(run_id)
        .await
        .map_err(map_err)
}

/// Retry then start when needed (UDS: single RPC; Embedded: create+start once).
pub async fn retry_and_start(run_id: &str) -> Result<assistant_protocol::v2::RunV2, String> {
    // Both adapters already implement correct once-only start semantics.
    retry_run(run_id).await
}

pub async fn get_run(run_id: &str) -> Result<Option<assistant_protocol::v2::RunV2>, String> {
    current_authority()
        .await?
        .get_run(run_id)
        .await
        .map_err(map_err)
}

pub async fn list_runs(
    conversation_id: Option<&str>,
) -> Result<Vec<assistant_protocol::v2::RunV2>, String> {
    current_authority()
        .await?
        .list_runs(conversation_id)
        .await
        .map_err(map_err)
}

pub async fn replay_events(
    run_id: &str,
    after_sequence: u64,
) -> Result<Vec<assistant_protocol::v2::RunEventV2>, String> {
    current_authority()
        .await?
        .replay_events(run_id, after_sequence)
        .await
        .map_err(map_err)
}

pub async fn respond_permission(request_id: &str, approved: bool) -> Result<(), String> {
    respond_permission_for_run(request_id, approved, None).await
}

pub async fn respond_permission_for_run(
    request_id: &str,
    approved: bool,
    run_id: Option<&str>,
) -> Result<(), String> {
    current_authority()
        .await?
        .respond_permission(request_id, approved, run_id)
        .await
        .map_err(map_err)?;
    Ok(())
}
