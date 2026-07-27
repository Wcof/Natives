//! Single reusable child WebView for external creative apps (Embed surface).
//!
//! Uses Tauri `unstable` child webview when available. Capability isolation:
//! child is created without remote capability grants for the main app IPC.

use super::model::BrowserBounds;
use super::service::navigation_allowed;
use crate::{Error, Result};
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

const CHILD_LABEL: &str = "creative-app-browser";

#[derive(Default)]
pub struct BrowserState {
    pub active_app_id: Option<String>,
    pub current_url: Option<String>,
}

impl BrowserState {
    pub fn new() -> Self {
        Self::default()
    }
}

pub type BrowserStateHandle = Mutex<BrowserState>;

/// Show or reuse the child webview at bounds with a validated local URL.
pub fn browser_show(
    app: &AppHandle,
    state: &BrowserStateHandle,
    app_id: &str,
    url: &str,
    bounds: BrowserBounds,
) -> Result<()> {
    if !navigation_allowed(url) {
        return Err(Error::InvalidInput(
            "navigation blocked: only http/https allowed".into(),
        ));
    }
    super::service::validate_local_url(url)?;

    {
        let mut st = state.lock().map_err(|e| Error::Internal(e.to_string()))?;
        st.active_app_id = Some(app_id.to_string());
        st.current_url = Some(url.to_string());
    }

    // Prefer existing child webview reuse
    if let Some(wv) = app.get_webview(CHILD_LABEL) {
        if let Ok(parsed) = url.parse() {
            let _ = wv.navigate(parsed);
        }
        let _ = set_bounds_webview(&wv, &bounds);
        let _ = wv.show();
        return Ok(());
    }

    let main = app
        .get_webview_window("main")
        .ok_or_else(|| Error::Internal("main window not found".into()))?;

    // Window::add_child is gated on unstable; call via window handle.
    use tauri::webview::WebviewBuilder;
    use tauri::{LogicalPosition, LogicalSize};

    let parsed: tauri::Url = url
        .parse()
        .map_err(|e| Error::InvalidInput(format!("url parse: {e}")))?;

    let builder = WebviewBuilder::new(CHILD_LABEL, tauri::WebviewUrl::External(parsed))
        .on_navigation(|nav_url| navigation_allowed(nav_url.as_str()));

    let window = main.as_ref().window();
    let _webview = window
        .add_child(
            builder,
            LogicalPosition::new(bounds.x, bounds.y),
            LogicalSize::new(bounds.width.max(1.0), bounds.height.max(1.0)),
        )
        .map_err(|e| Error::Internal(format!("add_child webview: {e}")))?;

    Ok(())
}

fn set_bounds_webview(wv: &tauri::Webview, bounds: &BrowserBounds) -> Result<()> {
    use tauri::{LogicalPosition, LogicalSize, Position, Size};
    wv.set_position(Position::Logical(LogicalPosition::new(bounds.x, bounds.y)))
        .map_err(|e| Error::Internal(format!("set_position: {e}")))?;
    wv.set_size(Size::Logical(LogicalSize::new(
        bounds.width.max(1.0),
        bounds.height.max(1.0),
    )))
    .map_err(|e| Error::Internal(format!("set_size: {e}")))?;
    Ok(())
}

pub fn browser_set_bounds(app: &AppHandle, bounds: BrowserBounds) -> Result<()> {
    if let Some(wv) = app.get_webview(CHILD_LABEL) {
        set_bounds_webview(&wv, &bounds)?;
    }
    Ok(())
}

pub fn browser_hide(app: &AppHandle) -> Result<()> {
    if let Some(wv) = app.get_webview(CHILD_LABEL) {
        let _ = wv.hide();
    }
    Ok(())
}

pub fn browser_close(app: &AppHandle, state: &BrowserStateHandle) -> Result<()> {
    if let Some(wv) = app.get_webview(CHILD_LABEL) {
        let _ = wv.close();
    }
    if let Ok(mut st) = state.lock() {
        st.active_app_id = None;
        st.current_url = None;
    }
    Ok(())
}

pub fn browser_reload(app: &AppHandle) -> Result<()> {
    if let Some(wv) = app.get_webview(CHILD_LABEL) {
        let _ = wv.reload();
    }
    Ok(())
}

pub fn browser_back(app: &AppHandle) -> Result<()> {
    if let Some(wv) = app.get_webview(CHILD_LABEL) {
        let _ = wv.eval("window.history.back()");
    }
    Ok(())
}

pub fn browser_forward(app: &AppHandle) -> Result<()> {
    if let Some(wv) = app.get_webview(CHILD_LABEL) {
        let _ = wv.eval("window.history.forward()");
    }
    Ok(())
}

pub fn browser_current(state: &BrowserStateHandle) -> Result<serde_json::Value> {
    let st = state.lock().map_err(|e| Error::Internal(e.to_string()))?;
    Ok(serde_json::json!({
        "appId": st.active_app_id,
        "url": st.current_url,
    }))
}
