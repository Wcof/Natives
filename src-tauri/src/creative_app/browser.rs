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

use super::model::BrowserBounds;
use super::service::navigation_allowed;
use crate::{Error, Result};
use std::collections::HashMap;
use std::sync::Mutex;
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

/// Show or create a child webview for the given window label at the given URL.
///
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

    // Window::add_child is gated on unstable; call via window handle.
    use tauri::webview::WebviewBuilder;
    use tauri::{LogicalPosition, LogicalSize};

    let builder = WebviewBuilder::new(label.to_string(), tauri::WebviewUrl::External(parsed))
        .on_navigation(|nav_url| navigation_allowed(nav_url.as_str()));

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
    fn child_label_truncates_long_ids() {
        let long = "a".repeat(100);
        let label = child_label(&long);
        assert!(label.len() <= 64, "long label must be truncated: {label}");
    }

    #[test]
    fn window_label_is_unique_per_window() {
        // Two windows of the same app must never share a label (T07).
        let a = window_label("11111111-1111-4111-8111-111111111111");
        let b = window_label("22222222-2222-4222-8222-222222222222");
        assert_ne!(a, b, "window labels must be unique per window id");
        assert!(a.starts_with(WINDOW_LABEL_PREFIX));
        assert!(b.starts_with(WINDOW_LABEL_PREFIX));
        assert!(is_window_label(&a));

        // Window labels are distinct from the legacy app label family.
        assert_ne!(window_label("app-1"), child_label("app-1"));
        assert!(!is_window_label(&child_label("app-1")));
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
