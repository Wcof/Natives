use super::auth::AuthManager;
use super::static_files::{mime_type_for, serve_static_file};
use super::{default_port, DEFAULT_DEV_PORT};

#[test]
fn default_port_in_debug_matches_specification() {
    assert_eq!(default_port(), DEFAULT_DEV_PORT);
}

#[test]
fn auth_manager_bootstrap_token_lifecycle() {
    let mut auth = AuthManager::new(Some("test-bootstrap-token".into()));
    assert_eq!(auth.current_bootstrap_token(), Some("test-bootstrap-token"));

    // 错误 Token 拒绝
    assert!(auth.exchange_bootstrap("wrong-token").is_err());

    // 正确 Token 兑换成功并生成会话与 CSRF Token
    let (session_id, csrf_token) = auth.exchange_bootstrap("test-bootstrap-token").unwrap();
    assert!(!session_id.is_empty());
    assert!(!csrf_token.is_empty());

    // 一次性消费：再次使用同一 Token 必须拒绝
    assert!(auth.exchange_bootstrap("test-bootstrap-token").is_err());

    // 会话与 CSRF 有效性核验
    assert!(auth.validate_session(&session_id));
    assert!(auth.validate_csrf(&session_id, &csrf_token));
    assert!(!auth.validate_csrf(&session_id, "wrong-csrf"));

    // 撤销会话
    auth.revoke_session(&session_id);
    assert!(!auth.validate_session(&session_id));
}

#[test]
fn auth_manager_pairing_code_lockout() {
    let mut auth = AuthManager::new(None);
    let code = auth.generate_pairing_code().unwrap();
    assert_eq!(code.len(), 10);

    // 连续错误尝试，第 5 次锁定
    for _ in 0..4 {
        assert_eq!(
            auth.exchange_pairing_code("WRONGCODE1"),
            Err("ERR_PAIRING_CODE_MISMATCH")
        );
    }
    assert_eq!(
        auth.exchange_pairing_code("WRONGCODE1"),
        Err("ERR_PAIRING_CODE_LOCKED")
    );
}

#[test]
fn auth_manager_surface_lease_and_revocation() {
    let mut auth = AuthManager::new(Some("seed-token".into()));
    let (session_id, _) = auth.exchange_bootstrap("seed-token").unwrap();

    assert_eq!(auth.active_surface_count(), 0);

    // 注册两个 Surface
    let surf1 = auth
        .register_surface(&session_id, Some("fund".into()))
        .unwrap();
    let _surf2 = auth
        .register_surface(&session_id, Some("tokenusage".into()))
        .unwrap();
    assert_eq!(auth.active_surface_count(), 2);

    // 续约 Surface
    assert!(auth.renew_surface(&session_id, &surf1).is_ok());

    // 跨会话撤销拒绝
    assert_eq!(
        auth.revoke_surface("other-session", &surf1),
        Err("ERR_SURFACE_NOT_OWNED")
    );

    // 正确撤销 Surface 1，Surface 2 继续保持
    assert!(auth.revoke_surface(&session_id, &surf1).is_ok());
    assert_eq!(auth.active_surface_count(), 1);

    // 撤销整个会话连带清除所有 Surface
    auth.revoke_session(&session_id);
    assert_eq!(auth.active_surface_count(), 0);
}

#[test]
fn app_surface_cannot_cross_session_or_module() {
    let mut auth = AuthManager::new(Some("surface-owner-test".into()));
    let (first, _) = auth.exchange_bootstrap("surface-owner-test").unwrap();
    let bootstrap = auth.generate_bootstrap_token().unwrap();
    let (second, _) = auth.exchange_bootstrap(&bootstrap).unwrap();
    assert!(auth.exchange_bootstrap(&bootstrap).is_err());
    let surface = auth.register_surface(&first, Some("fund".into())).unwrap();
    assert!(!auth.authorize_app_surface(&second, &surface, Some("fund")));
    assert!(!auth.authorize_app_surface(&first, &surface, Some("tokenusage")));
    assert!(auth.authorize_app_surface(&first, &surface, Some("fund")));
    assert_eq!(
        auth.revoke_surface(&second, &surface),
        Err("ERR_SURFACE_NOT_OWNED")
    );
    assert_eq!(auth.revoke_surface(&first, &surface), Ok(true));
    assert_eq!(auth.revoke_surface(&first, &surface), Ok(false));
}

#[test]
fn static_files_mime_types() {
    assert_eq!(mime_type_for("index.html"), "text/html; charset=utf-8");
    assert_eq!(
        mime_type_for("app.js"),
        "application/javascript; charset=utf-8"
    );
    assert_eq!(mime_type_for("theme.css"), "text/css; charset=utf-8");
    assert_eq!(mime_type_for("icon.svg"), "image/svg+xml");
    assert_eq!(
        mime_type_for("data.json"),
        "application/json; charset=utf-8"
    );
}

#[test]
fn static_files_path_traversal_rejection() {
    // 包含 .. 的路径直接拒绝
    assert!(serve_static_file("../Cargo.toml").is_none());
    assert!(serve_static_file("foo/../../etc/passwd").is_none());
    assert!(serve_static_file("..\\Cargo.toml").is_none());
}

pub(super) fn make_test_state(port: u16) -> std::sync::Arc<super::server::ServiceState> {
    std::sync::Arc::new(super::server::ServiceState::new(
        port,
        "test-epoch".into(),
        "test-relay-secret-123".into(),
        Some("test-bootstrap".into()),
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        None,
        None,
    ))
}

fn request(
    state: std::sync::Arc<super::server::ServiceState>,
    method: &str,
    route: &str,
    body: &str,
    headers: &[(&str, &str)],
    authenticated: bool,
) -> String {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::time::Duration;

    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let cookie = if authenticated {
        let (id, _) = state
            .auth
            .lock()
            .unwrap()
            .exchange_bootstrap("test-bootstrap")
            .unwrap();
        format!("Cookie: natives_dev_session={id}\r\n")
    } else {
        String::new()
    };
    let handler = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        super::server::handle_client(stream, state);
    });
    let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(
        client,
        "{method} {route} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{cookie}Content-Length: {}\r\n",
        body.len()
    )
    .unwrap();
    for (key, value) in headers {
        write!(client, "{key}: {value}\r\n").unwrap();
    }
    write!(client, "Connection: close\r\n\r\n{body}").unwrap();
    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();
    handler.join().unwrap();
    response
}

#[test]
fn unauthenticated_native_relay_must_reject() {
    let state = make_test_state(0);
    let response = request(
        state,
        "POST",
        "/native/relay",
        r#"{"id":"audit","method":"version","params":{}}"#,
        &[
            ("Origin", "https://untrusted.invalid"),
            ("Content-Type", "text/plain"),
        ],
        false,
    );
    assert!(
        response.starts_with("HTTP/1.1 403"),
        "unpaired cross-origin caller must be rejected with 403"
    );
}

#[test]
fn replacing_cookie_revokes_only_its_old_session_and_surfaces() {
    let state = make_test_state(0);
    let (old, surface, other, bootstrap) = {
        let mut auth = state.auth.lock().unwrap();
        let (old, _) = auth.exchange_bootstrap("test-bootstrap").unwrap();
        let surface = auth.register_surface(&old, None).unwrap();
        let token = auth.generate_bootstrap_token().unwrap();
        let (other, _) = auth.exchange_bootstrap(&token).unwrap();
        let bootstrap = auth.generate_bootstrap_token().unwrap();
        (old, surface, other, bootstrap)
    };
    let cookie = format!("natives_dev_session={old}");
    let body = serde_json::json!({"bootstrapToken":bootstrap}).to_string();
    let response = request(
        state.clone(),
        "POST",
        "/web/session/exchange",
        &body,
        &[
            ("Origin", "http://127.0.0.1:0"),
            ("Content-Type", "application/json"),
            ("Cookie", &cookie),
        ],
        false,
    );
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    let mut auth = state.auth.lock().unwrap();
    assert!(!auth.validate_session(&old));
    assert!(!auth.owns_active_surface(&old, &surface));
    assert!(auth.validate_session(&other));
}

#[test]
fn authenticated_sse_must_exist() {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::time::Duration;

    let state = make_test_state(0);
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let (id, _) = state
        .auth
        .lock()
        .unwrap()
        .exchange_bootstrap("test-bootstrap")
        .unwrap();
    let state_clone = std::sync::Arc::clone(&state);
    let handler = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        super::server::handle_client(stream, state_clone);
    });
    let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    write!(
        client,
        "GET /web/events HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nCookie: natives_dev_session={id}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut buf = [0u8; 256];
    let n = client.read(&mut buf).unwrap();
    let response = String::from_utf8_lossy(&buf[..n]);
    assert!(
        response.contains("text/event-stream"),
        "authenticated SSE route must return text/event-stream"
    );
    state
        .shutdown_signal
        .store(true, std::sync::atomic::Ordering::SeqCst);
    state.broadcast_event("{\"type\":\"shutdown\"}");
    let _ = handler.join();
}

#[test]
fn surface_mutation_without_csrf_must_reject() {
    let state = make_test_state(0);
    let response = request(
        state,
        "POST",
        "/web/surfaces",
        r#"{"appId":"fund"}"#,
        &[("Content-Type", "application/json")],
        true,
    );
    assert!(
        response.starts_with("HTTP/1.1 403"),
        "surface creation without CSRF and valid Origin must reject with 403"
    );
}

#[test]
fn unknown_module_surface_must_reject() {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::time::Duration;

    let state = make_test_state(0);
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let (id, csrf) = state
        .auth
        .lock()
        .unwrap()
        .exchange_bootstrap("test-bootstrap")
        .unwrap();
    let state_clone = std::sync::Arc::clone(&state);
    let handler = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        super::server::handle_client(stream, state_clone);
    });
    let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let origin = format!("http://127.0.0.1:{port}");
    let body = r#"{"appId":"not-a-registered-module"}"#;
    write!(
        client,
        "POST /web/surfaces HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: {origin}\r\nX-CSRF-Token: {csrf}\r\nCookie: natives_dev_session={id}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();
    handler.join().unwrap();
    assert!(
        response.starts_with("HTTP/1.1 400"),
        "unknown appId surface must reject with 400"
    );
}

#[test]
fn rotating_one_surface_must_preserve_another() {
    let mut manager = app_runtime_core::session::SessionManager::new().unwrap();
    let s1 = manager
        .issue_for_surface("surface-a", "audit-first-surface-challenge-128")
        .unwrap();
    let s2 = manager
        .issue_for_surface("surface-b", "audit-second-surface-challenge-128")
        .unwrap();
    manager.rotate_surface("surface-b").unwrap();
    let result = manager.authorize(&s1.generation, &format!("Bearer {}", s1.token));
    assert!(
        result.is_ok(),
        "second shell createSandbox rotate must preserve first shell"
    );
    assert!(
        manager
            .authorize(&s2.generation, &format!("Bearer {}", s2.token))
            .is_err(),
        "rotation must invalidate its own old bearer"
    );
}

#[test]
fn test_check_service_health() {
    let state = make_test_state(0);
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let state_clone = std::sync::Arc::clone(&state);
    let handler = std::thread::spawn(move || {
        for _ in 0..2 {
            let (stream, _) = listener.accept().unwrap();
            super::server::handle_client(stream, std::sync::Arc::clone(&state_clone));
        }
    });
    assert!(!super::relay::check_service_health_with_secret(
        port,
        "wrong-secret"
    ));
    assert!(super::relay::check_service_health_with_secret(
        port,
        "test-relay-secret-123"
    ));
    state
        .shutdown_signal
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let _ = handler.join();
}

#[test]
fn slow_sse_consumer_is_removed_when_queue_fills() {
    let state = make_test_state(0);
    let (sender, _receiver) = std::sync::mpsc::sync_channel(1);
    state.sse_senders.lock().unwrap().push((1, sender, None));
    state.broadcast_event("first");
    state.broadcast_event("second");
    assert!(state.sse_senders.lock().unwrap().is_empty());
}

#[test]
fn targeted_event_reaches_only_the_owning_surface() {
    let state = make_test_state(0);
    let (owner_tx, owner_rx) = std::sync::mpsc::sync_channel(2);
    let (other_tx, other_rx) = std::sync::mpsc::sync_channel(2);
    state
        .sse_senders
        .lock()
        .unwrap()
        .push((1, owner_tx, Some("surface-a".into())));
    state
        .sse_senders
        .lock()
        .unwrap()
        .push((2, other_tx, Some("surface-b".into())));
    state.broadcast_event_to("surface-a", r#"{"type":"task_progress","domain":"files"}"#);
    let owned = owner_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("owner event");
    assert!(owned.contains("task_progress"));
    assert!(other_rx
        .recv_timeout(std::time::Duration::from_millis(100))
        .is_err());
}
