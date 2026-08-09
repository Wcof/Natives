//! Child WebView management for external creative apps (Embed surface).
//!
//! Uses Tauri `unstable` child webview when available. Capability isolation:
//! child is created without remote capability grants for the main app IPC.
//!
//! Window-id labels (T07): every window instance gets a unique label
//! `"creative-window-{windowId}"` so one app can host multiple child WebViews.
//! The legacy `child_label(app_id)` app-level label is kept for compatibility
//! tests only — production window lifecycle always uses the window-id label.
//! The navigation hook and absence of capability inheritance keep each
//! instance isolated from the main app's Tauri permissions.

use super::downloads;
use super::grant_store;
use super::model::AppGrant;
use super::model::BrowserBounds;
use super::oauth::{self, NewWindowDecision};
use super::profile_store;
use super::service::navigation_allowed;
use crate::{db, Error, Result};
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::webview::DownloadEvent;
use tauri::{AppHandle, Manager};

/// Legacy label prefix for app-scoped child WebViews.
const CHILD_LABEL_PREFIX: &str = "creative-app-";
/// Label prefix for window-instance child WebViews (T07). One label per window.
pub const WINDOW_LABEL_PREFIX: &str = "creative-window-";

/// Sanitize an id for use as a WebView label.
/// Labels must be non-empty, <= 64 chars, and contain only
/// alphanumeric, dash, underscore, and dot characters.
fn sanitize_label(id: &str) -> String {
    id.chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_' || *c == '.')
        .take(48)
        .collect()
}

/// Build the legacy app-level WebView label for a given app id.
pub fn child_label(app_id: &str) -> String {
    format!("{}{}", CHILD_LABEL_PREFIX, sanitize_label(app_id))
}

/// Build the WebView label for a window instance id. Unique per window — two
/// windows of the same app never share a label (T07 multi-window).
pub fn window_label(window_id: &str) -> String {
    format!("{}{}", WINDOW_LABEL_PREFIX, sanitize_label(window_id))
}

/// True when a label belongs to the window-instance label family.
pub fn is_window_label(label: &str) -> bool {
    label.starts_with(WINDOW_LABEL_PREFIX)
}

/// Label prefix for grant-approved popup webviews (window.open with a
/// `window_open` grant). Distinct from the Embed and OAuth namespaces and from
/// the `main` capability filter.
const POPUP_LABEL_PREFIX: &str = "creative-popup-";

/// Label prefix for non-owned (attached/remote) child WebViews (T09). The label
/// never matches the `main` webview capability filter, so a non-owned app never
/// receives Tauri Host capability.
pub const NON_OWNED_LABEL_PREFIX: &str = "creative-nonowned-";

/// Build a unique child-WebView label for a non-owned app record.
pub fn non_owned_label(id: &str) -> String {
    format!("{}{}", NON_OWNED_LABEL_PREFIX, sanitize_label(id))
}

/// Open a non-owned app in a child WebView restricted to its trust domain
/// (T09).
///
/// - Attached (`approved_origins` is `None`): loopback-only navigation, same as
///   managed Embed surfaces.
/// - Remote (`Some(origins)`): navigation restricted to the approved origins.
///   The child label never matches the `main` capability filter, so the remote
///   app never gets a Tauri capability. There is no download/grant/OAuth wiring
///   because a non-owned driver has no Host capability to grant.
pub fn browser_show_non_owned(
    app: &AppHandle,
    state: &BrowserStateHandle,
    label: &str,
    app_id: &str,
    url: &str,
    bounds: BrowserBounds,
    approved_origins: Option<&[String]>,
) -> Result<()> {
    let nav_fn: Box<dyn Fn(&str) -> bool + Send + Sync> = match approved_origins {
        Some(origins) => {
            if !crate::creative_app::non_owned::remote_navigation_allowed(url, origins) {
                return Err(Error::InvalidInput(
                    "navigation blocked: URL is not in the app's approved origins".into(),
                ));
            }
            let origins = origins.to_vec();
            Box::new(move |nav_url: &str| {
                crate::creative_app::non_owned::remote_navigation_allowed(nav_url, &origins)
            })
        }
        None => {
            if !navigation_allowed(url) {
                return Err(Error::InvalidInput(
                    "navigation blocked: only loopback http/https allowed".into(),
                ));
            }
            Box::new(navigation_allowed)
        }
    };

    let parsed: tauri::Url = url
        .parse()
        .map_err(|e| Error::InvalidInput(format!("url parse: {e}")))?;

    if let Some(wv) = app.get_webview(label) {
        wv.navigate(parsed)
            .map_err(|e| Error::Internal(format!("webview navigate: {e}")))?;
        set_bounds_webview(&wv, &bounds)?;
        wv.show()
            .map_err(|e| Error::Internal(format!("webview show: {e}")))?;
        set_active(state, label, app_id, url)?;
        return Ok(());
    }

    let main = app
        .get_webview_window("main")
        .ok_or_else(|| Error::Internal("main window not found".into()))?;
    use tauri::webview::WebviewBuilder;
    use tauri::{LogicalPosition, LogicalSize};
    let builder = WebviewBuilder::new(label, tauri::WebviewUrl::External(parsed))
        .on_navigation(move |nav_url| nav_fn(nav_url.as_str()));
    let window = main.as_ref().window();
    let _webview = window
        .add_child(
            builder,
            LogicalPosition::new(bounds.x, bounds.y),
            LogicalSize::new(bounds.width.max(1.0), bounds.height.max(1.0)),
        )
        .map_err(|e| Error::Internal(format!("add_child webview: {e}")))?;
    set_active(state, label, app_id, url)?;
    Ok(())
}
/// Open a grant-approved popup: a child webview in the main window with a
/// unique label, the app's profile data store (shared cookies), loopback-only
/// navigation, and no Tauri capability. Returns the new webview's label.
///
/// The default OS popup (`NewWindowResponse::Allow`) is never used — its
/// navigation would not be restricted. A granted window.open is re-homed into
/// this controlled child surface instead.
pub fn open_popup(app: &AppHandle, app_id: &str, url: &tauri::Url) -> Result<String> {
    if !navigation_allowed(url.as_str()) {
        return Err(Error::InvalidInput(
            "popup must target a loopback address".into(),
        ));
    }
    let main = app
        .get_webview_window("main")
        .ok_or_else(|| Error::Internal("main window not found".into()))?;
    let store_identifier = db::get_main_conn()
        .ok()
        .and_then(|conn| profile_store::profile_for_app(&conn, app_id).ok())
        .and_then(|profile| profile_store::data_store_identifier(&profile));

    let label = format!("{POPUP_LABEL_PREFIX}{}", uuid::Uuid::new_v4());
    let mut builder = tauri::webview::WebviewBuilder::new(
        label.clone(),
        tauri::WebviewUrl::External(url.clone()),
    )
    .on_navigation(|nav_url| navigation_allowed(nav_url.as_str()));
    if let Some(identifier) = store_identifier {
        builder = builder.data_store_identifier(identifier);
    }
    let window = main.as_ref().window();
    window
        .add_child(
            builder,
            tauri::LogicalPosition::new(160.0, 120.0),
            tauri::LogicalSize::new(560.0, 480.0),
        )
        .map_err(|e| Error::Internal(format!("popup add_child: {e}")))?;
    Ok(label)
}

#[derive(Default, Clone)]
pub struct ActiveEntry {
    pub app_id: String,
    pub url: String,
}

/// Per-app state tracked by the BrowserState.
#[derive(Default)]
pub struct BrowserState {
    pub entries: HashMap<String, ActiveEntry>,
}

impl BrowserState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the active entry for a given app id, if any.
    pub fn get_entry(&self, app_id: &str) -> Option<&ActiveEntry> {
        self.entries.get(app_id)
    }

    /// Set the active entry for a given app id.
    fn set_entry(&mut self, app_id: String, entry: ActiveEntry) {
        self.entries.insert(app_id, entry);
    }

    /// Remove the entry for a given app id.
    fn remove_entry(&mut self, app_id: &str) {
        self.entries.remove(app_id);
    }
}

pub type BrowserStateHandle = Mutex<BrowserState>;

/// If `url` targets an allowlisted OAuth domain, spawn a temp OAuth surface
/// for it and return `true` (the caller must cancel the original
/// navigation/window). Returns `false` for every other URL.
fn divert_oauth(
    app: &AppHandle,
    registry: &Option<std::sync::Arc<oauth::OAuthFlowRegistry>>,
    app_id: &str,
    url: &tauri::Url,
) -> bool {
    let Ok(conn) = db::get_main_conn() else {
        return false;
    };
    if !matches!(
        oauth::decide_new_window(&conn, app_id, url),
        Ok(NewWindowDecision::OAuthDivert)
    ) {
        return false;
    }
    let Some(registry) = registry else {
        return false;
    };
    match oauth::start_oauth_surface(app, &conn, registry.clone(), app_id, url.as_str()) {
        Ok(start) => {
            oauth::spawn_background_flow(app.clone(), registry.clone(), start);
            true
        }
        Err(_) => false,
    }
}

/// Show or create a child webview for the given window label at the given URL.
/// Window-id labels (T07): each window owns a unique `"creative-window-{id}"`
/// label, so one app can host multiple Embed surfaces. The navigation hook
/// restricts subsequent navigation to loopback addresses only.
pub fn browser_show(
    app: &AppHandle,
    state: &BrowserStateHandle,
    label: &str,
    app_id: &str,
    url: &str,
    bounds: BrowserBounds,
) -> Result<()> {
    if !navigation_allowed(url) {
        return Err(Error::InvalidInput(
            "navigation blocked: only loopback http/https allowed".into(),
        ));
    }
    super::service::validate_local_url(url)?;
    let parsed: tauri::Url = url
        .parse()
        .map_err(|e| Error::InvalidInput(format!("url parse: {e}")))?;

    // Prefer existing child webview reuse; propagate errors instead of swallowing them.
    if let Some(wv) = app.get_webview(label) {
        wv.navigate(parsed)
            .map_err(|e| Error::Internal(format!("webview navigate: {e}")))?;
        set_bounds_webview(&wv, &bounds)?;
        wv.show()
            .map_err(|e| Error::Internal(format!("webview show: {e}")))?;
        set_active(state, label, app_id, url)?;
        return Ok(());
    }

    let main = app
        .get_webview_window("main")
        .ok_or_else(|| Error::Internal("main window not found".into()))?;

    // T08: per-profile WebKit data store isolation (macOS 14+). Resolve the
    // app's profile and hand its identifier to the builder — cookies,
    // localStorage, IndexedDB and service workers are then profile-scoped.
    let store_identifier = db::get_main_conn()
        .ok()
        .and_then(|conn| profile_store::profile_for_app(&conn, app_id).ok())
        .and_then(|profile| profile_store::data_store_identifier(&profile));

    // T08: the OAuth flow registry (managed in lib.rs) powers `window.open`
    // divert to a temp OAuth surface. Absent in tests → new windows just deny.
    let oauth_registry = app
        .try_state::<std::sync::Arc<oauth::OAuthFlowRegistry>>()
        .map(|s| s.inner().clone());

    // Window::add_child is gated on unstable; call via window handle.
    use tauri::webview::WebviewBuilder;
    use tauri::{LogicalPosition, LogicalSize};

    let nav_app_handle = app.clone();
    let nav_app_id = app_id.to_string();
    let nav_registry = oauth_registry.clone();
    let mut builder = WebviewBuilder::new(label, tauri::WebviewUrl::External(parsed))
        .on_navigation(move |nav_url| {
            if navigation_allowed(nav_url.as_str()) {
                return true;
            }
            // An allowlisted OAuth domain navigated directly (full-page OAuth
            // redirect) is diverted to a temp surface; the Embed surface stays
            // on loopback. Everything else is blocked.
            divert_oauth(&nav_app_handle, &nav_registry, &nav_app_id, nav_url);
            false
        });
    if let Some(identifier) = store_identifier {
        builder = builder.data_store_identifier(identifier);
    }
    builder = builder.on_new_window({
        let app_id_owned = app_id.to_string();
        let app_handle = app.clone();
        let win_registry = oauth_registry.clone();
        move |url, _features| {
            let decision = db::get_main_conn()
                .ok()
                .and_then(|conn| oauth::decide_new_window(&conn, &app_id_owned, &url).ok());
            match decision {
                Some(NewWindowDecision::OAuthDivert) => {
                    // Divert to a controlled temp OAuth surface; the popup
                    // itself is denied so it can never outlive the flow.
                    divert_oauth(&app_handle, &win_registry, &app_id_owned, &url);
                    tauri::webview::NewWindowResponse::Deny
                }
                Some(NewWindowDecision::GrantAllow) => {
                    // Grant-approved loopback popup: re-home into a controlled
                    // child webview (loopback-only, shared profile store) and
                    // deny the default OS popup whose navigation would be
                    // unrestricted.
                    let _ = open_popup(&app_handle, &app_id_owned, &url);
                    tauri::webview::NewWindowResponse::Deny
                }
                _ => {
                    // Default deny — surface the denial so the Renderer can
                    // offer a grant. No window is created, no side effect.
                    crate::emit_db_state_changed(
                        &app_handle,
                        "creative-grant-requested",
                        serde_json::json!({
                            "appId": app_id_owned,
                            "kind": AppGrant::KIND_WINDOW_OPEN,
                            "target": url.to_string(),
                        }),
                    );
                    tauri::webview::NewWindowResponse::Deny
                }
            }
        }
    });
    builder = builder.on_download({
        let app_id_owned = app_id.to_string();
        let app_handle = app.clone();
        move |_webview, event| match event {
            DownloadEvent::Requested { url, destination } => {
                let Ok(conn) = db::get_main_conn() else {
                    return false;
                };
                // Resolve the Host-chosen destination first (grant scope dir or
                // the managed per-app dir) and a sanitized filename, then gate
                // it with the download grant against that directory.
                match downloads::safe_download_destination(&conn, &app_id_owned, &url, destination)
                {
                    Some(path) => {
                        let dir = path.parent().and_then(|p| p.to_str());
                        match grant_store::check_grant(
                            &conn,
                            &app_id_owned,
                            AppGrant::KIND_DOWNLOAD,
                            dir,
                        ) {
                            Ok(outcome) if outcome.allowed() => {
                                *destination = path;
                                true
                            }
                            _ => {
                                crate::emit_db_state_changed(
                                    &app_handle,
                                    "creative-grant-requested",
                                    serde_json::json!({
                                        "appId": app_id_owned,
                                        "kind": AppGrant::KIND_DOWNLOAD,
                                        "target": url.to_string(),
                                    }),
                                );
                                false
                            }
                        }
                    }
                    None => false,
                }
            }
            DownloadEvent::Finished { .. } => true,
            _ => true,
        }
    });

    let window = main.as_ref().window();
    let _webview = window
        .add_child(
            builder,
            LogicalPosition::new(bounds.x, bounds.y),
            LogicalSize::new(bounds.width.max(1.0), bounds.height.max(1.0)),
        )
        .map_err(|e| Error::Internal(format!("add_child webview: {e}")))?;

    set_active(state, label, app_id, url)?;
    Ok(())
}

/// Whether a child WebView with the given label currently exists.
pub fn browser_exists(app: &AppHandle, label: &str) -> bool {
    app.get_webview(label).is_some()
}

fn set_active(state: &BrowserStateHandle, label: &str, app_id: &str, url: &str) -> Result<()> {
    let mut st = state.lock().map_err(|e| Error::Internal(e.to_string()))?;
    st.set_entry(
        label.to_string(),
        ActiveEntry {
            app_id: app_id.to_string(),
            url: url.to_string(),
        },
    );
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

pub fn browser_set_bounds(app: &AppHandle, label: &str, bounds: BrowserBounds) -> Result<()> {
    if let Some(wv) = app.get_webview(label) {
        set_bounds_webview(&wv, &bounds)?;
    }
    Ok(())
}

pub fn browser_hide(app: &AppHandle, label: &str) -> Result<()> {
    if let Some(wv) = app.get_webview(label) {
        wv.hide()
            .map_err(|e| Error::Internal(format!("webview hide failed: {e}")))?;
    }
    Ok(())
}

pub fn browser_close(app: &AppHandle, state: &BrowserStateHandle, label: &str) -> Result<()> {
    // A close failure must stay observable: do NOT clear BrowserState if the
    // WebView could not actually be closed (P0: close failure swallowed).
    if let Some(wv) = app.get_webview(label) {
        wv.close()
            .map_err(|e| Error::Internal(format!("webview close failed: {e}")))?;
    }
    let mut st = state.lock().map_err(|e| Error::Internal(e.to_string()))?;
    st.remove_entry(label);
    Ok(())
}

pub fn browser_reload(app: &AppHandle, label: &str) -> Result<()> {
    if let Some(wv) = app.get_webview(label) {
        wv.reload()
            .map_err(|e| Error::Internal(format!("webview reload failed: {e}")))?;
    }
    Ok(())
}

pub fn browser_back(app: &AppHandle, label: &str) -> Result<()> {
    if let Some(wv) = app.get_webview(label) {
        wv.eval("window.history.back()")
            .map_err(|e| Error::Internal(format!("webview back failed: {e}")))?;
    }
    Ok(())
}

pub fn browser_forward(app: &AppHandle, label: &str) -> Result<()> {
    if let Some(wv) = app.get_webview(label) {
        wv.eval("window.history.forward()")
            .map_err(|e| Error::Internal(format!("webview forward failed: {e}")))?;
    }
    Ok(())
}

pub fn browser_current(state: &BrowserStateHandle, label: &str) -> Result<serde_json::Value> {
    let st = state.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let entry = st.get_entry(label);
    Ok(serde_json::json!({
        "appId": entry.map(|e| e.app_id.as_str()),
        "url": entry.map(|e| e.url.as_str()),
    }))
}

#[cfg(test)]
#[path = "browser_tests.rs"]
mod browser_tests;
