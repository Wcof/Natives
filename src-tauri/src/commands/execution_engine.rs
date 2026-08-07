//! Execution Engine V2 settings commands (A6).
//!
//! Single durable authority for runtime policy:
//! - `execution_engine_get_snapshot`
//! - `execution_engine_save_settings`
//! - `execution_engine_detect_runtimes`
//! - `execution_engine_get_diagnostics`
//!
//! The React side no longer derives runtime truth from localStorage; it reads
//! the backend snapshot. localStorage migration is a one-shot frontend step
//! (A7) that only deletes the old key after a successful durable save.

use crate::execution_engine_settings::{
    build_execution_engine_snapshot, detect_runtimes, load_execution_engine_settings,
    save_execution_engine_settings, ExecutionEngineSettingsV2,
};
use crate::{Error, Result};

/// Derive the diagnostics summary from the live authority mode + a real ping.
async fn authority_diagnostics() -> (String, String, String, bool) {
    let mode = crate::daemon_authority::authority_mode_label().to_string();
    let protocol = "2".to_string();
    // With A2/A3 the daemon advertises event_stream_v1 and run.watch; the host
    // negotiates the persistent stream and falls back to long-poll only for
    // older daemons. Report the mode honestly instead of a fake number.
    let stream_transport = match mode.as_str() {
        "uds" => "persistent_stream".to_string(),
        "embedded" => "embedded".to_string(),
        other => other.to_string(),
    };
    let daemon_ready = crate::daemon_authority::request("daemon.ping", serde_json::json!({}))
        .await
        .is_ok();
    (mode, protocol, stream_transport, daemon_ready)
}

/// Full settings + runtime + resolution snapshot (backend-derived truth).
#[tauri::command]
pub async fn execution_engine_get_snapshot() -> Result<serde_json::Value> {
    let (mode, protocol, transport, daemon_ready) = authority_diagnostics().await;
    let snapshot = build_execution_engine_snapshot(&mode, &protocol, &transport, daemon_ready);
    serde_json::to_value(&snapshot).map_err(Error::Json)
}

/// Persist V2 settings (revision bump; codex force-closed; subtractive only).
#[tauri::command]
pub fn execution_engine_save_settings(
    settings: ExecutionEngineSettingsV2,
) -> Result<ExecutionEngineSettingsV2> {
    save_execution_engine_settings(settings).map_err(Error::Internal)
}

/// Re-detect external runtimes (refresh action from the settings UI).
#[tauri::command]
pub fn execution_engine_detect_runtimes() -> Result<serde_json::Value> {
    let runtimes = detect_runtimes();
    serde_json::to_value(runtimes).map_err(Error::Json)
}

/// Read-only diagnostics for the folded Advanced pane (no low-level switches).
#[tauri::command]
pub async fn execution_engine_get_diagnostics() -> Result<serde_json::Value> {
    let settings = load_execution_engine_settings();
    let (mode, protocol, transport, daemon_ready) = authority_diagnostics().await;
    Ok(serde_json::json!({
        "authorityMode": mode,
        "protocolVersion": protocol,
        "streamTransport": transport,
        "daemonReady": daemon_ready,
        "eventStreamV1": true,
        "maxSteps": settings.native.max_steps,
        "externalUnavailablePolicy": settings.external_unavailable_policy,
        "defaultRuntime": settings.default_runtime,
        "nativeDisabledTools": settings.native.disabled_tools,
    }))
}
