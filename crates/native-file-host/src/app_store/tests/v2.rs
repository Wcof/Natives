use super::*;

#[test]
fn migration_marks_runtime_apps_once_without_touching_install_state() {
    let env = temp_store("v2-migrate");
    env.store.with_conn(|conn| {
        conn.execute("DELETE FROM app_meta WHERE key = 'v2_migration'", [])?;
        conn.execute("INSERT INTO apps (app_id, kind, name, version, runtime_spec_json, surface_json, manifest_json, installed_at, updated_at)
            VALUES ('legacy', 'extension_app', 'Legacy', '1.1.0', '{\"host\":\"com.natives.app.legacy\"}', '{}', '{}', 1, 1)", [])?;
        conn.execute("INSERT INTO app_packages (app_id, package_id, kind, version, platform, arch, wire_size, payload_size, artifact_sha256, payload_sha256, installed_path, installed_at)
            VALUES ('legacy', 'runtime', 'runtime', '1.1.0', 'darwin', 'arm64', 1, 1, '', '', '', 1)", [])?;
        conn.execute("INSERT INTO app_install_transactions (install_id, app_id, from_version, to_version, request_json, state, staging_path, started_at)
            VALUES ('active', 'other', '', '2.0.0', '{}', 'staging', '', 1)", [])?;
        Ok(())
    }).unwrap();
    let reopened = AppStore::open_at(&env.db(), env.app_root()).unwrap();
    assert!(reopened.app("legacy").unwrap().needs_migration);
    assert_eq!(reopened.transaction("active").unwrap().state, "staging");
    drop(reopened);
    env.drop();
}

#[test]
fn resource_reads_are_chunked_and_keep_version_and_format() {
    let env = a5_env("resource-chunks");
    let mut bytes = b"{\"message\":\"\xE4\xB8\xAD\xE6\x96\x87".to_vec();
    bytes.extend(std::iter::repeat_n(b'a', 1024 * 1024));
    bytes.extend_from_slice(b"\"}");
    let request = data_request(
        "demo",
        "2.0.0",
        bytes.len() as u64,
        &crate::app_install::hex_sha256(&bytes),
    );
    let tx = env.store.install_begin(&request).unwrap();
    stage_fake_binary(&env, &tx.install_id, &bytes);
    env.store.install_commit(&tx.install_id).unwrap();
    let first = env
        .store
        .read_resource("demo", "demo-data", Some(0), None)
        .unwrap();
    let second = env
        .store
        .read_resource("demo", "demo-data", Some(first.length as u64), None)
        .unwrap();
    let third = env
        .store
        .read_resource(
            "demo",
            "demo-data",
            Some((first.length + second.length) as u64),
            None,
        )
        .unwrap();
    assert_eq!(
        [
            first.format.as_str(),
            second.format.as_str(),
            third.format.as_str()
        ],
        ["json", "json", "json"]
    );
    assert_eq!(
        [
            first.version.as_str(),
            second.version.as_str(),
            third.version.as_str()
        ],
        ["2.0.0", "2.0.0", "2.0.0"]
    );
    assert_eq!(first.length, 512 * 1024);
    assert_eq!(first.length + second.length + third.length, bytes.len());
    assert!(env
        .store
        .read_resource("demo", "demo-data", Some(0), Some(512 * 1024 + 1))
        .is_err());
    env.drop();
}

#[test]
fn legacy_manifest_cleanup_requires_recorded_owner_and_owned_path() {
    let env = a5_env("legacy-manifest");
    let manifests = env.base.join("nm-hosts");
    let owned_dir = env.app_root().join("legacy/runtime/1.1.0");
    std::fs::create_dir_all(&owned_dir).unwrap();
    env.store.with_conn(|conn| {
        conn.execute("INSERT INTO apps (app_id, kind, name, version, runtime_spec_json, surface_json, manifest_json, installed_at, updated_at)
            VALUES ('legacy', 'extension_app', 'Legacy', '1.1.0', '{\"host\":\"com.natives.app.legacy\"}', '{}', '{}', 1, 1)", [])?;
        Ok(())
    }).unwrap();
    let write_manifest = |name: &str, path: &Path| {
        std::fs::write(
            manifests.join(format!("{name}.json")),
            serde_json::json!({ "name": name, "path": path }).to_string(),
        )
        .unwrap()
    };
    write_manifest("com.natives.app.legacy", &owned_dir.join("host"));
    write_manifest(
        "com.natives.app.foreign",
        &env.app_root().join("foreign/host"),
    );
    write_manifest(
        "com.natives.app.legacy-outside",
        &env.base.join("outside/host"),
    );
    env.store.clean_legacy_manifests().unwrap();
    assert!(!manifests.join("com.natives.app.legacy.json").exists());
    assert!(manifests.join("com.natives.app.foreign.json").exists());
    assert!(manifests
        .join("com.natives.app.legacy-outside.json")
        .exists());
    env.drop();
}
