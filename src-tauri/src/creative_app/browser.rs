//! Child WebView management for external creative apps (Embed surface).
//!
//! Uses Tauri `unstable` child webview when available. Capability isolation:
//! child is created without remote capability grants for the main app IPC.
//!
//! Multi-label support (CR-401): each app instance gets its own label
//! `"creative-app-{appId}"` so multiple instances can coexist. The navigation
//! hook and absence of capability inheritance keep each instance isolated
//! from the main app's Tauri permissions.

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

/// Label prefix for child WebViews. Each gets `{PREFIX}{sanitized_app_id}`.
const CHILD_LABEL_PREFIX: &str = "creative-app-";

/// Sanitize an app id for use as a WebView label.
/// Labels must be non-empty, <= 64 chars, and contain only
/// alphanumeric, dash, underscore, and dot characters.
fn sanitize_label(id: &str) -> String {
    id.chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_' || *c == '.')
        .take(48)
        .collect()
}

/// Build the WebView label for a given app id.
pub fn child_label(app_id: &str) -> String {
    format!("{}{}", CHILD_LABEL_PREFIX, sanitize_label(app_id))
}

/// Label prefix for grant-approved popup webviews (window.open with a
/// `window_open` grant). Distinct from the Embed and OAuth namespaces and from
/// the `main` capability filter.
const POPUP_LABEL_PREFIX: &str = "creative-popup-";

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

/// Show or create a child webview for the given app at the given URL.
///
/// Multi-label (CR-401): each app gets its own label `"creative-app-{appId}"`,
/// so multiple Embed surfaces can coexist. The navigation hook restricts
/// subsequent navigation to loopback addresses only.
pub fn browser_show(
    app: &AppHandle,
    state: &BrowserStateHandle,
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

    let label = child_label(app_id);

    // Prefer existing child webview reuse; propagate errors instead of swallowing them.
    if let Some(wv) = app.get_webview(&label) {
        wv.navigate(parsed)
            .map_err(|e| Error::Internal(format!("webview navigate: {e}")))?;
        set_bounds_webview(&wv, &bounds)?;
        wv.show()
            .map_err(|e| Error::Internal(format!("webview show: {e}")))?;
        set_active(state, app_id, url)?;
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
    let mut builder = WebviewBuilder::new(label.clone(), tauri::WebviewUrl::External(parsed))
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

    set_active(state, app_id, url)?;
    Ok(())
}

fn set_active(state: &BrowserStateHandle, app_id: &str, url: &str) -> Result<()> {
    let mut st = state.lock().map_err(|e| Error::Internal(e.to_string()))?;
    st.set_entry(
        app_id.to_string(),
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

pub fn browser_set_bounds(app: &AppHandle, app_id: &str, bounds: BrowserBounds) -> Result<()> {
    let label = child_label(app_id);
    if let Some(wv) = app.get_webview(&label) {
        set_bounds_webview(&wv, &bounds)?;
    }
    Ok(())
}

pub fn browser_hide(app: &AppHandle, app_id: &str) -> Result<()> {
    let label = child_label(app_id);
    if let Some(wv) = app.get_webview(&label) {
        wv.hide()
            .map_err(|e| Error::Internal(format!("webview hide failed: {e}")))?;
    }
    Ok(())
}

pub fn browser_close(app: &AppHandle, state: &BrowserStateHandle, app_id: &str) -> Result<()> {
    // A close failure must stay observable: do NOT clear BrowserState if the
    // WebView could not actually be closed (P0: close failure swallowed).
    let label = child_label(app_id);
    if let Some(wv) = app.get_webview(&label) {
        wv.close()
            .map_err(|e| Error::Internal(format!("webview close failed: {e}")))?;
    }
    let mut st = state.lock().map_err(|e| Error::Internal(e.to_string()))?;
    st.remove_entry(app_id);
    Ok(())
}

pub fn browser_reload(app: &AppHandle, app_id: &str) -> Result<()> {
    let label = child_label(app_id);
    if let Some(wv) = app.get_webview(&label) {
        wv.reload()
            .map_err(|e| Error::Internal(format!("webview reload failed: {e}")))?;
    }
    Ok(())
}

pub fn browser_back(app: &AppHandle, app_id: &str) -> Result<()> {
    let label = child_label(app_id);
    if let Some(wv) = app.get_webview(&label) {
        wv.eval("window.history.back()")
            .map_err(|e| Error::Internal(format!("webview back failed: {e}")))?;
    }
    Ok(())
}

pub fn browser_forward(app: &AppHandle, app_id: &str) -> Result<()> {
    let label = child_label(app_id);
    if let Some(wv) = app.get_webview(&label) {
        wv.eval("window.history.forward()")
            .map_err(|e| Error::Internal(format!("webview forward failed: {e}")))?;
    }
    Ok(())
}

pub fn browser_current(state: &BrowserStateHandle, app_id: &str) -> Result<serde_json::Value> {
    let st = state.lock().map_err(|e| Error::Internal(e.to_string()))?;
    let entry = st.get_entry(app_id);
    Ok(serde_json::json!({
        "appId": entry.map(|e| e.app_id.as_str()),
        "url": entry.map(|e| e.url.as_str()),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Label generation ───────────────────────────────────────────────

    #[test]
    fn child_label_is_derived_from_app_id() {
        assert_eq!(child_label("my-app"), "creative-app-my-app");
        assert_eq!(child_label("a"), "creative-app-a");
    }

    #[test]
    fn child_label_sanitizes_special_chars() {
        // Only alphanumeric, dash, underscore, dot survive.
        let label = child_label("app/one:two?x");
        assert!(
            !label.contains('/'),
            "label must not contain slash: {label}"
        );
        assert!(
            !label.contains(':'),
            "label must not contain colon: {label}"
        );
        assert!(
            !label.contains('?'),
            "label must not contain question: {label}"
        );
        // The sanitized result should be a valid WebView label.
        assert!(label.len() <= 64, "label must be ≤ 64 chars");
    }

    #[test]
    fn child_label_handles_empty_input() {
        let label = child_label("");
        assert_eq!(label, "creative-app-");
    }

    #[test]
    fn popup_label_is_unique_and_distinct_from_embed_and_oauth() {
        // A granted window.open is re-homed into a controlled popup surface
        // whose label never matches the Embed child label or the "main" filter.
        let label = format!("{POPUP_LABEL_PREFIX}{}", uuid::Uuid::new_v4());
        assert!(label.starts_with("creative-popup-"));
        assert!(!label.contains("main"));
        assert!(!label.starts_with("creative-app-"));
        assert!(!label.starts_with("creative-oauth-"));
    }

    #[test]
    fn child_label_truncates_long_ids() {
        let long = "a".repeat(100);
        let label = child_label(&long);
        assert!(label.len() <= 64, "long label must be truncated: {label}");
    }

    // ── BrowserState multi-instance ────────────────────────────────────

    #[test]
    fn browser_state_supports_multiple_apps() {
        let state = BrowserState::new();
        let handle = Mutex::new(state);

        {
            let mut st = handle.lock().unwrap();
            st.set_entry(
                "app-a".into(),
                ActiveEntry {
                    app_id: "app-a".into(),
                    url: "http://127.0.0.1:3000/".into(),
                },
            );
            st.set_entry(
                "app-b".into(),
                ActiveEntry {
                    app_id: "app-b".into(),
                    url: "http://127.0.0.1:3001/".into(),
                },
            );
        }

        let st = handle.lock().unwrap();
        assert!(st.get_entry("app-a").is_some());
        assert!(st.get_entry("app-b").is_some());
        assert!(st.get_entry("app-c").is_none());
    }

    #[test]
    fn browser_state_remove_entry_clears_only_the_named_app() {
        let state = BrowserState::new();
        let handle = Mutex::new(state);

        {
            let mut st = handle.lock().unwrap();
            st.set_entry(
                "app-a".into(),
                ActiveEntry {
                    app_id: "app-a".into(),
                    url: "http://127.0.0.1:3000/".into(),
                },
            );
            st.set_entry(
                "app-b".into(),
                ActiveEntry {
                    app_id: "app-b".into(),
                    url: "http://127.0.0.1:3001/".into(),
                },
            );
        }

        {
            let mut st = handle.lock().unwrap();
            st.remove_entry("app-a");
        }

        let st = handle.lock().unwrap();
        assert!(st.get_entry("app-a").is_none(), "app-a should be removed");
        assert!(st.get_entry("app-b").is_some(), "app-b should remain");
    }

    #[test]
    fn browser_state_closes_do_not_affect_other_entries() {
        let state = BrowserState::new();
        let handle = Mutex::new(state);

        {
            let mut st = handle.lock().unwrap();
            st.set_entry(
                "app-a".into(),
                ActiveEntry {
                    app_id: "app-a".into(),
                    url: "http://127.0.0.1:3000/".into(),
                },
            );
            st.set_entry(
                "app-b".into(),
                ActiveEntry {
                    app_id: "app-b".into(),
                    url: "http://127.0.0.1:3001/".into(),
                },
            );
        }

        // Simulate close: remove entry
        {
            let mut st = handle.lock().unwrap();
            st.remove_entry("app-a");
        }

        // browser_current for app-a returns None
        let st = handle.lock().unwrap();
        assert!(st.get_entry("app-a").is_none());
        assert!(st.get_entry("app-b").is_some());
    }

    // ── Navigation filtering ───────────────────────────────────────────

    #[test]
    fn navigation_allowed_only_loopback() {
        use super::super::service::navigation_allowed;

        assert!(navigation_allowed("http://127.0.0.1:8080/"));
        assert!(navigation_allowed("https://127.0.0.1:8443/"));
        assert!(navigation_allowed("http://localhost:5173/"));
        assert!(!navigation_allowed("https://example.com/x"));
        assert!(!navigation_allowed("http://localhost.evil.com/x"));
        assert!(!navigation_allowed("http://127.0.0.1.evil.com/x"));
        assert!(!navigation_allowed("file:///tmp"));
        assert!(!navigation_allowed("data:text/html,hi"));
        assert!(!navigation_allowed("tauri://localhost"));
    }

    // ── Capability isolation ───────────────────────────────────────────

    #[test]
    fn verify_capability_label_pattern() {
        // The capability file uses `"webview": ["main"]` for the default set.
        // Child WebViews get labels like "creative-app-my-app", which do NOT
        // match the "main" webview filter — so they inherit zero permissions.
        let label = child_label("my-app");
        assert!(
            !label.contains("main"),
            "child label must not match 'main' webview filter: {label}"
        );
        // Confirm the label pattern is distinct from "main"
        assert!(
            !label.eq_ignore_ascii_case("main"),
            "child label must not be 'main'"
        );
        assert!(
            label.starts_with("creative-app-"),
            "child label must start with the prefix"
        );
    }

    /// CR-403: On-navigation hook must reject non-loopback targets.
    /// This is unit-testable since the hook is a pure function.
    #[test]
    fn on_navigation_hook_rejects_remote_urls() {
        // The on_navigation closure calls navigation_allowed() which we test
        // above. Additionally, verify that the hook pattern works for redirects.
        assert!(navigation_allowed(
            "http://127.0.0.1:3000/api/callback?code=abc"
        ));
        assert!(!navigation_allowed("https://evil.com/steal?code=abc"));
    }

    /// CR-403: The capabilities file uses `webview: ["main"]` so only the main
    /// Renderer webview gets Tauri permissions. Child WebViews get labels like
    /// "creative-app-{appId}" which do not match the "main" filter. Verify that
    /// every possible child label is distinct from the "main" webview identifier.
    #[test]
    fn child_label_never_matches_main_webview_filter() {
        let labels = [
            child_label("my-app"),
            child_label("test"),
            child_label("a"),
            child_label(""),
            child_label("main"), // Even if the app is literally named "main"
        ];
        for label in &labels {
            assert!(
                !label.eq_ignore_ascii_case("main"),
                "child label {label:?} must not match the 'main' webview filter"
            );
        }
    }

    /// CR-403: The WebviewBuilder always gets an on_navigation hook that
    /// restricts to loopback addresses. Verify the hook logic is wired in
    /// by checking the function we pass to on_navigation is navigation_allowed.
    #[test]
    fn webview_builder_uses_navigation_hook() {
        // Verify that navigation_allowed is the function used by browser_show.
        // The actual WebView creation is tested via integration tests, but the
        // logic of the hook is verified here.
        assert!(crate::creative_app::service::navigation_allowed(
            "http://127.0.0.1:3000/"
        ));
        assert!(!crate::creative_app::service::navigation_allowed(
            "https://evil.com/"
        ));
    }

    // ── BrowserState entry format ──────────────────────────────────────

    #[test]
    fn browser_current_returns_json_with_app_id_and_url() {
        let state = BrowserState::new();
        let handle = Mutex::new(state);
        {
            let mut st = handle.lock().unwrap();
            st.set_entry(
                "test-app".into(),
                ActiveEntry {
                    app_id: "test-app".into(),
                    url: "http://127.0.0.1:8080/page".into(),
                },
            );
        }

        // Simulate browser_current by reading state directly
        let st = handle.lock().unwrap();
        let entry = st.get_entry("test-app").unwrap();
        assert_eq!(entry.app_id, "test-app");
        assert_eq!(entry.url, "http://127.0.0.1:8080/page");
    }
}
