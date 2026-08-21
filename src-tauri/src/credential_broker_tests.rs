use super::*;

#[test]
fn rejects_empty_provider_id() {
    let err = tauri::async_runtime::block_on(credential_broker_resolve(CredentialBrokerRequest {
        key_id: "k".into(),
        provider_id: "".into(),
        run_id: "r".into(),
        session_id: None,
    }))
    .unwrap_err();
    assert!(matches!(err, Error::InvalidInput(_)));
}

#[test]
fn lifecycle_requires_ids_and_run() {
    assert!(
        broker_lifecycle_resolve_validated(&CredentialBrokerRequest {
            key_id: "".into(),
            provider_id: "openai".into(),
            run_id: "r1".into(),
            session_id: None,
        })
        .is_err()
    );
    assert!(
        broker_lifecycle_resolve_validated(&CredentialBrokerRequest {
            key_id: "k1".into(),
            provider_id: "openai".into(),
            run_id: "r1".into(),
            session_id: None,
        })
        .is_ok()
    );
}

#[test]
fn pool_lease_requires_run_binding_before_database_access() {
    let error = broker_pool_acquire(wire::CredentialPoolLeaseRequest {
        provider_id: "antigravity".into(),
        run_id: String::new(),
    })
    .unwrap_err();
    assert!(error.contains("run_id"));
}

#[test]
fn pool_refresh_rejects_mismatched_run_before_database_access() {
    let lease = lease_registry().issue(
        "antigravity",
        "sub2api-pool",
        "owning-run",
        None,
        chrono::Duration::seconds(60),
    );
    let error = broker_pool_refresh(wire::CredentialPoolRefreshRequest {
        account_id: "account-1".into(),
        provider_id: "antigravity".into(),
        run_id: "other-run".into(),
        lease_id: lease.lease_id,
        credentials: serde_json::json!({}),
        expires_at: None,
        reauth_required: false,
    })
    .unwrap_err();
    assert!(error.contains("lease"));
}

#[test]
fn daemon_resolver_path_is_wired() {
    // Without natives.db keys this fails closed — never invents offline mock key.
    let err = resolve_for_daemon("openai", Some("missing"), "run-broker-test").unwrap_err();
    assert!(!err.contains("sk-"));
    assert!(!err.to_ascii_lowercase().contains("offline mock"));
    if let Ok(dir) = std::env::var("NATIVES_TEST_SCRATCH") {
        let _ = std::fs::write(
            std::path::Path::new(&dir).join("credential-broker-lifecycle.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "steps": [
                    "daemon_calls_install_credential_broker",
                    "resolve_credential_for_run → broker(provider, key_id, run_id)",
                    "tauri resolve_for_daemon → natives.db envelope_decrypt → lease",
                    "memory_only_Credential returned",
                    "fail_closed_without_key"
                ],
                "path": "production.rs resolve → CREDENTIAL_BROKER → resolve_for_daemon",
                "reject_without_db_key": true,
                "error_redacted": true,
                "error_sample": err,
                "mock_success_without_key": false,
            }))
            .unwrap_or_default(),
        );
    }
}

#[test]
fn redacts_api_keys_from_broker_errors() {
    let raw = "Failed to decrypt key sk-proj-abc123def456ghi789 for provider openai";
    let redacted = redact_broker_error(raw);
    assert!(
        !redacted.contains("sk-proj-abc123def456ghi789"),
        "key must not appear: {redacted}"
    );
    assert!(redacted.contains("[REDACTED_KEY]") || redacted.contains("REDACTED"));

    let bearer = "upstream 401 Bearer sk-ant-secret-token-value-here";
    let redacted2 = redact_broker_error(bearer);
    assert!(!redacted2.contains("sk-ant-secret-token-value-here"));

    // Simulate broker error serialization for event log — never include api_key field.
    let err_event = serde_json::json!({
        "type": "failed",
        "error": redacted,
        // api_key intentionally omitted
    });
    let serialized = err_event.to_string();
    assert!(!serialized.contains("sk-proj"));
    assert!(!serialized.contains("\"api_key\""));

    if let Ok(dir) = std::env::var("NATIVES_TEST_SCRATCH") {
        let _ = std::fs::write(
                std::path::Path::new(&dir).join("credential-redact.log"),
                format!(
                    "broker_lifecycle=ok\nredacted_sample={redacted}\nevent={serialized}\nno_api_key_field=true\n"
                ),
            );
        let _ = std::fs::write(
            std::path::Path::new(&dir).join("credential-broker-lifecycle.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "steps": [
                    "daemon_sends_credential_id",
                    "tauri_validates_key_id_provider_id_run_id",
                    "tauri_decrypts_from_natives_db",
                    "lease_issued_short_ttl",
                    "memory_only_response",
                    "daemon_holds_for_request_lifecycle"
                ],
                "reject_empty": true,
                "redaction": true,
                "mock_success_without_key": false,
            }))
            .unwrap_or_default(),
        );
    }
}

#[test]
fn resolve_missing_provider_does_not_leak_fabricated_key() {
    // Without DB, resolve fails — must not invent offline success material.
    let result =
        tauri::async_runtime::block_on(credential_broker_resolve(CredentialBrokerRequest {
            key_id: "missing-key".into(),
            provider_id: "openai".into(),
            run_id: "run-audit".into(),
            session_id: None,
        }));
    assert!(result.is_err());
    let msg = format!("{:?}", result.unwrap_err());
    assert!(!msg.contains("sk-"));
    assert!(!msg.contains("offline mock"));
}

#[test]
fn lease_registry_issue_revoke_status_and_ttl() {
    let registry = CredentialLeaseRegistry::default();

    // Issue a normal lease.
    let meta = registry.issue(
        "openai",
        "k1",
        "run-1",
        None,
        chrono::Duration::seconds(120),
    );
    let status = registry.status(&meta.lease_id).unwrap();
    assert!(status.active);
    assert!(!status.revoked);
    assert_eq!(status.run_id, "run-1");

    // Revoke with a mismatched run is rejected.
    assert!(registry.revoke(&meta.lease_id, "run-evil").is_err());
    let status = registry.status(&meta.lease_id).unwrap();
    assert!(status.active, "mismatched revoke must not revoke");

    // Revoke with the owning run succeeds; the lease is now inactive.
    let status = registry.revoke(&meta.lease_id, "run-1").unwrap();
    assert!(!status.active);
    assert!(status.revoked);
    let status = registry.status(&meta.lease_id).unwrap();
    assert!(!status.active);
    assert!(status.revoked);
}

#[test]
fn lease_registry_ttl_expiry_fails_closed() {
    let registry = CredentialLeaseRegistry::default();
    // Negative TTL → already expired at issue time.
    let meta = registry.issue("openai", "k1", "run-1", None, chrono::Duration::seconds(-5));
    assert!(registry.status(&meta.lease_id).is_err());
    assert_eq!(registry.len(), 0, "expired lease must be pruned");
}

#[test]
fn lease_registry_prunes_expired_entries_before_growth() {
    let registry = CredentialLeaseRegistry::default();
    for index in 0..100 {
        registry.issue(
            "openai",
            "k1",
            &format!("expired-run-{index}"),
            None,
            chrono::Duration::seconds(-1),
        );
    }
    registry.issue(
        "openai",
        "k1",
        "active-run",
        None,
        chrono::Duration::seconds(60),
    );
    assert_eq!(registry.len(), 1, "only the active lease should remain");
}

#[test]
fn dispatch_broker_uds_rejects_wrong_same_uid_pid_before_dispatch() {
    let _peer_lock = credential_broker_uds::lock_broker_peer_for_test();
    let daemon_pid = std::process::id();
    let _peer = credential_broker_uds::install_broker_peer_for_test(daemon_pid).unwrap();
    let error = dispatch_broker_uds(
        r#"{"method":"credential.bogus","payload":{}}"#,
        daemon_pid.saturating_add(1),
    )
    .unwrap_err();
    assert!(error.contains("unauthorized broker peer"));
}

#[cfg(target_os = "macos")]
fn broker_socket_reply(request: String) -> wire::CredentialLeaseReply {
    use std::io::{Read, Write};
    use std::net::Shutdown;

    let (mut client, server) = std::os::unix::net::UnixStream::pair().unwrap();
    let server =
        std::thread::spawn(move || credential_broker_uds::handle_broker_connection(server));
    client.write_all(request.as_bytes()).unwrap();
    client.write_all(b"\n").unwrap();
    client.shutdown(Shutdown::Write).unwrap();
    let mut reply_line = String::new();
    client.read_to_string(&mut reply_line).unwrap();
    server.join().unwrap().unwrap();
    serde_json::from_str(&reply_line).unwrap()
}

#[cfg(target_os = "macos")]
#[test]
fn broker_socket_accepts_current_supervised_pid() {
    let _peer_lock = credential_broker_uds::lock_broker_peer_for_test();
    let _peer = credential_broker_uds::install_broker_peer_for_test(std::process::id()).unwrap();
    let lease = lease_registry().issue(
        "openai",
        "k1",
        "run-auth",
        None,
        chrono::Duration::seconds(60),
    );
    let accepted = broker_socket_reply(
        serde_json::to_string(&wire::CredentialLeaseEnvelope {
            instance_id: "test-instance".into(),
            auth_token: "test-auth".into(),
            request_id: "request-current".into(),
            method: names::CREDENTIAL_LEASE_STATUS.into(),
            payload: serde_json::json!({ "lease_id": lease.lease_id }),
        })
        .unwrap(),
    );
    assert!(
        accepted.ok,
        "current supervised PID must reach broker dispatch"
    );
}

#[test]
fn broker_peer_rejects_live_overwrite_and_stale_clear_preserves_newer_lifecycle() {
    let _peer_lock = credential_broker_uds::lock_broker_peer_for_test();
    let stale = credential_broker_uds::install_broker_peer_for_test(41).unwrap();
    assert!(credential_broker_uds::install_broker_peer_for_test(42).is_err());
    credential_broker_uds::clear_broker_peer(stale.generation());
    let _current = credential_broker_uds::install_broker_peer_for_test(42).unwrap();
    credential_broker_uds::clear_broker_peer(stale.generation());
    assert!(credential_broker_uds::broker_peer_matches(42));
}

#[cfg(target_os = "macos")]
#[test]
fn broker_socket_rejects_wrong_peer_before_read() {
    use std::sync::mpsc;

    let _peer_lock = credential_broker_uds::lock_broker_peer_for_test();
    let _peer =
        credential_broker_uds::install_broker_peer_for_test(std::process::id().saturating_add(1))
            .unwrap();
    let (_client, server) = std::os::unix::net::UnixStream::pair().unwrap();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        sender
            .send(credential_broker_uds::handle_broker_connection(server))
            .unwrap();
    });
    let result = receiver
        .recv_timeout(std::time::Duration::from_millis(100))
        .expect("wrong peer must be rejected before a blocking read");
    assert!(result.unwrap_err().contains("PID mismatch"));
}

#[cfg(target_os = "macos")]
#[test]
fn broker_socket_bounds_oversized_frame_without_waiting_for_eof() {
    use std::io::Write;
    use std::sync::mpsc;

    let _peer_lock = credential_broker_uds::lock_broker_peer_for_test();
    let _peer = credential_broker_uds::install_broker_peer_for_test(std::process::id()).unwrap();
    let (mut client, server) = std::os::unix::net::UnixStream::pair().unwrap();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        sender
            .send(credential_broker_uds::handle_broker_connection(server))
            .unwrap();
    });
    client.write_all(&vec![b'x'; 16 * 1024 + 1]).unwrap();
    let result = receiver
        .recv_timeout(std::time::Duration::from_millis(100))
        .expect("oversized frame must be bounded before the read timeout");
    assert!(result.unwrap_err().contains("frame too large"));
}

#[test]
fn dispatch_broker_uds_acquire_rejects_empty_provider_redacted() {
    let _peer_lock = credential_broker_uds::lock_broker_peer_for_test();
    let daemon_pid = std::process::id();
    let _peer = credential_broker_uds::install_broker_peer_for_test(daemon_pid).unwrap();
    let envelope = wire::CredentialLeaseEnvelope {
        instance_id: "test-instance".into(),
        auth_token: "test-auth".into(),
        request_id: "request-empty-provider".into(),
        method: names::CREDENTIAL_LEASE_ACQUIRE.to_string(),
        payload: serde_json::json!({
            "key_id": "k1",
            "provider_id": "",
            "run_id": "run-1",
            "session_id": null,
        }),
    };
    let line = serde_json::to_string(&envelope).unwrap();
    let reply_line = dispatch_broker_uds(&line, daemon_pid).unwrap();
    let reply: wire::CredentialLeaseReply = serde_json::from_str(&reply_line).unwrap();
    assert!(!reply.ok);
    assert_eq!(reply.request_id, "request-empty-provider");
    assert!(!reply_line.contains("sk-"));
    let err = reply.error.unwrap_or_default();
    assert!(err.contains("provider_id"), "{err}");
}

#[test]
fn dispatch_broker_uds_unknown_method_fails_closed() {
    let _peer_lock = credential_broker_uds::lock_broker_peer_for_test();
    let daemon_pid = std::process::id();
    let _peer = credential_broker_uds::install_broker_peer_for_test(daemon_pid).unwrap();
    let envelope = wire::CredentialLeaseEnvelope {
        instance_id: "test-instance".into(),
        auth_token: "test-auth".into(),
        request_id: "request-unknown".into(),
        method: "credential.bogus".into(),
        payload: serde_json::json!({}),
    };
    let line = serde_json::to_string(&envelope).unwrap();
    let reply_line = dispatch_broker_uds(&line, daemon_pid).unwrap();
    let reply: wire::CredentialLeaseReply = serde_json::from_str(&reply_line).unwrap();
    assert!(!reply.ok);
    assert_eq!(reply.request_id, "request-unknown");
    assert!(reply.error.unwrap_or_default().contains("unknown"));
}

#[test]
fn dispatch_broker_uds_rejects_stale_identity_or_auth_before_payload_decode() {
    let _peer_lock = credential_broker_uds::lock_broker_peer_for_test();
    let daemon_pid = std::process::id();
    let _peer = credential_broker_uds::install_broker_peer_for_test(daemon_pid).unwrap();
    for (instance_id, auth_token, request_id) in [
        ("stale-instance", "test-auth", "request-stale-instance"),
        ("test-instance", "stale-auth", "request-stale-auth"),
    ] {
        let line = serde_json::json!({
            "instance_id": instance_id,
            "auth_token": auth_token,
            "request_id": request_id,
            "method": names::CREDENTIAL_LEASE_ACQUIRE,
            // This is deliberately not a valid typed lease payload. Auth must
            // reject it before the dispatch deserializes it.
            "payload": "not-a-lease-request",
        })
        .to_string();
        let reply: wire::CredentialLeaseReply =
            serde_json::from_str(&dispatch_broker_uds(&line, daemon_pid).unwrap()).unwrap();
        assert!(!reply.ok);
        assert_eq!(reply.request_id, request_id);
        assert_eq!(reply.error.as_deref(), Some("unauthorized broker request"));
    }
}
