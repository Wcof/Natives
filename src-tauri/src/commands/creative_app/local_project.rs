use super::*;

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

pub(crate) fn create_local_app(
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

pub(crate) fn update_local_app(
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
        let _ = rt.block_on(local::lifecycle::poll_and_reconcile_exits::<tauri::Wry>(
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
