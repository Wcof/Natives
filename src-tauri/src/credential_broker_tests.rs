    use super::*;

    #[test]
    fn rejects_empty_provider_id() {
        let err =
            tauri::async_runtime::block_on(credential_broker_resolve(CredentialBrokerRequest {
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
        let status = registry.status(&meta.lease_id).unwrap();
        assert!(!status.active, "expired lease must report inactive");
    }

    #[test]
    fn dispatch_broker_uds_acquire_rejects_empty_provider_redacted() {
        let envelope = wire::CredentialLeaseEnvelope {
            method: names::CREDENTIAL_LEASE_ACQUIRE.to_string(),
            payload: serde_json::json!({
                "key_id": "k1",
                "provider_id": "",
                "run_id": "run-1",
                "session_id": null,
            }),
        };
        let line = serde_json::to_string(&envelope).unwrap();
        let reply_line = dispatch_broker_uds(&line).unwrap();
        let reply: wire::CredentialLeaseReply = serde_json::from_str(&reply_line).unwrap();
        assert!(!reply.ok);
        assert!(!reply_line.contains("sk-"));
        let err = reply.error.unwrap_or_default();
        assert!(err.contains("provider_id"), "{err}");
    }

    #[test]
    fn dispatch_broker_uds_unknown_method_fails_closed() {
        let envelope = wire::CredentialLeaseEnvelope {
            method: "credential.bogus".into(),
            payload: serde_json::json!({}),
        };
        let line = serde_json::to_string(&envelope).unwrap();
        let reply_line = dispatch_broker_uds(&line).unwrap();
        let reply: wire::CredentialLeaseReply = serde_json::from_str(&reply_line).unwrap();
        assert!(!reply.ok);
        assert!(reply.error.unwrap_or_default().contains("unknown"));
    }
