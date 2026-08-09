//! Window registry (batch 5 CR-502) + WindowController (T07).
//!
//! `WindowInstance` rows are a projection of real Tauri child WebViews. Every
//! window mutation goes through [`WindowController`], which journals an
//! operation, drives the real WebView (via [`WebviewGateway`]), verifies the
//! result, and then commits the Window / Preview DB state in a transaction.
//! Any step failure performs deterministic compensation; [`WindowController::reconcile`]
//! closes the DB↔WebView gap on Host restart (missing rows → closed, orphaned
//! WebViews → closed).

use super::browser::{self, BrowserStateHandle};
use super::model::{BrowserBounds, WindowInstance};
use super::{operation as op, runtime_store, surface_store};
use crate::{Error, Result};
use rusqlite::Connection;
use tauri::{AppHandle, Manager};

/// Operation kinds for window mutations (journal data only — no schema change).
pub const OP_WINDOW_OPEN: &str = "window_open";
pub const OP_WINDOW_CLOSE: &str = "window_close";
pub const OP_WINDOW_MINIMIZE: &str = "window_minimize";
pub const OP_WINDOW_RESTORE: &str = "window_restore";

/// Maximum concurrently-open child WebView windows. Every open window holds a
/// live WKWebView (renderer process + surface); beyond this documented support
/// ceiling a new `open` is refused with a typed error (R-P9 / T11). Re-opening
/// an already-open surface reuses its window and is exempt.
pub const MAX_LIVE_WINDOWS: usize = 10;

/// Outcome counts of a window reconcile sweep.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReconcileReport {
    /// DB rows in open/minimized state without a live WebView (reconciled → closed).
    pub missing: u32,
    /// Live child WebViews with no DB row (closed).
    pub orphaned: u32,
}

/// Platform WebView seam. Production impl is [`RealWebviewGateway`]; tests
/// inject a fake to verify decision + compensation logic hermetically.
pub trait WebviewGateway {
    fn exists(&self, label: &str) -> bool;
    fn labels(&self) -> Vec<String>;
    fn show(&self, label: &str, app_id: &str, url: &str, bounds: BrowserBounds) -> Result<()>;
    fn close(&self, label: &str) -> Result<()>;
    fn hide(&self, label: &str) -> Result<()>;
}

/// Real gateway: drives the actual Tauri child WebView via `browser::*`.
pub struct RealWebviewGateway<'a> {
    app: &'a AppHandle,
    browser_state: &'a BrowserStateHandle,
}

impl<'a> RealWebviewGateway<'a> {
    pub fn new(app: &'a AppHandle, browser_state: &'a BrowserStateHandle) -> Self {
        Self { app, browser_state }
    }
}

impl WebviewGateway for RealWebviewGateway<'_> {
    fn exists(&self, label: &str) -> bool {
        browser::browser_exists(self.app, label)
    }

    fn labels(&self) -> Vec<String> {
        self.app
            .webviews()
            .keys()
            .filter(|l| browser::is_window_label(l))
            .cloned()
            .collect()
    }

    fn show(&self, label: &str, app_id: &str, url: &str, bounds: BrowserBounds) -> Result<()> {
        browser::browser_show(self.app, self.browser_state, label, app_id, url, bounds)
    }

    fn close(&self, label: &str) -> Result<()> {
        browser::browser_close(self.app, self.browser_state, label)
    }

    fn hide(&self, label: &str) -> Result<()> {
        browser::browser_hide(self.app, label)
    }
}

/// Redacted journal snapshot for a window operation. Never stores URLs or
/// user content — only the window id.
fn redacted(kind: &str, window_id: &str) -> String {
    serde_json::json!({ "kind": kind, "windowId": window_id }).to_string()
}

fn require_running(conn: &Connection, application_id: &str) -> Result<String> {
    runtime_store::active_instance_id(conn, application_id)?
        .ok_or_else(|| Error::InvalidInput("app is not running; cannot open preview".into()))
}

/// Parse stored window bounds, falling back to a sane default when absent.
fn default_bounds() -> BrowserBounds {
    BrowserBounds {
        x: 0.0,
        y: 0.0,
        width: 900.0,
        height: 640.0,
    }
}

fn stored_bounds(window: &WindowInstance) -> BrowserBounds {
    window
        .bounds_json
        .as_deref()
        .and_then(|j| serde_json::from_str(j).ok())
        .unwrap_or_else(default_bounds)
}

/// Single window authority: journals + drives WebView + commits Window/Preview.
pub struct WindowController;

impl WindowController {
    /// Open a child WebView for an app surface. Requires the app to be running
    /// (CR-303: never show a stopped app). Reuses an existing open window for
    /// the same surface so repeated "open" does not stack duplicates.
    pub fn open(
        gw: &dyn WebviewGateway,
        conn: &mut Connection,
        app_id: &str,
        application_id: &str,
        surface_id: &str,
        url: &str,
        bounds: BrowserBounds,
    ) -> Result<WindowInstance> {
        // A live preview bind requires a live runtime.
        let runtime_instance_id = require_running(conn, application_id)?;
        surface_store::find_main_surface(conn, application_id)?
            .ok_or_else(|| Error::Internal(format!("no main surface for app {application_id}")))?;

        // R-P9 / T11: bound concurrent child WebViews. Re-opening an
        // already-open surface reuses its live window (no new WebView), so the
        // cap applies only when this open would add a new live WebView.
        let reuses_open_window =
            surface_store::find_latest_surface_window(conn, application_id, surface_id)?
                .map(|w| w.state == WindowInstance::STATE_OPEN)
                .unwrap_or(false);
        if !reuses_open_window {
            let open = surface_store::count_open_windows(conn)?;
            if open >= MAX_LIVE_WINDOWS {
                return Err(Error::Internal(format!(
                    "window limit reached ({open}/{MAX_LIVE_WINDOWS} open): \
                     close a window before opening another"
                )));
            }
        }

        // Journal the operation before any external side effect.
        let op_id = op::create_operation(
            conn,
            Some(application_id),
            OP_WINDOW_OPEN,
            "user",
            Some(
                &serde_json::json!({ "kind": OP_WINDOW_OPEN, "applicationId": application_id })
                    .to_string(),
            ),
        )?;
        op::transition(conn, op_id, &[op::PHASE_PENDING], op::PHASE_RUNNING)?;

        // Reuse this surface's last window (any state), else create one whose
        // label is derived from the window id.
        let window =
            match surface_store::find_latest_surface_window(conn, application_id, surface_id)? {
                Some(existing) => existing,
                None => {
                    let id = surface_store::create_window(
                        conn,
                        application_id,
                        surface_id,
                        Some(&runtime_instance_id),
                    )?;
                    surface_store::find_window(conn, &id)?
                        .ok_or_else(|| Error::Internal("window vanished after create".into()))?
                }
            };
        let label = browser::window_label(&window.id);

        // Execute the WebView: a show failure keeps the row closed — never a
        // fake open (T07).
        gw.show(&label, app_id, url, bounds.clone())
            .inspect_err(|e| {
                let _ = op::finish_failure(conn, op_id, Some("window_show_failed"), &e.to_string());
            })?;

        // Verify: a WebView that vanishes right after show is a real failure.
        if !gw.exists(&label) {
            let msg = format!("webview {label} missing after show");
            let _ = op::finish_failure(conn, op_id, Some("window_missing"), &msg);
            return Err(Error::Internal(msg));
        }

        // Commit Window + Preview in one transaction. On DB failure compensate
        // by closing the WebView we just opened (deterministic compensation).
        if let Err(e) =
            surface_store::commit_window_open(conn, &window.id, &runtime_instance_id, url, &bounds)
        {
            let _ = gw.close(&label);
            let _ = op::finish_failure(conn, op_id, Some("window_commit_failed"), &e.to_string());
            return Err(e);
        }

        let _ = op::finish_success(conn, op_id);
        surface_store::find_window(conn, &window.id)?
            .ok_or_else(|| Error::Internal("window vanished after commit".into()))
    }

    /// Close a window: close the real WebView (a WebView that is already gone
    /// is reconciled to closed — never faked open), then commit the closed
    /// state + clear the preview bind. A DB commit failure returns a typed
    /// error and keeps the journal failed for recovery.
    pub fn close(gw: &dyn WebviewGateway, conn: &mut Connection, window_id: &str) -> Result<()> {
        let window = surface_store::find_window(conn, window_id)?
            .ok_or_else(|| Error::NotFound(format!("window {window_id}")))?;
        let label = browser::window_label(window_id);
        let op_id = op::create_operation(
            conn,
            Some(&window.application_id),
            OP_WINDOW_CLOSE,
            "user",
            Some(&redacted(OP_WINDOW_CLOSE, window_id)),
        )?;
        op::transition(conn, op_id, &[op::PHASE_PENDING], op::PHASE_RUNNING)?;

        // External action first: close the WebView. Failure must stay observable.
        let was_missing = if gw.exists(&label) {
            gw.close(&label).inspect_err(|e| {
                let _ =
                    op::finish_failure(conn, op_id, Some("window_close_failed"), &e.to_string());
            })?;
            if gw.exists(&label) {
                let msg = format!("webview {label} still open after close");
                let _ = op::finish_failure(conn, op_id, Some("window_close_unverified"), &msg);
                return Err(Error::Internal(msg));
            }
            false
        } else {
            true
        };

        surface_store::commit_window_closed(
            conn,
            window_id,
            window.runtime_instance_id.as_deref(),
            was_missing,
        )
        .inspect_err(|e| {
            let _ = op::finish_failure(conn, op_id, Some("window_commit_failed"), &e.to_string());
        })?;

        let _ = op::finish_success(conn, op_id);
        Ok(())
    }

    /// Minimize a window: hide the real WebView, then commit the minimized
    /// state. A WebView that is already gone is reconciled to closed.
    pub fn minimize(gw: &dyn WebviewGateway, conn: &mut Connection, window_id: &str) -> Result<()> {
        let window = surface_store::find_window(conn, window_id)?
            .ok_or_else(|| Error::NotFound(format!("window {window_id}")))?;
        let label = browser::window_label(window_id);
        let op_id = op::create_operation(
            conn,
            Some(&window.application_id),
            OP_WINDOW_MINIMIZE,
            "user",
            Some(&redacted(OP_WINDOW_MINIMIZE, window_id)),
        )?;
        op::transition(conn, op_id, &[op::PHASE_PENDING], op::PHASE_RUNNING)?;

        if gw.exists(&label) {
            gw.hide(&label).inspect_err(|e| {
                let _ = op::finish_failure(conn, op_id, Some("window_hide_failed"), &e.to_string());
            })?;
            if let Err(e) =
                surface_store::update_window_state(conn, window_id, WindowInstance::STATE_MINIMIZED)
            {
                // Deterministic compensation: re-show the WebView we just hid so
                // the DB row and the real WebView do not diverge.
                let app_id =
                    runtime_store::source_id_for_application(conn, &window.application_id)?
                        .unwrap_or_else(|| window.application_id.clone());
                let url = window.url.as_deref().unwrap_or_default();
                let _ = gw.show(&label, &app_id, url, stored_bounds(&window));
                let _ =
                    op::finish_failure(conn, op_id, Some("window_commit_failed"), &e.to_string());
                return Err(e);
            }
        } else {
            surface_store::reconcile_window_missing(
                conn,
                window_id,
                "webview missing during minimize; reconciled to closed",
            )?;
        }

        let _ = op::finish_success(conn, op_id);
        Ok(())
    }

    /// Restore a window: re-show the existing real WebView at its committed
    /// URL. A WebView that is gone is reconciled to closed and reported — never
    /// faked open.
    pub fn restore(
        gw: &dyn WebviewGateway,
        conn: &mut Connection,
        window_id: &str,
        bounds: Option<BrowserBounds>,
    ) -> Result<()> {
        let window = surface_store::find_window(conn, window_id)?
            .ok_or_else(|| Error::NotFound(format!("window {window_id}")))?;
        let label = browser::window_label(window_id);
        let op_id = op::create_operation(
            conn,
            Some(&window.application_id),
            OP_WINDOW_RESTORE,
            "user",
            Some(&redacted(OP_WINDOW_RESTORE, window_id)),
        )?;
        op::transition(conn, op_id, &[op::PHASE_PENDING], op::PHASE_RUNNING)?;

        if !gw.exists(&label) {
            let msg = format!("webview {label} missing; cannot restore");
            surface_store::reconcile_window_missing(
                conn,
                window_id,
                "webview missing during restore; reconciled to closed",
            )?;
            let _ = op::finish_failure(conn, op_id, Some("window_missing"), &msg);
            return Err(Error::NotFound(msg));
        }

        let url = window.url.as_deref().ok_or_else(|| {
            let msg = format!("window {window_id} has no content URL; cannot restore");
            let _ = surface_store::reconcile_window_missing(
                conn,
                window_id,
                "no content url; reconciled to closed",
            );
            let _ = op::finish_failure(conn, op_id, Some("window_no_url"), &msg);
            Error::Conflict(msg)
        })?;

        let app_id = runtime_store::source_id_for_application(conn, &window.application_id)?
            .unwrap_or_else(|| window.application_id.clone());
        gw.show(
            &label,
            &app_id,
            url,
            bounds.unwrap_or_else(|| stored_bounds(&window)),
        )
        .inspect_err(|e| {
            let _ = op::finish_failure(conn, op_id, Some("window_show_failed"), &e.to_string());
        })?;
        if !gw.exists(&label) {
            let msg = format!("webview {label} missing after restore");
            let _ = op::finish_failure(conn, op_id, Some("window_missing"), &msg);
            return Err(Error::Internal(msg));
        }

        if let Err(e) =
            surface_store::update_window_state(conn, window_id, WindowInstance::STATE_OPEN)
        {
            // Deterministic compensation: hide the WebView we just restored so
            // the DB row and the real WebView do not diverge.
            let _ = gw.hide(&label);
            let _ = op::finish_failure(conn, op_id, Some("window_commit_failed"), &e.to_string());
            return Err(e);
        }

        let _ = op::finish_success(conn, op_id);
        Ok(())
    }

    /// Close every non-closed window of an application. Used by stop (offline
    /// policy) and delete (prove child closed before removing references).
    pub fn close_app_windows(
        gw: &dyn WebviewGateway,
        conn: &mut Connection,
        application_id: &str,
    ) -> Result<u32> {
        let mut n = 0;
        for w in surface_store::list_windows(conn, application_id)? {
            if w.state == WindowInstance::STATE_CLOSED {
                continue;
            }
            Self::close(gw, conn, &w.id)?;
            n += 1;
        }
        Ok(n)
    }

    /// Minimize every non-closed window of an application.
    pub fn minimize_app_windows(
        gw: &dyn WebviewGateway,
        conn: &mut Connection,
        application_id: &str,
    ) -> Result<u32> {
        let mut n = 0;
        for w in surface_store::list_windows(conn, application_id)? {
            if w.state == WindowInstance::STATE_CLOSED {
                continue;
            }
            Self::minimize(gw, conn, &w.id)?;
            n += 1;
        }
        Ok(n)
    }

    /// Reconcile DB windows vs live child WebViews. A DB row in open/minimized
    /// state without a live WebView is marked missing → closed; a live child
    /// WebView with no DB row is closed as an orphan. Never fabricates an open
    /// state for a window that has no WebView.
    pub fn reconcile(gw: &dyn WebviewGateway, conn: &Connection) -> Result<ReconcileReport> {
        let mut report = ReconcileReport::default();
        for w in surface_store::list_all_windows(conn)? {
            if w.state == WindowInstance::STATE_CLOSED {
                continue;
            }
            let label = browser::window_label(&w.id);
            if !gw.exists(&label) {
                surface_store::reconcile_window_missing(
                    conn,
                    &w.id,
                    "webview missing after restart; reconciled to closed",
                )?;
                report.missing += 1;
            }
        }
        for label in gw.labels() {
            let Some(window_id) = label.strip_prefix(browser::WINDOW_LABEL_PREFIX) else {
                continue;
            };
            if surface_store::find_window(conn, window_id)?.is_none() {
                let _ = gw.close(&label);
                report.orphaned += 1;
            }
        }
        Ok(report)
    }
}

/// Reconcile DB windows vs live child WebViews at Host startup. Returns the
/// number of reconciled rows (missing + orphaned). Emits a bus event when any
/// window was reconciled so the Renderer refreshes (R-S9).
pub fn reconcile_all(app: &AppHandle, conn: &Connection) -> Result<u32> {
    let Some(browser_state) = app.try_state::<BrowserStateHandle>() else {
        return Ok(0);
    };
    let gw = RealWebviewGateway::new(app, &browser_state);
    let report = WindowController::reconcile(&gw, conn)?;
    let total = report.missing + report.orphaned;
    if total > 0 {
        crate::emit_db_state_changed(
            app,
            "creative-window-reconcile",
            serde_json::json!({ "missing": report.missing, "orphaned": report.orphaned }),
        );
    }
    Ok(total)
}

#[cfg(test)]
#[path = "window_tests.rs"]
mod window_tests;
