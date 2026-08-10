//! Local lifecycle orphan/exit process handling (W3 split from lifecycle.rs).
//!
//! Owns PID liveness checks, verified identity kills, process-exit settling,
//! and exit polling/reconciliation. The main lifecycle module composes these.

use super::logs::LogStream;
use super::runtime::LocalRuntimeManager;
use super::store;
use crate::creative_app::model::*;
use crate::{Error, Result};
use rusqlite::Connection;
use tauri::AppHandle;

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn broadcast<R: tauri::Runtime>(app: &tauri::AppHandle<R>, action: &str, id: &str) {
    crate::emit_db_state_changed(
        app,
        "creative-app",
        serde_json::json!({ "action": action, "id": id }),
    );
}

fn pid_is_alive(pid: Option<u32>) -> bool {
    let Some(pid) = pid else {
        return false;
    };
    use sysinfo::{Pid, ProcessesToUpdate, System};
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::Some(&[Pid::from_u32(pid)]), true);
    sys.process(Pid::from_u32(pid)).is_some()
}

/// Resolve an orphaned process: kill the verified identity (if it still matches
/// live) and settle the orphaned instance to stopped. Never auto-takeover pipes.
/// Restart is handled by the caller (adapter) as a fresh start after this.
pub async fn resolve_orphan<R: tauri::Runtime>(
    conn: &Connection,
    app: &tauri::AppHandle<R>,
    runtime: &LocalRuntimeManager,
    id: &str,
) -> Result<CreativeAppSummary> {
    let rec = store::get_app(conn, id)?.ok_or_else(|| Error::NotFound(id.into()))?;
    if let Some(ident_json) = &rec.process_identity_json {
        if let Ok(ident) = serde_json::from_str::<ProcessIdentity>(ident_json) {
            // Only kill if identity still matches (pid + executable + cwd fingerprint).
            if identity_matches_live(&ident) {
                // On failure keep the orphaned identity so a retry stays possible.
                force_kill_identity(&ident).await?;
            }
        }
    }
    // The user explicitly resolved the orphan: settle the orphaned instance to
    // stopped so a retry stop / restart can proceed (mirrors "clear identity").
    if let Ok(Some(app_id)) = crate::creative_app::runtime_store::application_id_for(
        conn,
        CreativeAppSource::LocalProject,
        id,
    ) {
        if let Ok(Some(iid)) = crate::creative_app::runtime_store::active_instance_id(conn, &app_id)
        {
            let _ = runtime
                .logs()
                .get_or_open(id, &iid)
                .append(LogStream::System, "orphaned process terminated by user");
            let _ =
                crate::creative_app::runtime_store::settle_instance_by_id(conn, &iid, "stopped");
        }
    }
    // Clear identity
    let mut rec = store::get_app(conn, id)?.unwrap();
    rec.process_identity_json = None;
    rec.status_detail_json = None;
    rec.state = CreativeAppState::InstalledStopped;
    rec.updated_at = now();
    store::update_app(conn, &rec)?;
    broadcast(app, "orphan_resolved", id);
    Ok(store::summary_from_local(&rec))
}

fn identity_looks_orphaned(ident: &ProcessIdentity) -> bool {
    super::runtime::identity_matches_live_strict(ident)
}

fn identity_matches_live(ident: &ProcessIdentity) -> bool {
    super::runtime::identity_matches_live_strict(ident)
}

async fn force_kill_identity(ident: &ProcessIdentity) -> Result<()> {
    #[cfg(unix)]
    {
        if let Some(pgid) = ident.process_group_id {
            unsafe {
                let _ = libc::kill(-pgid, libc::SIGTERM);
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
            unsafe {
                let _ = libc::kill(-pgid, libc::SIGKILL);
            }
            // Verify the group is really gone before claiming success.
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
            while super::runtime::process_group_exists(pgid) {
                if std::time::Instant::now() >= deadline {
                    return Err(Error::Internal(format!(
                        "process group {pgid} still has members after kill"
                    )));
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            return Ok(());
        }
        if let Some(pid) = ident.pid {
            unsafe {
                let _ = libc::kill(pid as i32, libc::SIGTERM);
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
            unsafe {
                let _ = libc::kill(pid as i32, libc::SIGKILL);
            }
        }
    }
    #[cfg(windows)]
    {
        if let Some(pid) = ident.pid {
            let _ = tokio::task::spawn_blocking(move || {
                std::process::Command::new("taskkill")
                    .args(["/PID", &pid.to_string(), "/T", "/F"])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
            })
            .await;
        }
    }
    Ok(())
}

/// Apply an exited process cleanup to DB. `runtime_id` identifies the exact run
/// that exited (CR-301): the instance is settled by id, and the source record is
/// only touched when that run is still the app's ACTIVE instance — a late exit
/// from a superseded run must never flip a newer running run to stopped.
pub fn mark_process_exited<R: tauri::Runtime>(
    conn: &Connection,
    app: Option<&tauri::AppHandle<R>>,
    runtime_id: &str,
    exit_code: i32,
) -> Result<()> {
    let Some(app_id) =
        crate::creative_app::runtime_store::instance_application_id(conn, runtime_id)?
    else {
        // Unknown / already-gone instance: nothing to settle.
        return Ok(());
    };
    // Settle the exact instance (idempotent; guarded on running/starting).
    let _ = crate::creative_app::runtime_store::mark_exited(conn, runtime_id, exit_code);

    // Only mirror onto the source record when this run is still the active one.
    let active = crate::creative_app::runtime_store::active_instance_id(conn, &app_id)?;
    if active.as_deref() != Some(runtime_id) {
        return Ok(());
    }
    let Some(source_id) =
        crate::creative_app::runtime_store::source_id_for_application(conn, &app_id)?
    else {
        return Ok(());
    };
    let Some(mut rec) = store::get_app(conn, &source_id)? else {
        return Ok(());
    };
    if !matches!(
        rec.state,
        CreativeAppState::Running | CreativeAppState::Starting | CreativeAppState::StartFailed
    ) {
        return Ok(());
    }
    rec.state = CreativeAppState::InstalledStopped;
    rec.open_url = None;
    rec.current_port = None;
    rec.process_identity_json = None;
    rec.last_exit_reason = Some(format!("process_exit_{exit_code}"));
    if exit_code != 0 {
        rec.last_error = Some(format!("process exited with code {exit_code}"));
    }
    rec.status_detail_json = None;
    rec.updated_at = now();
    store::update_app(conn, &rec)?;
    if let Some(a) = app {
        broadcast(a, "process_exited", &source_id);
    }
    Ok(())
}

/// Poll runtime exits and write DB. Safe to call frequently.
pub async fn poll_and_reconcile_exits<R: tauri::Runtime>(
    conn: &Connection,
    app: Option<&tauri::AppHandle<R>>,
    runtime: &LocalRuntimeManager,
) -> Result<u32> {
    let exited = runtime.poll_exits().await;
    let mut n = 0u32;
    for (runtime_id, code) in exited {
        mark_process_exited(conn, app, &runtime_id, code)?;
        n += 1;
    }
    // Refresh heartbeats for still-running instances so the ledger never looks
    // stale while the process is alive (keyed by runtime id, CR-301).
    for runtime_id in runtime.live_runtime_ids().await {
        let _ = crate::creative_app::runtime_store::heartbeat(conn, &runtime_id);
    }
    Ok(n)
}
