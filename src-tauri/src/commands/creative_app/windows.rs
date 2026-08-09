use super::*;

/// List all windows for the given application (CR-501).
#[tauri::command]
pub fn creative_app_window_list(
    application_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<WindowInstance>> {
    let c = conn(&state.db)?;
    surface_store::list_windows(&c, &application_id)
}

/// Open a window for the given application surface (CR-501, T07).
/// Requires the app to be running; drives the real WebView and commits the
/// Window / Preview bind through the WindowController.
#[tauri::command]
pub fn creative_app_window_open(
    application_id: String,
    surface_id: String,
    url: String,
    bounds: BrowserBounds,
    app_handle: tauri::AppHandle,
    browser: State<'_, BrowserStateHandle>,
    state: State<'_, AppState>,
) -> Result<WindowInstance> {
    let mut c = conn(&state.db)?;
    let app_id = runtime_store::source_id_for_application(&c, &application_id)?
        .ok_or_else(|| Error::InvalidInput("application has no source row".into()))?;
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

/// Close a window (CR-501, T07): close the real WebView then commit closed.
#[tauri::command]
pub fn creative_app_window_close(
    window_id: String,
    app_handle: tauri::AppHandle,
    browser: State<'_, BrowserStateHandle>,
    state: State<'_, AppState>,
) -> Result<()> {
    let mut c = conn(&state.db)?;
    let gw = window::RealWebviewGateway::new(&app_handle, &browser);
    window::WindowController::close(&gw, &mut c, &window_id)
}

/// Minimize a window (CR-501, T07): hide the real WebView then commit minimized.
#[tauri::command]
pub fn creative_app_window_minimize(
    window_id: String,
    app_handle: tauri::AppHandle,
    browser: State<'_, BrowserStateHandle>,
    state: State<'_, AppState>,
) -> Result<()> {
    let mut c = conn(&state.db)?;
    let gw = window::RealWebviewGateway::new(&app_handle, &browser);
    window::WindowController::minimize(&gw, &mut c, &window_id)
}

/// Restore a window (CR-501, T07): re-show the real WebView then commit open.
#[tauri::command]
pub fn creative_app_window_restore(
    window_id: String,
    app_handle: tauri::AppHandle,
    browser: State<'_, BrowserStateHandle>,
    state: State<'_, AppState>,
) -> Result<()> {
    let mut c = conn(&state.db)?;
    let gw = window::RealWebviewGateway::new(&app_handle, &browser);
    window::WindowController::restore(&gw, &mut c, &window_id, None)
}
