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

    #[test]
    fn open_refuses_beyond_live_window_cap() {
        let conn = fixture();
        let (surface_id, _iid) = seed_running_app(&conn, "app-1");
        let gw = FakeGateway::default();
        let mut c = conn;
        // First open (counts as one live WebView).
        WindowController::open(
            &gw,
            &mut c,
            "app-1",
            "app-1",
            &surface_id,
            "http://127.0.0.1:8080/",
            bounds(),
        )
        .unwrap();
        // Fill the remaining cap slots with distinct embed surfaces.
        for i in 1..MAX_LIVE_WINDOWS {
            let s = surface_store::create_surface(
                &c,
                "app-1",
                &format!("embed{i}"),
                &format!("Embed {i}"),
                None,
            )
            .unwrap();
            WindowController::open(
                &gw,
                &mut c,
                "app-1",
                "app-1",
                &s,
                "http://127.0.0.1:8080/embed",
                bounds(),
            )
            .unwrap();
        }
        // A new surface at the cap is refused with a typed error.
        let extra =
            surface_store::create_surface(&c, "app-1", "embed-extra", "Extra", None).unwrap();
        let err = WindowController::open(
            &gw,
            &mut c,
            "app-1",
            "app-1",
            &extra,
            "http://127.0.0.1:8080/embed",
            bounds(),
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("window limit reached"),
            "expected window-limit error, got {err}"
        );
        assert_eq!(
            gw.live.borrow().len(),
            MAX_LIVE_WINDOWS,
            "no WebView beyond the cap"
        );
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
