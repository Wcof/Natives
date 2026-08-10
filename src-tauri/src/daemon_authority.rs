//! Run Authority façade for the Tauri host.
//!
//! Thin wrappers over [`natives_agent_daemon::ExecutionAuthority`]:
//! - **Embedded**: in-process `RunManager` + credential broker install (tests/dev).
//! - **UDS**: independent Agent Daemon; Tauri is a client only.
//!
//! Production: set `NATIVES_DAEMON_MODE=uds` / `NATIVES_REQUIRE_UDS=1`.
//! UDS failure must not silently become Embedded (see SidecarSupervisor).

use natives_agent_daemon::{
    resolve_run_authority_mode, AuthorityError, ExecutionAuthority,
    RunAuthorityMode, UdsAuthority,
};
#[cfg(any(test, feature = "diagnostic"))]
use natives_agent_daemon::EmbeddedAuthority;
use std::sync::{Arc, OnceLock};
use tokio::sync::RwLock;

static AUTHORITY: OnceLock<RwLock<Option<Arc<dyn ExecutionAuthority>>>> = OnceLock::new();

fn authority_slot() -> &'static RwLock<Option<Arc<dyn ExecutionAuthority>>> {
    AUTHORITY.get_or_init(|| RwLock::new(None))
}

/// Install the in-process credential broker for **Embedded** mode (tests/dev).
///
/// 仅在 `cfg(test)` 或 `diagnostic` feature 下编译。
/// 生产 UDS 模式不注册 host credential broker —— daemon 有自己的 broker。
#[cfg(any(test, feature = "diagnostic"))]
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

/// 生产 stub：UDS 模式不注册 host credential broker。
#[cfg(not(any(test, feature = "diagnostic")))]
fn ensure_embedded_broker() {}

/// 构造 Embedded authority（仅 test/diagnostic）。
///
/// `EmbeddedAuthority::with_broker_install` 只能存在于 test/diagnostic 路径，
/// 因为它拉入完整的 in-process daemon 实现（架构目标 #5）。
#[cfg(any(test, feature = "diagnostic"))]
fn build_embedded_authority() -> Result<Arc<dyn ExecutionAuthority>, String> {
    ensure_embedded_broker();
    Ok(Arc::new(EmbeddedAuthority::with_broker_install(Arc::new(
        ensure_embedded_broker,
    ))) as Arc<dyn ExecutionAuthority>)
}

/// 生产 stub：Embedded authority 不可用 —— 生产必须走 UDS。
#[cfg(not(any(test, feature = "diagnostic")))]
fn build_embedded_authority() -> Result<Arc<dyn ExecutionAuthority>, String> {
    Err("Embedded authority is not available in production (UDS only)".into())
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
    let mode = resolve_run_authority_mode();
    if let RunAuthorityMode::Uds {
        socket,
        bootstrap_token,
    } = &mode
    {
        if bootstrap_token.is_empty() || !socket.exists() {
            crate::sidecar_supervisor::global_supervisor()
                .ensure_healthy_or_restart()
                .map_err(|e| format!("UDS daemon unavailable (no embedded fallback): {e}"))?;
            *guard = None;
        }
    }
    let built = match resolve_run_authority_mode() {
        RunAuthorityMode::Embedded => build_embedded_authority()?,
        RunAuthorityMode::Uds {
            socket,
            bootstrap_token,
        } => {
            if bootstrap_token.is_empty() {
                return Err(
                    "NATIVES_DAEMON_BOOTSTRAP required for UDS mode (no embedded fallback)".into(),
                );
            }
            Arc::new(UdsAuthority::from_mode(socket, bootstrap_token).map_err(map_err)?)
                as Arc<dyn ExecutionAuthority>
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

async fn recover_uds_authority(first_error: String) -> Result<(), String> {
    if authority_mode_label() != "uds" {
        return Err(first_error);
    }
    reset_authority_cache().await;
    crate::sidecar_supervisor::global_supervisor()
        .ensure_healthy_or_restart()
        .map_err(|restart_error| {
            format!("{first_error}; UDS reconnect failed: {restart_error} (no embedded fallback)")
        })?;
    reset_authority_cache().await;
    Ok(())
}

fn retry_error(first_error: String, second_error: AuthorityError) -> String {
    format!(
        "{} (after UDS reconnect; first error: {first_error})",
        second_error.message()
    )
}

pub fn authority_mode_label() -> &'static str {
    match resolve_run_authority_mode() {
        RunAuthorityMode::Embedded => "embedded",
        RunAuthorityMode::Uds { .. } => "uds",
    }
}

pub async fn request(method: &str, params: serde_json::Value) -> Result<serde_json::Value, String> {
    let authority = current_authority().await?;
    match authority.request(method, params.clone()).await {
        Ok(value) => Ok(value),
        Err(first) => {
            let first_error = map_err(first);
            recover_uds_authority(first_error.clone()).await?;
            current_authority()
                .await?
                .request(method, params)
                .await
                .map_err(|second| retry_error(first_error, second))
        }
    }
}

pub async fn create_run(
    req: assistant_protocol::v2::CreateRunRequest,
) -> Result<assistant_protocol::v2::RunV2, String> {
    let authority = current_authority().await?;
    match authority.create_run(req.clone()).await {
        Ok(run) => Ok(run),
        Err(first) => {
            let first_error = map_err(first);
            recover_uds_authority(first_error.clone()).await?;
            current_authority()
                .await?
                .create_run(req)
                .await
                .map_err(|second| retry_error(first_error, second))
        }
    }
}

/// Non-blocking start.
pub async fn start_run(
    req: assistant_protocol::v2::StartRunRequest,
) -> Result<assistant_protocol::v2::RunV2, String> {
    let authority = current_authority().await?;
    match authority.start_run(req.clone()).await {
        Ok(run) => Ok(run),
        Err(first) => {
            let first_error = map_err(first);
            recover_uds_authority(first_error.clone()).await?;
            current_authority()
                .await?
                .start_run(req)
                .await
                .map_err(|second| retry_error(first_error, second))
        }
    }
}

pub async fn cancel_run(run_id: &str) -> Result<assistant_protocol::v2::RunV2, String> {
    let authority = current_authority().await?;
    match authority.cancel_run(run_id).await {
        Ok(run) => Ok(run),
        Err(first) => {
            let first_error = map_err(first);
            recover_uds_authority(first_error.clone()).await?;
            current_authority()
                .await?
                .cancel_run(run_id)
                .await
                .map_err(|second| retry_error(first_error, second))
        }
    }
}

pub async fn retry_run(run_id: &str) -> Result<assistant_protocol::v2::RunV2, String> {
    let authority = current_authority().await?;
    match authority.retry_run(run_id).await {
        Ok(run) => Ok(run),
        Err(first) => {
            let first_error = map_err(first);
            recover_uds_authority(first_error.clone()).await?;
            current_authority()
                .await?
                .retry_run(run_id)
                .await
                .map_err(|second| retry_error(first_error, second))
        }
    }
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
