use super::*;

/// Register an attached local service: a loopback URL Natives did not start.
/// Natives records URL + ownership and can probe/open/delete the record only —
/// never start/stop (no fake lifecycle).
#[tauri::command]
pub async fn creative_app_attached_register(
    url: String,
    title: String,
    state: State<'_, AppState>,
) -> Result<crate::creative_app::model::NonOwnedAppSummary> {
    let pool = state.db.clone();
    tokio::task::spawn_blocking(move || {
        let c = conn(&pool)?;
        let app = crate::creative_app::non_owned::register(
            &c,
            crate::creative_app::model::OwnershipMode::Attached,
            &url,
            &[],
            &title,
        )?;
        Ok(crate::creative_app::model::NonOwnedAppSummary::project(app))
    })
    .await
    .map_err(|e| Error::Internal(format!("attached_register join: {e}")))?
}

/// Register a remote web app: an approved-origin URL. Natives records the
/// origins and can probe/open/delete the record only; the app never gets a
/// Tauri Host capability.
#[tauri::command]
pub async fn creative_app_remote_register(
    url: String,
    approved_origins: Vec<String>,
    title: String,
    state: State<'_, AppState>,
) -> Result<crate::creative_app::model::NonOwnedAppSummary> {
    let pool = state.db.clone();
    tokio::task::spawn_blocking(move || {
        let c = conn(&pool)?;
        let app = crate::creative_app::non_owned::register(
            &c,
            crate::creative_app::model::OwnershipMode::Remote,
            &url,
            &approved_origins,
            &title,
        )?;
        Ok(crate::creative_app::model::NonOwnedAppSummary::project(app))
    })
    .await
    .map_err(|e| Error::Internal(format!("remote_register join: {e}")))?
}

/// List all non-owned apps (attached + remote) with their honest action matrix.
#[tauri::command]
pub async fn creative_app_non_owned_list(
    state: State<'_, AppState>,
) -> Result<Vec<crate::creative_app::model::NonOwnedAppSummary>> {
    let pool = state.db.clone();
    tokio::task::spawn_blocking(move || {
        let c = conn(&pool)?;
        let apps = crate::creative_app::non_owned::list(&c)?;
        Ok(apps
            .into_iter()
            .map(crate::creative_app::model::NonOwnedAppSummary::project)
            .collect())
    })
    .await
    .map_err(|e| Error::Internal(format!("non_owned_list join: {e}")))?
}

/// Probe a non-owned app: is its origin currently reachable? Attached/Remote
/// probe is a read-only reachability check — never a lifecycle mutation.
#[tauri::command]
pub async fn creative_app_non_owned_probe(
    id: String,
    state: State<'_, AppState>,
) -> Result<crate::creative_app::model::NonOwnedProbe> {
    let pool = state.db.clone();
    tokio::task::spawn_blocking(move || {
        let c = conn(&pool)?;
        crate::creative_app::non_owned::probe_record(&c, &id)
    })
    .await
    .map_err(|e| Error::Internal(format!("non_owned_probe join: {e}")))?
}

/// Open a non-owned app in a child WebView restricted to its trust domain.
/// Attached → loopback only; Remote → approved origins only. The child label
/// never matches the `main` capability filter, so the app gets no Host
/// capability.
#[tauri::command]
pub fn creative_app_non_owned_open(
    id: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    browser: State<'_, BrowserStateHandle>,
) -> Result<()> {
    let c = conn(&state.db)?;
    let app =
        crate::creative_app::non_owned::get(&c, &id)?.ok_or_else(|| Error::NotFound(id.clone()))?;
    let url = crate::creative_app::non_owned::open_url_for(&app)?;
    let label = browser::non_owned_label(&app.id);
    let approved = match app.ownership {
        crate::creative_app::model::OwnershipMode::Remote => Some(app.approved_origins.as_slice()),
        _ => None,
    };
    let bounds = crate::creative_app::model::BrowserBounds {
        x: 160.0,
        y: 120.0,
        width: 960.0,
        height: 720.0,
    };
    browser::browser_show_non_owned(
        &app_handle,
        &browser,
        &label,
        &app.id,
        &url,
        bounds,
        approved,
    )
}

/// Delete a non-owned app record. This never stops or kills the external
/// service — the record is the only thing Natives owns.
#[tauri::command]
pub async fn creative_app_non_owned_delete(
    id: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
    browser: State<'_, BrowserStateHandle>,
) -> Result<()> {
    // Close any live child WebView bound to this record, then drop the record.
    let label = browser::non_owned_label(&id);
    let _ = browser::browser_close(&app_handle, &browser, &label);
    let pool = state.db.clone();
    tokio::task::spawn_blocking(move || {
        let c = conn(&pool)?;
        crate::creative_app::non_owned::delete(&c, &id)
    })
    .await
    .map_err(|e| Error::Internal(format!("non_owned_delete join: {e}")))?
}
