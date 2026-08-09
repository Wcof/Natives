use super::*;

#[tauri::command]
pub fn creative_app_browser_show(
    app_id: String,
    url: String,
    bounds: BrowserBounds,
    app_handle: tauri::AppHandle,
    browser: State<'_, BrowserStateHandle>,
    state: State<'_, AppState>,
) -> Result<WindowInstance> {
    // T07: preview opens go through the WindowController (single window
    // authority) — journal → WebView show → verify → Window/Preview commit.
    let mut c = conn(&state.db)?;
    let application_id = resolve_application_id(&c, &app_id)?;
    let surface_id = surface_store::find_main_surface(&c, &application_id)?
        .ok_or_else(|| Error::Internal(format!("no main surface for app {application_id}")))?;
    let gw = window::RealWebviewGateway::new(&app_handle, &browser);
    window::WindowController::open(
        &gw,
        &mut c,
        &app_id,
        &application_id,
        &surface_id,
        &url,
        bounds,
    )
}

#[tauri::command]
pub fn creative_app_browser_set_bounds(
    app_id: String,
    bounds: BrowserBounds,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<()> {
    let c = conn(&state.db)?;
    // A deleted app has no window rows to resize; ResizeObserver callbacks can
    // fire after delete, so a missing identity is a no-op, not an error.
    let Some(application_id) = resolve_application_id(&c, &app_id).ok() else {
        return Ok(());
    };
    for label in open_window_labels(&c, &application_id)? {
        browser::browser_set_bounds(&app_handle, &label, bounds.clone())?;
    }
    Ok(())
}

#[tauri::command]
pub fn creative_app_browser_back(
    app_id: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<()> {
    let c = conn(&state.db)?;
    let Some(application_id) = resolve_application_id(&c, &app_id).ok() else {
        return Ok(());
    };
    for label in open_window_labels(&c, &application_id)? {
        browser::browser_back(&app_handle, &label)?;
    }
    Ok(())
}

#[tauri::command]
pub fn creative_app_browser_forward(
    app_id: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<()> {
    let c = conn(&state.db)?;
    let Some(application_id) = resolve_application_id(&c, &app_id).ok() else {
        return Ok(());
    };
    for label in open_window_labels(&c, &application_id)? {
        browser::browser_forward(&app_handle, &label)?;
    }
    Ok(())
}

#[tauri::command]
pub fn creative_app_browser_reload(
    app_id: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<()> {
    let c = conn(&state.db)?;
    let Some(application_id) = resolve_application_id(&c, &app_id).ok() else {
        return Ok(());
    };
    for label in open_window_labels(&c, &application_id)? {
        browser::browser_reload(&app_handle, &label)?;
    }
    Ok(())
}

#[tauri::command]
pub fn creative_app_browser_hide(
    app_id: String,
    app_handle: tauri::AppHandle,
    browser: State<'_, BrowserStateHandle>,
    state: State<'_, AppState>,
) -> Result<()> {
    let mut c = conn(&state.db)?;
    let application_id = resolve_application_id(&c, &app_id)?;
    let gw = window::RealWebviewGateway::new(&app_handle, &browser);
    window::WindowController::minimize_app_windows(&gw, &mut c, &application_id)?;
    Ok(())
}

#[tauri::command]
pub fn creative_app_browser_close(
    app_id: String,
    app_handle: tauri::AppHandle,
    browser: State<'_, BrowserStateHandle>,
    state: State<'_, AppState>,
) -> Result<()> {
    // T07: close the app's real WebViews (missing → reconciled closed) and
    // commit the closed state + clear the preview bind. When the app identity
    // is already gone (deleted), its window rows are cascade-deleted with it
    // and any orphaned WebView is the reconcile sweep's job — nothing to close.
    let mut c = conn(&state.db)?;
    let Some(application_id) = resolve_application_id(&c, &app_id).ok() else {
        return Ok(());
    };
    let gw = window::RealWebviewGateway::new(&app_handle, &browser);
    window::WindowController::close_app_windows(&gw, &mut c, &application_id)?;
    Ok(())
}

#[tauri::command]
pub fn creative_app_browser_current(
    app_id: String,
    browser: State<'_, BrowserStateHandle>,
    state: State<'_, AppState>,
) -> Result<serde_json::Value> {
    let c = conn(&state.db)?;
    let Ok(application_id) = resolve_application_id(&c, &app_id) else {
        return Ok(serde_json::json!({ "appId": null, "url": null }));
    };
    for label in open_window_labels(&c, &application_id)? {
        let v = browser::browser_current(&browser, &label)?;
        if v.get("url").and_then(|u| u.as_str()).is_some() {
            return Ok(v);
        }
    }
    Ok(serde_json::json!({ "appId": null, "url": null }))
}

#[tauri::command]
pub fn creative_app_profile_list(state: State<'_, AppState>) -> Result<Vec<BrowserProfile>> {
    let c = conn(&state.db)?;
    profile_store::list_profiles(&c)
}

#[tauri::command]
pub fn creative_app_profile_create(
    name: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<BrowserProfile> {
    let c = conn(&state.db)?;
    let id = profile_store::create_profile(&c, name.trim(), false)?;
    emit_db_state_changed(
        &app_handle,
        "creative-profile",
        serde_json::json!({ "action": "create", "profileId": id }),
    );
    profile_store::find_profile(&c, &id)?
        .ok_or_else(|| Error::Internal("profile vanished after create".into()))
}

#[tauri::command]
pub fn creative_app_profile_delete(
    profile_id: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<()> {
    let c = conn(&state.db)?;
    profile_store::delete_profile(&c, &profile_id)?;
    emit_db_state_changed(
        &app_handle,
        "creative-profile",
        serde_json::json!({ "action": "delete", "profileId": profile_id }),
    );
    Ok(())
}

#[tauri::command]
pub fn creative_app_profile_bindings(state: State<'_, AppState>) -> Result<Vec<ProfileBinding>> {
    let c = conn(&state.db)?;
    profile_store::list_profile_bindings(&c)
}

#[tauri::command]
pub fn creative_app_profile_bind(
    app_id: String,
    profile_id: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<()> {
    let c = conn(&state.db)?;
    profile_store::bind_profile_to_app(&c, &app_id, &profile_id)?;
    emit_db_state_changed(
        &app_handle,
        "creative-profile",
        serde_json::json!({ "action": "bind", "appId": app_id, "profileId": profile_id }),
    );
    Ok(())
}

#[tauri::command]
pub fn creative_app_profile_unbind(
    app_id: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<()> {
    let c = conn(&state.db)?;
    profile_store::unbind_profile(&c, &app_id)?;
    emit_db_state_changed(
        &app_handle,
        "creative-profile",
        serde_json::json!({ "action": "unbind", "appId": app_id }),
    );
    Ok(())
}

#[tauri::command]
pub fn creative_app_grant_set(
    app_id: String,
    kind: String,
    policy: String,
    path: Option<String>,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<AppGrant> {
    let c = conn(&state.db)?;
    if ![
        AppGrant::KIND_UPLOAD,
        AppGrant::KIND_DOWNLOAD,
        AppGrant::KIND_CLIPBOARD,
        AppGrant::KIND_WINDOW_OPEN,
    ]
    .contains(&kind.as_str())
    {
        return Err(Error::InvalidInput(format!("unknown grant kind '{kind}'")));
    }
    if ![
        AppGrant::POLICY_DEFAULT_DENY,
        AppGrant::POLICY_ONE_TIME,
        AppGrant::POLICY_PERSISTENT,
    ]
    .contains(&policy.as_str())
    {
        return Err(Error::InvalidInput(format!(
            "unknown grant policy '{policy}'"
        )));
    }
    grant_store::set_grant(&c, &app_id, &kind, &policy, path.as_deref())?;
    emit_db_state_changed(
        &app_handle,
        "creative-grant",
        serde_json::json!({ "action": "set", "appId": app_id, "kind": kind }),
    );
    grant_store::get_grant(&c, &app_id, &kind)
}

#[tauri::command]
pub fn creative_app_grant_list(
    app_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<AppGrant>> {
    let c = conn(&state.db)?;
    grant_store::list_grants(&c, &app_id)
}

#[tauri::command]
pub fn creative_app_grant_delete(
    grant_id: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<()> {
    let c = conn(&state.db)?;
    grant_store::delete_grant(&c, &grant_id)?;
    emit_db_state_changed(
        &app_handle,
        "creative-grant",
        serde_json::json!({ "action": "delete", "grantId": grant_id }),
    );
    Ok(())
}

#[tauri::command]
pub fn creative_app_grant_events(
    app_id: String,
    limit: Option<i64>,
    state: State<'_, AppState>,
) -> Result<Vec<GrantEvent>> {
    let c = conn(&state.db)?;
    grant_store::list_grant_events(&c, &app_id, limit.unwrap_or(50).clamp(1, 200))
}

/// Upload: the user picks files via the OS dialog; only the user-chosen
/// absolute paths are exposed to the caller. Requires an `upload` grant
/// (one-time atomic or persistent); a grant path scope presets the dialog dir.
#[tauri::command]
pub async fn creative_app_upload_files(
    app_id: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<String>> {
    let scope = {
        let c = conn(&state.db)?;
        let outcome = grant_store::check_grant(&c, &app_id, AppGrant::KIND_UPLOAD, None)?;
        if !outcome.allowed() {
            return Err(Error::InvalidInput(
                "upload permission not granted for this app".into(),
            ));
        }
        grant_store::grant_scope(&c, &app_id, AppGrant::KIND_UPLOAD)?
    };
    let mut dialog = app_handle.dialog().file();
    if let Some(scope) = scope {
        dialog = dialog.set_directory(std::path::PathBuf::from(scope));
    }
    let (tx, rx) = tokio::sync::oneshot::channel();
    dialog.pick_files(move |files| {
        let paths = files
            .map(|list| {
                list.into_iter()
                    .filter_map(|p| p.into_path().ok())
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let _ = tx.send(paths);
    });
    let paths = rx
        .await
        .map_err(|e| Error::Internal(format!("file picker failed: {e}")))?;
    if paths.is_empty() {
        return Err(Error::Cancelled("upload cancelled by user".into()));
    }
    Ok(paths)
}

/// Clipboard read through the Host, gated by a `clipboard` grant. The child
/// webview has no Tauri capability; this is the only clipboard path.
#[tauri::command]
pub fn creative_app_clipboard_read(app_id: String, state: State<'_, AppState>) -> Result<String> {
    let c = conn(&state.db)?;
    let outcome = grant_store::check_grant(&c, &app_id, AppGrant::KIND_CLIPBOARD, None)?;
    if !outcome.allowed() {
        return Err(Error::InvalidInput(
            "clipboard permission not granted for this app".into(),
        ));
    }
    crate::commands::clipboard::clipboard_read()
}

/// Clipboard write through the Host, gated by a `clipboard` grant.
#[tauri::command]
pub fn creative_app_clipboard_write(
    app_id: String,
    text: String,
    state: State<'_, AppState>,
) -> Result<()> {
    let c = conn(&state.db)?;
    let outcome = grant_store::check_grant(&c, &app_id, AppGrant::KIND_CLIPBOARD, None)?;
    if !outcome.allowed() {
        return Err(Error::InvalidInput(
            "clipboard permission not granted for this app".into(),
        ));
    }
    crate::commands::clipboard::clipboard_write(text)
}

/// List the OAuth allowlist domains for an app.
#[tauri::command]
pub fn creative_app_oauth_domains(
    app_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<OAuthAllowlistEntry>> {
    let c = conn(&state.db)?;
    grant_store::list_oauth_domains(&c, &app_id)
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
