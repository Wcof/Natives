use super::*;
    use crate::creative_draft::store;

    struct DraftFixture {
        _tmp: tempfile::TempDir,
        data_dir: PathBuf,
        modules_dir: PathBuf,
        db_path: PathBuf,
        conn: Connection,
    }

    /// Build the same layout `lib.rs` builds: `<data_dir>/modules`, `<data_dir>/drafts`
    /// and `<data_dir>/natives.db`, so the data-dir derivation is exercised for real.
    fn fixture() -> DraftFixture {
        let tmp = tempfile::tempdir().expect("temp dir");
        let data_dir = tmp.path().to_path_buf();
        let modules_dir = data_dir.join("modules");
        std::fs::create_dir_all(&modules_dir).expect("modules dir");
        let db_path = data_dir.join("natives.db");
        let conn = Connection::open(&db_path).expect("open db");
        crate::db::create_tables(&conn).expect("base tables");
        crate::db::apply_migrations(&conn).expect("migrations");
        DraftFixture {
            _tmp: tmp,
            data_dir,
            modules_dir,
            db_path,
            conn,
        }
    }

    const PAGE: &str = "<html><head></head><body>draft</body></html>";

    fn file_name_of(path: &Path) -> String {
        path.file_name()
            .and_then(|n| n.to_str())
            .expect("file name")
            .to_string()
    }

    #[test]
    fn derives_data_dir_from_modules_dir() {
        let data_dir = Path::new("/home/u/.natives");
        assert_eq!(
            data_dir_from_modules_dir(&data_dir.join("modules")),
            Some(data_dir)
        );
    }

    #[test]
    fn serves_an_explicitly_named_revision() {
        let f = fixture();
        store::create_draft(&f.conn, "draft-1", "App", "intent", None, None).expect("create");
        store::append_revision(&f.conn, &f.data_dir, "draft-1", PAGE).expect("rev1");

        let resolved = resolve_draft_file(&f.data_dir, &f.db_path, "draft-1/rev-1.html")
            .expect("revision resolves");
        assert!(resolved.is_file());
        assert_eq!(file_name_of(&resolved), "rev-1.html");
    }

    #[test]
    fn default_file_follows_the_database_pointer_not_the_newest_file() {
        let f = fixture();
        store::create_draft(&f.conn, "draft-1", "App", "intent", None, None).expect("create");
        store::append_revision(&f.conn, &f.data_dir, "draft-1", PAGE).expect("rev1");
        store::append_revision(&f.conn, &f.data_dir, "draft-1", PAGE).expect("rev2");

        let current = resolve_draft_file(&f.data_dir, &f.db_path, "draft-1").expect("current");
        assert_eq!(file_name_of(&current), "rev-2.html");

        // After an undo the rev-2 file still exists; the pointer is what decides.
        store::rollback(&f.conn, "draft-1").expect("rollback");
        let after = resolve_draft_file(&f.data_dir, &f.db_path, "draft-1/").expect("current");
        assert_eq!(file_name_of(&after), "rev-1.html");
    }

    #[test]
    fn default_file_is_absent_before_the_first_revision() {
        let f = fixture();
        store::create_draft(&f.conn, "draft-1", "App", "intent", None, None).expect("create");
        assert!(resolve_draft_file(&f.data_dir, &f.db_path, "draft-1").is_none());
    }

    #[test]
    fn rejects_traversal_out_of_the_draft_directory() {
        let f = fixture();
        store::create_draft(&f.conn, "draft-1", "App", "intent", None, None).expect("create");
        store::append_revision(&f.conn, &f.data_dir, "draft-1", PAGE).expect("rev1");

        for bad in [
            "draft-1/../../natives.db",
            "draft-1/../draft-1/rev-1.html",
            "draft-1/sub/../../rev-1.html",
            "draft-1/a\0b",
            "../drafts/draft-1/rev-1.html",
        ] {
            assert!(
                resolve_draft_file(&f.data_dir, &f.db_path, bad).is_none(),
                "expected {bad:?} to be rejected"
            );
        }
    }

    #[test]
    fn unknown_draft_id_resolves_to_nothing() {
        let f = fixture();
        assert!(resolve_draft_file(&f.data_dir, &f.db_path, "draft-missing").is_none());
        assert!(resolve_draft_file(&f.data_dir, &f.db_path, "draft-missing/rev-1.html").is_none());
        // An id the validator refuses never reaches the filesystem either.
        assert!(resolve_draft_file(&f.data_dir, &f.db_path, "Draft_1/rev-1.html").is_none());
    }

    fn http_get(port: u16, path: &str) -> String {
        use std::io::{Read, Write};
        let mut stream =
            std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect to test server");
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
        )
        .expect("write request");
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).expect("read response");
        String::from_utf8_lossy(&raw).into_owned()
    }

    #[test]
    fn draft_preview_carries_the_same_csp_as_a_module() {
        let f = fixture();
        store::create_draft(&f.conn, "draft-1", "App", "intent", None, None).expect("create");
        store::append_revision(&f.conn, &f.data_dir, "draft-1", PAGE).expect("rev1");

        let token_manager = Arc::new(TokenManager::new(&f.conn));
        let mut server = HttpServer::new(f.modules_dir.clone(), token_manager, f.db_path.clone());
        let port = server.start(0).expect("start server");

        let response = http_get(port, "/drafts/draft-1");
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(
            response.contains(&format!("Content-Security-Policy: {DRAFT_CSP}")),
            "draft preview must carry the module CSP verbatim: {response}"
        );
        assert!(response.contains("draft"), "body should be the revision");

        // Unresolvable drafts answer 404 without saying why.
        let missing = http_get(port, "/drafts/draft-missing/rev-1.html");
        assert!(missing.starts_with("HTTP/1.1 404"), "{missing}");
        let traversal = http_get(port, "/drafts/draft-1/../../natives.db");
        assert!(traversal.starts_with("HTTP/1.1 404"), "{traversal}");
    }

    /// CR-303: a local static preview URL is instance-scoped and revocable.
    /// While the runtime is active the tokenized URL serves 200 (with a base
    /// href anchoring relative subresources); after stop the same URL is gone.
    #[test]
    fn local_project_static_url_is_revoked_after_stop() {
        use crate::creative_app::model::CreativeAppSource;
        let f = fixture();
        let app_id = "loc-static";
        let root = f.data_dir.join("proj");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("index.html"),
            "<html><head></head><body>hi</body></html>",
        )
        .unwrap();
        f.conn
            .execute(
                "INSERT INTO local_creative_apps
                    (id, title, canonical_project_root, device_id, device_name, project_kind,
                     launch_mode, launch_plan_json, plan_fingerprint, state, auto_open,
                     startup_timeout_ms, created_at, updated_at)
                 VALUES ('loc-static','S',?1,'d','n','html','smart','{}','','running',1,60000,'t','t')",
                rusqlite::params![root.to_string_lossy().to_string()],
            )
            .unwrap();
        let app = crate::creative_app::runtime_store::find_or_create_application(
            &f.conn,
            CreativeAppSource::LocalProject,
            app_id,
        )
        .unwrap();
        let iid =
            crate::creative_app::runtime_store::create_instance(&f.conn, &app, None, "host_http")
                .unwrap();
        crate::creative_app::runtime_store::mark_running(&f.conn, &iid, &[], None, None, None)
            .unwrap();

        let token_manager = Arc::new(TokenManager::new(&f.conn));
        let mut server = HttpServer::new(f.modules_dir.clone(), token_manager, f.db_path.clone());
        let port = server.start(0).expect("start server");

        let token_url = format!("/local-projects/{iid}/{app_id}/");
        let ok = http_get(port, &token_url);
        assert!(ok.starts_with("HTTP/1.1 200"), "{ok}");
        assert!(
            ok.contains("<base href="),
            "HTML must carry the tokenized base href: {ok}"
        );

        // After stop the same URL is dead.
        crate::creative_app::runtime_store::mark_stopped(&f.conn, &iid).unwrap();
        let gone = http_get(port, &token_url);
        assert!(
            gone.starts_with("HTTP/1.1 410"),
            "stopped run URL must be gone: {gone}"
        );
    }

    /// MIG-004: the retired single-segment `/local-projects/{creativeId}/…`
    /// URL is never redirected — it answers 410 Gone like any other dead path,
    /// even while the app is running.
    #[test]
    fn legacy_local_url_is_gone_not_redirected() {
        use crate::creative_app::model::CreativeAppSource;
        let f = fixture();
        let app_id = "loc-legacy";
        let root = f.data_dir.join("proj2");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("index.html"),
            "<html><head></head><body>legacy</body></html>",
        )
        .unwrap();
        f.conn
            .execute(
                "INSERT INTO local_creative_apps
                    (id, title, canonical_project_root, device_id, device_name, project_kind,
                     launch_mode, launch_plan_json, plan_fingerprint, state, auto_open,
                     startup_timeout_ms, created_at, updated_at)
                 VALUES ('loc-legacy','L',?1,'d','n','html','smart','{}','','running',1,60000,'t','t')",
                rusqlite::params![root.to_string_lossy().to_string()],
            )
            .unwrap();
        let app = crate::creative_app::runtime_store::find_or_create_application(
            &f.conn,
            CreativeAppSource::LocalProject,
            app_id,
        )
        .unwrap();
        let iid =
            crate::creative_app::runtime_store::create_instance(&f.conn, &app, None, "host_http")
                .unwrap();
        crate::creative_app::runtime_store::mark_running(&f.conn, &iid, &[], None, None, None)
            .unwrap();

        let token_manager = Arc::new(TokenManager::new(&f.conn));
        let mut server = HttpServer::new(f.modules_dir.clone(), token_manager, f.db_path.clone());
        let port = server.start(0).expect("start server");

        // The tokenized URL still serves while the app is running…
        let ok = http_get(port, &format!("/local-projects/{iid}/{app_id}/"));
        assert!(ok.starts_with("HTTP/1.1 200"), "{ok}");

        // …but the retired single-segment URL is gone, not a 302 redirect.
        let gone = http_get(port, &format!("/local-projects/{app_id}/"));
        assert!(
            gone.starts_with("HTTP/1.1 410"),
            "retired legacy URL must answer 410, not redirect: {gone}"
        );
        assert!(!gone.contains("Location:"), "no redirect header: {gone}");
    }

    // ── PREV-001: authorized /fs/{token}/{path} preview resources ──

    #[test]
    fn preview_fs_route_serves_authorized_sibling_with_preview_csp() {
        let f = fixture();
        let file = f.data_dir.join("page.html");
        std::fs::write(&file, "<html><body>preview</body></html>").unwrap();
        let token = crate::html_preview::register_preview_session(f.data_dir.clone());

        let token_manager = Arc::new(TokenManager::new(&f.conn));
        let mut server = HttpServer::new(f.modules_dir.clone(), token_manager, f.db_path.clone());
        let port = server.start(0).expect("start server");

        let response = http_get(port, &format!("/fs/{token}/page.html"));
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(
            response.contains(&format!("Content-Security-Policy: {PREVIEW_CSP}")),
            "preview resource must carry the preview CSP: {response}"
        );
        assert!(
            response.contains("preview"),
            "body should be the served file"
        );
    }

    #[test]
    fn preview_fs_route_rejects_unknown_or_revoked_session() {
        let f = fixture();
        let file = f.data_dir.join("x.png");
        std::fs::write(&file, "png").unwrap();

        let token_manager = Arc::new(TokenManager::new(&f.conn));
        let mut server = HttpServer::new(f.modules_dir.clone(), token_manager, f.db_path.clone());
        let port = server.start(0).expect("start server");

        // Unknown token: uniform 404, no probing signal.
        let unknown = http_get(port, &format!("/fs/deadbeefdeadbeefdeadbeefdeadbeef/x.png"));
        assert!(unknown.starts_with("HTTP/1.1 404"), "{unknown}");

        // Explicitly revoked session dies too.
        let token = crate::html_preview::register_preview_session(f.data_dir.clone());
        crate::html_preview::revoke_preview_session(&token);
        let revoked = http_get(port, &format!("/fs/{token}/x.png"));
        assert!(revoked.starts_with("HTTP/1.1 404"), "{revoked}");
    }

    #[test]
    fn preview_fs_route_rejects_traversal_and_non_hex_token() {
        let f = fixture();
        let secret = f.data_dir.join("secret.txt");
        std::fs::write(&secret, "top secret").unwrap();
        let token = crate::html_preview::register_preview_session(f.data_dir.clone());

        let token_manager = Arc::new(TokenManager::new(&f.conn));
        let mut server = HttpServer::new(f.modules_dir.clone(), token_manager, f.db_path.clone());
        let port = server.start(0).expect("start server");

        for bad in [
            format!("/fs/{token}/../secret.txt"),
            format!("/fs/{token}/../../etc/passwd"),
            format!("/fs/{token}/sub/../../secret.txt"),
            format!("/fs/{token}/%2e%2e/secret.txt"),
            format!("/fs/not-hex-token/secret.txt"),
        ] {
            let response = http_get(port, &bad);
            assert!(
                response.starts_with("HTTP/1.1 403") || response.starts_with("HTTP/1.1 404"),
                "expected 403/404 for {bad}: {response}"
            );
            assert!(
                !response.contains("top secret"),
                "secret leaked via {bad}: {response}"
            );
        }
    }

    #[test]
    fn preview_fs_route_rejects_escape_outside_base_and_blocklist() {
        let f = fixture();
        let token = crate::html_preview::register_preview_session(f.data_dir.clone());

        let token_manager = Arc::new(TokenManager::new(&f.conn));
        let mut server = HttpServer::new(f.modules_dir.clone(), token_manager, f.db_path.clone());
        let port = server.start(0).expect("start server");

        // A file physically outside the base dir is unreachable even though the
        // token is valid (containment is the boundary).
        let outside = std::env::temp_dir().join(format!(
            "natives-preview-outside-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("escape.png"), "png").unwrap();
        let abs = outside.to_string_lossy().to_string();
        let escaped = http_get(port, &format!("/fs/{token}/{abs}/escape.png"));
        assert!(escaped.starts_with("HTTP/1.1 403"), "{escaped}");
        let _ = std::fs::remove_dir_all(&outside);

        // Blocklisted dotfiles inside the base dir are rejected by the kernel.
        let ssh = f.data_dir.join(".ssh");
        std::fs::create_dir_all(&ssh).unwrap();
        std::fs::write(ssh.join("config"), "Host *").unwrap();
        let blocked = http_get(port, &format!("/fs/{token}/.ssh/config"));
        assert!(blocked.starts_with("HTTP/1.1 403"), "{blocked}");
    }

    #[test]
    fn preview_fs_route_supports_head_requests() {
        let f = fixture();
        let file = f.data_dir.join("a.css");
        std::fs::write(&file, "body{}").unwrap();
        let token = crate::html_preview::register_preview_session(f.data_dir.clone());

        let token_manager = Arc::new(TokenManager::new(&f.conn));
        let mut server = HttpServer::new(f.modules_dir.clone(), token_manager, f.db_path.clone());
        let port = server.start(0).expect("start server");

        use std::io::{Read, Write};
        let mut stream =
            std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect to test server");
        write!(
            stream,
            "HEAD /fs/{token}/a.css HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
        )
        .expect("write request");
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).expect("read response");
        let response = String::from_utf8_lossy(&raw);
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(response.contains("Content-Length"), "{response}");
    }

    #[test]
    fn base_href_is_injected_before_head_close() {
        let html = "<html><head><title>x</title></head><body>a</body></html>";
        let out = inject_base_href(html, "/local-projects/rt/cid/");
        assert!(out.contains("<base href=\"/local-projects/rt/cid/\">"));
        let base_pos = out.find("<base").unwrap();
        let head_close = out.find("</head>").unwrap();
        assert!(base_pos < head_close, "base must live inside <head>");
    }

    // ── CR-402: HTTP security — body limits, CSP partitioning, host validation ──

    #[allow(dead_code)] // 测试辅助：保留供后续 bridge 测试使用
    fn http_post(port: u16, path: &str, body: &str, extra_headers: &[&str]) -> String {
        use std::io::{Read, Write};
        let mut stream =
            std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect to test server");
        let mut req = format!(
            "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\nConnection: close\r\n",
            body.len()
        );
        for h in extra_headers {
            req.push_str(h);
            req.push_str("\r\n");
        }
        req.push_str("\r\n");
        req.push_str(body);
        write!(stream, "{req}").expect("write request");
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).expect("read response");
        String::from_utf8_lossy(&raw).into_owned()
    }

    #[test]
    fn host_validation_rejects_non_loopback() {
        assert!(!validate_host("example.com"));
        assert!(!validate_host("evil.com:80"));
        assert!(!validate_host("192.168.1.1"));
        assert!(validate_host("127.0.0.1"));
        assert!(validate_host("127.0.0.1:3000"));
        assert!(validate_host("localhost"));
        assert!(validate_host("localhost:5173"));
        assert!(validate_host("[::1]"));
        assert!(validate_host("[::1]:3000"));
    }

    #[test]
    fn origin_validation_rejects_empty_origin_and_referer() {
        // POST without Origin or Referer must be rejected
        assert!(!validate_origin(&None, &None));
        // Loopback origin is accepted
        assert!(validate_origin(
            &Some("http://127.0.0.1:3000".into()),
            &None
        ));
        // External origin is rejected
        assert!(!validate_origin(&Some("https://evil.com".into()), &None));
    }

    #[test]
    fn csp_is_partitioned_per_domain() {
        // Verify that the partitioned CSP constants are distinct and have the
        // expected properties.
        // Workshop CSP: no external connect-src, no eval. P0-013: modules are
        // displayed inside a sandboxed iframe, so `frame-ancestors 'none'`
        // would contradict the display model — it must NOT forbid framing.
        assert!(
            !WORKSHOP_CSP.contains("frame-ancestors 'none'"),
            "workshop CSP must allow the sandboxed iframe display model: {WORKSHOP_CSP}"
        );
        assert!(
            !WORKSHOP_CSP.contains("unsafe-eval"),
            "workshop CSP must not allow eval: {WORKSHOP_CSP}"
        );
        assert!(
            !WORKSHOP_CSP.contains("connect-src https:"),
            "workshop CSP must not allow external connect: {WORKSHOP_CSP}"
        );

        // Draft CSP: same as Workshop (defense-in-depth)
        assert_eq!(
            WORKSHOP_CSP, DRAFT_CSP,
            "draft CSP must be identical to workshop CSP"
        );

        // Preview CSP: same strictness as Workshop (unreviewed local HTML must
        // not get a weaker CSP), and distinct so domains stay partitioned.
        assert_eq!(
            PREVIEW_CSP, WORKSHOP_CSP,
            "preview CSP must be as strict as workshop CSP"
        );
        assert!(
            !PREVIEW_CSP.contains("frame-ancestors 'none'"),
            "preview CSP must allow the sandboxed iframe display model: {PREVIEW_CSP}"
        );
        assert!(
            !PREVIEW_CSP.contains("unsafe-eval"),
            "preview CSP must not allow eval: {PREVIEW_CSP}"
        );
        assert!(
            !PREVIEW_CSP.contains("connect-src https:"),
            "preview CSP must not allow external connect: {PREVIEW_CSP}"
        );

        // Local Project CSP: allows loopback WS for Vite HMR
        assert!(
            LOCAL_PROJECT_CSP.contains("ws://127.0.0.1:*"),
            "local project CSP must allow loopback WS: {LOCAL_PROJECT_CSP}"
        );

        // Bridge CSP: default-src 'none' (pure JSON API)
        assert!(
            BRIDGE_CSP.contains("default-src 'none'"),
            "bridge CSP must have default-src none: {BRIDGE_CSP}"
        );
    }

    #[test]
    fn bridge_body_limit_returns_413_when_exceeded() {
        // Create a fixture with a running server, then POST an oversized body.
        let f = fixture();
        let token_manager = Arc::new(TokenManager::new(&f.conn));
        let mut server = HttpServer::new(f.modules_dir.clone(), token_manager, f.db_path.clone());
        let port = server.start(0).expect("start server");

        // Build a body that exceeds the bridge limit
        let oversized = "x".repeat((MAX_BRIDGE_BODY + 1) as usize);
        // Content-Length check: send with declared oversized length
        use std::io::{Read, Write};
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect");
        let req = format!(
            "POST /api/bridge/settings/getTheme HTTP/1.1\r\n\
             Host: 127.0.0.1\r\n\
             Origin: http://127.0.0.1:{port}\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\
             \r\n",
            oversized.len()
        );
        write!(stream, "{req}").expect("write request");
        // Write only the first few bytes of the body — the server should
        // reject based on Content-Length before reading the full body.
        write!(stream, "{}", &oversized[..100]).expect("write partial body");
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).expect("read response");
        let response = String::from_utf8_lossy(&raw);
        assert!(
            response.starts_with("HTTP/1.1 413"),
            "oversized bridge body must return 413: {response}"
        );
    }

    #[test]
    fn validate_host_rejects_ipv4_foreign() {
        assert!(!validate_host("10.0.0.1"));
        assert!(!validate_host("172.16.0.1"));
        assert!(!validate_host("192.168.1.1"));
        assert!(!validate_host("0.0.0.0"));
    }

    #[test]
    fn validate_host_strips_port_correctly() {
        assert!(validate_host("127.0.0.1:9999"));
        assert!(validate_host("localhost:65535"));
        assert!(!validate_host("evil.com:443"));
    }
