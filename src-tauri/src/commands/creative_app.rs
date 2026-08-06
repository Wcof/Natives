//! Tauri commands for multi-source Creative Apps (ADR-0013 + local_project).
//!
//! All async commands open rusqlite connections only in short synchronous
//! scopes (or inside `spawn_blocking`) so `Connection` is never held across
//! `.await` — Connection is !Send.

use crate::creative_app::adapters::{self, LifecycleCtx, ResolvedSource};
use crate::creative_app::browser::{self, BrowserStateHandle};
use crate::creative_app::docker;
use crate::creative_app::install;
use crate::creative_app::local::{self, LocalRuntimeHandle};
use crate::creative_app::model::*;
use crate::creative_app::operation as op;
use crate::creative_app::runtime_store;
use crate::creative_app::service::{self, MutationLock};
use crate::creative_app::store;
use crate::creative_app::surface_store;
use crate::db::DbPool;
use crate::emit_db_state_changed;
use crate::{Error, Result};
use tauri::State;

use crate::AppState;

fn host_http_port(state: &AppState) -> u16 {
    *state.http_port.lock().unwrap_or_else(|e| e.into_inner())
}

fn modules_dir() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".natives")
        .join("modules")
}

fn conn(pool: &DbPool) -> Result<r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>> {
    pool.get().map_err(|e| Error::Internal(format!("db: {e}")))
}

fn lifecycle_ctx(
    app: tauri::AppHandle,
    local_runtime: LocalRuntimeHandle,
    host_http_port: u16,
) -> LifecycleCtx {
    LifecycleCtx::new(app, modules_dir(), Some(local_runtime), host_http_port)
}

/// Resolve which per-runtime log to read for a caller-supplied id (CR-301).
///
/// `id` may be a source id (app-scoped: read the active runtime; aggregate when
/// stopped) or a runtime instance id (runtime-scoped). Returns
/// `(source, runtime_id_to_read, app_id_for_dir)`.
fn resolve_log_scope(
    conn: &rusqlite::Connection,
    id: &str,
) -> Result<(ResolvedSource, Option<String>, String)> {
    if let Ok(ResolvedSource::LocalProject) = adapters::resolve(conn, id) {
        let app_id = runtime_store::application_id_for(conn, CreativeAppSource::LocalProject, id)?
            .unwrap_or_default();
        let active = if app_id.is_empty() {
            None
        } else {
            runtime_store::active_instance_id(conn, &app_id)?
        };
        return Ok((ResolvedSource::LocalProject, active, id.to_string()));
    }
    if let Ok(Some(app_id)) = runtime_store::instance_application_id(conn, id) {
        // `id` is a runtime instance id belonging to a local process source.
        let src = runtime_store::source_id_for_application(conn, &app_id)?
            .unwrap_or_else(|| id.to_string());
        return Ok((ResolvedSource::LocalProject, Some(id.to_string()), src));
    }
    let source = adapters::resolve(conn, id)?;
    Ok((source, None, id.to_string()))
}

fn format_local_log_lines(mem: &[crate::creative_app::local::logs::LogLine]) -> String {
    mem.iter()
        .map(|l| format!("[{}] {}: {}", l.ts_ms, l.stream.as_str(), l.text))
        .collect::<Vec<_>>()
        .join("\n")
}

// ── Operation journal helpers (batch 2 CR-201) ──────────────────────────

/// Unified application id for a source row, read-only (never fabricates a row).
fn operation_application_id(conn: &rusqlite::Connection, id: &str) -> Option<String> {
    adapters::resolve(conn, id).ok().and_then(|s| {
        runtime_store::application_id_for(conn, s.as_source(), id)
            .ok()
            .flatten()
    })
}

/// Redacted input snapshot for the journal. Never stores env values or secrets.
fn redacted_for(kind: &str, id: &str) -> String {
    serde_json::json!({ "kind": kind, "appId": id }).to_string()
}

fn emit_operation(app: &tauri::AppHandle, conn: &rusqlite::Connection, op_id: i64) -> Result<()> {
    if let Some(operation) = op::get_operation(conn, op_id)? {
        emit_db_state_changed(
            app,
            "creative-operation",
            serde_json::to_value(&operation).unwrap_or_else(|_| serde_json::json!({ "id": op_id })),
        );
    }
    Ok(())
}

/// Guarded phase change + emit (called from the DB scopes of each mutation).
fn journal(
    app: &tauri::AppHandle,
    conn: &rusqlite::Connection,
    op_id: i64,
    from: &[&str],
    to: &str,
) -> Result<()> {
    op::transition(conn, op_id, from, to)?;
    emit_operation(app, conn, op_id)
}

fn settle_success(app: &tauri::AppHandle, conn: &rusqlite::Connection, op_id: i64) -> Result<()> {
    op::finish_success(conn, op_id)?;
    emit_operation(app, conn, op_id)
}

fn settle_failure(
    app: &tauri::AppHandle,
    conn: &rusqlite::Connection,
    op_id: i64,
    code: &str,
    message: &str,
) -> Result<()> {
    op::finish_failure(conn, op_id, Some(code), message)?;
    emit_operation(app, conn, op_id)
}

fn settle_cancelled(
    app: &tauri::AppHandle,
    conn: &rusqlite::Connection,
    op_id: i64,
    reason: &str,
) -> Result<()> {
    op::finish_cancelled(conn, op_id, Some(reason))?;
    emit_operation(app, conn, op_id)
}

#[tauri::command]
pub fn creative_app_list(state: State<'_, AppState>) -> Result<Vec<CreativeAppSummary>> {
    let c = conn(&state.db)?;
    service::CreativeAppService::list(&c)
}

#[tauri::command]
pub async fn creative_app_start(
    id: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    lock: State<'_, MutationLock>,
    local_runtime: State<'_, LocalRuntimeHandle>,
) -> Result<MutationResult> {
    let pool = state.db.clone();
    let host_port = host_http_port(&state);
    let local_runtime = local_runtime.inner().clone();
    let lock = lock.inner().clone();
    let ctx = lifecycle_ctx(app_handle, local_runtime, host_port);

    // Journal: create the operation BEFORE the app lock so `waiting` is visible
    // to the Renderer while the mutation waits its turn (CR-201 + CR-202).
    let (op_id, op_app_id) = {
        let c = conn(&pool)?;
        let app_id = operation_application_id(&c, &id);
        let redacted = redacted_for(op::KIND_START, &id);
        (
            op::create_operation(
                &c,
                app_id.as_deref(),
                op::KIND_START,
                "user",
                Some(&redacted),
            )?,
            app_id,
        )
    };
    {
        let c = conn(&pool)?;
        op::transition(&c, op_id, &[op::PHASE_PENDING], op::PHASE_WAITING)?;
    }
    let _ = op_app_id;

    // Phase 1 (spawn) holds the per-application lock so the same app stays
    // exclusive; other apps proceed in parallel (CR-202). Phase 2 (health) runs
    // WITHOUT the lock so a concurrent stop can cancel a long start.
    let spawned = {
        let _guard = lock.acquire_app(&id).await;
        let pool_spawn = pool.clone();
        let id_spawn = id.clone();
        let app = ctx.app.clone();
        let ctx2 = ctx.clone();
        tokio::task::spawn_blocking(move || {
            let rt = tokio::runtime::Handle::current();
            let c = conn(&pool_spawn)?;
            journal(
                &app,
                &c,
                op_id,
                &[op::PHASE_WAITING, op::PHASE_PENDING],
                op::PHASE_RUNNING,
            )?;
            rt.block_on(adapters::spawn_start(&c, &ctx2, &id_spawn))
        })
        .await
        .map_err(|e| Error::Internal(format!("start join: {e}")))?
    }?;

    let ctx3 = ctx;
    let settle = tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Handle::current();
        let c = conn(&pool)?;
        let result = rt.block_on(adapters::await_ready(&c, &ctx3, &id, &spawned));
        match result {
            Ok(summary) if summary.state == CreativeAppState::Running => {
                settle_success(&ctx3.app, &c, op_id)?;
                Ok(summary)
            }
            Ok(summary) => {
                // Stop preempted the health wait; the instance is owned by the
                // stop path and the start operation is cancelled.
                let _ = settle_cancelled(
                    &ctx3.app,
                    &c,
                    op_id,
                    "start superseded by a concurrent stop",
                );
                Ok(summary)
            }
            Err(e) => {
                let _ = settle_failure(&ctx3.app, &c, op_id, "start_failed", &e.to_string());
                Err(e)
            }
        }
    })
    .await
    .map_err(|e| Error::Internal(format!("start health join: {e}")))??;

    Ok(MutationResult {
        operation_id: op_id,
        summary: settle,
    })
}

#[tauri::command]
pub async fn creative_app_stop(
    id: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    lock: State<'_, MutationLock>,
    local_runtime: State<'_, LocalRuntimeHandle>,
) -> Result<MutationResult> {
    let pool = state.db.clone();
    let host_port = host_http_port(&state);
    let local_runtime = local_runtime.inner().clone();
    let lock = lock.inner().clone();
    let ctx = lifecycle_ctx(app_handle, local_runtime, host_port);

    let op_id = {
        let c = conn(&pool)?;
        let app_id = operation_application_id(&c, &id);
        let redacted = redacted_for(op::KIND_STOP, &id);
        op::create_operation(
            &c,
            app_id.as_deref(),
            op::KIND_STOP,
            "user",
            Some(&redacted),
        )?
    };
    {
        let c = conn(&pool)?;
        op::transition(&c, op_id, &[op::PHASE_PENDING], op::PHASE_WAITING)?;
    }

    let result = {
        let _guard = lock.acquire_app(&id).await;
        let app = ctx.app.clone();
        let pool_inner = pool.clone();
        let id_inner = id.clone();
        let ctx_inner = ctx.clone();
        tokio::task::spawn_blocking(move || {
            let rt = tokio::runtime::Handle::current();
            let c = conn(&pool_inner)?;
            journal(
                &app,
                &c,
                op_id,
                &[op::PHASE_WAITING, op::PHASE_PENDING],
                op::PHASE_RUNNING,
            )?;
            rt.block_on(adapters::facade::stop(&c, &ctx_inner, &id_inner))
        })
        .await
        .map_err(|e| Error::Internal(format!("stop join: {e}")))?
    };

    match result {
        Ok(summary) => {
            let c = conn(&pool)?;
            settle_success(&ctx.app, &c, op_id)?;
            Ok(MutationResult {
                operation_id: op_id,
                summary,
            })
        }
        Err(e) => {
            // Stop failure leaves the instance cleanup_failed (active-like);
            // the operation itself is failed — resources are NOT verified
            // released, so restart stays blocked until a retry stop succeeds.
            let c = conn(&pool)?;
            settle_failure(&ctx.app, &c, op_id, "stop_failed", &e.to_string())?;
            Err(e)
        }
    }
}

#[tauri::command]
pub async fn creative_app_delete(
    id: String,
    options: Option<DeleteOptions>,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    lock: State<'_, MutationLock>,
    local_runtime: State<'_, LocalRuntimeHandle>,
    browser: State<'_, BrowserStateHandle>,
) -> Result<DeleteMutationResult> {
    let pool = state.db.clone();
    let host_port = host_http_port(&state);
    let local_runtime = local_runtime.inner().clone();
    let lock = lock.inner().clone();
    let browser = browser.inner();
    let ctx = lifecycle_ctx(app_handle, local_runtime, host_port);
    let opts = options.unwrap_or_default();

    let op_id = {
        let c = conn(&pool)?;
        let app_id = operation_application_id(&c, &id);
        let redacted = redacted_for(op::KIND_DELETE, &id);
        op::create_operation(
            &c,
            app_id.as_deref(),
            op::KIND_DELETE,
            "user",
            Some(&redacted),
        )?
    };
    {
        let c = conn(&pool)?;
        op::transition(&c, op_id, &[op::PHASE_PENDING], op::PHASE_WAITING)?;
    }

    let result = {
        let _guard = lock.acquire_app(&id).await;
        let app = ctx.app.clone();
        let pool_inner = pool.clone();
        let id_inner = id.clone();
        let opts_inner = opts.clone();
        let ctx_inner = ctx.clone();
        tokio::task::spawn_blocking(move || {
            let rt = tokio::runtime::Handle::current();
            let c = conn(&pool_inner)?;
            journal(
                &app,
                &c,
                op_id,
                &[op::PHASE_WAITING, op::PHASE_PENDING],
                op::PHASE_RUNNING,
            )?;
            rt.block_on(adapters::facade::delete(
                &c, &ctx_inner, &id_inner, opts_inner,
            ))
        })
        .await
        .map_err(|e| Error::Internal(format!("delete join: {e}")))?
    };

    match result {
        Ok(delete_result) => {
            // CR-303: a delete must not leave a child WebView showing the removed
            // app — close it when it was showing this app (audit #09).
            let showing = browser::browser_current(browser, &id)
                .ok()
                .and_then(|v| v.get("appId").and_then(|x| x.as_str()).map(str::to_string));
            if showing.as_deref() == Some(id.as_str()) {
                let _ = browser::browser_close(&ctx.app, browser, &id);
            }
            // On success the application row is gone; the operation survives with
            // application_id NULL via the FK (audit trail).
            let c = conn(&pool)?;
            settle_success(&ctx.app, &c, op_id)?;
            Ok(DeleteMutationResult {
                operation_id: op_id,
                result: delete_result,
            })
        }
        Err(e) => {
            let c = conn(&pool)?;
            settle_failure(&ctx.app, &c, op_id, "delete_failed", &e.to_string())?;
            Err(e)
        }
    }
}

#[tauri::command]
pub async fn creative_app_restart(
    id: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    lock: State<'_, MutationLock>,
    local_runtime: State<'_, LocalRuntimeHandle>,
) -> Result<MutationResult> {
    let pool = state.db.clone();
    let host_port = host_http_port(&state);
    let local_runtime = local_runtime.inner().clone();
    let lock = lock.inner().clone();
    let ctx = lifecycle_ctx(app_handle, local_runtime, host_port);

    let op_id = {
        let c = conn(&pool)?;
        let app_id = operation_application_id(&c, &id);
        let redacted = redacted_for(op::KIND_RESTART, &id);
        op::create_operation(
            &c,
            app_id.as_deref(),
            op::KIND_RESTART,
            "user",
            Some(&redacted),
        )?
    };
    {
        let c = conn(&pool)?;
        op::transition(&c, op_id, &[op::PHASE_PENDING], op::PHASE_WAITING)?;
    }

    let result = {
        let _guard = lock.acquire_app(&id).await;
        let app = ctx.app.clone();
        let pool_inner = pool.clone();
        let id_inner = id.clone();
        let ctx_inner = ctx.clone();
        tokio::task::spawn_blocking(move || {
            let rt = tokio::runtime::Handle::current();
            let c = conn(&pool_inner)?;
            journal(
                &app,
                &c,
                op_id,
                &[op::PHASE_WAITING, op::PHASE_PENDING],
                op::PHASE_RUNNING,
            )?;
            rt.block_on(adapters::restart(&c, &ctx_inner, &id_inner))
        })
        .await
        .map_err(|e| Error::Internal(format!("restart join: {e}")))?
    };

    match result {
        Ok(summary) => {
            let c = conn(&pool)?;
            settle_success(&ctx.app, &c, op_id)?;
            Ok(MutationResult {
                operation_id: op_id,
                summary,
            })
        }
        Err(e) => {
            let c = conn(&pool)?;
            settle_failure(&ctx.app, &c, op_id, "restart_failed", &e.to_string())?;
            Err(e)
        }
    }
}

#[tauri::command]
pub fn creative_app_get_open_target(id: String, state: State<'_, AppState>) -> Result<OpenTarget> {
    let c = conn(&state.db)?;
    service::CreativeAppService::get_open_target(&c, &id)
}

/// async + spawn_blocking：inspect 会做 GitHub 网络 IO，同步命令会阻塞
/// Tauri 主线程（整窗口卡死直到探测返回）。
#[tauri::command]
pub async fn creative_app_inspect_github(
    request: InspectGithubRequest,
    state: State<'_, AppState>,
) -> Result<InspectGithubResult> {
    let pool = state.db.clone();
    tokio::task::spawn_blocking(move || {
        let c = conn(&pool)?;
        install::inspect(&c, &request)
    })
    .await
    .map_err(|e| Error::Internal(format!("inspect join: {e}")))?
}

#[tauri::command]
pub async fn creative_app_install_github(
    request: InstallGithubRequest,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    lock: State<'_, MutationLock>,
) -> Result<MutationResult> {
    let pool = state.db.clone();
    let lock = lock.inner().clone();
    let handle = app_handle.clone();

    // No application identity exists yet — the operation is created without one
    // and bound after install (CR-201). Install is bounded by the shared
    // install/Docker semaphore instead of a per-app lock (CR-202).
    let op_id = {
        let c = conn(&pool)?;
        let redacted = serde_json::json!({ "kind": op::KIND_INSTALL }).to_string();
        op::create_operation(&c, None, op::KIND_INSTALL, "user", Some(&redacted))?
    };
    let _permit = lock.acquire_install().await;

    let pool_inner = pool.clone();
    let handle_inner = handle.clone();
    let inner = tokio::task::spawn_blocking(move || -> crate::Result<CreativeAppSummary> {
        let rt = tokio::runtime::Handle::current();
        let c = conn(&pool_inner)?;
        journal(
            &handle_inner,
            &c,
            op_id,
            &[op::PHASE_PENDING],
            op::PHASE_RUNNING,
        )?;
        let summary = rt.block_on(install::install_github(&c, &handle_inner, request))?;
        // Unified identity + startup plan for the newly installed external app.
        let app_id = runtime_store::find_or_create_application(
            &c,
            CreativeAppSource::ExternalGithub,
            &summary.id,
        )?;
        if let Ok(Some(rec)) = store::get_app(&c, &summary.id) {
            let _ = runtime_store::upsert_active_plan(&c, &app_id, &rec.runtime_config_json);
        }
        let _ = op::set_application(&c, op_id, &app_id);
        runtime_store::attach_identity(&c, summary)
    })
    .await
    .map_err(|e| Error::Internal(format!("install join: {e}")))?;

    match inner {
        Ok(summary) => {
            let c = conn(&pool)?;
            settle_success(&handle, &c, op_id)?;
            Ok(MutationResult {
                operation_id: op_id,
                summary,
            })
        }
        Err(e) => {
            let c = conn(&pool)?;
            let _ = settle_failure(&handle, &c, op_id, "install_failed", &e.to_string());
            Err(e)
        }
    }
}

/// Snapshot of all non-terminal operations (renderer projection, CR-203).
#[tauri::command]
pub fn creative_app_operations(state: State<'_, AppState>) -> Result<Vec<op::Operation>> {
    let c = conn(&state.db)?;
    op::active_operations(&c)
}

#[tauri::command]
pub fn creative_app_operation_get(id: i64, state: State<'_, AppState>) -> Result<op::Operation> {
    let c = conn(&state.db)?;
    op::get_operation_or(&c, id)
}

/// Cancel an operation that has not started any side effect (pending/waiting).
#[tauri::command]
pub fn creative_app_operation_cancel(
    id: i64,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<op::Operation> {
    let c = conn(&state.db)?;
    let operation = op::cancel(&c, id)?;
    emit_db_state_changed(
        &app_handle,
        "creative-operation",
        serde_json::to_value(&operation).unwrap_or_default(),
    );
    Ok(operation)
}

#[tauri::command]
pub async fn creative_app_logs(
    runtime_id: String,
    tail: Option<u32>,
    cursor: Option<u64>,
    state: State<'_, AppState>,
    local_runtime: State<'_, LocalRuntimeHandle>,
) -> Result<String> {
    let pool = state.db.clone();
    let tail = tail.unwrap_or(200) as usize;
    let cursor = cursor.unwrap_or(0);
    let local_runtime = local_runtime.inner().clone();

    let (source, rt_to_read, app_scope) = {
        let c = conn(&pool)?;
        resolve_log_scope(&c, &runtime_id)?
    };

    match source {
        ResolvedSource::LocalProject => {
            if let Some(rt) = &rt_to_read {
                // Runtime-scoped: the exact run the caller asked for (CR-301).
                let mem = local_runtime.recent_logs(&app_scope, rt, cursor, tail);
                if !mem.is_empty() {
                    return Ok(format_local_log_lines(&mem));
                }
                return Ok(local_runtime.persisted_tail(&app_scope, rt, 256 * 1024));
            }
            // Stopped app (or pre-runtime legacy): app-level aggregate (dual-read).
            Ok(local_runtime.app_aggregate_tail(&app_scope, 256 * 1024))
        }
        ResolvedSource::ExternalGithub => {
            let cfg_json = {
                let c = conn(&pool)?;
                let rec = store::get_app(&c, &app_scope)?
                    .ok_or_else(|| Error::NotFound(app_scope.clone()))?;
                rec.runtime_config_json
            };
            let cfg = store::parse_runtime_config(&cfg_json)?;
            match cfg {
                RuntimeConfig::DockerCompose {
                    project_name,
                    compose_file,
                    ..
                } => {
                    docker::compose_logs(
                        &project_name,
                        &std::path::PathBuf::from(compose_file),
                        tail,
                    )
                    .await
                }
                RuntimeConfig::DockerRun { container_name, .. } => {
                    docker::docker_logs(&container_name, tail).await
                }
            }
        }
        ResolvedSource::Internal => Err(Error::InvalidInput(
            "workshop modules do not expose process logs via creativeApp.logs".into(),
        )),
    }
}

#[tauri::command]
pub async fn creative_app_reconcile(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    local_runtime: State<'_, LocalRuntimeHandle>,
) -> Result<u32> {
    let pool = state.db.clone();
    let handle = app_handle.clone();
    let local_runtime = local_runtime.inner().clone();
    tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Handle::current();
        let c = conn(&pool)?;
        let exits = rt.block_on(local::lifecycle::poll_and_reconcile_exits(
            &c,
            Some(&handle),
            local_runtime.as_ref(),
        ))?;
        let docker_n = rt.block_on(install::reconcile_all(&c, Some(&handle)))? as u32;
        let local_n = local::lifecycle::reconcile_local_apps(&c, Some(&handle))?;
        Ok(docker_n.saturating_add(local_n).saturating_add(exits))
    })
    .await
    .map_err(|e| Error::Internal(format!("reconcile join: {e}")))?
}

#[tauri::command]
pub fn creative_app_github_token_status(state: State<'_, AppState>) -> Result<GithubTokenStatus> {
    let c = conn(&state.db)?;
    store::github_token_status(&c)
}

#[tauri::command]
pub fn creative_app_github_token_set(
    token: String,
    state: State<'_, AppState>,
) -> Result<GithubTokenStatus> {
    let c = conn(&state.db)?;
    store::set_github_token(&c, token.trim())?;
    store::github_token_status(&c)
}

#[tauri::command]
pub fn creative_app_github_token_clear(state: State<'_, AppState>) -> Result<GithubTokenStatus> {
    let c = conn(&state.db)?;
    store::clear_github_token(&c)?;
    store::github_token_status(&c)
}

#[tauri::command]
pub async fn creative_app_docker_status() -> Result<DockerEngineStatus> {
    Ok(docker::engine_status().await)
}

// ── Child browser ──────────────────────────────────────────────

#[tauri::command]
pub fn creative_app_browser_show(
    app_id: String,
    url: String,
    bounds: BrowserBounds,
    app_handle: tauri::AppHandle,
    browser: State<'_, BrowserStateHandle>,
    state: State<'_, AppState>,
) -> Result<()> {
    // CR-303: never show a preview for an app without an active running
    // instance — a stopped app's URL must not become visible in a WebView.
    let active_instance = {
        let c = conn(&state.db)?;
        let source = adapters::resolve(&c, &app_id)?;
        let app_identity = runtime_store::application_id_for(&c, source.as_source(), &app_id)?
            .ok_or_else(|| Error::InvalidInput("app has no registered identity".into()))?;
        runtime_store::active_instance_id(&c, &app_identity)?
            .ok_or_else(|| Error::InvalidInput("app is not running; cannot open preview".into()))?
    };
    // External action first: show the WebView. A show failure must not leave a
    // DB preview bind behind (audit #02: DB / BrowserState must not diverge).
    browser::browser_show(&app_handle, &browser, &app_id, &url, bounds)?;
    // Commit the preview bind; on DB failure compensate by hiding the WebView.
    let c = conn(&state.db)?;
    if let Err(e) =
        runtime_store::upsert_preview_target(&c, &active_instance, &url, "child_webview")
    {
        let _ = browser::browser_hide(&app_handle, &app_id);
        return Err(e);
    }
    Ok(())
}

#[tauri::command]
pub fn creative_app_browser_set_bounds(
    app_id: String,
    bounds: BrowserBounds,
    app_handle: tauri::AppHandle,
) -> Result<()> {
    browser::browser_set_bounds(&app_handle, &app_id, bounds)
}

#[tauri::command]
pub fn creative_app_browser_back(app_id: String, app_handle: tauri::AppHandle) -> Result<()> {
    browser::browser_back(&app_handle, &app_id)
}

#[tauri::command]
pub fn creative_app_browser_forward(app_id: String, app_handle: tauri::AppHandle) -> Result<()> {
    browser::browser_forward(&app_handle, &app_id)
}

#[tauri::command]
pub fn creative_app_browser_reload(app_id: String, app_handle: tauri::AppHandle) -> Result<()> {
    browser::browser_reload(&app_handle, &app_id)
}

#[tauri::command]
pub fn creative_app_browser_hide(app_id: String, app_handle: tauri::AppHandle) -> Result<()> {
    browser::browser_hide(&app_handle, &app_id)
}

#[tauri::command]
pub fn creative_app_browser_close(
    app_id: String,
    app_handle: tauri::AppHandle,
    browser: State<'_, BrowserStateHandle>,
    state: State<'_, AppState>,
) -> Result<()> {
    // CR-303: close the WebView FIRST (external action); only on success clear
    // the DB preview bind. A close failure must not lose the bind while the
    // WebView is still open (audit #02).
    browser::browser_close(&app_handle, &browser, &app_id)?;
    // The `app_id` is passed from the frontend — clear the DB bind for this app.
    if let Ok(c) = conn(&state.db) {
        if let Ok(source) = adapters::resolve(&c, &app_id) {
            if let Ok(Some(app_identity)) =
                runtime_store::application_id_for(&c, source.as_source(), &app_id)
            {
                if let Ok(Some(iid)) = runtime_store::active_instance_id(&c, &app_identity) {
                    let _ = runtime_store::clear_preview_targets(&c, &iid);
                }
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub fn creative_app_browser_current(
    app_id: String,
    browser: State<'_, BrowserStateHandle>,
) -> Result<serde_json::Value> {
    browser::browser_current(&browser, &app_id)
}

/// List all surfaces for the given application (CR-501).
#[tauri::command]
pub fn creative_app_surface_list(
    application_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<ApplicationSurface>> {
    let c = conn(&state.db)?;
    surface_store::list_surfaces(&c, &application_id)
}

/// List all windows for the given application (CR-501).
#[tauri::command]
pub fn creative_app_window_list(
    application_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<WindowInstance>> {
    let c = conn(&state.db)?;
    surface_store::list_windows(&c, &application_id)
}

/// Open a window for the given application (CR-501).
/// Creates a window instance if one doesn't exist for the surface.
#[tauri::command]
pub fn creative_app_window_open(
    application_id: String,
    surface_id: String,
    label: String,
    state: State<'_, AppState>,
) -> Result<WindowInstance> {
    let c = conn(&state.db)?;
    // Check if a window with this label already exists
    if let Some(existing) = surface_store::find_window_by_label(&c, &label)? {
        // Re-open it
        surface_store::update_window_state(&c, &existing.id, WindowInstance::STATE_OPEN)?;
        return Ok(existing);
    }
    let wid = surface_store::create_window(&c, &application_id, &surface_id, None, &label)?;
    let w = surface_store::find_window_by_label(&c, &label)?
        .ok_or_else(|| Error::Internal("window vanished after create".into()))?;
    surface_store::update_window_state(&c, &wid, WindowInstance::STATE_OPEN)?;
    Ok(w)
}

/// Close a window (CR-501).
#[tauri::command]
pub fn creative_app_window_close(window_id: String, state: State<'_, AppState>) -> Result<()> {
    let c = conn(&state.db)?;
    surface_store::update_window_state(&c, &window_id, WindowInstance::STATE_CLOSED)
}

/// Minimize a window (CR-501).
#[tauri::command]
pub fn creative_app_window_minimize(window_id: String, state: State<'_, AppState>) -> Result<()> {
    let c = conn(&state.db)?;
    surface_store::update_window_state(&c, &window_id, WindowInstance::STATE_MINIMIZED)
}

/// Restore a window (CR-501).
#[tauri::command]
pub fn creative_app_window_restore(window_id: String, state: State<'_, AppState>) -> Result<()> {
    let c = conn(&state.db)?;
    surface_store::update_window_state(&c, &window_id, WindowInstance::STATE_OPEN)
}

// ── Local project (third source) ───────────────────────────────────

// ── Agent proposal gate (batch 10 CR-1001/1002) ───────────────────

/// Validate an agent proposal through the Host gate. Returns the proposal
/// (if valid) with a redacted journal snapshot. Never registers anything.
#[tauri::command]
pub fn creative_app_proposal_validate(
    proposal: crate::creative_app::proposal::AgentProposal,
) -> Result<crate::creative_app::proposal::ValidatedProposal> {
    proposal.validate()?;
    Ok(crate::creative_app::proposal::ValidatedProposal {
        redacted: crate::creative_app::proposal::redacted_proposal_input(&proposal),
        proposal,
    })
}

/// Reject an agent proposal by its stable proposal id. The Host looks up the
/// persisted inbox row (never a Renderer-supplied proposal body), CASes
/// pending → rejected, and returns a tagged result. Rejecting an already-
/// decided proposal is an idempotent no-op (`already_decided`).
#[tauri::command]
pub async fn creative_app_proposal_reject(
    proposal_id: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<crate::creative_app::proposal_inbox::ProposalRejectResult> {
    use crate::creative_app::proposal_inbox::{self, ProposalRejectResult};
    let pool = state.db.clone();
    // Best-effort sync: the proposal may have been produced after the Renderer's
    // last list. A sync failure must not block a reject of an already-known row.
    if let Err(e) = proposal_inbox::sync_pending_from_daemon(&pool).await {
        eprintln!("[proposal] sync before reject failed (continuing): {e}");
    }
    let app = app_handle.clone();
    tokio::task::spawn_blocking(move || {
        let c = conn(&pool)?;
        let Some(stored) = proposal_inbox::get_stored(&c, &proposal_id)? else {
            return Err(Error::InvalidInput(format!(
                "proposal not found or not pending: {proposal_id}"
            )));
        };
        match stored.status.as_str() {
            proposal_inbox::STATUS_PENDING => {}
            other => {
                return Ok(ProposalRejectResult::AlreadyDecided {
                    proposal_id: proposal_id.clone(),
                    current_status: other.to_string(),
                });
            }
        }
        let redacted = crate::creative_app::proposal::redacted_proposal_input(
            &crate::creative_app::proposal::validate_protocol_proposal(&stored.envelope.payload)?
                .proposal,
        );
        let op_id = op::create_operation(&c, None, "proposal_reject", "user", Some(&redacted))?;
        if !proposal_inbox::cas_status(
            &c,
            &proposal_id,
            proposal_inbox::STATUS_PENDING,
            proposal_inbox::STATUS_REJECTED,
        )? {
            op::finish_failure(
                &c,
                op_id,
                Some("proposal_already_decided"),
                "concurrent decision",
            )?;
            emit_operation(&app, &c, op_id)?;
            return Ok(ProposalRejectResult::AlreadyDecided {
                proposal_id: proposal_id.clone(),
                current_status: stored.status.clone(),
            });
        }
        op::finish_success(&c, op_id)?;
        emit_operation(&app, &c, op_id)?;
        crate::emit_db_state_changed(
            &app,
            "creative-app",
            serde_json::json!({ "action": "proposal_rejected", "id": proposal_id }),
        );
        Ok(ProposalRejectResult::Rejected { proposal_id })
    })
    .await
    .map_err(|e| Error::Internal(format!("proposal_reject join: {e}")))?
}

/// Approve an agent proposal by its stable proposal id.
///
/// The Host looks up the persisted, Host-validated inbox row, re-resolves the
/// executable/interpreter identity (canonical path + Host-recomputed SHA-256),
/// records the executable approval, CASes pending → approved, and registers
/// the application. The Renderer never supplies executable paths or hashes.
///
/// Failures keep the proposal visible to the user: a verification failure
/// leaves the status `pending`; a registration failure CASes it to `failed`
/// and returns the error — never a success value.
#[tauri::command]
pub async fn creative_app_proposal_approve(
    proposal_id: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    lock: State<'_, MutationLock>,
) -> Result<crate::creative_app::proposal_inbox::ProposalApproveResult> {
    use crate::creative_app::proposal_inbox::{self, ProposalApproveResult};
    let pool = state.db.clone();
    if let Err(e) = proposal_inbox::sync_pending_from_daemon(&pool).await {
        eprintln!("[proposal] sync before approve failed (continuing): {e}");
    }
    let lock = lock.inner().clone();
    let _guard = lock.acquire_app("__registration__").await;

    let handle = app_handle.clone();
    tokio::task::spawn_blocking(move || {
        let mut c = conn(&pool)?;
        let Some(stored) = proposal_inbox::get_stored(&c, &proposal_id)? else {
            return Err(Error::InvalidInput(format!(
                "proposal not found or not pending: {proposal_id}"
            )));
        };
        match stored.status.as_str() {
            proposal_inbox::STATUS_PENDING => {}
            other => {
                return Ok(ProposalApproveResult::AlreadyDecided {
                    proposal_id: proposal_id.clone(),
                    current_status: other.to_string(),
                });
            }
        }

        // Re-validate through the Host gate (defense in depth) and resolve the
        // executable/interpreter identity. A shell pseudo-python, a missing
        // binary, or a fake agent hash never gets here: the profile that was
        // stored at merge time already passed the structural gate, and the
        // identity resolution below re-checks existence + content.
        let validated =
            crate::creative_app::proposal::validate_protocol_proposal(&stored.envelope.payload)?;
        let proposal = validated.proposal;
        let (canonical, identity, verified_proposal) =
            proposal_inbox::resolve_proposal_identity(&proposal)?;

        let redacted = crate::creative_app::proposal::redacted_proposal_input(&proposal);
        let op_id = op::create_operation(&c, None, "proposal_approve", "user", Some(&redacted))?;
        op::transition(&c, op_id, &[op::PHASE_PENDING], op::PHASE_RUNNING)?;

        // Pin the executable approval record BEFORE the CAS so a verification
        // failure leaves the proposal pending (the user sees the error and can
        // still reject).
        if !canonical.is_empty() {
            proposal_inbox::record_executable_approval(
                &c,
                &canonical,
                &identity,
                &format!("proposal:{proposal_id}"),
                "user",
                &proposal_id,
            )?;
        }

        if !proposal_inbox::cas_status(
            &c,
            &proposal_id,
            proposal_inbox::STATUS_PENDING,
            proposal_inbox::STATUS_APPROVED,
        )? {
            op::finish_failure(
                &c,
                op_id,
                Some("proposal_already_decided"),
                "concurrent decision",
            )?;
            emit_operation(&handle, &c, op_id)?;
            return Ok(ProposalApproveResult::AlreadyDecided {
                proposal_id: proposal_id.clone(),
                current_status: stored.status.clone(),
            });
        }

        match register_proposal_app(&mut c, &verified_proposal) {
            Ok(summary) => {
                op::finish_success(&c, op_id)?;
                emit_operation(&handle, &c, op_id)?;
                crate::emit_db_state_changed(
                    &handle,
                    "creative-app",
                    serde_json::json!({ "action": "proposal_approved", "id": summary.id }),
                );
                Ok(ProposalApproveResult::Approved {
                    proposal_id: proposal_id.clone(),
                    app: summary,
                })
            }
            Err(e) => {
                // Registration failed — CAS pending → failed (terminal) and
                // return the error. The approval card stays visible with the
                // error; it is never shown as success.
                let _ = proposal_inbox::cas_status(
                    &c,
                    &proposal_id,
                    proposal_inbox::STATUS_APPROVED,
                    proposal_inbox::STATUS_FAILED,
                );
                op::finish_failure(&c, op_id, Some("proposal_register_failed"), &e.to_string())?;
                emit_operation(&handle, &c, op_id)?;
                Err(e)
            }
        }
    })
    .await
    .map_err(|e| Error::Internal(format!("proposal_approve join: {e}")))?
}

/// List pending proposals awaiting user approval. The Host first pulls fresh
/// proposal facts from the daemon (reconnect-recoverable), then returns the
/// pending inbox rows — each flattened with its validated proposal intent.
#[tauri::command]
pub async fn creative_app_proposal_list(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<crate::creative_app::proposal_inbox::ProposalInboxEntry>> {
    use crate::creative_app::proposal_inbox;
    let pool = state.db.clone();
    proposal_inbox::sync_pending_from_daemon(&pool).await?;
    let _app = app_handle;
    tokio::task::spawn_blocking(move || {
        let c = conn(&pool)?;
        proposal_inbox::list_pending(&c)
    })
    .await
    .map_err(|e| Error::Internal(format!("proposal_list join: {e}")))?
}

/// Register an approved proposal as a real application. Only drivers with a
/// concrete registration path are supported; others return a clear error.
fn register_proposal_app(
    c: &mut rusqlite::Connection,
    proposal: &crate::creative_app::proposal::AgentProposal,
) -> Result<CreativeAppSummary> {
    use crate::creative_app::local;
    use crate::creative_app::model::{
        LaunchMode, LaunchPlanSource, LaunchProgram, LocalLaunchRuntime, LocalProjectKind,
    };
    use crate::creative_app::proposal::ProposedDriver;

    match &proposal.driver {
        ProposedDriver::StaticHttp => {
            // Static HTTP app inside the proposed project root.
            let root = local::canonical_project_root(&proposal.project_root)?;
            let root_s = root.to_string_lossy().to_string();
            if local::get_app_by_root(c, &root_s)?.is_some() {
                return Err(Error::InvalidInput(
                    "path already registered as local creative app".into(),
                ));
            }
            let kind = if root.join("index.html").is_file() {
                LocalProjectKind::Html
            } else {
                LocalProjectKind::Unknown
            };
            let plan = LaunchPlan {
                schema_version: 1,
                source: LaunchPlanSource::Ai,
                project_kind: kind,
                runtime: LocalLaunchRuntime::StaticHttp,
                program: LaunchProgram::Internal,
                cwd_relative: ".".into(),
                script: None,
                entry_file: Some("index.html".into()),
                script_runner: None,
                args: vec![],
                environment_keys: proposal.environment_keys.clone(),
                port: proposal_port(proposal),
                open_path: proposal.open_path.clone(),
                health_path: proposal.health_path.clone(),
                startup_timeout_ms: 60_000,
                auto_open: true,
                confidence: Some(0.9),
                reason: "agent proposal approved".into(),
                compose: None,
                process_profile: None,
                trade_approval: None,
            };
            let summary = create_local_app(
                c,
                CreateLocalRequest {
                    project_root: root_s,
                    title: proposal.title.clone(),
                    description: None,
                    icon: None,
                    launch_mode: LaunchMode::Custom,
                    launch_plan: Some(plan),
                    env: vec![],
                    auto_open: Some(true),
                    startup_timeout_ms: Some(60_000),
                },
            )?;
            Ok(summary)
        }
        ProposedDriver::Python(p) => {
            // Python WebUI: build a managed-process plan carrying the profile.
            let root = local::canonical_project_root(&proposal.project_root)?;
            let root_s = root.to_string_lossy().to_string();
            if local::get_app_by_root(c, &root_s)?.is_some() {
                return Err(Error::InvalidInput(
                    "path already registered as local creative app".into(),
                ));
            }
            crate::creative_app::process_driver::validate_python_profile(p)?;
            let plan = LaunchPlan {
                schema_version: 1,
                source: LaunchPlanSource::Ai,
                project_kind: LocalProjectKind::Unknown,
                runtime: LocalLaunchRuntime::NodeDevServer, // managed-process family
                program: LaunchProgram::Node,
                cwd_relative: p.cwd_relative.clone(),
                script: Some(p.entry.clone()),
                entry_file: Some(p.entry.clone()),
                script_runner: None,
                args: p.args.clone(),
                environment_keys: proposal.environment_keys.clone(),
                port: p.port.clone(),
                open_path: proposal.open_path.clone(),
                health_path: proposal.health_path.clone(),
                startup_timeout_ms: p.startup_timeout_ms,
                auto_open: true,
                confidence: Some(0.85),
                reason: "agent proposal: python webui".into(),
                compose: None,
                process_profile: Some(crate::creative_app::model::ProcessProfile::Python(
                    p.clone(),
                )),
                trade_approval: None,
            };
            let summary = create_local_app(
                c,
                CreateLocalRequest {
                    project_root: root_s,
                    title: proposal.title.clone(),
                    description: None,
                    icon: None,
                    launch_mode: LaunchMode::Custom,
                    launch_plan: Some(plan),
                    env: vec![],
                    auto_open: Some(true),
                    startup_timeout_ms: Some(p.startup_timeout_ms),
                },
            )?;
            Ok(summary)
        }
        ProposedDriver::Binary(b) => {
            // Binary WebUI: build a managed-process plan carrying the profile.
            let root = local::canonical_project_root(&proposal.project_root)?;
            let root_s = root.to_string_lossy().to_string();
            if local::get_app_by_root(c, &root_s)?.is_some() {
                return Err(Error::InvalidInput(
                    "path already registered as local creative app".into(),
                ));
            }
            crate::creative_app::process_driver::validate_binary_profile(b)?;
            let plan = LaunchPlan {
                schema_version: 1,
                source: LaunchPlanSource::Ai,
                project_kind: LocalProjectKind::Unknown,
                runtime: LocalLaunchRuntime::NodeDevServer, // managed-process family
                program: LaunchProgram::Node,
                cwd_relative: b.cwd_relative.clone(),
                script: None,
                entry_file: None,
                script_runner: None,
                args: b.args.clone(),
                environment_keys: proposal.environment_keys.clone(),
                port: b.port.clone(),
                open_path: proposal.open_path.clone(),
                health_path: proposal.health_path.clone(),
                startup_timeout_ms: b.startup_timeout_ms,
                auto_open: true,
                confidence: Some(0.85),
                reason: "agent proposal: binary webui".into(),
                compose: None,
                process_profile: Some(crate::creative_app::model::ProcessProfile::Binary(
                    b.clone(),
                )),
                trade_approval: None,
            };
            let summary = create_local_app(
                c,
                CreateLocalRequest {
                    project_root: root_s,
                    title: proposal.title.clone(),
                    description: None,
                    icon: None,
                    launch_mode: LaunchMode::Custom,
                    launch_plan: Some(plan),
                    env: vec![],
                    auto_open: Some(true),
                    startup_timeout_ms: Some(b.startup_timeout_ms),
                },
            )?;
            Ok(summary)
        }
        ProposedDriver::Compose {
            command,
            privileged: _,
        } => {
            // Compose app: derive the compose file from the project root.
            // The proposal gate already rejected privileged containers and
            // command overrides (validate), so `command` here is empty.
            let root = local::canonical_project_root(&proposal.project_root)?;
            let root_s = root.to_string_lossy().to_string();
            if local::get_app_by_root(c, &root_s)?.is_some() {
                return Err(Error::InvalidInput(
                    "path already registered as local creative app".into(),
                ));
            }
            let compose_file = [
                "docker-compose.yml",
                "docker-compose.yaml",
                "compose.yml",
                "compose.yaml",
            ]
            .iter()
            .find(|f| root.join(f).is_file())
            .map(|f| f.to_string())
            .ok_or_else(|| {
                Error::InvalidInput(
                    "no docker-compose.yml / compose.yml found in project root".into(),
                )
            })?;
            let plan = LaunchPlan {
                schema_version: 1,
                source: LaunchPlanSource::Ai,
                project_kind: LocalProjectKind::Unknown,
                runtime: LocalLaunchRuntime::DockerCompose,
                program: LaunchProgram::Internal,
                cwd_relative: ".".into(),
                script: None,
                entry_file: None,
                script_runner: None,
                args: vec![],
                environment_keys: proposal.environment_keys.clone(),
                port: crate::creative_app::model::LaunchPort {
                    mode: crate::creative_app::model::LaunchPortMode::Auto,
                    value: None,
                },
                open_path: proposal.open_path.clone(),
                health_path: proposal.health_path.clone(),
                startup_timeout_ms: 60_000,
                auto_open: true,
                confidence: Some(0.8),
                reason: "agent proposal: docker compose".into(),
                compose: Some(crate::creative_app::model::ComposePlanDetail {
                    compose_file,
                    project_seed: "agent".into(),
                    service: None,
                    command: command.clone().unwrap_or_default(),
                    health_path: proposal.health_path.clone(),
                    host_port: None,
                }),
                process_profile: None,
                trade_approval: None,
            };
            let summary = create_local_app(
                c,
                CreateLocalRequest {
                    project_root: root_s,
                    title: proposal.title.clone(),
                    description: None,
                    icon: None,
                    launch_mode: LaunchMode::Custom,
                    launch_plan: Some(plan),
                    env: vec![],
                    auto_open: Some(true),
                    startup_timeout_ms: Some(60_000),
                },
            )?;
            Ok(summary)
        }
    }
}

/// Derive a LaunchPort from a proposal's intended open path (auto port).
fn proposal_port(
    _proposal: &crate::creative_app::proposal::AgentProposal,
) -> crate::creative_app::model::LaunchPort {
    crate::creative_app::model::LaunchPort {
        mode: crate::creative_app::model::LaunchPortMode::Auto,
        value: None,
    }
}

#[tauri::command]
pub async fn creative_app_inspect_local(
    request: InspectLocalRequest,
    state: State<'_, AppState>,
) -> Result<LocalProjectScanResult> {
    let pool = state.db.clone();
    tokio::task::spawn_blocking(move || {
        let c = conn(&pool)?;
        crate::creative_app::local::inspect_local_project(&c, &request)
    })
    .await
    .map_err(|e| Error::Internal(format!("inspect join: {e}")))?
}

#[tauri::command]
pub async fn creative_app_create_local(
    request: CreateLocalRequest,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    lock: State<'_, MutationLock>,
) -> Result<CreativeAppSummary> {
    let pool = state.db.clone();
    let lock = lock.inner().clone();
    // New registration: serialize registrations against each other, but do not
    // block lifecycle ops on unrelated apps (CR-202).
    let _guard = lock.acquire_app("__registration__").await;
    let handle = app_handle.clone();
    tokio::task::spawn_blocking(move || {
        let mut c = conn(&pool)?;
        let summary = create_local_app(&mut c, request)?;
        crate::emit_db_state_changed(
            &handle,
            "creative-app",
            serde_json::json!({ "action": "create_local", "id": summary.id }),
        );
        Ok(summary)
    })
    .await
    .map_err(|e| Error::Internal(format!("create_local join: {e}")))?
}

#[tauri::command]
pub async fn creative_app_update_local(
    request: UpdateLocalRequest,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    lock: State<'_, MutationLock>,
) -> Result<CreativeAppSummary> {
    let pool = state.db.clone();
    let lock = lock.inner().clone();
    let _guard = lock.acquire_app(&request.id).await;
    let handle = app_handle.clone();
    tokio::task::spawn_blocking(move || {
        let mut c = conn(&pool)?;
        let summary = update_local_app(&mut c, request)?;
        crate::emit_db_state_changed(
            &handle,
            "creative-app",
            serde_json::json!({ "action": "update_local", "id": summary.id }),
        );
        Ok(summary)
    })
    .await
    .map_err(|e| Error::Internal(format!("update_local join: {e}")))?
}

#[tauri::command]
pub fn creative_app_rescan_local(
    id: String,
    state: State<'_, AppState>,
) -> Result<LocalProjectScanResult> {
    let c = conn(&state.db)?;
    let rec =
        crate::creative_app::local::get_app(&c, &id)?.ok_or_else(|| Error::NotFound(id.clone()))?;
    crate::creative_app::local::inspect_local_project(
        &c,
        &InspectLocalRequest {
            project_root: rec.canonical_project_root,
        },
    )
}

fn create_local_app(
    conn: &mut rusqlite::Connection,
    request: CreateLocalRequest,
) -> Result<CreativeAppSummary> {
    use crate::creative_app::local::{self, fingerprint_plan, validate_launch_plan};

    let root = local::canonical_project_root(&request.project_root)?;
    let root_s = root.to_string_lossy().to_string();

    if let Some(existing) = local::get_app_by_root(conn, &root_s)? {
        return Err(Error::InvalidInput(format!(
            "path already registered as local creative app: {}",
            existing.id
        )));
    }

    let scan = local::inspect_local_project(
        conn,
        &InspectLocalRequest {
            project_root: root_s.clone(),
        },
    )?;

    let plan = match request.launch_mode {
        LaunchMode::Smart => {
            if let Some(p) = request.launch_plan {
                validate_launch_plan(&root, p)?
            } else {
                scan.rule_plan.ok_or_else(|| {
                    Error::InvalidInput(
                        "smart launch could not build a plan; provide a custom LaunchPlan".into(),
                    )
                })?
            }
        }
        LaunchMode::Custom => {
            let p = request.launch_plan.ok_or_else(|| {
                Error::InvalidInput("custom launch_mode requires launchPlan".into())
            })?;
            validate_launch_plan(&root, p)?
        }
    };

    let (device_id, device_name) = local::store::current_device();
    let now = chrono::Utc::now().to_rfc3339();
    let id = uuid::Uuid::new_v4().to_string();
    let title = {
        let t = request.title.trim();
        if t.is_empty() {
            root.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("Local project")
                .to_string()
        } else {
            t.to_string()
        }
    };
    let timeout = request
        .startup_timeout_ms
        .unwrap_or(plan.startup_timeout_ms)
        .clamp(5_000, 300_000);
    let auto_open = request.auto_open.unwrap_or(plan.auto_open);
    let mut plan = plan;
    plan.startup_timeout_ms = timeout;
    plan.auto_open = auto_open;
    let fp = fingerprint_plan(&root, &plan);

    let status_detail_json = if scan.dependencies_missing {
        Some(
            serde_json::to_string(&CreativeAppStatusDetail {
                code: LocalCreativeIssueCode::DependenciesMissing,
                message: "node_modules missing; install dependencies before start".into(),
                recovery_actions: vec![
                    "install_dependencies".into(),
                    "open_terminal".into(),
                    "copy_install_command".into(),
                ],
            })
            .unwrap_or_default(),
        )
    } else {
        None
    };

    let rec = LocalCreativeAppRecord {
        id: id.clone(),
        title,
        description: request.description,
        icon: request.icon,
        canonical_project_root: root_s,
        device_id,
        device_name,
        project_kind: plan.project_kind,
        launch_mode: request.launch_mode,
        launch_plan_json: plan.to_json().map_err(|e| Error::Internal(e.to_string()))?,
        plan_fingerprint: fp,
        state: CreativeAppState::InstalledStopped,
        status_detail_json,
        open_url: None,
        current_port: None,
        process_identity_json: None,
        volume_identity: local::volume_identity(&root),
        auto_open,
        startup_timeout_ms: timeout,
        last_started_at: None,
        last_exit_reason: None,
        last_error: if scan.dependencies_missing {
            Some("dependencies_missing".into())
        } else {
            None
        },
        created_at: now.clone(),
        updated_at: now,
    };

    // Keep the record and encrypted env in one SQLite transaction.
    let tx = conn.transaction().map_err(Error::Database)?;
    local::insert_app(&tx, &rec)?;
    if !request.env.is_empty() {
        let pairs: Vec<(String, String)> =
            request.env.into_iter().map(|p| (p.key, p.value)).collect();
        local::store::replace_env(&tx, &id, &pairs)?;
    }
    tx.commit().map_err(Error::Database)?;

    // The dead `start_after_save` flag was removed (batch 7): the UI calls start
    // explicitly after create; auto-start after save is a product no-op.

    // Unified identity + startup plan for the newly registered local app.
    let app_id =
        runtime_store::find_or_create_application(conn, CreativeAppSource::LocalProject, &id)?;
    let plan_json = plan.to_json().map_err(|e| Error::Internal(e.to_string()))?;
    let _ = runtime_store::upsert_active_plan(conn, &app_id, &plan_json);
    let summary = local::summary_from_local(
        &local::get_app(conn, &id)?.ok_or_else(|| Error::Internal("insert vanished".into()))?,
    );
    runtime_store::attach_identity(conn, summary)
}

fn update_local_app(
    conn: &mut rusqlite::Connection,
    request: UpdateLocalRequest,
) -> Result<CreativeAppSummary> {
    use crate::creative_app::local::{self, fingerprint_plan, validate_launch_plan};

    let mut rec =
        local::get_app(conn, &request.id)?.ok_or_else(|| Error::NotFound(request.id.clone()))?;

    if let Some(title) = request.title {
        let t = title.trim();
        if !t.is_empty() {
            rec.title = t.to_string();
        }
    }
    if let Some(d) = request.description {
        rec.description = Some(d);
    }
    if let Some(i) = request.icon {
        rec.icon = Some(i);
    }
    if let Some(ao) = request.auto_open {
        rec.auto_open = ao;
    }
    if let Some(t) = request.startup_timeout_ms {
        rec.startup_timeout_ms = t.clamp(5_000, 300_000);
    }

    let mut root = std::path::PathBuf::from(&rec.canonical_project_root);
    if let Some(new_root) = request.project_root {
        let canon = local::canonical_project_root(&new_root)?;
        let root_s = canon.to_string_lossy().to_string();
        if root_s != rec.canonical_project_root {
            if let Some(other) = local::get_app_by_root(conn, &root_s)? {
                if other.id != rec.id {
                    return Err(Error::InvalidInput(format!(
                        "path already registered as local creative app: {}",
                        other.id
                    )));
                }
            }
            rec.canonical_project_root = root_s;
            root = canon;
        }
    }

    if let Some(mode) = request.launch_mode {
        rec.launch_mode = mode;
    }

    if let Some(plan) = request.launch_plan {
        let validated = validate_launch_plan(&root, plan)?;
        rec.project_kind = validated.project_kind;
        rec.plan_fingerprint = fingerprint_plan(&root, &validated);
        rec.launch_plan_json = validated
            .to_json()
            .map_err(|e| Error::Internal(e.to_string()))?;
        rec.startup_timeout_ms = validated.startup_timeout_ms;
        rec.auto_open = validated.auto_open;
    }

    let tx = conn.transaction().map_err(Error::Database)?;
    if let Some(env) = request.env {
        let pairs: Vec<(String, String)> = env.into_iter().map(|p| (p.key, p.value)).collect();
        local::store::replace_env(&tx, &rec.id, &pairs)?;
    }
    if let Some(upsert) = request.env_upsert {
        let pairs: Vec<(String, String)> = upsert.into_iter().map(|p| (p.key, p.value)).collect();
        local::store::upsert_env(&tx, &rec.id, &pairs)?;
    }
    if let Some(remove) = request.env_remove_keys {
        local::store::remove_env_keys(&tx, &rec.id, &remove)?;
    }

    rec.updated_at = chrono::Utc::now().to_rfc3339();
    local::update_app(&tx, &rec)?;
    tx.commit().map_err(Error::Database)?;
    // Refresh the active startup plan + unified identity for the app.
    let app_id =
        runtime_store::find_or_create_application(conn, CreativeAppSource::LocalProject, &rec.id)?;
    let _ = runtime_store::upsert_active_plan(conn, &app_id, &rec.launch_plan_json);
    runtime_store::attach_identity(conn, local::summary_from_local(&rec))
}

#[tauri::command]
pub async fn creative_app_resolve_orphan(
    id: String,
    restart: bool,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    lock: State<'_, MutationLock>,
    local_runtime: State<'_, LocalRuntimeHandle>,
) -> Result<CreativeAppSummary> {
    let pool = state.db.clone();
    let host_port = host_http_port(&state);
    let local_runtime = local_runtime.inner().clone();
    let lock = lock.inner().clone();
    let _guard = lock.acquire_app(&id).await;
    let handle = app_handle.clone();
    let ctx = lifecycle_ctx(app_handle, local_runtime.clone(), host_port);
    tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Handle::current();
        let c = conn(&pool)?;
        // Kill the verified orphan, clear identity, settle the orphaned instance
        // (never auto-takeover pipes; CR-302 proof chain).
        let summary = rt.block_on(local::resolve_orphan(
            &c,
            &handle,
            local_runtime.as_ref(),
            &id,
        ))?;
        if restart {
            // Restart is a fresh start on a NEW runtime instance (the resolved
            // orphan is already settled to stopped, so the instance CAS passes).
            // Routes through the driver facade (CR-702).
            let summary = rt.block_on(adapters::facade::start(&c, &ctx, &id))?;
            return runtime_store::attach_identity(&c, summary);
        }
        runtime_store::attach_identity(&c, summary)
    })
    .await
    .map_err(|e| Error::Internal(format!("resolve_orphan join: {e}")))?
}

#[tauri::command]
pub async fn creative_app_get_local_logs(
    id: String,
    limit: Option<u32>,
    state: State<'_, AppState>,
    local_runtime: State<'_, LocalRuntimeHandle>,
) -> Result<Vec<serde_json::Value>> {
    let limit = limit.unwrap_or(200) as usize;
    let local_runtime = local_runtime.inner().clone();
    let (rt_to_read, app_scope) = {
        let c = conn(&state.db)?;
        let (source, rt, scope) = resolve_log_scope(&c, &id)?;
        if source != ResolvedSource::LocalProject {
            return Err(Error::InvalidInput(
                "getLocalLogs is for local project sources only".into(),
            ));
        }
        (rt, scope)
    };
    // Runtime-scoped structured lines; empty when the app is stopped (the
    // Renderer then falls back to the app-level aggregate via `logs`).
    let Some(rt) = rt_to_read else {
        return Ok(Vec::new());
    };
    let lines = local_runtime.recent_logs(&app_scope, &rt, 0, limit);
    Ok(lines
        .into_iter()
        .map(|l| {
            serde_json::json!({
                "seq": l.seq,
                "tsMs": l.ts_ms,
                "stream": l.stream.as_str(),
                "text": l.text,
            })
        })
        .collect())
}

#[tauri::command]
pub async fn creative_app_install_local_dependencies(
    id: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    lock: State<'_, MutationLock>,
    local_runtime: State<'_, LocalRuntimeHandle>,
) -> Result<CreativeAppSummary> {
    let pool = state.db.clone();
    let logs = local_runtime.logs().clone_registry();
    let lock = lock.inner().clone();
    let _guard = lock.acquire_app(&id).await;
    let handle = app_handle.clone();
    tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Handle::current();
        let c = conn(&pool)?;
        let summary = rt.block_on(local::deps::install_dependencies(&c, &handle, &logs, &id))?;
        runtime_store::attach_identity(&c, summary)
    })
    .await
    .map_err(|e| Error::Internal(format!("install deps join: {e}")))?
}

#[tauri::command]
pub fn creative_app_preview_local_dependency_install(
    id: String,
    state: State<'_, AppState>,
) -> Result<serde_json::Value> {
    let c = conn(&state.db)?;
    let (program, args, pm) = local::deps::preview_install_command(&c, &id)?;
    Ok(serde_json::json!({
        "program": program,
        "args": args,
        "packageManager": pm,
        "requiresConfirmation": true,
        "display": format!("{} {}", program, args.join(" ")),
    }))
}

#[tauri::command]
pub fn creative_app_get_local_ai_settings(
    state: State<'_, AppState>,
) -> Result<local::ai::LocalAiSettings> {
    let c = conn(&state.db)?;
    local::ai::get_ai_settings(&c)
}

#[tauri::command]
pub fn creative_app_save_local_ai_settings(
    settings: local::ai::LocalAiSettings,
    state: State<'_, AppState>,
) -> Result<local::ai::LocalAiSettings> {
    let c = conn(&state.db)?;
    local::ai::save_ai_settings(&c, &settings)
}

#[tauri::command]
pub fn creative_app_preview_local_ai(
    project_root: String,
    state: State<'_, AppState>,
) -> Result<serde_json::Value> {
    let c = conn(&state.db)?;
    let (scan, preview, settings) = local::ai::preview_local_ai(&c, &project_root)?;
    Ok(serde_json::json!({
        "scan": scan,
        "payloadPreview": preview,
        "settings": settings,
        "requiresConfirmation": true,
    }))
}

#[tauri::command]
pub async fn creative_app_analyze_local_with_ai(
    project_root: String,
    confirmed: bool,
    state: State<'_, AppState>,
) -> Result<serde_json::Value> {
    let pool = state.db.clone();
    tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Handle::current();
        let c = conn(&pool)?;
        let (scan, plan, preview) =
            rt.block_on(local::ai::analyze_with_ai(&c, &project_root, confirmed))?;
        Ok(serde_json::json!({
            "scan": scan,
            "aiPlan": plan,
            "payloadPreview": preview,
        }))
    })
    .await
    .map_err(|e| Error::Internal(format!("analyze join: {e}")))?
}

#[tauri::command]
pub async fn creative_app_diagnose_local_with_ai(
    id: String,
    state: State<'_, AppState>,
    local_runtime: State<'_, LocalRuntimeHandle>,
) -> Result<local::ai::AiDiagnosisResult> {
    let pool = state.db.clone();
    let local_runtime = local_runtime.inner().clone();
    // App-level aggregate tail (legacy + per-runtime runs) for AI context.
    let tail = local_runtime.app_aggregate_tail(&id, 12_000);
    tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Handle::current();
        let c = conn(&pool)?;
        // Also apply any process exits observed while diagnosing.
        let _ = rt.block_on(local::lifecycle::poll_and_reconcile_exits(
            &c,
            None,
            local_runtime.as_ref(),
        ));
        rt.block_on(local::ai::diagnose_with_ai(&c, &id, &tail))
    })
    .await
    .map_err(|e| Error::Internal(format!("diagnose join: {e}")))?
}

#[tauri::command]
pub fn creative_app_get_local_config(
    id: String,
    state: State<'_, AppState>,
) -> Result<LocalCreativeConfig> {
    let c = conn(&state.db)?;
    let mut cfg = local::lifecycle::get_local_config(&c, &id)?;
    cfg.summary = runtime_store::attach_identity(&c, cfg.summary)?;
    Ok(cfg)
}

#[tauri::command]
pub async fn creative_app_poll_local_exits(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    local_runtime: State<'_, LocalRuntimeHandle>,
) -> Result<u32> {
    let pool = state.db.clone();
    let handle = app_handle.clone();
    let local_runtime = local_runtime.inner().clone();
    tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Handle::current();
        let c = conn(&pool)?;
        rt.block_on(local::lifecycle::poll_and_reconcile_exits(
            &c,
            Some(&handle),
            local_runtime.as_ref(),
        ))
    })
    .await
    .map_err(|e| Error::Internal(format!("poll exits join: {e}")))?
}

#[cfg(test)]
mod proposal_tests {
    use super::*;
    use crate::creative_app::model::OwnershipMode;
    use crate::creative_app::proposal::{AgentProposal, ProposalKind, ProposedDriver};

    fn static_proposal(root: &str) -> AgentProposal {
        AgentProposal {
            schema_version: 1,
            kind: ProposalKind::Create,
            ownership: OwnershipMode::Managed,
            title: "Approved App".into(),
            project_root: root.into(),
            driver: ProposedDriver::StaticHttp,
            open_path: "/".into(),
            health_path: "/".into(),
            environment_keys: vec!["PORT".into()],
        }
    }

    #[test]
    fn static_proposal_registers_local_app() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("proj");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("index.html"), "<html>hi</html>").unwrap();
        let root_s = root.to_string_lossy().to_string();

        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::create_tables(&conn).unwrap();
        crate::db::apply_migrations(&conn).unwrap();

        let proposal = static_proposal(&root_s);
        proposal.validate().unwrap();

        let summary = register_proposal_app(&mut conn, &proposal).unwrap();
        assert_eq!(summary.title, "Approved App");
        assert_eq!(summary.source, CreativeAppSource::LocalProject);
        assert_eq!(summary.runtime, CreativeAppRuntime::LocalStatic);
    }

    #[test]
    fn duplicate_proposal_path_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("proj");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("index.html"), "<html>hi</html>").unwrap();
        let root_s = root.to_string_lossy().to_string();

        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::create_tables(&conn).unwrap();
        crate::db::apply_migrations(&conn).unwrap();

        let proposal = static_proposal(&root_s);
        proposal.validate().unwrap();
        let _first = register_proposal_app(&mut conn, &proposal).unwrap();

        // Registering the same path again must fail.
        let err = register_proposal_app(&mut conn, &proposal).unwrap_err();
        assert!(err.to_string().contains("already registered"));
    }

    #[test]
    fn compose_proposal_registers_local_app() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("composeproj");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("compose.yml"),
            "services:\n  web:\n    image: nginx\n",
        )
        .unwrap();
        let root_s = root.to_string_lossy().to_string();

        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::create_tables(&conn).unwrap();
        crate::db::apply_migrations(&conn).unwrap();

        let proposal = AgentProposal {
            schema_version: 1,
            kind: ProposalKind::Start,
            ownership: OwnershipMode::Managed,
            title: "Compose".into(),
            project_root: root_s,
            driver: ProposedDriver::Compose {
                command: None,
                privileged: false,
            },
            open_path: "/".into(),
            health_path: "/".into(),
            environment_keys: vec![],
        };
        proposal.validate().unwrap();
        let summary = register_proposal_app(&mut conn, &proposal).unwrap();
        assert_eq!(summary.title, "Compose");
        assert_eq!(summary.source, CreativeAppSource::LocalProject);
    }

    #[test]
    fn compose_proposal_without_compose_file_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("nocompose");
        std::fs::create_dir_all(&root).unwrap();
        let root_s = root.to_string_lossy().to_string();

        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::create_tables(&conn).unwrap();
        crate::db::apply_migrations(&conn).unwrap();

        let proposal = AgentProposal {
            schema_version: 1,
            kind: ProposalKind::Start,
            ownership: OwnershipMode::Managed,
            title: "Compose".into(),
            project_root: root_s,
            driver: ProposedDriver::Compose {
                command: None,
                privileged: false,
            },
            open_path: "/".into(),
            health_path: "/".into(),
            environment_keys: vec![],
        };
        proposal.validate().unwrap();
        let err = register_proposal_app(&mut conn, &proposal).unwrap_err();
        assert!(err.to_string().contains("compose.yml"), "got: {err}");
    }

    #[test]
    fn python_proposal_registers_local_app() {
        use crate::creative_app::model::{LaunchPort, LaunchPortMode, PythonLaunchProfile};
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("pyproj");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("app.py"), "from flask import Flask\n").unwrap();
        let root_s = root.to_string_lossy().to_string();

        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::create_tables(&conn).unwrap();
        crate::db::apply_migrations(&conn).unwrap();

        let py = PythonLaunchProfile {
            schema_version: 1,
            interpreter: "/usr/bin/python3".into(),
            entry: "app.py".into(),
            args: vec![],
            cwd_relative: ".".into(),
            environment_keys: vec!["PORT".into()],
            port: LaunchPort {
                mode: LaunchPortMode::Auto,
                value: None,
            },
            open_path: "/".into(),
            health_path: "/".into(),
            startup_timeout_ms: 60_000,
            is_venv: false,
        };
        let proposal = AgentProposal {
            schema_version: 1,
            kind: ProposalKind::Create,
            ownership: OwnershipMode::Managed,
            title: "Python App".into(),
            project_root: root_s,
            driver: ProposedDriver::Python(py),
            open_path: "/".into(),
            health_path: "/".into(),
            environment_keys: vec!["PORT".into()],
        };
        proposal.validate().unwrap();
        let summary = register_proposal_app(&mut conn, &proposal).unwrap();
        assert_eq!(summary.title, "Python App");
        assert_eq!(summary.source, CreativeAppSource::LocalProject);
    }

    #[test]
    fn binary_proposal_registers_local_app() {
        use crate::creative_app::model::{BinaryLaunchProfile, LaunchPort, LaunchPortMode};
        use crate::creative_app::process_driver::sha256_hex;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("binproj");
        std::fs::create_dir_all(&root).unwrap();
        // A real executable file so the hash is computed from actual content.
        let bin_path = root.join("myapp");
        std::fs::write(&bin_path, "#!/bin/sh\necho hi\n").unwrap();
        let hash = sha256_hex(&bin_path).unwrap();
        let bin_s = bin_path.to_string_lossy().to_string();
        let root_s = root.to_string_lossy().to_string();

        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::create_tables(&conn).unwrap();
        crate::db::apply_migrations(&conn).unwrap();

        let b = BinaryLaunchProfile {
            schema_version: 1,
            executable_path: bin_s,
            executable_hash: hash,
            approved: true,
            args: vec![],
            cwd_relative: ".".into(),
            environment_keys: vec![],
            port: LaunchPort {
                mode: LaunchPortMode::Auto,
                value: None,
            },
            open_path: "/".into(),
            health_path: "/".into(),
            startup_timeout_ms: 60_000,
        };
        let proposal = AgentProposal {
            schema_version: 1,
            kind: ProposalKind::Create,
            ownership: OwnershipMode::Managed,
            title: "Binary App".into(),
            project_root: root_s,
            driver: ProposedDriver::Binary(b),
            open_path: "/".into(),
            health_path: "/".into(),
            environment_keys: vec![],
        };
        proposal.validate().unwrap();
        let summary = register_proposal_app(&mut conn, &proposal).unwrap();
        assert_eq!(summary.title, "Binary App");
        assert_eq!(summary.source, CreativeAppSource::LocalProject);
    }
}
