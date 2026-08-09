use super::*;

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
        // T07: also sweep windows vs real child WebViews (missing → closed,
        // orphaned → closed) so a manual reconcile converges window truth too.
        let window_n = crate::creative_app::window::reconcile_all(&handle, &c)?;
        Ok(docker_n
            .saturating_add(local_n)
            .saturating_add(exits)
            .saturating_add(window_n))
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
