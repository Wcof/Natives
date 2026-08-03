//! Lifecycle behavior matrix across the three real sources.
//!
//! Guards:
//! - resolve order: external → local → internal
//! - actions matrix matches CreativeAppActions::for_state
//! - open_target kind by source
//! - LocalProjectSummary.auto_open is projected
//! - Workshop Bridge surface is never selected for external/local

use super::*;
use crate::db::{apply_migrations, create_tables};
use crate::env_manager;
use rusqlite::Connection;

fn mem() -> Connection {
    env_manager::reset_env_key_cache_for_tests();
    let conn = Connection::open_in_memory().unwrap();
    create_tables(&conn).unwrap();
    apply_migrations(&conn).unwrap();
    env_manager::init_env_encryption_key(&conn).unwrap();
    conn
}

fn insert_module(conn: &Connection, id: &str, enabled: i32) {
    conn.execute(
        "INSERT INTO modules (id, name, version, entry, type, enabled, state, created_at, updated_at)
         VALUES (?1, ?2, '1.0.0', 'index.html', 'web', ?3, 'installed', datetime('now'), datetime('now'))",
        rusqlite::params![id, format!("Mod {id}"), enabled],
    )
    .unwrap();
}

fn sample_local(id: &str, root: &str, auto_open: bool) -> LocalCreativeAppRecord {
    let plan = LaunchPlan {
        schema_version: 1,
        source: LaunchPlanSource::Rule,
        project_kind: LocalProjectKind::Html,
        runtime: LocalLaunchRuntime::StaticHttp,
        program: LaunchProgram::Internal,
        cwd_relative: ".".into(),
        script: None,
        entry_file: Some("index.html".into()),
        script_runner: None,
        args: vec![],
        environment_keys: vec![],
        port: LaunchPort {
            mode: LaunchPortMode::Auto,
            value: None,
        },
        open_path: "/".into(),
        health_path: "/".into(),
        startup_timeout_ms: 60_000,
        auto_open,
        confidence: None,
        reason: "test".into(),
    };
    let now = chrono::Utc::now().to_rfc3339();
    LocalCreativeAppRecord {
        id: id.into(),
        title: format!("Local {id}"),
        description: None,
        icon: None,
        canonical_project_root: root.into(),
        device_id: "dev".into(),
        device_name: "test".into(),
        project_kind: LocalProjectKind::Html,
        launch_mode: LaunchMode::Smart,
        launch_plan_json: plan.to_json().unwrap(),
        plan_fingerprint: "fp".into(),
        state: CreativeAppState::InstalledStopped,
        status_detail_json: None,
        open_url: None,
        current_port: None,
        process_identity_json: None,
        auto_open,
        startup_timeout_ms: 60_000,
        last_started_at: None,
        last_exit_reason: None,
        last_error: None,
        created_at: now.clone(),
        updated_at: now,
    }
}

fn sample_external(id: &str, state: CreativeAppState) -> ExternalCreativeAppRecord {
    let now = chrono::Utc::now().to_rfc3339();
    let cfg = RuntimeConfig::DockerRun {
        container_name: format!("natives-ca-{id}"),
        image: "example/app:latest".into(),
        container_port: 80,
        host_port: 18080,
        open_path: "/".into(),
        health_path: Some("/".into()),
        env_keys: vec![],
    };
    ExternalCreativeAppRecord {
        id: id.into(),
        title: format!("Ext {id}"),
        description: None,
        icon: None,
        version: "1.0.0".into(),
        owner: "o".into(),
        repo: "r".into(),
        repository_url: "https://github.com/o/r".into(),
        release_tag: "v1".into(),
        release_id: Some(1),
        runtime: CreativeAppRuntime::DockerRun,
        state,
        open_url: if state == CreativeAppState::Running {
            Some("http://127.0.0.1:18080/".into())
        } else {
            None
        },
        health_url: None,
        host_port: Some(18080),
        runtime_config_json: cfg.to_json().unwrap(),
        last_error: None,
        created_at: now.clone(),
        updated_at: now,
    }
}

#[test]
fn resolve_order_three_sources() {
    let conn = mem();
    insert_module(&conn, "mod-1", 1);
    crate::creative_app::local::insert_app(&conn, &sample_local("loc-1", "/tmp/loc-1", true))
        .unwrap();
    crate::creative_app::store::insert_app(
        &conn,
        &sample_external("ext-1", CreativeAppState::InstalledStopped),
    )
    .unwrap();

    assert_eq!(
        resolve(&conn, "ext-1").unwrap(),
        ResolvedSource::ExternalGithub
    );
    assert_eq!(
        resolve(&conn, "loc-1").unwrap(),
        ResolvedSource::LocalProject
    );
    assert_eq!(resolve(&conn, "mod-1").unwrap(), ResolvedSource::Internal);
    assert!(resolve(&conn, "missing").is_err());
}

#[test]
fn list_merges_all_sources() {
    let conn = mem();
    insert_module(&conn, "mod-1", 1);
    crate::creative_app::local::insert_app(&conn, &sample_local("loc-1", "/tmp/loc-1", false))
        .unwrap();
    crate::creative_app::store::insert_app(
        &conn,
        &sample_external("ext-1", CreativeAppState::Running),
    )
    .unwrap();

    let list = list_all(&conn).unwrap();
    assert_eq!(list.len(), 3);
    // running first
    assert_eq!(list[0].id, "ext-1");
    assert_eq!(list[0].source, CreativeAppSource::ExternalGithub);
    assert!(list.iter().any(|a| a.source == CreativeAppSource::Internal));
    assert!(list
        .iter()
        .any(|a| a.source == CreativeAppSource::LocalProject));
    assert!(list
        .iter()
        .any(|a| a.source == CreativeAppSource::ExternalGithub));
}

#[test]
fn actions_matrix_per_source_state() {
    // Internal available → open + stop(disable) + delete
    let a = CreativeAppActions::for_state(CreativeAppSource::Internal, CreativeAppState::Available);
    assert!(a.can_open && a.can_stop && a.can_delete && !a.can_start);

    // External running → open + stop + delete
    let a =
        CreativeAppActions::for_state(CreativeAppSource::ExternalGithub, CreativeAppState::Running);
    assert!(a.can_open && a.can_stop && a.can_delete && !a.can_start);

    // Local installed_stopped → start + delete
    let a = CreativeAppActions::for_state(
        CreativeAppSource::LocalProject,
        CreativeAppState::InstalledStopped,
    );
    assert!(a.can_start && a.can_delete && !a.can_open && !a.can_stop);

    // Process/container sources: transient locks all actions.
    // Internal workshop has no install/start/stop transient machine — only
    // available/disabled; unknown states still allow delete of the module record.
    for src in [
        CreativeAppSource::ExternalGithub,
        CreativeAppSource::LocalProject,
    ] {
        let a = CreativeAppActions::for_state(src, CreativeAppState::Starting);
        assert!(!a.can_open && !a.can_start && !a.can_stop && !a.can_delete && !a.can_retry);
    }

    // Docker runtime_unavailable: no start without engine; local can still try start
    let ext = CreativeAppActions::for_state(
        CreativeAppSource::ExternalGithub,
        CreativeAppState::RuntimeUnavailable,
    );
    assert!(!ext.can_start && !ext.can_delete);
    let loc = CreativeAppActions::for_state(
        CreativeAppSource::LocalProject,
        CreativeAppState::RuntimeUnavailable,
    );
    assert!(loc.can_start && loc.can_delete);
}

#[test]
fn open_target_never_workshop_for_external_or_local() {
    let conn = mem();
    insert_module(&conn, "mod-1", 1);
    let mut loc = sample_local("loc-1", "/tmp/loc-1", true);
    loc.state = CreativeAppState::Running;
    loc.open_url = Some("http://127.0.0.1:19000/".into());
    crate::creative_app::local::insert_app(&conn, &loc).unwrap();
    crate::creative_app::store::insert_app(
        &conn,
        &sample_external("ext-1", CreativeAppState::Running),
    )
    .unwrap();

    match open_target(&conn, "mod-1").unwrap() {
        OpenTarget::WorkshopModule { module_id } => assert_eq!(module_id, "mod-1"),
        other => panic!("expected workshop open: {other:?}"),
    }
    match open_target(&conn, "ext-1").unwrap() {
        OpenTarget::LocalUrl { url, app_id } => {
            assert!(url.starts_with("http://127.0.0.1"));
            assert_eq!(app_id, "ext-1");
        }
        other => panic!("external must not use workshop bridge: {other:?}"),
    }
    match open_target(&conn, "loc-1").unwrap() {
        OpenTarget::LocalUrl { url, app_id } => {
            assert!(url.starts_with("http://127.0.0.1"));
            assert_eq!(app_id, "loc-1");
        }
        other => panic!("local must not use workshop bridge: {other:?}"),
    }
}

#[test]
fn local_summary_projects_auto_open() {
    let conn = mem();
    crate::creative_app::local::insert_app(&conn, &sample_local("loc-1", "/tmp/loc-1", true))
        .unwrap();
    let s = get_summary(&conn, "loc-1").unwrap();
    assert_eq!(s.source, CreativeAppSource::LocalProject);
    let lp = s.local_project.expect("local_project");
    assert!(lp.auto_open);
    assert_eq!(lp.project_root, "/tmp/loc-1");
}

#[test]
fn disabled_internal_cannot_open() {
    let conn = mem();
    insert_module(&conn, "mod-off", 0);
    let s = get_summary(&conn, "mod-off").unwrap();
    assert_eq!(s.state, CreativeAppState::Disabled);
    assert!(!s.actions.can_open);
    assert!(s.actions.can_start);
    assert!(open_target(&conn, "mod-off").is_err());
}

#[test]
fn external_restart_propagates_stop_failure_contract() {
    // Contract: adapters::restart must stop before start, and stop Err must not
    // be swallowed. Batch 1 routes restart through the unified stop()/start()
    // wrappers (which own the instance CAS); stop still uses `?` before start.
    let src = include_str!("mod.rs");
    assert!(
        src.contains("stop(conn, ctx, id).await?;"),
        "restart must propagate stop failure with `?` before start"
    );
    assert!(
        !src.contains("let _ = stop(") && !src.contains("let _ = external::stop"),
        "restart must not ignore stop errors"
    );
    let install = include_str!("../install.rs");
    assert!(
        install.contains("stop failed") || install.contains("stop_failed"),
        "stop_app must surface stop failure to callers"
    );
}
