use super::*;

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
    browser: State<'_, BrowserStateHandle>,
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
            let mut c = conn(&pool)?;
            // T07: a stopped runtime has no live preview — close its windows so
            // the UI never shows dead content (offline policy). A window-close
            // failure must not hide the successful stop; the next reconcile
            // sweep closes any leftover orphaned WebView.
            if let Ok(application_id) = resolve_application_id(&c, &id) {
                let gw = window::RealWebviewGateway::new(&ctx.app, &browser);
                if let Err(e) =
                    window::WindowController::close_app_windows(&gw, &mut c, &application_id)
                {
                    eprintln!("warning: close windows after stop failed: {e}");
                }
            }
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
) -> Result<DeleteMutationResult> {
    let pool = state.db.clone();
    let host_port = host_http_port(&state);
    let local_runtime = local_runtime.inner().clone();
    let lock = lock.inner().clone();
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
            let mut c = conn(&pool_inner)?;
            journal(
                &app,
                &c,
                op_id,
                &[op::PHASE_WAITING, op::PHASE_PENDING],
                op::PHASE_RUNNING,
            )?;
            // T07: prove every child WebView is closed before the app references
            // are removed. A close failure keeps the delete failed so resources
            // are never silently orphaned. The app identity is resolved from the
            // still-present source row; window rows cascade-delete with it.
            if let Ok(application_id) = resolve_application_id(&c, &id_inner) {
                // Managed in setup; try_state avoids a panic if it is ever absent.
                let browser_state = app
                    .try_state::<BrowserStateHandle>()
                    .ok_or_else(|| Error::Internal("browser state not managed".into()))?;
                let gw = window::RealWebviewGateway::new(&app, browser_state.inner());
                window::WindowController::close_app_windows(&gw, &mut c, &application_id)?;
            }
            rt.block_on(adapters::facade::delete(
                &c, &ctx_inner, &id_inner, opts_inner,
            ))
        })
        .await
        .map_err(|e| Error::Internal(format!("delete join: {e}")))?
    };

    match result {
        Ok(delete_result) => {
            // On success the application row is gone; the operation survives with
            // application_id NULL via the FK (audit trail). Window rows are
            // cascade-deleted with the application; any orphaned WebView is
            // closed by the reconcile sweep on the next Host start.
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
