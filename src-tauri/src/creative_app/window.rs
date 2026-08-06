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
            .into_iter()
            .map(|(label, _wv)| label)
            .filter(|l| browser::is_window_label(l))
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
        gw.show(&label, app_id, url, bounds.clone()).map_err(|e| {
            let _ = op::finish_failure(conn, op_id, Some("window_show_failed"), &e.to_string());
            e
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
            gw.close(&label).map_err(|e| {
                let _ =
                    op::finish_failure(conn, op_id, Some("window_close_failed"), &e.to_string());
                e
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
        .map_err(|e| {
            let _ = op::finish_failure(conn, op_id, Some("window_commit_failed"), &e.to_string());
            e
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
            gw.hide(&label).map_err(|e| {
                let _ = op::finish_failure(conn, op_id, Some("window_hide_failed"), &e.to_string());
                e
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
        .map_err(|e| {
            let _ = op::finish_failure(conn, op_id, Some("window_show_failed"), &e.to_string());
            e
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

    /// Reconcile DB windows vs live child WebViews:
    /// - a DB row in open/minimized state without a live WebView → missing → closed;
    /// - a live child WebView with no DB row → orphaned → closed.
    /// Never fabricates an open state for a window that has no WebView.
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
mod tests {
    use super::*;
    use crate::db;
    use rusqlite::params;
    use std::cell::RefCell;

    /// Hermetic WebView seam: records every call, exposes a configurable live set.
    ///
    /// `delete_row_on_next_hide` / `delete_row_on_next_show` simulate the DB
    /// row vanishing right after a WebView mutation (via a second connection to
    /// the same file DB), so the compensation path can be exercised hermetically.
    #[derive(Default)]
    struct FakeGateway {
        live: RefCell<std::collections::HashSet<String>>,
        shown: RefCell<Vec<String>>,
        closed: RefCell<Vec<String>>,
        hidden: RefCell<Vec<String>>,
        close_fails: bool,
        db2: RefCell<Option<rusqlite::Connection>>,
        delete_row_on_next_hide: bool,
        delete_row_on_next_show: bool,
    }

    impl FakeGateway {
        fn delete_window_row(&self, label: &str) {
            if let (Some(conn), Some(id)) = (
                &*self.db2.borrow(),
                label.strip_prefix(browser::WINDOW_LABEL_PREFIX),
            ) {
                let _ = conn.execute("DELETE FROM window_instances WHERE id = ?1", params![id]);
            }
        }
    }

    impl WebviewGateway for FakeGateway {
        fn exists(&self, label: &str) -> bool {
            self.live.borrow().contains(label)
        }
        fn labels(&self) -> Vec<String> {
            self.live.borrow().iter().cloned().collect()
        }
        fn show(
            &self,
            label: &str,
            _app_id: &str,
            _url: &str,
            _bounds: BrowserBounds,
        ) -> Result<()> {
            self.live.borrow_mut().insert(label.to_string());
            self.shown.borrow_mut().push(label.to_string());
            if self.delete_row_on_next_show {
                self.delete_window_row(label);
            }
            Ok(())
        }
        fn close(&self, label: &str) -> Result<()> {
            self.closed.borrow_mut().push(label.to_string());
            if self.close_fails {
                return Err(Error::Internal("webview close failed".into()));
            }
            self.live.borrow_mut().remove(label);
            Ok(())
        }
        fn hide(&self, label: &str) -> Result<()> {
            self.hidden.borrow_mut().push(label.to_string());
            if self.delete_row_on_next_hide {
                self.delete_window_row(label);
            }
            Ok(())
        }
    }

    fn fixture() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        db::create_tables(&conn).unwrap();
        db::apply_migrations(&conn).unwrap();
        conn
    }

    /// A file-backed fixture so a second connection can observe the same rows.
    fn file_fixture() -> (Connection, Connection, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let conn = Connection::open(&path).unwrap();
        db::create_tables(&conn).unwrap();
        db::apply_migrations(&conn).unwrap();
        let second = Connection::open(&path).unwrap();
        (conn, second, dir)
    }

    /// Seed an application + main surface + a running runtime instance.
    fn seed_running_app(conn: &Connection, app_id: &str) -> (String, String) {
        conn.execute(
            "INSERT INTO applications (id, source, source_id, title, version, created_at, updated_at)
             VALUES (?1, 'local_project', ?2, 'Test', '1', 't', 't')",
            params![app_id, app_id],
        )
        .unwrap();
        let surface_id = surface_store::create_surface(conn, app_id, "main", "Main", None).unwrap();
        let iid = runtime_store::create_instance(conn, app_id, None, "host_http").unwrap();
        runtime_store::mark_running(conn, &iid, &[], None, None, None).unwrap();
        (surface_id, iid)
    }

    fn bounds() -> BrowserBounds {
        BrowserBounds {
            x: 10.0,
            y: 20.0,
            width: 800.0,
            height: 600.0,
        }
    }

    #[test]
    fn window_state_constants() {
        assert_eq!(WindowInstance::STATE_OPEN, "open");
        assert_eq!(WindowInstance::STATE_CLOSED, "closed");
        assert_eq!(WindowInstance::STATE_MINIMIZED, "minimized");
        assert_eq!(WindowInstance::STATE_BACKGROUND, "background");
    }

    #[test]
    fn close_and_stop_are_distinct() {
        // Verify that the WindowInstance state constants are distinct from
        // RuntimeInstanceStatus values, so close≠stop is enforced at the type level.
        let close_state = WindowInstance::STATE_CLOSED;
        let stop_state = crate::creative_app::model::RuntimeInstanceStatus::Stopped.as_str();
        assert_ne!(
            close_state, stop_state,
            "window closed state must differ from runtime stopped state"
        );
    }

    // ── Open drives the real WebView (no fake open) ────────────────────

    #[test]
    fn open_shows_webview_with_window_id_label() {
        let conn = fixture();
        let (surface_id, _iid) = seed_running_app(&conn, "app-1");
        let gw = FakeGateway::default();
        let mut c = conn;

        let w = WindowController::open(
            &gw,
            &mut c,
            "app-1",
            "app-1",
            &surface_id,
            "http://127.0.0.1:8080/",
            bounds(),
        )
        .unwrap();

        assert_eq!(w.state, WindowInstance::STATE_OPEN);
        assert_eq!(w.label, browser::window_label(&w.id));
        assert!(
            gw.live.borrow().contains(&w.label),
            "open must create a live WebView"
        );
        assert_eq!(gw.shown.borrow().len(), 1);
        assert_eq!(gw.shown.borrow()[0], w.label);

        // The window row is committed with its content URL.
        let row = surface_store::find_window(&c, &w.id).unwrap().unwrap();
        assert_eq!(row.state, WindowInstance::STATE_OPEN);
        assert_eq!(row.url.as_deref(), Some("http://127.0.0.1:8080/"));
        // Re-open reuses the same window (no duplicate stacking).
        let w2 = WindowController::open(
            &gw,
            &mut c,
            "app-1",
            "app-1",
            &surface_id,
            "http://127.0.0.1:8080/",
            bounds(),
        )
        .unwrap();
        assert_eq!(w2.id, w.id);
    }

    #[test]
    fn open_requires_running_instance() {
        let conn = fixture();
        // app + surface but NO running runtime instance.
        conn.execute(
            "INSERT INTO applications (id, source, source_id, title, version, created_at, updated_at)
             VALUES ('app-1', 'local_project', 'src-1', 'Test', '1', 't', 't')",
            [],
        )
        .unwrap();
        let surface_id =
            surface_store::create_surface(&conn, "app-1", "main", "Main", None).unwrap();
        let gw = FakeGateway::default();
        let mut c = conn;

        let err = WindowController::open(
            &gw,
            &mut c,
            "app-1",
            "app-1",
            &surface_id,
            "http://127.0.0.1:8080/",
            bounds(),
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("not running"),
            "open must refuse to show a stopped app, got {err}"
        );
        assert!(gw.shown.borrow().is_empty(), "no WebView may be shown");
    }

    #[test]
    fn open_two_surfaces_get_distinct_labels() {
        let conn = fixture();
        let (surface_id, _iid) = seed_running_app(&conn, "app-1");
        let gw = FakeGateway::default();
        let mut c = conn;

        let w1 = WindowController::open(
            &gw,
            &mut c,
            "app-1",
            "app-1",
            &surface_id,
            "http://127.0.0.1:8080/",
            bounds(),
        )
        .unwrap();
        // A second embed surface for the same app gets its own window + label.
        let surface2 =
            surface_store::create_surface(&c, "app-1", "embed", "Embed 2", None).unwrap();
        let w2 = WindowController::open(
            &gw,
            &mut c,
            "app-1",
            "app-1",
            &surface2,
            "http://127.0.0.1:8080/embed",
            bounds(),
        )
        .unwrap();

        assert_ne!(w1.id, w2.id);
        assert_ne!(
            w1.label, w2.label,
            "multi-surface windows need unique labels"
        );
        assert!(gw.live.borrow().contains(&w1.label));
        assert!(gw.live.borrow().contains(&w2.label));
    }

    // ── Close drives the real WebView; missing → reconciled closed ─────

    #[test]
    fn close_closes_real_webview_and_commits_closed() {
        let conn = fixture();
        let (surface_id, _iid) = seed_running_app(&conn, "app-1");
        let gw = FakeGateway::default();
        let mut c = conn;
        let w = WindowController::open(
            &gw,
            &mut c,
            "app-1",
            "app-1",
            &surface_id,
            "http://127.0.0.1:8080/",
            bounds(),
        )
        .unwrap();

        WindowController::close(&gw, &mut c, &w.id).unwrap();

        assert_eq!(gw.closed.borrow().len(), 1);
        assert_eq!(gw.closed.borrow()[0], w.label);
        assert!(
            !gw.live.borrow().contains(&w.label),
            "webview must be closed"
        );
        let row = surface_store::find_window(&c, &w.id).unwrap().unwrap();
        assert_eq!(row.state, WindowInstance::STATE_CLOSED);
        assert_eq!(row.reconcile_state, "ok");
    }

    #[test]
    fn close_missing_webview_reconciles_to_closed() {
        let conn = fixture();
        let (surface_id, _iid) = seed_running_app(&conn, "app-1");
        let gw = FakeGateway::default();
        let mut c = conn;
        let w = WindowController::open(
            &gw,
            &mut c,
            "app-1",
            "app-1",
            &surface_id,
            "http://127.0.0.1:8080/",
            bounds(),
        )
        .unwrap();
        // The WebView disappears out-of-band (Host crash, manual close).
        gw.live.borrow_mut().remove(&w.label);

        WindowController::close(&gw, &mut c, &w.id).unwrap();

        // No fake close; the row is reconciled to closed with the gap recorded.
        assert!(gw.closed.borrow().is_empty());
        let row = surface_store::find_window(&c, &w.id).unwrap().unwrap();
        assert_eq!(row.state, WindowInstance::STATE_CLOSED);
        assert_eq!(row.reconcile_state, "missing");
        assert!(row.last_error.is_some());
    }

    #[test]
    fn close_returns_typed_error_when_db_row_vanished() {
        let conn = fixture();
        let gw = FakeGateway::default();
        let mut c = conn;
        // No window row exists at all.
        let err = WindowController::close(&gw, &mut c, "no-such-window").unwrap_err();
        assert!(matches!(err, Error::NotFound(_)));
    }

    #[test]
    fn close_propagates_webview_close_failure() {
        let conn = fixture();
        let (surface_id, _iid) = seed_running_app(&conn, "app-1");
        let mut gw = FakeGateway::default();
        let mut c = conn;
        let w = WindowController::open(
            &gw,
            &mut c,
            "app-1",
            "app-1",
            &surface_id,
            "http://127.0.0.1:8080/",
            bounds(),
        )
        .unwrap();
        gw.close_fails = true;

        let err = WindowController::close(&gw, &mut c, &w.id).unwrap_err();
        assert!(err.to_string().contains("close failed"));
        // The journal op is left failed for recovery, and the webview stays live.
        assert!(gw.live.borrow().contains(&w.label));
    }

    // ── Minimize / Restore drive the real WebView ──────────────────────

    #[test]
    fn minimize_hides_webview_and_commits_minimized() {
        let conn = fixture();
        let (surface_id, _iid) = seed_running_app(&conn, "app-1");
        let gw = FakeGateway::default();
        let mut c = conn;
        let w = WindowController::open(
            &gw,
            &mut c,
            "app-1",
            "app-1",
            &surface_id,
            "http://127.0.0.1:8080/",
            bounds(),
        )
        .unwrap();

        WindowController::minimize(&gw, &mut c, &w.id).unwrap();

        assert_eq!(gw.hidden.borrow().len(), 1);
        assert_eq!(gw.hidden.borrow()[0], w.label);
        let row = surface_store::find_window(&c, &w.id).unwrap().unwrap();
        assert_eq!(row.state, WindowInstance::STATE_MINIMIZED);
    }

    #[test]
    fn restore_shows_webview_and_commits_open() {
        let conn = fixture();
        let (surface_id, _iid) = seed_running_app(&conn, "app-1");
        let gw = FakeGateway::default();
        let mut c = conn;
        let w = WindowController::open(
            &gw,
            &mut c,
            "app-1",
            "app-1",
            &surface_id,
            "http://127.0.0.1:8080/",
            bounds(),
        )
        .unwrap();
        WindowController::minimize(&gw, &mut c, &w.id).unwrap();

        WindowController::restore(&gw, &mut c, &w.id, None).unwrap();

        let row = surface_store::find_window(&c, &w.id).unwrap().unwrap();
        assert_eq!(row.state, WindowInstance::STATE_OPEN);
        // Restore re-shows the existing webview at its committed URL.
        assert_eq!(gw.shown.borrow().len(), 2);
    }

    #[test]
    fn restore_missing_webview_reconciles_to_closed_not_open() {
        let conn = fixture();
        let (surface_id, _iid) = seed_running_app(&conn, "app-1");
        let gw = FakeGateway::default();
        let mut c = conn;
        let w = WindowController::open(
            &gw,
            &mut c,
            "app-1",
            "app-1",
            &surface_id,
            "http://127.0.0.1:8080/",
            bounds(),
        )
        .unwrap();
        gw.live.borrow_mut().remove(&w.label);

        let err = WindowController::restore(&gw, &mut c, &w.id, None).unwrap_err();
        assert!(
            matches!(err, Error::NotFound(_)),
            "restore of a dead WebView must not fake an open state, got {err}"
        );
        let row = surface_store::find_window(&c, &w.id).unwrap().unwrap();
        assert_eq!(row.state, WindowInstance::STATE_CLOSED);
        assert_eq!(row.reconcile_state, "missing");
    }

    // ── Reconcile (Host restart) ───────────────────────────────────────

    #[test]
    fn reconcile_marks_open_window_without_webview_as_closed() {
        let conn = fixture();
        let (surface_id, _iid) = seed_running_app(&conn, "app-1");
        let gw = FakeGateway::default();
        let mut c = conn;
        let w = WindowController::open(
            &gw,
            &mut c,
            "app-1",
            "app-1",
            &surface_id,
            "http://127.0.0.1:8080/",
            bounds(),
        )
        .unwrap();
        // Host restart: the real WebView is gone, the DB row still says open.
        gw.live.borrow_mut().clear();

        let report = WindowController::reconcile(&gw, &c).unwrap();

        assert_eq!(report.missing, 1, "the open row has no WebView → missing");
        assert_eq!(report.orphaned, 0);
        let row = surface_store::find_window(&c, &w.id).unwrap().unwrap();
        assert_eq!(row.state, WindowInstance::STATE_CLOSED);
        assert_eq!(row.reconcile_state, "missing");
        assert!(
            row.last_error
                .as_deref()
                .unwrap_or_default()
                .contains("restart"),
            "last_error must explain the reconcile, got {:?}",
            row.last_error
        );
    }

    #[test]
    fn reconcile_keeps_windows_with_live_webview() {
        let conn = fixture();
        let (surface_id, _iid) = seed_running_app(&conn, "app-1");
        let gw = FakeGateway::default();
        let mut c = conn;
        let w = WindowController::open(
            &gw,
            &mut c,
            "app-1",
            "app-1",
            &surface_id,
            "http://127.0.0.1:8080/",
            bounds(),
        )
        .unwrap();

        let report = WindowController::reconcile(&gw, &c).unwrap();
        assert_eq!(report.missing, 0);
        let row = surface_store::find_window(&c, &w.id).unwrap().unwrap();
        assert_eq!(row.state, WindowInstance::STATE_OPEN);
    }

    #[test]
    fn reconcile_closes_orphaned_webview_without_db_row() {
        let conn = fixture();
        let gw = FakeGateway::default();
        // A live WebView with no DB row (orphan from a deleted app).
        gw.live
            .borrow_mut()
            .insert("creative-window-orphan-1".into());

        let report = WindowController::reconcile(&gw, &conn).unwrap();
        assert_eq!(report.orphaned, 1);
        assert_eq!(report.missing, 0);
        assert_eq!(gw.closed.borrow().len(), 1);
        assert_eq!(gw.closed.borrow()[0], "creative-window-orphan-1");
    }

    #[test]
    fn close_app_windows_closes_every_open_window() {
        let conn = fixture();
        let (surface_id, _iid) = seed_running_app(&conn, "app-1");
        let gw = FakeGateway::default();
        let mut c = conn;
        let w1 = WindowController::open(
            &gw,
            &mut c,
            "app-1",
            "app-1",
            &surface_id,
            "http://127.0.0.1:8080/",
            bounds(),
        )
        .unwrap();
        let surface2 =
            surface_store::create_surface(&c, "app-1", "embed", "Embed 2", None).unwrap();
        let w2 = WindowController::open(
            &gw,
            &mut c,
            "app-1",
            "app-1",
            &surface2,
            "http://127.0.0.1:8080/embed",
            bounds(),
        )
        .unwrap();

        let n = WindowController::close_app_windows(&gw, &mut c, "app-1").unwrap();
        assert_eq!(n, 2);
        assert_eq!(gw.closed.borrow().len(), 2);
        assert!(
            surface_store::find_window(&c, &w1.id)
                .unwrap()
                .unwrap()
                .state
                == WindowInstance::STATE_CLOSED
        );
        assert!(
            surface_store::find_window(&c, &w2.id)
                .unwrap()
                .unwrap()
                .state
                == WindowInstance::STATE_CLOSED
        );
    }

    fn count_previews(conn: &Connection, runtime_instance_id: &str) -> i64 {
        conn.query_row(
            "SELECT COUNT(*) FROM preview_targets WHERE runtime_instance_id = ?1",
            params![runtime_instance_id],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn closing_one_of_two_windows_keeps_the_runtime_preview() {
        let conn = fixture();
        let (surface_id, iid) = seed_running_app(&conn, "app-1");
        let gw = FakeGateway::default();
        let mut c = conn;
        let w1 = WindowController::open(
            &gw,
            &mut c,
            "app-1",
            "app-1",
            &surface_id,
            "http://127.0.0.1:8080/",
            bounds(),
        )
        .unwrap();
        let surface2 =
            surface_store::create_surface(&c, "app-1", "embed", "Embed 2", None).unwrap();
        let w2 = WindowController::open(
            &gw,
            &mut c,
            "app-1",
            "app-1",
            &surface2,
            "http://127.0.0.1:8080/embed",
            bounds(),
        )
        .unwrap();
        assert_eq!(
            count_previews(&c, &iid),
            1,
            "both windows share one runtime preview"
        );

        // Closing the first window must NOT drop the preview while the second
        // window still shows the runtime content.
        WindowController::close(&gw, &mut c, &w1.id).unwrap();
        assert_eq!(
            count_previews(&c, &iid),
            1,
            "preview must survive while another window is open"
        );

        // Closing the last window drops the runtime preview.
        WindowController::close(&gw, &mut c, &w2.id).unwrap();
        assert_eq!(count_previews(&c, &iid), 0);
    }

    #[test]
    fn minimize_compensates_by_reshowing_when_commit_fails() {
        let (conn, second, _dir) = file_fixture();
        let (surface_id, _iid) = seed_running_app(&conn, "app-1");
        let mut gw = FakeGateway::default();
        gw.db2.borrow_mut().replace(second);
        let mut c = conn;
        let w = WindowController::open(
            &gw,
            &mut c,
            "app-1",
            "app-1",
            &surface_id,
            "http://127.0.0.1:8080/",
            bounds(),
        )
        .unwrap();

        // The DB row vanishes right after the hide (out-of-band delete).
        gw.delete_row_on_next_hide = true;
        let err = WindowController::minimize(&gw, &mut c, &w.id).unwrap_err();
        assert!(
            matches!(err, Error::Conflict(_)),
            "commit failure must surface as a typed error, got {err}"
        );

        // Deterministic compensation: the WebView we just hid is shown again.
        assert_eq!(gw.hidden.borrow().len(), 1);
        assert_eq!(
            gw.shown.borrow().len(),
            2,
            "compensation re-shows the webview"
        );
        assert!(gw.live.borrow().contains(&w.label), "webview stays live");
    }

    #[test]
    fn restore_compensates_by_hiding_when_commit_fails() {
        let (conn, second, _dir) = file_fixture();
        let (surface_id, _iid) = seed_running_app(&conn, "app-1");
        let mut gw = FakeGateway::default();
        gw.db2.borrow_mut().replace(second);
        let mut c = conn;
        let w = WindowController::open(
            &gw,
            &mut c,
            "app-1",
            "app-1",
            &surface_id,
            "http://127.0.0.1:8080/",
            bounds(),
        )
        .unwrap();
        WindowController::minimize(&gw, &mut c, &w.id).unwrap();

        // The DB row vanishes right after the restore show (out-of-band delete).
        gw.delete_row_on_next_show = true;
        let err = WindowController::restore(&gw, &mut c, &w.id, None).unwrap_err();
        assert!(
            matches!(err, Error::Conflict(_)),
            "commit failure must surface as a typed error, got {err}"
        );

        // Deterministic compensation: the WebView we just restored is hidden again.
        assert_eq!(
            gw.shown.borrow().len(),
            2,
            "restore show ran once after open"
        );
        assert_eq!(
            gw.hidden.borrow().len(),
            2,
            "compensation hides the webview again"
        );
    }

    #[test]
    fn child_label_generates_correct_window_label() {
        // The legacy app-level label still exists for compatibility; window
        // lifecycle uses the window-id label family.
        let label = browser::child_label("my-app");
        assert_eq!(label, "creative-app-my-app");
        assert!(!label.contains("main"));
    }
}
