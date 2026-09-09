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
    assert!(!env.app_root().join("demo/packages/1.0.0").exists());
    assert!(env.app_root().join("demo/packages/1.1.0").exists());
    env.drop();
}

#[test]
#[cfg(unix)]
fn committed_cleanup_failure_can_retry_without_rolling_back_new_version() {
    let env = a5_env("committed-cleanup");
    install_version(&env, "1.0.0");
    let old = env.app_root().join("demo/packages/1.0.0");
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
    assert!(!old.exists());
    env.drop();
}
