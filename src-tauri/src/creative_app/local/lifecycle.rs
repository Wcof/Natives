//! Start / stop / restart / open-target for local creative apps.

use super::logs::LogStream;
use super::path::canonical_project_root;
use super::runtime::{self, LocalRuntimeManager};
use super::store;
use crate::creative_app::model::*;
use crate::creative_app::state_machine;
use crate::{emit_db_state_changed, Error, Result};
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::AppHandle;

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn broadcast(app: &AppHandle, action: &str, id: &str) {
    emit_db_state_changed(
        app,
        "creative-app",
        serde_json::json!({ "action": action, "id": id }),
    );
}

fn parse_plan(rec: &LocalCreativeAppRecord) -> Result<LaunchPlan> {
    LaunchPlan::from_json(&rec.launch_plan_json)
        .map_err(|e| Error::InvalidInput(format!("launch_plan: {e}")))
}

#[allow(dead_code)]
fn parse_status(json: &Option<String>) -> Option<CreativeAppStatusDetail> {
    json.as_deref().and_then(|s| serde_json::from_str(s).ok())
}

fn set_status_detail(
    conn: &Connection,
    id: &str,
    state: CreativeAppState,
    detail: Option<&CreativeAppStatusDetail>,
    last_error: Option<&str>,
) -> Result<()> {
    let detail_json = detail
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| Error::Internal(e.to_string()))?;
    store::set_state(conn, id, state, last_error, detail_json.as_deref(), &now())
}

pub async fn start_app(
    conn: &Connection,
    app: &AppHandle,
    runtime: &LocalRuntimeManager,
    host_http_port: u16,
    id: &str,
) -> Result<CreativeAppSummary> {
    let mut rec = store::get_app(conn, id)?.ok_or_else(|| Error::NotFound(id.into()))?;
    let plan = parse_plan(&rec)?;

    // Path must still exist
    let root = match canonical_project_root(&rec.canonical_project_root) {
        Ok(p) => p,
        Err(e) => {
            let detail = CreativeAppStatusDetail {
                code: LocalCreativeIssueCode::PathMissing,
                message: e.to_string(),
                recovery_actions: vec!["rescan".into(), "delete_record".into()],
            };
            set_status_detail(
                conn,
                id,
                CreativeAppState::StartFailed,
                Some(&detail),
                Some(&detail.message),
            )?;
            broadcast(app, "start_failed", id);
            return Err(e);
        }
    };

    // Orphan recovery gate: if DB says running but supervisor has no live child,
    // surface orphaned when identity is present.
    if matches!(rec.state, CreativeAppState::Running) && !runtime.is_running(id).await {
        if let Some(ident_json) = &rec.process_identity_json {
            if let Ok(ident) = serde_json::from_str::<ProcessIdentity>(ident_json) {
                if identity_looks_orphaned(&ident) {
                    let detail = CreativeAppStatusDetail {
                        code: LocalCreativeIssueCode::OrphanedProcess,
                        message:
                            "found leftover process identity after crash; choose stop or restart"
                                .into(),
                        recovery_actions: vec![
                            "resolve_orphan_stop".into(),
                            "resolve_orphan_restart".into(),
                        ],
                    };
                    set_status_detail(
                        conn,
                        id,
                        CreativeAppState::Orphaned,
                        Some(&detail),
                        Some(&detail.message),
                    )?;
                    broadcast(app, "orphaned_process", id);
                    return Err(Error::InvalidInput(detail.message));
                }
            }
        }
    }

    let next = state_machine::transition(rec.state, CreativeAppState::Starting)?;
    store::set_state(conn, id, next, None, None, &now())?;
    broadcast(app, "starting", id);

    if runtime::plan_is_static(&plan) {
        let open_url = runtime::static_open_url(host_http_port, id, &plan.open_path);
        // Verify entry is readable via filesystem (HTTP route will serve it).
        let entry = plan.entry_file.as_deref().unwrap_or("index.html");
        let entry_path = if plan.cwd_relative == "." {
            root.join(entry)
        } else {
            root.join(&plan.cwd_relative).join(entry)
        };
        if !entry_path.is_file() {
            let detail = CreativeAppStatusDetail {
                code: LocalCreativeIssueCode::ConfigInvalid,
                message: format!("entry file missing: {entry}"),
                recovery_actions: vec!["rescan".into(), "edit_plan".into()],
            };
            set_status_detail(
                conn,
                id,
                CreativeAppState::StartFailed,
                Some(&detail),
                Some(&detail.message),
            )?;
            broadcast(app, "start_failed", id);
            return Err(Error::InvalidInput(detail.message));
        }

        rec = store::get_app(conn, id)?.unwrap();
        rec.state = CreativeAppState::Running;
        rec.open_url = Some(open_url);
        rec.current_port = Some(host_http_port);
        rec.last_started_at = Some(now());
        rec.last_error = None;
        rec.status_detail_json = None;
        rec.process_identity_json = None;
        rec.updated_at = now();
        store::update_app(conn, &rec)?;
        broadcast(app, "started", id);
        return Ok(store::summary_from_local(&rec));
    }

    // node_dev_server
    let env = store::get_env_map(conn, id)?;
    match runtime
        .start_node_dev(
            app,
            id,
            &root,
            &plan,
            &rec.plan_fingerprint,
            &env,
            plan.port.value,
        )
        .await
    {
        Ok((port, open_url, identity)) => {
            match runtime
                .wait_healthy(app, id, &plan.health_path, rec.startup_timeout_ms)
                .await
            {
                Ok(()) => {
                    rec = store::get_app(conn, id)?.unwrap();
                    rec.state = CreativeAppState::Running;
                    rec.open_url = Some(open_url);
                    rec.current_port = Some(port);
                    rec.last_started_at = Some(now());
                    rec.last_error = None;
                    rec.status_detail_json = None;
                    rec.process_identity_json =
                        Some(serde_json::to_string(&identity).unwrap_or_else(|_| "{}".into()));
                    rec.updated_at = now();
                    store::update_app(conn, &rec)?;
                    broadcast(app, "started", id);
                    Ok(store::summary_from_local(&rec))
                }
                Err(e) => {
                    let detail = CreativeAppStatusDetail {
                        code: LocalCreativeIssueCode::StartUnhealthy,
                        message: e.to_string(),
                        recovery_actions: vec!["view_logs".into(), "stop".into(), "restart".into()],
                    };
                    // Keep process for log inspection; mark start_failed / unhealthy.
                    set_status_detail(
                        conn,
                        id,
                        CreativeAppState::StartFailed,
                        Some(&detail),
                        Some(&detail.message),
                    )?;
                    // still persist port/url/identity so stop works
                    if let Ok(Some(mut r)) = store::get_app(conn, id) {
                        r.open_url = Some(open_url);
                        r.current_port = Some(port);
                        r.process_identity_json =
                            Some(serde_json::to_string(&identity).unwrap_or_else(|_| "{}".into()));
                        r.updated_at = now();
                        let _ = store::update_app(conn, &r);
                    }
                    broadcast(app, "start_unhealthy", id);
                    Err(e)
                }
            }
        }
        Err(e) => {
            let msg = e.to_string();
            let code = if msg.contains("port") {
                LocalCreativeIssueCode::PortConflict
            } else if msg.to_lowercase().contains("node")
                || msg.to_lowercase().contains("npm")
                || msg.to_lowercase().contains("pnpm")
                || msg.to_lowercase().contains("yarn")
                || msg.to_lowercase().contains("spawn")
            {
                LocalCreativeIssueCode::EnvironmentMissing
            } else {
                LocalCreativeIssueCode::ConfigInvalid
            };
            let detail = CreativeAppStatusDetail {
                code,
                message: msg.clone(),
                recovery_actions: vec!["view_logs".into(), "edit_plan".into()],
            };
            set_status_detail(
                conn,
                id,
                CreativeAppState::StartFailed,
                Some(&detail),
                Some(&msg),
            )?;
            broadcast(app, "start_failed", id);
            Err(e)
        }
    }
}

/// Apply a stop outcome to the local record. On failure the record keeps its
/// identity/port/URL and moves to `CleanupFailed` — it must NEVER claim stopped
/// when resources were not verified released.
fn record_stop_outcome(
    mut rec: LocalCreativeAppRecord,
    released: bool,
    error: Option<String>,
) -> LocalCreativeAppRecord {
    if released {
        rec.state = CreativeAppState::InstalledStopped;
        rec.open_url = None;
        rec.current_port = None;
        rec.process_identity_json = None;
        rec.status_detail_json = None;
        rec.last_error = None;
        rec.last_exit_reason = Some("stopped_by_user".into());
    } else {
        let msg = error.unwrap_or_else(|| "stop did not verify resource release".to_string());
        let detail = CreativeAppStatusDetail {
            code: LocalCreativeIssueCode::StopFailed,
            message: msg.clone(),
            recovery_actions: vec![
                "stop".into(),
                "view_logs".into(),
                "open_terminal".into(),
                "open_folder".into(),
            ],
        };
        rec.state = CreativeAppState::CleanupFailed;
        rec.last_error = Some(msg);
        rec.status_detail_json = Some(serde_json::to_string(&detail).unwrap_or_default());
        rec.last_exit_reason = None;
        // identity / port / url are intentionally preserved for a retry stop.
    }
    rec.updated_at = now();
    rec
}

pub async fn stop_app(
    conn: &Connection,
    app: &AppHandle,
    runtime: &LocalRuntimeManager,
    id: &str,
) -> Result<CreativeAppSummary> {
    let rec = store::get_app(conn, id)?.ok_or_else(|| Error::NotFound(id.into()))?;
    let next = state_machine::transition(rec.state, CreativeAppState::Stopping)
        .unwrap_or(CreativeAppState::Stopping);
    store::set_state(conn, id, next, None, None, &now())?;
    broadcast(app, "stopping", id);

    let plan = parse_plan(&rec).ok();
    let mut failures: Vec<String> = Vec::new();
    if plan.as_ref().map(runtime::plan_is_static).unwrap_or(false) {
        // static apps keep no process; nothing to verify
    } else {
        // Prefer live child; if missing, only kill when persisted identity fully matches.
        if runtime.is_running(id).await {
            if let Err(e) = runtime.stop(id, Some(app)).await {
                failures.push(e.to_string());
            }
        } else if let Some(ident_json) = &rec.process_identity_json {
            if let Ok(ident) = serde_json::from_str::<ProcessIdentity>(ident_json) {
                if identity_matches_live(&ident) {
                    if let Err(e) = force_kill_identity(&ident).await {
                        failures.push(e.to_string());
                    }
                } else if ident.pid.is_some() {
                    failures.push(
                        "a process with this PID is alive but identity does not match; not killed"
                            .into(),
                    );
                }
            }
        }
    }

    let mut rec = store::get_app(conn, id)?.unwrap();
    let released = failures.is_empty();
    let error = if released {
        None
    } else {
        Some(failures.join("; "))
    };
    rec = record_stop_outcome(rec, released, error);
    store::update_app(conn, &rec)?;
    broadcast(app, if released { "stopped" } else { "stop_failed" }, id);
    Ok(store::summary_from_local(&rec))
}

pub async fn restart_app(
    conn: &Connection,
    app: &AppHandle,
    runtime: &LocalRuntimeManager,
    host_http_port: u16,
    id: &str,
) -> Result<CreativeAppSummary> {
    // Stop failure must propagate — never start a new process on a half-stopped app.
    stop_app(conn, app, runtime, id).await?;
    start_app(conn, app, runtime, host_http_port, id).await
}

pub async fn delete_app(
    conn: &Connection,
    app: &AppHandle,
    runtime: &LocalRuntimeManager,
    id: &str,
) -> Result<DeleteResult> {
    let rec = store::get_app(conn, id)?.ok_or_else(|| Error::NotFound(id.into()))?;

    // A delete must never orphan a live process: propagate stop failures and keep
    // the record (with identity) so the user can stop it first.
    if runtime.is_running(id).await {
        runtime.stop(id, Some(app)).await?;
    } else if let Some(ident_json) = &rec.process_identity_json {
        if let Ok(ident) = serde_json::from_str::<ProcessIdentity>(ident_json) {
            if identity_matches_live(&ident) {
                force_kill_identity(&ident).await?;
            } else if pid_is_alive(ident.pid) {
                return Err(Error::InvalidInput(
                    "cannot delete: a process is alive with this PID but identity does not match; stop it manually first".into(),
                ));
            }
        }
    }

    runtime.purge_logs(id);
    store::delete_app(conn, id)?;
    broadcast(app, "deleted", id);
    Ok(DeleteResult {
        ok: true,
        warnings: vec![],
    })
}

/// Startup reconcile for local apps (called alongside Docker reconcile).
pub fn reconcile_local_apps(conn: &Connection, app: Option<&AppHandle>) -> Result<u32> {
    let mut n = 0u32;
    for rec in store::list_apps(conn)? {
        let mut target = rec.state;
        let mut clear_runtime = false;
        let mut orphan = false;

        if rec.state.is_transient() {
            match rec.state {
                CreativeAppState::Starting => {
                    target = CreativeAppState::StartFailed;
                    clear_runtime = true;
                }
                CreativeAppState::Stopping => {
                    // The Host died mid-stop. If a live process still matches the
                    // persisted identity it is orphaned — do NOT claim stopped.
                    if let Some(ident_json) = &rec.process_identity_json {
                        if let Ok(ident) = serde_json::from_str::<ProcessIdentity>(ident_json) {
                            if identity_matches_live(&ident) {
                                target = CreativeAppState::Orphaned;
                                orphan = true;
                                clear_runtime = false;
                            } else {
                                target = CreativeAppState::InstalledStopped;
                                clear_runtime = true;
                            }
                        } else {
                            target = CreativeAppState::InstalledStopped;
                            clear_runtime = true;
                        }
                    } else {
                        target = CreativeAppState::InstalledStopped;
                        clear_runtime = true;
                    }
                }
                CreativeAppState::Deleting => {
                    target = CreativeAppState::InstalledStopped;
                    clear_runtime = true;
                }
                other => {
                    target = other;
                }
            }
        } else if matches!(rec.state, CreativeAppState::Running) {
            let plan_static = parse_plan(&rec)
                .map(|p| runtime::plan_is_static(&p))
                .unwrap_or(false);
            if plan_static {
                // Static apps do not keep a process; URL may point at a dead host port after restart.
                target = CreativeAppState::InstalledStopped;
                clear_runtime = true;
            } else if let Some(ident_json) = &rec.process_identity_json {
                if let Ok(ident) = serde_json::from_str::<ProcessIdentity>(ident_json) {
                    if identity_matches_live(&ident) {
                        // Live leftover — do not auto-takeover pipes; mark orphaned.
                        target = CreativeAppState::Orphaned;
                        orphan = true;
                        clear_runtime = false;
                    } else {
                        target = CreativeAppState::InstalledStopped;
                        clear_runtime = true;
                    }
                } else {
                    target = CreativeAppState::InstalledStopped;
                    clear_runtime = true;
                }
            } else {
                target = CreativeAppState::InstalledStopped;
                clear_runtime = true;
            }
        }

        if target != rec.state || clear_runtime || orphan {
            let mut next = rec;
            next.state = target;
            if clear_runtime {
                next.open_url = None;
                next.current_port = None;
                next.process_identity_json = None;
            }
            if orphan {
                next.status_detail_json = Some(
                    serde_json::to_string(&CreativeAppStatusDetail {
                        code: LocalCreativeIssueCode::OrphanedProcess,
                        message: "orphaned process detected after restart".into(),
                        recovery_actions: vec![
                            "resolve_orphan_stop".into(),
                            "resolve_orphan_restart".into(),
                        ],
                    })
                    .unwrap_or_default(),
                );
                next.last_error = Some("orphaned process".into());
            } else if clear_runtime {
                next.status_detail_json = None;
                if matches!(target, CreativeAppState::StartFailed) {
                    next.last_error = Some("startup interrupted".into());
                }
            }
            next.updated_at = now();
            store::update_app(conn, &next)?;
            n += 1;
            if let Some(a) = app {
                broadcast(a, "reconcile", &next.id);
            }
        }
    }
    Ok(n)
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

/// Resolve orphaned process: stop only or stop+restart. Never auto-takeover pipes.
pub async fn resolve_orphan(
    conn: &Connection,
    app: &AppHandle,
    runtime: &LocalRuntimeManager,
    host_http_port: u16,
    id: &str,
    restart: bool,
) -> Result<CreativeAppSummary> {
    let rec = store::get_app(conn, id)?.ok_or_else(|| Error::NotFound(id.into()))?;
    if let Some(ident_json) = &rec.process_identity_json {
        if let Ok(ident) = serde_json::from_str::<ProcessIdentity>(ident_json) {
            // Only kill if identity still matches (pid + executable + cwd fingerprint).
            if identity_matches_live(&ident) {
                // On failure keep the orphaned identity so a retry stays possible.
                force_kill_identity(&ident).await?;
                let _ = runtime
                    .logs()
                    .get_or_open(id)
                    .append(LogStream::System, "orphaned process terminated by user");
            }
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

    if restart {
        start_app(conn, app, runtime, host_http_port, id).await
    } else {
        Ok(store::summary_from_local(&rec))
    }
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
            let _ = std::process::Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
    }
    Ok(())
}

/// Apply exited process cleanup to DB (call after poll_exits).
pub fn mark_process_exited(
    conn: &Connection,
    app: Option<&AppHandle>,
    id: &str,
    exit_code: i32,
) -> Result<()> {
    let Some(mut rec) = store::get_app(conn, id)? else {
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
        broadcast(a, "process_exited", id);
    }
    Ok(())
}

/// Poll runtime exits and write DB. Safe to call frequently.
pub async fn poll_and_reconcile_exits(
    conn: &Connection,
    app: Option<&AppHandle>,
    runtime: &LocalRuntimeManager,
) -> Result<u32> {
    let exited = runtime.poll_exits().await;
    let mut n = 0u32;
    for (id, code) in exited {
        mark_process_exited(conn, app, &id, code)?;
        n += 1;
    }
    Ok(n)
}

pub fn get_local_config(conn: &Connection, id: &str) -> Result<LocalCreativeConfig> {
    let rec = store::get_app(conn, id)?.ok_or_else(|| Error::NotFound(id.into()))?;
    let plan = parse_plan(&rec)?;
    let env_keys = store::list_env_keys(conn, id)?;
    let dependency_install = if plan.runtime == LocalLaunchRuntime::NodeDevServer {
        let root = PathBuf::from(&rec.canonical_project_root);
        if !root.join("node_modules").is_dir() {
            super::deps::preview_install_command(conn, id)
                .ok()
                .map(|(program, args, pm)| DependencyInstallPreview {
                    display: format!("{program} {}", args.join(" ")),
                    program,
                    args,
                    package_manager: pm,
                    requires_confirmation: true,
                })
        } else {
            None
        }
    } else {
        None
    };
    Ok(LocalCreativeConfig {
        summary: store::summary_from_local(&rec),
        launch_plan: plan,
        env_keys,
        dependency_install,
    })
}

/// On normal app exit: stop all local processes.
pub async fn shutdown_all(runtime: &LocalRuntimeManager, app: Option<&AppHandle>) {
    runtime.stop_all(app).await;
}

/// Shared handle type managed in Tauri state.
pub type LocalRuntimeHandle = Arc<LocalRuntimeManager>;

pub fn new_runtime_manager() -> LocalRuntimeHandle {
    Arc::new(LocalRuntimeManager::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_rec() -> LocalCreativeAppRecord {
        let t = now();
        LocalCreativeAppRecord {
            id: "loc1".into(),
            title: "Local".into(),
            description: None,
            icon: None,
            canonical_project_root: "/tmp/proj".into(),
            device_id: "d".into(),
            device_name: "n".into(),
            project_kind: LocalProjectKind::Vite,
            launch_mode: LaunchMode::Smart,
            launch_plan_json: "{}".into(),
            plan_fingerprint: "fp".into(),
            state: CreativeAppState::Running,
            status_detail_json: None,
            open_url: Some("http://127.0.0.1:5173/".into()),
            current_port: Some(5173),
            process_identity_json: Some(r#"{"pid":123,"processGroupId":123}"#.into()),
            auto_open: true,
            startup_timeout_ms: 60_000,
            last_started_at: Some(t.clone()),
            last_exit_reason: None,
            last_error: None,
            created_at: t.clone(),
            updated_at: t,
        }
    }

    #[test]
    fn identity_without_pid_not_orphan() {
        let id = ProcessIdentity::default();
        assert!(!identity_matches_live(&id));
    }

    /// P0: a failed stop must never write installed_stopped and must preserve the
    /// identity/port/url so a retry stop stays possible.
    #[test]
    fn stop_failure_keeps_identity_and_non_stopped_state() {
        let rec = sample_rec();
        let failed = record_stop_outcome(rec, false, Some("process group 123 still alive".into()));
        assert_eq!(failed.state, CreativeAppState::CleanupFailed);
        assert_ne!(failed.state.as_str(), "installed_stopped");
        assert!(
            failed.process_identity_json.is_some(),
            "identity must be preserved on stop failure"
        );
        assert_eq!(failed.current_port, Some(5173), "port must be preserved");
        assert_eq!(
            failed.open_url.as_deref(),
            Some("http://127.0.0.1:5173/"),
            "url must be preserved"
        );
        assert!(failed.last_error.is_some());
        let detail: CreativeAppStatusDetail =
            serde_json::from_str(failed.status_detail_json.as_deref().unwrap()).unwrap();
        assert_eq!(detail.code, LocalCreativeIssueCode::StopFailed);
    }

    #[test]
    fn stop_success_clears_runtime_fields() {
        let rec = sample_rec();
        let ok = record_stop_outcome(rec, true, None);
        assert_eq!(ok.state, CreativeAppState::InstalledStopped);
        assert!(ok.process_identity_json.is_none());
        assert!(ok.current_port.is_none());
        assert!(ok.open_url.is_none());
        assert_eq!(ok.last_exit_reason.as_deref(), Some("stopped_by_user"));
    }
}
