use super::*;

#[test]
fn update_failure_preserves_previous_and_success_preserves_preferences() {
    let env = a5_env("atomic-update");
    install_version(&env, "1.0.0");
    env.store.set_enabled("demo", false).unwrap();
    env.store.set_sidebar("demo", false, Some(7)).unwrap();

    let (mut request, _bytes) = version_request("1.1.0");
    // Introduce broken digest that will fail verification
    request.packages[0].payload_sha256 = "00".repeat(32);
    let tx = env.store.install_begin(&request).unwrap();
    use base64::Engine as _;
    let encoded =
        base64::engine::general_purpose::STANDARD.encode(b"{\"message\":\"version 1.1.0\"}");
    assert!(env
        .store
        .install_package(&tx.install_id, "demo-data", &encoded)
        .is_err());

    assert_eq!(env.store.app("demo").unwrap().version, "1.0.0");
    assert!(!env.app_root().join("demo/packages/1.1.0").exists());

    // Successful update
    install_version(&env, "1.1.0");
    let app = env.store.app("demo").unwrap();
    assert_eq!(app.version, "1.1.0");
    assert!(!app.enabled);
    assert!(!app.show_in_sidebar);
    assert_eq!(app.sidebar_order, 7);
    // ADR-0027 contract §5 step 7: exactly ONE previous version is retained
    // for rollback; only older ones are cleaned.
    assert!(env.app_root().join("demo/packages/1.0.0").exists());
    assert!(env.app_root().join("demo/packages/1.1.0").exists());
    env.drop();
}

#[test]
#[cfg(unix)]
fn committed_cleanup_failure_can_retry_without_rolling_back_new_version() {
    let env = a5_env("committed-cleanup");
    // Three versions: the update to 1.1.0 retains 1.0.0 and must clean the
    // older 0.9.0 — which we trap behind a symlink to prove cleanup never
    // follows it out of the app root.
    install_version(&env, "0.9.0");
    install_version(&env, "1.0.0");
    let old = env.app_root().join("demo/packages/0.9.0");
    let held = env.base.join("old-version");
    std::fs::rename(&old, &held).unwrap();
    std::os::unix::fs::symlink(&held, &old).unwrap();
    let (request, bytes) = version_request("1.1.0");
    let tx = env.store.install_begin(&request).unwrap();
    stage_fake_binary(&env, &tx.install_id, &bytes);
    assert!(env.store.install_commit(&tx.install_id).is_err());
    assert_eq!(
        env.store.transaction(&tx.install_id).unwrap().state,
        "installed"
    );
    assert_eq!(env.store.app("demo").unwrap().version, "1.1.0");
    assert!(env.store.app("demo").unwrap().recovery_pending);
    assert!(
        held.exists(),
        "cleanup must not follow the substituted link"
    );
    std::fs::remove_file(&old).unwrap();
    std::fs::rename(&held, &old).unwrap();
    env.store.recover_install("demo").unwrap();
    assert!(!env.store.app("demo").unwrap().recovery_pending);
    assert_eq!(env.store.app("demo").unwrap().version, "1.1.0");
    assert!(!old.exists(), "stale version cleaned after recovery");
    // Retention: the one previous version survives recovery.
    assert!(env.app_root().join("demo/packages/1.0.0").exists());
    env.drop();
}

#[test]
fn inspect_allows_rollback_default_denies_invalid_or_incompatible() {
    use crate::app_store::mutation::inspect_allows_rollback;

    // 1. Compatible output: must succeed
    let compatible = br#"{"currentSchema":1,"migrationState":"committed","hasCommittedNewWrites":false,"previousVersionCompatible":true}"#;
    assert!(inspect_allows_rollback(compatible, None).is_ok());

    // 2. Incompatible output: previousVersionCompatible = false
    let incompatible = br#"{"currentSchema":2,"migrationState":"committed","hasCommittedNewWrites":true,"previousVersionCompatible":false}"#;
    let err = inspect_allows_rollback(incompatible, None).unwrap_err();
    assert!(err.to_string().contains("APP_DATA_SCHEMA_INCOMPATIBLE"));

    // 3. Unsettled migration state (e.g. migrating, prepared, failed)
    let migrating = br#"{"currentSchema":2,"migrationState":"migrating","hasCommittedNewWrites":false,"previousVersionCompatible":true}"#;
    let err = inspect_allows_rollback(migrating, None).unwrap_err();
    assert!(err.to_string().contains("migration journal not settled"));

    // 4. Broken JSON
    let broken = b"not-json";
    let err = inspect_allows_rollback(broken, None).unwrap_err();
    assert!(err.to_string().contains("unreadable --inspect-data output"));

    // 5. Missing required fields
    let missing_field = br#"{"currentSchema":1}"#;
    let err = inspect_allows_rollback(missing_field, None).unwrap_err();
    assert!(err.to_string().contains("unreadable --inspect-data output"));
}

#[test]
fn set_enabled_rejects_running_app_and_advances_generation() {
    let env = a5_env("set-enabled-test");
    install_version(&env, "1.0.0");

    // Write initial projection with generation = 1
    let app_id = "demo";
    let initial_proj = serde_json::json!({
        "receiptVersion": 1,
        "appId": app_id,
        "runtimeHost": "com.natives.app.demo",
        "activeVersion": "1.0.0",
        "generation": 1,
        "activationState": "ready",
        "enabled": true,
        "appProtocolVersion": 1,
        "allowedOrigins": ["chrome-extension://abcdefghijklmnop"],
    });
    crate::app_activation::write_activation_projection(env.app_root(), app_id, &initial_proj)
        .unwrap();

    // 1. Acquire runtime lock to simulate running app
    let runtime_lock = crate::app_install::acquire_app_lock(env.app_root(), app_id, true).unwrap();
    let disable_err = match env.store.set_enabled(app_id, false) {
        Err(e) => e,
        Ok(_) => panic!("expected running app disable to fail"),
    };
    assert_eq!(disable_err.code(), "APP_CONFLICT");

    // Release runtime lock
    drop(runtime_lock);

    // 2. Disable when stopped: succeeds, projection updated to generation = 2
    let disabled_app = env.store.set_enabled(app_id, false).unwrap();
    assert!(!disabled_app.enabled);
    let proj = crate::app_activation::read_activation_projection(env.app_root(), app_id)
        .unwrap()
        .unwrap();
    assert_eq!(proj["enabled"], false);
    assert_eq!(proj["activationState"], "disabled");
    assert_eq!(proj["generation"], 2);

    // 3. Re-enable: succeeds, projection updated to generation = 3
    let enabled_app = env.store.set_enabled(app_id, true).unwrap();
    assert!(enabled_app.enabled);
    let proj = crate::app_activation::read_activation_projection(env.app_root(), app_id)
        .unwrap()
        .unwrap();
    assert_eq!(proj["enabled"], true);
    assert_eq!(proj["activationState"], "ready");
    assert_eq!(proj["generation"], 3);

    // 4. Non-existent app returns NotFound
    let not_found_err = match env.store.set_enabled("nonexistent-app", true) {
        Err(e) => e,
        Ok(_) => panic!("expected nonexistent app to fail"),
    };
    assert_eq!(not_found_err.code(), "APP_NOT_FOUND");

    env.drop();
}

#[test]
fn rollback_rejects_no_previous_version_or_active_runtime() {
    let env = a5_env("rollback-rejects");
    install_version(&env, "1.0.0");

    // 1. No previous version available
    let err = match env.store.rollback("demo") {
        Err(e) => e,
        Ok(_) => panic!("expected rollback without previous version to fail"),
    };
    assert!(err.to_string().contains("APP_NO_PREVIOUS_VERSION"));

    // Update to 1.1.0
    install_version(&env, "1.1.0");

    // 2. Active runtime lock must reject rollback (must be stopped)
    let runtime_lock = crate::app_install::acquire_app_lock(env.app_root(), "demo", true).unwrap();
    let err = match env.store.rollback("demo") {
        Err(e) => e,
        Ok(_) => panic!("expected rollback on running app to fail"),
    };
    assert_eq!(err.code(), "APP_CONFLICT");
    drop(runtime_lock);

    env.drop();
}

#[test]
#[cfg(unix)]
fn rollback_inspect_validation_and_state_switch() {
    let env = a5_env("rollback-switch");
    let app_id = "demo";

    // Set up version 1.0.0 with fake runtime executable
    install_version(&env, "1.0.0");
    let v1_dir = env.app_root().join(app_id).join("runtime").join("1.0.0");
    std::fs::create_dir_all(&v1_dir).unwrap();
    let v1_bin = v1_dir.join("demo-bin");
    std::fs::write(
        &v1_bin,
        "#!/bin/sh\nif [ \"$1\" = \"--inspect-data\" ]; then echo '{\"currentSchema\":1,\"migrationState\":\"committed\",\"hasCommittedNewWrites\":false,\"previousVersionCompatible\":true}'; exit 0; fi\nexit 0\n",
    ).unwrap();
    crate::app_activation::make_executable(&v1_bin).unwrap();

    // Update to 1.1.0
    install_version(&env, "1.1.0");
    let v11_dir = env.app_root().join(app_id).join("runtime").join("1.1.0");
    std::fs::create_dir_all(&v11_dir).unwrap();
    let v11_bin = v11_dir.join("demo-bin");

    // 1. Current version output is incompatible: rollback must be rejected (default-deny)
    std::fs::write(
        &v11_bin,
        "#!/bin/sh\nif [ \"$1\" = \"--inspect-data\" ]; then echo '{\"currentSchema\":2,\"migrationState\":\"committed\",\"hasCommittedNewWrites\":true,\"previousVersionCompatible\":false}'; exit 0; fi\nexit 0\n",
    ).unwrap();
    crate::app_activation::make_executable(&v11_bin).unwrap();

    let err = match env.store.rollback(app_id) {
        Err(e) => e,
        Ok(_) => panic!("expected incompatible rollback to fail"),
    };
    assert!(err.to_string().contains("APP_DATA_SCHEMA_INCOMPATIBLE"));
    assert_eq!(env.store.app(app_id).unwrap().version, "1.1.0");

    // 2. Current version crashes on --inspect-data: rollback must be rejected (default-deny)
    std::fs::write(&v11_bin, "#!/bin/sh\nexit 1\n").unwrap();
    crate::app_activation::make_executable(&v11_bin).unwrap();
    let err = match env.store.rollback(app_id) {
        Err(e) => e,
        Ok(_) => panic!("expected inspect crash to fail"),
    };
    assert!(err.to_string().contains("APP_INSPECT_UNAVAILABLE"));
    assert_eq!(env.store.app(app_id).unwrap().version, "1.1.0");

    // 3. Current version output is compatible: rollback succeeds!
    std::fs::write(
        &v11_bin,
        "#!/bin/sh\nif [ \"$1\" = \"--inspect-data\" ]; then echo '{\"currentSchema\":1,\"migrationState\":\"committed\",\"hasCommittedNewWrites\":false,\"previousVersionCompatible\":true}'; exit 0; fi\nexit 0\n",
    ).unwrap();
    crate::app_activation::make_executable(&v11_bin).unwrap();

    // Write initial projection with generation = 2
    let initial_proj = serde_json::json!({
        "receiptVersion": 1,
        "appId": app_id,
        "runtimeHost": "com.natives.app.demo",
        "activeVersion": "1.1.0",
        "generation": 2,
        "activationState": "ready",
        "enabled": true,
        "appProtocolVersion": 1,
        "allowedOrigins": ["chrome-extension://abcdefghijklmnop"],
    });
    crate::app_activation::write_activation_projection(env.app_root(), app_id, &initial_proj)
        .unwrap();

    let rolled_back = env.store.rollback(app_id).unwrap();
    assert_eq!(rolled_back.version, "1.0.0");

    // Check projection was restored to version 1.0.0 and generation was bumped to 3
    let proj = crate::app_activation::read_activation_projection(env.app_root(), app_id)
        .unwrap()
        .unwrap();
    assert_eq!(proj["activeVersion"], "1.0.0");
    assert_eq!(proj["generation"], 3);
    assert_eq!(proj["activationState"], "ready");

    env.drop();
}

#[test]
fn inspect_rollback_rejects_current_schema_outside_target_range() {
    use crate::app_store::mutation::inspect_allows_rollback;

    // AC-05: the target version's declared dataSchema range participates in
    // the decision — a current schema above the target's readable max is
    // refused even when the inspect boolean claims compatibility.
    let raw = br#"{"currentSchema":4,"migrationState":"committed","hasCommittedNewWrites":true,"previousVersionCompatible":true,"lastDataWriterVersion":"2.0.0"}"#;
    let err = inspect_allows_rollback(raw, Some((1, 3))).unwrap_err();
    assert!(err.to_string().contains("outside the rollback target"));
    assert!(inspect_allows_rollback(raw, Some((1, 4))).is_ok());
    // Plain number form: schema must equal the declared version.
    let raw2 = br#"{"currentSchema":2,"migrationState":"committed","hasCommittedNewWrites":false,"previousVersionCompatible":true}"#;
    assert!(inspect_allows_rollback(raw2, Some((2, 2))).is_ok());
}
