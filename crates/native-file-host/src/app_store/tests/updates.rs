use super::*;

#[test]
fn update_failure_preserves_runtime_and_success_preserves_preferences() {
    let env = a5_env("atomic-update");
    install_version(&env, "1.0.0");
    env.store.set_enabled("demo", false).unwrap();
    env.store.set_sidebar("demo", false, Some(7)).unwrap();
    let manifest = env.base.join("nm-hosts/com.natives.app.demo.json");
    let original = std::fs::read(&manifest).unwrap();
    let broken = b"#!/bin/sh\nexit 1\n";
    let request = runtime_request(
        "demo",
        "com.natives.app.demo",
        "1.1.0",
        broken.len() as u64,
        &crate::app_install::hex_sha256(broken),
    );
    let tx = env.store.install_begin(&request).unwrap();
    stage_fake_binary(&env, &tx.install_id, broken);
    assert!(env.store.install_commit(&tx.install_id).is_err());
    assert_eq!(env.store.app("demo").unwrap().version, "1.0.0");
    assert_eq!(
        crate::app_install::read_runtime_current(env.app_root(), "demo").as_deref(),
        Some("1.0.0")
    );
    assert_eq!(std::fs::read(&manifest).unwrap(), original);
    assert!(!env.app_root().join("demo/runtime/1.1.0").exists());
    install_version(&env, "1.1.0");
    let app = env.store.app("demo").unwrap();
    assert_eq!(app.version, "1.1.0");
    assert!(!app.enabled);
    assert!(!app.show_in_sidebar);
    assert_eq!(app.sidebar_order, 7);
    assert!(!env.app_root().join("demo/runtime/1.0.0").exists());
    env.drop();
}

#[test]
#[cfg(unix)]
fn committed_cleanup_failure_can_retry_without_rolling_back_new_version() {
    let env = a5_env("committed-cleanup");
    install_version(&env, "1.0.0");
    let old = env.app_root().join("demo/runtime/1.0.0");
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

#[test]
fn interrupted_activation_restores_previous_manifest_and_version() {
    let env = a5_env("recover-activation");
    install_version(&env, "1.0.0");
    let (request, bytes) = version_request("1.1.0");
    let tx = env.store.install_begin(&request).unwrap();
    stage_fake_binary(&env, &tx.install_id, &bytes);
    let manifests = env.base.join("nm-hosts");
    let saved = crate::app_host::snapshot(env.app_root(), &request, &manifests).unwrap();
    env.store.with_conn(|conn| {
        conn.execute("UPDATE app_install_transactions SET state = 'committing', rollback_json = ?2 WHERE install_id = ?1",
            rusqlite::params![tx.install_id, serde_json::to_string(&saved).unwrap()])?;
        Ok(())
    }).unwrap();
    let binary = crate::app_host::prepare(env.app_root(), &request, &tx.install_id).unwrap();
    crate::app_host::activate(
        env.app_root(),
        &request,
        &binary,
        &env.store.caller_origin().unwrap(),
        &manifests,
    )
    .unwrap();
    let other = AppStore::open_at(&env.db(), env.app_root()).unwrap();
    other.set_manifest_dir(manifests.clone());
    other.recover_interrupted().unwrap();
    assert_eq!(
        other.transaction(&tx.install_id).unwrap().state,
        "committing",
        "live owner must be left alone"
    );
    // Simulate the OS releasing file locks after a killed Host, without Drop's graceful abort.
    env.store.installs.lock().unwrap().clear();
    other.recover_interrupted().unwrap();
    assert_eq!(other.transaction(&tx.install_id).unwrap().state, "failed");
    assert_eq!(other.app("demo").unwrap().version, "1.0.0");
    assert_eq!(
        std::fs::read(manifests.join("com.natives.app.demo.json")).unwrap(),
        saved.manifest.unwrap()
    );
    assert_eq!(
        crate::app_install::read_runtime_current(env.app_root(), "demo").as_deref(),
        Some("1.0.0")
    );
    assert!(!env.app_root().join("demo/runtime/1.1.0").exists());
    drop(other);
    env.drop();
}

#[test]
fn concurrent_store_connections_migrate_without_lock_errors() {
    let env = temp_store("concurrent-migrate");
    let barrier = std::sync::Barrier::new(8);
    std::thread::scope(|scope| {
        for _ in 0..8 {
            let barrier = &barrier;
            let db = env.base.join("fresh.db");
            let root = env.app_root().to_path_buf();
            scope.spawn(move || {
                barrier.wait();
                for _ in 0..10 {
                    AppStore::open_at(&db, &root).expect("concurrent open");
                }
            });
        }
    });
    env.drop();
}
