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
    migrate_legacy_runtime_pref, save_execution_engine_settings, ExecutionEngineSettingsV2,
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
///
/// When the frontend supplies the legacy localStorage runtime pref
/// (`natives.assistant.runtimePref.v1`), a **one-shot durable migration** into
/// Settings V2 `defaultRuntime` runs first (no-op once the backend is
/// authoritative; never overwrites an explicit V2 choice; revision CAS makes
/// it exactly-once). The snapshot then reflects the migrated value.
#[tauri::command]
pub async fn execution_engine_get_snapshot(
    legacy_runtime_id: Option<String>,
) -> Result<serde_json::Value> {
    if let Some(pref) = legacy_runtime_id {
        migrate_legacy_runtime_pref(Some(pref)).map_err(Error::Internal)?;
    }
    let (mode, protocol, transport, daemon_ready) = authority_diagnostics().await;
    let snapshot = build_execution_engine_snapshot(&mode, &protocol, &transport, daemon_ready)
        .map_err(Error::Internal)?;
    serde_json::to_value(&snapshot).map_err(Error::Json)
}

/// Persist V2 settings with **revision CAS**: `settings.revision` is the
/// caller's expected revision; a stale revision is rejected with a conflict
/// error (codex force-closed; subtractive only).
#[tauri::command]
pub fn execution_engine_save_settings(
    settings: ExecutionEngineSettingsV2,
) -> Result<ExecutionEngineSettingsV2> {
    save_execution_engine_settings(settings).map_err(Error::Internal)
}

/// Re-detect external runtimes (refresh action from the settings UI).
#[tauri::command]
pub fn execution_engine_detect_runtimes() -> Result<serde_json::Value> {
    let runtimes = detect_runtimes().map_err(Error::Internal)?;
    serde_json::to_value(runtimes).map_err(Error::Json)
}

/// Read-only diagnostics for the folded Advanced pane (no low-level switches).
#[tauri::command]
pub async fn execution_engine_get_diagnostics() -> Result<serde_json::Value> {
    let settings = load_execution_engine_settings().map_err(Error::Internal)?;
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
