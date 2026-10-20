//! Isolated TLS-over-TLS CONNECT fixture. No system trust changes or real credentials.
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{mpsc, Arc},
    time::Duration,
};
const CERT: &[u8] = include_bytes!("fixtures/network/test-server.der");
const KEY: &[u8] = include_bytes!("fixtures/network/test-server-key.der");
const CA: &[u8] = include_bytes!("fixtures/network/test-ca.der");
fn headers(stream: &mut impl Read) -> std::io::Result<String> {
    let mut bytes = vec![];
    while !bytes.ends_with(b"\r\n\r\n") && bytes.len() < 8192 {
        let mut b = [0];
        stream.read_exact(&mut b)?;
        bytes.push(b[0]);
    }
    String::from_utf8(bytes).map_err(|_| std::io::Error::other("bad_header"))
}
fn server() -> Arc<rustls::ServerConfig> {
    Arc::new(
        rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![rustls::pki_types::CertificateDer::from(CERT.to_vec())],
            rustls::pki_types::PrivateKeyDer::Pkcs8(rustls::pki_types::PrivatePkcs8KeyDer::from(
                KEY.to_vec(),
            )),
        )
        .unwrap(),
    )
}
#[test]
fn https_connect_auth_and_upstream_tls_are_both_verified() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let (socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        socket
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut outer =
            rustls::StreamOwned::new(rustls::ServerConnection::new(server()).unwrap(), socket);
        let connect = headers(&mut outer).unwrap();
        tx.send(connect).unwrap();
        outer
            .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
            .unwrap();
        outer.flush().unwrap();
        let mut inner =
            rustls::StreamOwned::new(rustls::ServerConnection::new(server()).unwrap(), outer);
        let request = headers(&mut inner).unwrap();
        assert!(request.starts_with("GET /probe HTTP/1.1"));
        assert!(!request.to_ascii_lowercase().contains("proxy-authorization"));
        inner
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")
            .unwrap();
        inner.flush().unwrap();
    });
    let proxy = ureq::Proxy::builder(ureq::ProxyProtocol::Https)
        .host("localhost")
        .port(port)
        .username("fixture-user")
        .password("fixture-password")
        .resolve_target(true)
        .build()
        .unwrap();
    let tls = ureq::tls::TlsConfig::builder()
        .root_certs(ureq::tls::RootCerts::new_with_certs(&[
            ureq::tls::Certificate::from_der(CA),
        ]))
        .build();
    let config = ureq::Agent::config_builder()
        .proxy(Some(proxy))
        .https_only(true)
        .max_redirects(0)
        .timeout_global(Some(Duration::from_secs(5)))
        .tls_config(tls)
        .build();
    let mut response = ureq::Agent::new_with_config(config)
        .get("https://localhost/probe")
        .call()
        .unwrap();
    assert_eq!(response.body_mut().read_to_string().unwrap(), "OK");
    let connect = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(connect.starts_with("CONNECT "));
    assert!(connect
        .to_ascii_lowercase()
        .contains("proxy-authorization: basic "));
    assert!(!connect.contains("fixture-password"));
    handle.join().unwrap();
}
#[test]
fn untrusted_proxy_certificate_is_rejected() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        let (socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut outer =
            rustls::StreamOwned::new(rustls::ServerConnection::new(server()).unwrap(), socket);
        assert!(headers(&mut outer).is_err());
    });
    let proxy = ureq::Proxy::builder(ureq::ProxyProtocol::Https)
        .host("localhost")
        .port(port)
        .build()
        .unwrap();
    let config = ureq::Agent::config_builder()
        .proxy(Some(proxy))
        .timeout_global(Some(Duration::from_secs(5)))
        .build();
    assert!(ureq::Agent::new_with_config(config)
        .get("https://localhost/probe")
        .call()
        .is_err());
    handle.join().unwrap();
}

/// Runs the real channel adapters and delivery bookkeeping against a local TLS
/// fixture. Only the test executor rewrites the authority; production DNS/SSRF
/// policy is never changed and no external destination receives this payload.
#[test]
fn nine_channel_tls_wire_delivery_and_failure_matrix() {
    use fund_module::{channels, storage::Store};
    use serde_json::json;
    let store = Store::open_in_memory().unwrap();
    let policy = channels::SendPolicy {
        silence_start: None,
        silence_end: None,
        retry_max: 0,
        backoff_base: 1,
        dedup_minutes: 60,
    };
    for kind in channels::CHANNEL_KINDS {
        let config = match *kind {
            "telegram" => json!({"chat_id":"42"}),
            "discord" => json!({"webhook_id":"123456"}),
            "dingtalk" => json!({"keyword":"review","phones":"13800138000"}),
            _ => json!({}),
        };
        let mut secrets = json!({});
        for field in channels::secret_fields(kind) {
            secrets[*field] = json!("fixture-secret");
        }
        let request = channels::build_request(
            kind,
            &config,
            &secrets,
            "a&b=测试",
            "Review facts + evidence",
        )
        .unwrap();
        let id = channels::save(&store, &json!({"kind":kind,"name":kind,"config":config})).unwrap()
            ["id"]
            .as_i64()
            .unwrap();
        let success = match *kind {
            "telegram" => "{\"ok\":true}",
            "bark" | "pushplus" => "{\"code\":200}",
            "dingtalk" | "wecom" => "{\"errcode\":0}",
            "feishu" | "serverchan" => "{\"code\":0}",
            "pushover" => "{\"status\":1}",
            "discord" => "",
            _ => unreachable!(),
        };
        for (case, http, response, expected) in [
            (
                "ok",
                if *kind == "discord" { 204 } else { 200 },
                success,
                "sent",
            ),
            (
                "business-reject",
                if *kind == "discord" { 400 } else { 200 },
                "{}",
                "failed",
            ),
            (
                "rate-limit",
                429,
                "{\"error\":\"fixture-secret\"}",
                "failed",
            ),
            ("timeout", 0, "", "failed"),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let expected_body = request.body.clone();
            let original = url::Url::parse(&request.url).unwrap();
            let path = format!(
                "{}{}",
                original.path(),
                original
                    .query()
                    .map(|q| format!("?{q}"))
                    .unwrap_or_default()
            );
            let expected_path = path.clone();
            let content_type = request
                .headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
                .unwrap()
                .1
                .clone();
            let handle = std::thread::spawn(move || {
                let (socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                socket
                    .set_write_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut tls = rustls::StreamOwned::new(
                    rustls::ServerConnection::new(server()).unwrap(),
                    socket,
                );
                let h = headers(&mut tls).unwrap();
                assert!(h.starts_with(&format!("POST {expected_path} HTTP/1.1")));
                assert!(h
                    .to_ascii_lowercase()
                    .contains(&format!("content-type: {content_type}")));
                let mut body = vec![0; expected_body.len()];
                tls.read_exact(&mut body).unwrap();
                assert_eq!(body, expected_body.as_bytes());
                if http == 0 {
                    std::thread::sleep(Duration::from_millis(500));
                    return;
                }
                tls.write_all(format!("HTTP/1.1 {http} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",response.len()).as_bytes()).unwrap();
                tls.flush().unwrap();
            });
            let tls = ureq::tls::TlsConfig::builder()
                .root_certs(ureq::tls::RootCerts::new_with_certs(&[
                    ureq::tls::Certificate::from_der(CA),
                ]))
                .build();
            let agent = ureq::Agent::new_with_config(
                ureq::Agent::config_builder()
                    .proxy(None)
                    .max_redirects(0)
                    .http_status_as_error(false)
                    .timeout_global(Some(if http == 0 {
                        Duration::from_millis(300)
                    } else {
                        Duration::from_secs(5)
                    }))
                    .tls_config(tls)
                    .build(),
            );
            let result = channels::send(
                &store,
                kind,
                &request,
                id,
                &format!("{kind}:{case}"),
                &policy,
                false,
                false,
                || false,
                |r| {
                    let mut call = agent.post(format!("https://localhost:{port}{path}"));
                    for (key, value) in &r.headers {
                        call = call.header(key, value);
                    }
                    let mut response = call
                        .send(r.body.as_str())
                        .map_err(|_| "fixture_transport_error".to_string())?;
                    let status = response.status().as_u16();
                    let text = response
                        .body_mut()
                        .read_to_string()
                        .map_err(|_| "fixture_body_error".to_string())?;
                    Ok((status, text))
                },
            )
            .unwrap();
            handle.join().unwrap();
            assert_eq!(result["status"], expected, "{kind}/{case}");
        }
        let duplicate = channels::send(
            &store,
            kind,
            &request,
            id,
            &format!("{kind}:ok"),
            &policy,
            false,
            false,
            || false,
            |_| panic!("duplicate must not send"),
        )
        .unwrap();
        assert_eq!(duplicate["status"], "skipped_dedup");
        let cancelled = channels::send(
            &store,
            kind,
            &request,
            id,
            &format!("{kind}:cancel"),
            &policy,
            false,
            false,
            || true,
            |_| panic!("cancelled must not send"),
        )
        .unwrap();
        assert_eq!(cancelled["status"], "cancelled");
        assert!(!channels::deliveries(&store, Some(id), 1)
            .unwrap()
            .to_string()
            .contains("fixture-secret"));
    }
}
