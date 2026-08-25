//! APP-015 验收测试：三类 app + sidebar + cascade metadata（in-memory v28 schema）。

use super::*;

fn v28() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    crate::db::create_tables(&conn).unwrap();
    crate::db::apply_migrations(&conn).unwrap();
    conn
}

#[test]
fn register_local_is_idempotent_by_source() {
    let conn = v28();
    let a = AppRepository::register_local(
        &conn,
        "My Project",
        "/tmp/proj-a",
        Some("desc"),
        None,
        RegistrationOrigin::LocalScan,
    )
    .unwrap();
    let b = AppRepository::register_local(
        &conn,
        "My Project (renamed)",
        "/tmp/proj-a",
        None,
        None,
        RegistrationOrigin::Manual,
    )
    .unwrap();
    assert_eq!(a, b, "same root → same application id (idempotent)");
    assert_eq!(AppRepository::list(&conn).unwrap().len(), 1);
    let (kind, origin, show, order) = AppRepository::identity(&conn, &a).unwrap();
    assert_eq!(kind, Some(AppKind::LocalProject));
    // 第二次注册刷新 origin。
    assert_eq!(origin, Some(RegistrationOrigin::Manual));
    assert!(!show);
    assert_eq!(order, None);
    let spec = AppRepository::find_by_source(&conn, "local_project", "/tmp/proj-a").unwrap();
    assert_eq!(spec, Some(a.clone()));
}

#[test]
fn register_system_writes_spec_and_upserts() {
    let conn = v28();
    let a = AppRepository::register_system(
        &conn,
        "Notes",
        "/Applications/Notes.app",
        Some("com.apple.Notes"),
        "macos",
        None,
        RegistrationOrigin::SystemDiscovery,
    )
    .unwrap();
    let spec = AppRepository::load_system_spec(&conn, &a).unwrap();
    assert_eq!(spec.application_path, "/Applications/Notes.app");
    assert_eq!(spec.bundle_identifier.as_deref(), Some("com.apple.Notes"));
    assert_eq!(spec.platform, "macos");
    assert_eq!(spec.launch_policy, "activate_existing");

    // 幂等 upsert：同 path 不新增行，spec 被刷新。
    let b = AppRepository::register_system(
        &conn,
        "Notes",
        "/Applications/Notes.app",
        None,
        "macos",
        Some("launch_new"),
        RegistrationOrigin::SystemDiscovery,
    )
    .unwrap();
    assert_eq!(a, b);
    let spec = AppRepository::load_system_spec(&conn, &a).unwrap();
    assert_eq!(spec.launch_policy, "launch_new");

    // 空 path 拒绝。
    assert!(AppRepository::register_system(
        &conn,
        "X",
        "  ",
        None,
        "macos",
        None,
        RegistrationOrigin::Manual,
    )
    .is_err());
}

#[test]
fn register_web_writes_spec_and_validates_url() {
    let conn = v28();
    let a = AppRepository::register_web(
        &conn,
        "GitHub",
        "https://github.com",
        &["github.com".into()],
        None,
        true,
    )
    .unwrap();
    let spec = AppRepository::load_web_spec(&conn, &a).unwrap();
    assert_eq!(spec.url, "https://github.com");
    assert_eq!(spec.approved_origins, vec!["github.com".to_string()]);
    assert_eq!(spec.open_behavior, "native_webview");
    assert!(spec.keep_alive);

    // 自动解析协议：github.com -> https://github.com, localhost:3000 -> http://localhost:3000
    let g = AppRepository::register_web(&conn, "GH", "github.com", &[], None, false).unwrap();
    assert_eq!(
        AppRepository::load_web_spec(&conn, &g).unwrap().url,
        "https://github.com"
    );
    assert_eq!(
        a, g,
        "github.com normalized to https://github.com matches existing app"
    );

    let loc =
        AppRepository::register_web(&conn, "Local", "http://localhost:3000", &[], None, false).unwrap();
    assert_eq!(
        AppRepository::load_web_spec(&conn, &loc).unwrap().url,
        "http://localhost:3000"
    );

    // 非法 URL 拒绝。
    assert!(AppRepository::register_web(&conn, "Bad", "ftp://x.com", &[], None, false).is_err());
    assert!(AppRepository::register_web(&conn, "Empty", "   ", &[], None, false).is_err());

    // 幂等。
    let b = AppRepository::register_web(&conn, "GitHub", "https://github.com", &[], None, false)
        .unwrap();
    assert_eq!(a, b);
    assert_eq!(AppRepository::list(&conn).unwrap().len(), 2);
}

/// APPV2-T01 回归（根因复现）：无 scheme 的用户输入（如 `chatgpt.com`）
/// 必须被规范化为 https 后成功持久化，而不是被注册校验直接拒绝。
#[test]
fn register_web_normalizes_schemeless_input_to_https() {
    let conn = v28();
    let a = AppRepository::register_web(
        &conn,
        "ChatGPT",
        "chatgpt.com",
        &["chatgpt.com".into()],
        None,
        false,
    )
    .expect("schemeless input must be normalized, not rejected");
    let spec = AppRepository::load_web_spec(&conn, &a).unwrap();
    assert_eq!(spec.url, "https://chatgpt.com");

    // 带路径/查询串的无 scheme 输入同样规范化。
    let b = AppRepository::register_web(
        &conn,
        "DingTalk",
        "alidocs.dingtalk.com/i/spaces/demo",
        &["alidocs.dingtalk.com".into()],
        None,
        false,
    )
    .unwrap();
    assert_eq!(
        AppRepository::load_web_spec(&conn, &b).unwrap().url,
        "https://alidocs.dingtalk.com/i/spaces/demo"
    );

    // 显式 http 的 loopback 开发地址允许；公网 http 必须拒绝（要求 https）。
    let c = AppRepository::register_web(
        &conn,
        "Local Dev",
        "http://127.0.0.1:8080",
        &["127.0.0.1".into()],
        None,
        false,
    )
    .expect("explicit loopback http must be allowed");
    assert_eq!(
        AppRepository::load_web_spec(&conn, &c).unwrap().url,
        "http://127.0.0.1:8080"
    );
    // 无 scheme 一律补 https（包括 loopback 输入）。
    let d = AppRepository::register_web(
        &conn,
        "Local Dev https",
        "localhost:5173",
        &["localhost".into()],
        None,
        false,
    )
    .unwrap();
    assert_eq!(
        AppRepository::load_web_spec(&conn, &d).unwrap().url,
        "https://localhost:5173"
    );
    assert!(
        AppRepository::register_web(
            &conn,
            "Insecure",
            "http://example.com",
            &["example.com".into()],
            None,
            false,
        )
        .is_err(),
        "public http must be rejected in favor of https"
    );
    assert!(
        AppRepository::register_web(&conn, "NoHost", "https://", &["x".into()], None, false,)
            .is_err(),
        "missing host must be rejected"
    );
}

#[test]
fn update_metadata_and_sidebar_are_typed() {
    let conn = v28();
    let a =
        AppRepository::register_web(&conn, "W", "https://w.example.com", &[], None, false).unwrap();

    let mut conn = conn;
    let view = AppRepository::update_metadata(
        &mut conn,
        &UpdateAppMetadataInput {
            app_id: a.clone(),
            title: Some("  ".into()),
            description: None,
            icon: None,
            show_in_sidebar: None,
            sidebar_order: None,
        },
    )
    .unwrap_err();
    assert!(
        matches!(view, Error::InvalidInput(_)),
        "empty title rejected"
    );

    let view = AppRepository::update_metadata(
        &mut conn,
        &UpdateAppMetadataInput {
            app_id: a.clone(),
            title: Some("  W2 ".into()),
            description: Some("d".into()),
            icon: Some("i".into()),
            show_in_sidebar: Some(true),
            sidebar_order: Some(7),
        },
    )
    .unwrap();
    assert_eq!(view.title, "W2");
    assert!(view.show_in_sidebar);
    assert_eq!(view.sidebar_order, Some(7));

    // sidebar 独立更新。
    let view = AppRepository::set_sidebar(&mut conn, &a, Some(false), Some(1)).unwrap();
    assert!(!view.show_in_sidebar);
    assert_eq!(view.sidebar_order, Some(1));

    // 未知 id → NotFound。
    assert!(matches!(
        AppRepository::set_sidebar(&mut conn, "app-nope", Some(true), None),
        Err(Error::NotFound(_))
    ));
}

#[test]
fn update_spec_partial_and_typed() {
    let conn = v28();
    let w =
        AppRepository::register_web(&conn, "W", "https://w.example.com", &[], None, false).unwrap();
    let mut conn = conn;
    let spec = AppRepository::update_web_spec(
        &mut conn,
        &w,
        Some("https://w2.example.com"),
        None,
        None,
        Some(true),
    )
    .unwrap();
    assert_eq!(spec.url, "https://w2.example.com");
    assert!(spec.keep_alive);

    // web spec 对 system app 不存在 → NotFound。
    let s = AppRepository::register_system(
        &conn,
        "S",
        "/Applications/S.app",
        None,
        "macos",
        None,
        RegistrationOrigin::Manual,
    )
    .unwrap();
    assert!(matches!(
        AppRepository::update_web_spec(
            &mut conn,
            &s,
            Some("https://x.example.com"),
            None,
            None,
            None
        ),
        Err(Error::NotFound(_))
    ));
    // system spec 更新（部分）。
    let spec = AppRepository::update_system_spec(
        &mut conn,
        &s,
        Some("/Applications/S2.app"),
        Some(Some("com.s.app")),
        None,
        None,
    )
    .unwrap();
    assert_eq!(spec.application_path, "/Applications/S2.app");
    assert_eq!(spec.bundle_identifier.as_deref(), Some("com.s.app"));
}

#[test]
fn remove_cascades_metadata_but_no_external_side_effects() {
    let conn = v28();
    let w =
        AppRepository::register_web(&conn, "W", "https://w.example.com", &[], None, false).unwrap();
    // 挂一些级联元数据：surface + window + runtime instance + plan。
    conn.execute(
        "INSERT INTO application_surfaces
            (id, application_id, kind, label, title, url, created_at, updated_at)
         VALUES ('surf-1', ?1, 'main', 'Main', NULL, 'https://w.example.com', 't0', 't0')",
        params![w],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO startup_plans
            (id, application_id, plan_version, plan_json, is_active, created_at, updated_at)
         VALUES ('plan-1', ?1, 1, '{}', 1, 't0', 't0')",
        params![w],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO runtime_instances
            (id, application_id, plan_id, status, owner_kind, created_at, updated_at)
         VALUES ('inst-1', ?1, 'plan-1', 'stopped', 'host_http', 't0', 't0')",
        params![w],
    )
    .unwrap();

    let removed = AppRepository::remove_application(&conn, &w).unwrap();
    assert!(removed);
    // 重复删除幂等 = false。
    assert!(!AppRepository::remove_application(&conn, &w).unwrap());
    // 级联清理：spec / surface / plan / instance 全没了。
    assert!(AppRepository::load_web_spec(&conn, &w).is_err());
    assert!(AppRepository::list_surfaces(&conn, &w).unwrap().is_empty());
    assert!(AppRepository::list_instances(&conn, &w).unwrap().is_empty());
    assert!(AppRepository::get(&conn, &w).unwrap().is_none());

    // 未知 id → false（不报错）。
    assert!(!AppRepository::remove_application(&conn, "app-nope").unwrap());
}

#[test]
fn instance_and_surface_queries_project_strong_types() {
    let conn = v28();
    let a =
        AppRepository::register_web(&conn, "W", "https://w.example.com", &[], None, false).unwrap();
    conn.execute(
        "INSERT INTO runtime_instances
            (id, application_id, plan_id, status, cleanup_status, owner_kind, pgid, current_port, pid, failure, created_at, updated_at)
         VALUES ('inst-1', ?1, NULL, 'running', NULL, 'local_process', 1234, 5173, 5678, NULL, 't0', 't1')",
        params![a],
    )
    .unwrap();
    let inst = AppRepository::active_instance(&conn, &a).unwrap().unwrap();
    assert_eq!(inst.status, "running");
    assert_eq!(inst.pgid, Some(1234));
    assert_eq!(inst.pid, Some(5678));
    assert_eq!(inst.current_port, Some(5173));
    assert_eq!(AppRepository::list_instances(&conn, &a).unwrap().len(), 1);

    // stopped 实例不算 active。
    conn.execute(
        "UPDATE runtime_instances SET status = 'stopped' WHERE id = 'inst-1'",
        [],
    )
    .unwrap();
    assert!(AppRepository::active_instance(&conn, &a).unwrap().is_none());
}
