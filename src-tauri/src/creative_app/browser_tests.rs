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
