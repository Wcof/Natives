//! OAuth authorization surface (T08).
//!
//! Two distinct trust domains for child webviews:
//!
//! - **Embed surface** (`creative-app-*`): loopback-only navigation (see
//!   `service::navigation_allowed`). It must never leave 127.0.0.1/localhost.
//! - **OAuth temp surface** (`creative-oauth-*`): a short-lived incognito
//!   child webview whose navigation is restricted to the app's OAuth
//!   allowlist (e.g. `accounts.google.com`) plus the loopback callback.
//!   Completing or cancelling closes and cleans up the surface.
//!
//! `window.open` from the Embed surface is denied unless the target is an
//! allowlisted OAuth domain (diverted to a temp surface) or a loopback URL
//! with an explicit `window_open` grant. The Embed surface never receives
//! Workshop Bridge / Tauri capability.

use super::grant_store;
use super::model::AppGrant;
use crate::{db, Error, Result};
use rusqlite::Connection;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};
use uuid::Uuid;

/// Label prefix for temporary OAuth surfaces. Distinct from the Embed
/// `creative-app-` namespace (T07) and never matches the `main` capability.
pub const OAUTH_LABEL_PREFIX: &str = "creative-oauth-";
const OAUTH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(180);

/// Default size for the temporary OAuth surface.
const OAUTH_WIDTH: f64 = 460.0;
const OAUTH_HEIGHT: f64 = 620.0;

// ── Pure policy (unit-testable, no AppHandle) ────────────────────────

/// Whether a host is in the app's OAuth allowlist (exact host match).
pub fn oauth_host_allowed(conn: &Connection, application_id: &str, host: &str) -> Result<bool> {
    grant_store::is_oauth_domain_allowed(conn, application_id, host)
}

/// Navigation policy for the OAuth temp surface: loopback (callback) or an
/// allowlisted OAuth domain. Everything else — evil redirects, file:, data:,
/// other schemes — is blocked.
pub fn oauth_navigation_allowed(conn: &Connection, application_id: &str, url: &tauri::Url) -> bool {
    if super::service::validate_local_url(url.as_str()).is_ok() {
        return true;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    oauth_host_allowed(conn, application_id, host).unwrap_or(false)
}

/// Decision for a `window.open` request from the Embed surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NewWindowDecision {
    /// The target is an allowlisted OAuth domain — divert to a temp surface.
    OAuthDivert,
    /// Loopback target and the app holds a `window_open` grant.
    GrantAllow,
    /// Everything else: deny.
    Deny,
}

/// Decide what to do with a `window.open` URL.
///
/// - Loopback + `window_open` grant → `GrantAllow` (atomic one-time consume
///   or persistent). The caller re-homes the popup into a controlled child
///   webview (loopback-only); the default OS popup is never allowed because
///   its navigation would not be restricted.
/// - Allowlisted OAuth host → `OAuthDivert` (the popup itself is denied; the
///   flow moves to a controlled temp surface).
/// - Any other remote host → `Deny`, even with a grant — a grant never widens
///   the Embed surface beyond loopback or the allowlist.
pub fn decide_new_window(
    conn: &Connection,
    application_id: &str,
    url: &tauri::Url,
) -> Result<NewWindowDecision> {
    if super::service::validate_local_url(url.as_str()).is_ok() {
        let outcome =
            grant_store::check_grant(conn, application_id, AppGrant::KIND_WINDOW_OPEN, None)?;
        return Ok(if outcome.allowed() {
            NewWindowDecision::GrantAllow
        } else {
            NewWindowDecision::Deny
        });
    }
    let Some(host) = url.host_str() else {
        return Ok(NewWindowDecision::Deny);
    };
    if oauth_host_allowed(conn, application_id, host)? {
        Ok(NewWindowDecision::OAuthDivert)
    } else {
        Ok(NewWindowDecision::Deny)
    }
}

/// Capture the loopback callback URL into a JSON map (url + query pairs).
pub fn extract_callback(url: &tauri::Url) -> serde_json::Map<String, serde_json::Value> {
    let mut map = serde_json::Map::new();
    map.insert(
        "url".to_string(),
        serde_json::Value::String(url.to_string()),
    );
    for (k, v) in url.query_pairs() {
        map.insert(k.into_owned(), serde_json::Value::String(v.into_owned()));
    }
    map
}

// ── Flow registry (managed state) ────────────────────────────────────

type CallbackSender = tokio::sync::oneshot::Sender<serde_json::Map<String, serde_json::Value>>;

struct OAuthFlowEntry {
    label: String,
    sender: Option<CallbackSender>,
}

/// Tracks live OAuth flows so the navigation handler can deliver the callback
/// to the awaiting command and cancel/cleanup can find the surface.
#[derive(Default)]
pub struct OAuthFlowRegistry {
    inner: Mutex<HashMap<String, OAuthFlowEntry>>,
}

impl OAuthFlowRegistry {
    fn register(&self, flow_id: String, entry: OAuthFlowEntry) -> Result<()> {
        let mut map = self
            .inner
            .lock()
            .map_err(|e| Error::Internal(e.to_string()))?;
        map.insert(flow_id, entry);
        Ok(())
    }

    /// Take the callback sender for a flow (at most one callback is delivered).
    fn take_sender(&self, flow_id: &str) -> Option<CallbackSender> {
        let mut map = self.inner.lock().ok()?;
        map.get_mut(flow_id).and_then(|e| e.sender.take())
    }

    /// Remove a flow and return its entry (used for cleanup).
    fn remove(&self, flow_id: &str) -> Option<OAuthFlowEntry> {
        self.inner.lock().ok()?.remove(flow_id)
    }
}

/// The outcome of creating an OAuth surface.
pub struct OAuthFlowStart {
    pub flow_id: String,
    pub label: String,
    pub receiver: tokio::sync::oneshot::Receiver<serde_json::Map<String, serde_json::Value>>,
}

/// Create the temporary OAuth surface for `authorize_url`.
///
/// Security gates before any WebView is created:
/// - the authorize host must be in the app's OAuth allowlist;
/// - the URL must be https (loopback http is allowed for local OAuth servers);
/// - the surface is incognito (ephemeral WebKit data store) so nothing
///   persists after the flow closes.
pub fn start_oauth_surface(
    app: &AppHandle,
    conn: &Connection,
    registry: std::sync::Arc<OAuthFlowRegistry>,
    app_id: &str,
    authorize_url: &str,
) -> Result<OAuthFlowStart> {
    let url: tauri::Url = authorize_url
        .parse()
        .map_err(|e| Error::InvalidInput(format!("authorize URL parse: {e}")))?;
    let host = url
        .host_str()
        .ok_or_else(|| Error::InvalidInput("authorize URL must have a host".into()))?
        .to_string();
    if !oauth_host_allowed(conn, app_id, &host)? {
        return Err(Error::InvalidInput(format!(
            "authorize URL host '{host}' is not in the app's OAuth allowlist"
        )));
    }
    if url.scheme() != "https" && super::service::validate_local_url(authorize_url).is_err() {
        return Err(Error::InvalidInput(
            "authorize URL must be https (or loopback http for local OAuth servers)".into(),
        ));
    }

    let flow_id = Uuid::new_v4().to_string();
    let label = format!("{OAUTH_LABEL_PREFIX}{flow_id}");
    let (tx, rx) = tokio::sync::oneshot::channel();

    registry.register(
        flow_id.clone(),
        OAuthFlowEntry {
            label: label.clone(),
            sender: Some(tx),
        },
    )?;

    let main = app
        .get_webview_window("main")
        .ok_or_else(|| Error::Internal("main window not found".into()))?;

    let registry_arc = registry;
    let app_id_owned = app_id.to_string();
    let flow_id_owned = flow_id.clone();
    let builder =
        tauri::webview::WebviewBuilder::new(label.clone(), tauri::WebviewUrl::External(url))
            .incognito(true)
            .on_navigation(move |nav_url| {
                // Any loopback navigation is the callback: capture it, deliver it to
                // the awaiting command, and cancel the navigation (the surface has no
                // reason to render the dead callback page).
                if super::service::validate_local_url(nav_url.as_str()).is_ok() {
                    let callback = extract_callback(nav_url);
                    if let Some(tx) = registry_arc.take_sender(&flow_id_owned) {
                        let _ = tx.send(callback);
                    }
                    return false;
                }
                match db::get_main_conn() {
                    Ok(conn) => oauth_navigation_allowed(&conn, &app_id_owned, nav_url),
                    Err(_) => false,
                }
            });

    let window = main.as_ref().window();
    let _webview = window
        .add_child(
            builder,
            tauri::LogicalPosition::new(0.0, 0.0),
            tauri::LogicalSize::new(OAUTH_WIDTH, OAUTH_HEIGHT),
        )
        .map_err(|e| Error::Internal(format!("oauth surface add_child: {e}")))?;

    Ok(OAuthFlowStart {
        flow_id,
        label,
        receiver: rx,
    })
}

/// Close the temp surface and drop the flow entry. Idempotent — safe to call
/// after a flow already completed or was cancelled.
pub fn cleanup_oauth_surface(
    app: &AppHandle,
    registry: &OAuthFlowRegistry,
    flow_id: &str,
) -> Result<()> {
    if let Some(entry) = registry.remove(flow_id) {
        if let Some(wv) = app.get_webview(&entry.label) {
            wv.close()
                .map_err(|e| Error::Internal(format!("oauth surface close: {e}")))?;
        }
    }
    Ok(())
}

/// Spawn a task that waits for the callback (or timeout), then closes and
/// cleans up the surface. Used for the `window.open` auto-divert path where no
/// command is awaiting the result.
pub fn spawn_background_flow(
    app: AppHandle,
    registry: std::sync::Arc<OAuthFlowRegistry>,
    start: OAuthFlowStart,
) {
    tauri::async_runtime::spawn(async move {
        let _ = tokio::time::timeout(OAUTH_TIMEOUT, start.receiver).await;
        let _ = cleanup_oauth_surface(&app, &registry, &start.flow_id);
    });
}

// ── Commands ─────────────────────────────────────────────────────────

/// Result of an OAuth flow delivered to the Renderer (callback query only —
/// never tokens; token exchange is the caller's step).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthFlowResult {
    pub ok: bool,
    pub callback: serde_json::Map<String, serde_json::Value>,
}

/// Start an OAuth authorization-code flow in a temp surface.
///
/// The authorize host must be in the app's OAuth allowlist (else
/// `InvalidInput`). The command blocks up to 180s waiting for the loopback
/// callback, then closes and cleans up the surface.
#[tauri::command]
pub async fn creative_app_oauth_start(
    app_id: String,
    authorize_url: String,
    app_handle: tauri::AppHandle,
    registry: tauri::State<'_, std::sync::Arc<OAuthFlowRegistry>>,
) -> Result<OAuthFlowResult> {
    let conn = db::get_main_conn()?;
    let start = start_oauth_surface(
        &app_handle,
        &conn,
        registry.inner().clone(),
        &app_id,
        &authorize_url,
    )?;
    let flow_id = start.flow_id.clone();
    match tokio::time::timeout(OAUTH_TIMEOUT, start.receiver).await {
        Ok(Ok(callback)) => {
            cleanup_oauth_surface(&app_handle, registry.inner(), &flow_id)?;
            Ok(OAuthFlowResult { ok: true, callback })
        }
        Ok(Err(_)) | Err(_) => {
            let _ = cleanup_oauth_surface(&app_handle, registry.inner(), &flow_id);
            Err(Error::Cancelled(
                "OAuth flow cancelled or timed out; the temp surface was closed".into(),
            ))
        }
    }
}

/// Cancel an in-flight OAuth flow and close its temp surface.
#[tauri::command]
pub fn creative_app_oauth_cancel(
    flow_id: String,
    app_handle: tauri::AppHandle,
    registry: tauri::State<'_, std::sync::Arc<OAuthFlowRegistry>>,
) -> Result<()> {
    cleanup_oauth_surface(&app_handle, registry.inner(), &flow_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn fixture() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        db::create_tables(&conn).unwrap();
        db::apply_migrations(&conn).unwrap();
        conn
    }

    fn ensure_app(conn: &Connection, id: &str) {
        conn.execute(
            "INSERT OR IGNORE INTO applications (id, source, source_id, title, version, created_at, updated_at)
             VALUES (?1, 'local_project', ?1, 'Test', '1', 't', 't')",
            rusqlite::params![id],
        )
        .unwrap();
    }

    fn url(s: &str) -> tauri::Url {
        s.parse().unwrap()
    }

    #[test]
    fn oauth_navigation_allows_allowlist_and_loopback() {
        let conn = fixture();
        ensure_app(&conn, "app-1");
        grant_store::add_oauth_domain(&conn, "app-1", "accounts.google.com").unwrap();

        assert!(oauth_navigation_allowed(
            &conn,
            "app-1",
            &url("https://accounts.google.com/o/oauth2/v2/auth")
        ));
        assert!(oauth_navigation_allowed(
            &conn,
            "app-1",
            &url("http://127.0.0.1:5173/callback?code=x")
        ));
        assert!(!oauth_navigation_allowed(
            &conn,
            "app-1",
            &url("https://evil.com/steal")
        ));
        assert!(!oauth_navigation_allowed(
            &conn,
            "app-1",
            &url("https://accounts.google.com.evil.com/x")
        ));
        assert!(!oauth_navigation_allowed(
            &conn,
            "app-1",
            &url("https://google.com/")
        ));
        assert!(!oauth_navigation_allowed(
            &conn,
            "app-1",
            &url("file:///etc/passwd")
        ));
        assert!(!oauth_navigation_allowed(
            &conn,
            "app-1",
            &url("data:text/html,hi")
        ));
    }

    #[test]
    fn oauth_navigation_never_allows_non_allowlisted_app() {
        let conn = fixture();
        ensure_app(&conn, "app-1");
        // No allowlist entries at all.
        assert!(!oauth_navigation_allowed(
            &conn,
            "app-1",
            &url("https://accounts.google.com/")
        ));
    }

    #[test]
    fn decide_new_window_diverts_allowlisted_oauth_domain() {
        let conn = fixture();
        ensure_app(&conn, "app-1");
        grant_store::add_oauth_domain(&conn, "app-1", "github.com").unwrap();
        assert_eq!(
            decide_new_window(
                &conn,
                "app-1",
                &url("https://github.com/login/oauth/authorize")
            )
            .unwrap(),
            NewWindowDecision::OAuthDivert
        );
    }

    #[test]
    fn decide_new_window_allows_loopback_only_with_grant() {
        let conn = fixture();
        ensure_app(&conn, "app-1");
        // Without a grant, loopback window.open is denied (no side effect).
        assert_eq!(
            decide_new_window(&conn, "app-1", &url("http://127.0.0.1:8080/preview")).unwrap(),
            NewWindowDecision::Deny
        );
        grant_store::set_grant(
            &conn,
            "app-1",
            AppGrant::KIND_WINDOW_OPEN,
            AppGrant::POLICY_PERSISTENT,
            None,
        )
        .unwrap();
        assert_eq!(
            decide_new_window(&conn, "app-1", &url("http://127.0.0.1:8080/preview")).unwrap(),
            NewWindowDecision::GrantAllow
        );
    }

    #[test]
    fn decide_new_window_denies_evil_remote_even_with_grant() {
        let conn = fixture();
        ensure_app(&conn, "app-1");
        grant_store::set_grant(
            &conn,
            "app-1",
            AppGrant::KIND_WINDOW_OPEN,
            AppGrant::POLICY_PERSISTENT,
            None,
        )
        .unwrap();
        // A persistent window_open grant must NOT widen the surface to remote
        // hosts — the grant only applies inside the loopback trust domain.
        assert_eq!(
            decide_new_window(&conn, "app-1", &url("https://evil.com/phish")).unwrap(),
            NewWindowDecision::Deny
        );
        assert_eq!(
            decide_new_window(&conn, "app-1", &url("http://localhost.evil.com/x")).unwrap(),
            NewWindowDecision::Deny
        );
        assert_eq!(
            decide_new_window(&conn, "app-1", &url("data:text/html,steal")).unwrap(),
            NewWindowDecision::Deny
        );
    }

    #[test]
    fn decide_new_window_consumes_one_time_grant() {
        let conn = fixture();
        ensure_app(&conn, "app-1");
        grant_store::set_grant(
            &conn,
            "app-1",
            AppGrant::KIND_WINDOW_OPEN,
            AppGrant::POLICY_ONE_TIME,
            None,
        )
        .unwrap();
        assert_eq!(
            decide_new_window(&conn, "app-1", &url("http://127.0.0.1:8080/a")).unwrap(),
            NewWindowDecision::GrantAllow
        );
        // Second window.open is denied: one-time consumed atomically.
        assert_eq!(
            decide_new_window(&conn, "app-1", &url("http://127.0.0.1:8080/b")).unwrap(),
            NewWindowDecision::Deny
        );
    }

    #[test]
    fn extract_callback_keeps_query_pairs() {
        let u = url("http://127.0.0.1:5173/callback?code=abc123&state=xyz");
        let cb = extract_callback(&u);
        assert_eq!(cb.get("code").unwrap(), "abc123");
        assert_eq!(cb.get("state").unwrap(), "xyz");
        assert!(cb
            .get("url")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("/callback"));
    }

    #[test]
    fn registry_delivers_callback_at_most_once() {
        let registry = OAuthFlowRegistry::default();
        let (tx, _rx) = tokio::sync::oneshot::channel();
        registry
            .register(
                "f1".into(),
                OAuthFlowEntry {
                    label: "creative-oauth-f1".into(),
                    sender: Some(tx),
                },
            )
            .unwrap();
        assert!(registry.take_sender("f1").is_some());
        assert!(
            registry.take_sender("f1").is_none(),
            "only one callback per flow"
        );
        assert!(registry.remove("f1").is_some());
        assert!(registry.remove("f1").is_none());
    }
}
