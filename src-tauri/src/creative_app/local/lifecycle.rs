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
use std::time::Duration;
use tauri::AppHandle;

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Resolve the base loopback URL for a Compose project (batch 5): explicit host
/// port wins, else the port the project actually published on 127.0.0.1.
async fn resolve_compose_url(
    project: &str,
    compose_file: &std::path::Path,
    detail: &ComposePlanDetail,
) -> Result<String> {
    let port = match detail.host_port {
        Some(p) => p,
        None => crate::creative_app::docker::compose_host_port(project, compose_file)
            .await?
            .unwrap_or(0),
    };
    if port == 0 {
        return Err(Error::Internal(
            "could not determine compose host port".into(),
        ));
    }
    Ok(format!("http://127.0.0.1:{port}"))
}

/// Absolute compose file path for a record + compose detail (batch 5).
fn compose_abs_path(
    rec: &LocalCreativeAppRecord,
    detail: &ComposePlanDetail,
) -> std::path::PathBuf {
    let root = std::path::PathBuf::from(&rec.canonical_project_root);
    let cwd = LaunchPlan::from_json(&rec.launch_plan_json)
        .ok()
        .map(|p| p.cwd_relative)
        .unwrap_or_else(|| ".".into());
    if cwd == "." {
        root.join(&detail.compose_file)
    } else {
        root.join(&cwd).join(&detail.compose_file)
    }
}

fn broadcast<R: tauri::Runtime>(app: &tauri::AppHandle<R>, action: &str, id: &str) {
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

pub async fn start_app<R: tauri::Runtime>(
    conn: &Connection,
    app: &tauri::AppHandle<R>,
    runtime: &LocalRuntimeManager,
    host_http_port: u16,
    id: &str,
    runtime_id: &str,
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

    // Orphan recovery gate: if DB says running but the new runtime has no live
    // child, surface orphaned when identity is present (identity is OS-based, so
    // it is independent of which runtime id the caller holds).
    if matches!(rec.state, CreativeAppState::Running) && !runtime.is_running(runtime_id).await {
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
        let open_url = runtime::static_open_url(host_http_port, id, runtime_id, &plan.open_path);
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

    // docker_compose (batch 5): up with a stable unique project, then health.
    if plan.runtime == LocalLaunchRuntime::DockerCompose {
        let detail = plan
            .compose
            .ok_or_else(|| Error::InvalidInput("compose plan missing detail".into()))?;
        let compose_file_abs = if plan.cwd_relative == "." {
            root.join(&detail.compose_file)
        } else {
            root.join(&plan.cwd_relative).join(&detail.compose_file)
        };
        let project = runtime::compose_project_name(id, &detail.project_seed);
        let env = store::get_env_map(conn, id)?;
        // P0 preflight: never start a compose command that can place real trades
        // without explicit user approval (batch 8). The assistant can never set
        // trade_approval — only a user action on the plan can.
        if let Some(msg) = crate::creative_app::local::scan::compose_command_risk(&compose_file_abs)
        {
            let approved = match plan.trade_approval {
                Some(TradeApproval::Webserver) => {
                    // The user pre-approved the non-trading webserver override:
                    // the effective command must actually be a webserver command.
                    detail.command.first().map(|t| t.as_str()) == Some("webserver")
                }
                Some(TradeApproval::DryRun) => {
                    // Proven dry-run projection; the command may stay default.
                    crate::creative_app::local::scan::config_proves_dry_run(&root)
                }
                None => false,
            };
            if !approved {
                let detail = CreativeAppStatusDetail {
                    code: LocalCreativeIssueCode::ConfigInvalid,
                    message: format!("{msg}; no explicit user approval"),
                    recovery_actions: vec!["edit_plan".into(), "open_folder".into()],
                };
                set_status_detail(
                    conn,
                    id,
                    CreativeAppState::StartFailed,
                    Some(&detail),
                    Some(&msg),
                )?;
                broadcast(app, "start_failed", id);
                return Err(Error::InvalidInput(detail.message));
            }
        }
        let up_result = match (plan.trade_approval, detail.command.as_slice()) {
            (Some(TradeApproval::Webserver), cmd) if !cmd.is_empty() => {
                let service = detail.service.clone().ok_or_else(|| {
                    Error::InvalidInput("webserver override requires a compose service".into())
                })?;
                let override_dir = dirs::home_dir()
                    .unwrap_or_else(|| std::path::PathBuf::from("."))
                    .join(".natives")
                    .join("creative-apps")
                    .join(id)
                    .join("runtime");
                crate::creative_app::docker::compose_up_override(
                    &project,
                    &compose_file_abs,
                    &service,
                    cmd,
                    &env,
                    &override_dir,
                )
                .await
            }
            _ => crate::creative_app::docker::compose_up(&project, &compose_file_abs, &env).await,
        };
        if let Err(e) = up_result {
            let detail = CreativeAppStatusDetail {
                code: LocalCreativeIssueCode::ConfigInvalid,
                message: e.to_string(),
                recovery_actions: vec!["view_logs".into(), "edit_plan".into()],
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
        // Resolve the host URL + health (explicit port → inspect fallback).
        let url = resolve_compose_url(&project, &compose_file_abs, &detail).await?;
        let health_url = format!("{}{}", url.trim_end_matches('/'), detail.health_path);
        let timeout = Duration::from_millis(rec.startup_timeout_ms as u64);
        match crate::creative_app::docker::wait_ready(&health_url, timeout).await {
            Ok(()) => {
                let mut rec = store::get_app(conn, id)?.unwrap();
                rec.state = CreativeAppState::Running;
                rec.open_url = Some(url.clone());
                rec.current_port = Some(
                    crate::creative_app::docker::compose_host_port(&project, &compose_file_abs)
                        .await
                        .ok()
                        .flatten()
                        .or(detail.host_port)
                        .unwrap_or(0),
                );
                rec.last_error = None;
                rec.status_detail_json = None;
                rec.last_started_at = Some(now());
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
                set_status_detail(
                    conn,
                    id,
                    CreativeAppState::StartFailed,
                    Some(&detail),
                    Some(&detail.message),
                )?;
                broadcast(app, "start_unhealthy", id);
                Err(e)
            }
        }
    } else {
        // node_dev_server — spawn phase only (under the caller's mutation lock).
        // Health is awaited later without the lock so stop can preempt a long start.
        let env = store::get_env_map(conn, id)?;
        match runtime
            .start_node_dev(
                app,
                runtime_id,
                id,
                &root,
                &plan,
                &rec.plan_fingerprint,
                &env,
                plan.port.value,
                None,
            )
            .await
        {
            Ok((port, open_url, identity)) => {
                let mut rec = store::get_app(conn, id)?.unwrap();
                rec.open_url = Some(open_url);
                rec.current_port = Some(port);
                rec.process_identity_json =
                    Some(serde_json::to_string(&identity).unwrap_or_else(|_| "{}".into()));
                rec.last_error = None;
                rec.updated_at = now();
                store::update_app(conn, &rec)?;
                broadcast(app, "starting", id);
                Ok(store::summary_from_local(&rec))
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
}

/// Await the spawned node dev server's health and settle the DB state. Runs
/// WITHOUT the global mutation lock so a concurrent stop can cancel the wait.
pub async fn await_start_ready<R: tauri::Runtime>(
    conn: &Connection,
    app: &tauri::AppHandle<R>,
    runtime: &LocalRuntimeManager,
    id: &str,
    runtime_id: &str,
) -> Result<CreativeAppSummary> {
    let rec = store::get_app(conn, id)?.ok_or_else(|| Error::NotFound(id.into()))?;
    let plan = parse_plan(&rec)?;
    // Static HTTP and Compose starts are fully synchronous in start_app
    // (Running is settled there), so the health phase is a no-op for them.
    if runtime::plan_is_static(&plan) || plan.runtime == LocalLaunchRuntime::DockerCompose {
        let summary = store::summary_from_local(&rec);
        record_ready_endpoint(conn, id, runtime_id, &summary);
        return Ok(summary);
    }
    match runtime
        .wait_healthy(
            app,
            runtime_id,
            id,
            &plan.health_path,
            rec.startup_timeout_ms,
        )
        .await
    {
        Ok(()) => {
            let mut rec = store::get_app(conn, id)?.unwrap();
            rec.state = CreativeAppState::Running;
            rec.open_url = rec.open_url.clone();
            rec.last_error = None;
            rec.status_detail_json = None;
            rec.last_started_at = Some(now());
            rec.updated_at = now();
            store::update_app(conn, &rec)?;
            broadcast(app, "started", id);
            let summary = store::summary_from_local(&rec);
            // T09: the driver owns the real endpoint + service rows once the
            // health pass proved the runtime bound its port.
            record_ready_endpoint(conn, id, runtime_id, &summary);
            Ok(summary)
        }
        Err(Error::Cancelled(_)) => {
            // Stop preempted the start; return the stop-owned current state.
            Ok(store::summary_from_local(
                &store::get_app(conn, id)?.unwrap(),
            ))
        }
        Err(e) => {
            let detail = CreativeAppStatusDetail {
                code: LocalCreativeIssueCode::StartUnhealthy,
                message: e.to_string(),
                recovery_actions: vec!["view_logs".into(), "stop".into(), "restart".into()],
            };
            // Keep the process for log inspection; mark start_failed / unhealthy.
            set_status_detail(
                conn,
                id,
                CreativeAppState::StartFailed,
                Some(&detail),
                Some(&detail.message),
            )?;
            broadcast(app, "start_unhealthy", id);
            Err(e)
        }
    }
}

/// T09: persist the REAL preview endpoint + service readiness for a runtime
/// instance whose health pass succeeded. Best-effort (never fails the start
/// when the projection write fails) — the runtime instance CAS is the
/// authoritative settle.
fn record_ready_endpoint(
    conn: &Connection,
    id: &str,
    runtime_id: &str,
    summary: &CreativeAppSummary,
) {
    if runtime_id.is_empty() {
        return;
    }
    let urls = summary.open_url.iter().cloned().collect::<Vec<_>>();
    if urls.is_empty() {
        return;
    }
    // The real port comes from the source record (settled by the health pass),
    // never a guessed value.
    let port = store::get_app(conn, id)
        .ok()
        .flatten()
        .and_then(|r| r.current_port);
    let _ =
        crate::creative_app::service_store::record_instance_ready(conn, runtime_id, &urls, port);
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

pub async fn stop_app<R: tauri::Runtime>(
    conn: &Connection,
    app: &tauri::AppHandle<R>,
    runtime: &LocalRuntimeManager,
    id: &str,
    runtime_id: &str,
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
    } else if matches!(
        plan.as_ref().map(|p| p.runtime),
        Some(LocalLaunchRuntime::DockerCompose)
    ) {
        // Compose stop: stop the unique project (volumes retained), then verify
        // no container of this project is still running.
        let detail = plan
            .as_ref()
            .and_then(|p| p.compose.clone())
            .ok_or_else(|| Error::Internal("compose plan missing detail".into()))?;
        let compose_file_abs = compose_abs_path(&rec, &detail);
        let project = runtime::compose_project_name(id, &detail.project_seed);
        if let Err(e) = crate::creative_app::docker::compose_stop(&project, &compose_file_abs).await
        {
            failures.push(e.to_string());
        }
        if let Ok(running) =
            crate::creative_app::docker::compose_ps_running(&project, &compose_file_abs).await
        {
            if running {
                failures.push("compose project still has running containers after stop".into());
            }
        }
    } else {
        // Prefer live child; if missing, only kill when persisted identity fully matches.
        if runtime.is_running(runtime_id).await {
            if let Err(e) = runtime.stop(runtime_id, Some(app)).await {
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
    let msg = error
        .clone()
        .unwrap_or_else(|| "stop did not verify resource release".to_string());
    rec = record_stop_outcome(rec, released, error);
    store::update_app(conn, &rec)?;
    // T09: the driver owns the endpoint/service lifecycle — a verified stop
    // drops the live endpoints and marks the service stopped; an unverified
    // stop leaves them marked unhealthy (honest, never a false stopped).
    if released {
        let _ = crate::creative_app::service_store::mark_instance_stopped(conn, runtime_id);
    } else {
        let _ = crate::creative_app::service_store::mark_instance_unhealthy(conn, runtime_id, &msg);
    }
    broadcast(app, if released { "stopped" } else { "stop_failed" }, id);
    if released {
        Ok(store::summary_from_local(&rec))
    } else {
        // CR-302: never claim stopped when resources were not verified released.
        // The caller (adapter) marks the instance cleanup_failed (active-like),
        // which blocks restart until a retry stop succeeds.
        Err(Error::Internal(msg))
    }
}

pub async fn delete_app<R: tauri::Runtime>(
    conn: &Connection,
    app: &tauri::AppHandle<R>,
    runtime: &LocalRuntimeManager,
    id: &str,
    runtime_id: &str,
) -> Result<DeleteResult> {
    let rec = store::get_app(conn, id)?.ok_or_else(|| Error::NotFound(id.into()))?;

    // A delete must never orphan a live process: propagate stop failures and keep
    // the record (with identity) so the user can stop it first.
    let plan = parse_plan(&rec).ok();
    if matches!(
        plan.as_ref().map(|p| p.runtime),
        Some(LocalLaunchRuntime::DockerCompose)
    ) {
        // Compose delete: down the unique project, volumes retained (never `-v`).
        if let Some(detail) = plan.and_then(|p| p.compose) {
            let compose_file_abs = compose_abs_path(&rec, &detail);
            let project = runtime::compose_project_name(id, &detail.project_seed);
            crate::creative_app::docker::compose_down(&project, &compose_file_abs, false, false)
                .await?;
        }
    } else if runtime.is_running(runtime_id).await {
        runtime.stop(runtime_id, Some(app)).await?;
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

    runtime.purge_app_logs(id);
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
            // Mirror the reconciled outcome onto the runtime instance (crash recovery).
            let _ = crate::creative_app::runtime_store::settle_instance(
                conn,
                CreativeAppSource::LocalProject,
                &next.id,
                match target {
                    CreativeAppState::Orphaned => "orphaned",
                    CreativeAppState::InstalledStopped => "stopped",
                    _ => "failed",
                },
            );
            n += 1;
            if let Some(a) = app {
                broadcast(a, "reconcile", &next.id);
            }
        }
    }
    Ok(n)
}

// Orphan / process-exit handling moved to `lifecycle_process` (W3); re-exported.
pub use super::lifecycle_process::{mark_process_exited, poll_and_reconcile_exits, resolve_orphan};

pub fn get_local_config(conn: &Connection, id: &str) -> Result<LocalCreativeConfig> {
    let rec = store::get_app(conn, id)?.ok_or_else(|| Error::NotFound(id.into()))?;
    let plan = parse_plan(&rec)?;
    let env_keys = store::list_env_keys(conn, id)?;
    let dependency_install =
        if plan.runtime == LocalLaunchRuntime::NodeDevServer && plan.process_profile.is_none() {
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
pub async fn shutdown_all<R: tauri::Runtime>(
    runtime: &LocalRuntimeManager,
    app: Option<&tauri::AppHandle<R>>,
) {
    runtime.stop_all(app).await;
}

/// Shared handle type managed in Tauri state.
pub type LocalRuntimeHandle = Arc<LocalRuntimeManager>;

pub fn new_runtime_manager() -> LocalRuntimeHandle {
    Arc::new(LocalRuntimeManager::new())
}

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod lifecycle_tests;
