use super::*;
use crate::db::{apply_migrations, create_tables};
use std::time::{SystemTime, UNIX_EPOCH};

fn mem() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    create_tables(&conn).unwrap();
    apply_migrations(&conn).unwrap();
    conn
}

fn temp_dir() -> PathBuf {
    let mut p = std::env::temp_dir();
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    p.push(format!("natives-local-scan-test-{n}"));
    fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn scans_static_html() {
    let dir = temp_dir();
    fs::write(dir.join("index.html"), "<html><body>hi</body></html>").unwrap();
    let conn = mem();
    let r = inspect_local_project(
        &conn,
        &InspectLocalRequest {
            project_root: dir.to_string_lossy().into(),
            entry_file: None,
        },
    )
    .unwrap();
    assert_eq!(r.project_kind, LocalProjectKind::Html);
    assert!(r.rule_plan.is_some());
    assert_eq!(
        r.rule_plan.as_ref().unwrap().runtime,
        LocalLaunchRuntime::StaticHttp
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn scans_vite_package() {
    let dir = temp_dir();
    fs::write(
        dir.join("package.json"),
        r#"{"name":"x","scripts":{"dev":"vite"},"devDependencies":{"vite":"5.0.0"}}"#,
    )
    .unwrap();
    fs::write(dir.join("vite.config.ts"), "export default {}").unwrap();
    fs::write(dir.join("index.html"), "<html/>").unwrap();
    let conn = mem();
    let r = inspect_local_project(
        &conn,
        &InspectLocalRequest {
            project_root: dir.to_string_lossy().into(),
            entry_file: None,
        },
    )
    .unwrap();
    assert!(matches!(
        r.project_kind,
        LocalProjectKind::Vite | LocalProjectKind::ViteOther | LocalProjectKind::VueVite
    ));
    assert_eq!(r.preferred_script.as_deref(), Some("dev"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn skips_env_files_in_tree() {
    let dir = temp_dir();
    fs::write(dir.join("index.html"), "<html/>").unwrap();
    fs::write(dir.join(".env"), "SECRET=1").unwrap();
    let conn = mem();
    let r = inspect_local_project(
        &conn,
        &InspectLocalRequest {
            project_root: dir.to_string_lossy().into(),
            entry_file: None,
        },
    )
    .unwrap();
    assert!(!r.tree_sample.iter().any(|t| t.contains(".env")));
    let _ = fs::remove_dir_all(&dir);
}

/// Batch 4: a Compose project is detected as evidence, and a `trade`
/// default command is a hard blocker (never silently runnable).
#[test]
fn compose_trade_command_is_blocked() {
    let dir = temp_dir();
    fs::write(
        dir.join("docker-compose.yml"),
        "services:\n  bot:\n    image: trading:latest\n    command: trade --config x.json\n",
    )
    .unwrap();
    let conn = mem();
    let r = inspect_local_project(
        &conn,
        &InspectLocalRequest {
            project_root: dir.to_string_lossy().into(),
            entry_file: None,
        },
    )
    .unwrap();
    assert!(
        r.extra_manifests.iter().any(|m| m == "docker-compose"),
        "compose manifest must be surfaced as evidence"
    );
    assert!(
        r.blockers
            .iter()
            .any(|b| b.contains("may place real trades")),
        "trade command must block: {:?}",
        r.blockers
    );
    assert!(
        r.rule_plan.is_none(),
        "a Compose-only project must not get a fake web plan"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn compose_webserver_command_is_not_blocked() {
    let dir = temp_dir();
    fs::write(
        dir.join("compose.yaml"),
        "services:\n  web:\n    image: trading:latest\n    command: webserver --config x.json\n",
    )
    .unwrap();
    let conn = mem();
    let r = inspect_local_project(
        &conn,
        &InspectLocalRequest {
            project_root: dir.to_string_lossy().into(),
            entry_file: None,
        },
    )
    .unwrap();
    assert!(
        r.blockers.is_empty(),
        "webserver must not block: {:?}",
        r.blockers
    );
    // Batch 5: a safe Compose project is plan-able with a Docker Compose plan.
    let plan = r.rule_plan.as_ref().expect("webserver compose gets a plan");
    assert_eq!(plan.runtime, LocalLaunchRuntime::DockerCompose);
    assert!(plan.compose.is_some());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn detects_python_dockerfile_makefile_evidence() {
    let dir = temp_dir();
    fs::write(dir.join("pyproject.toml"), "[project]\n").unwrap();
    fs::write(dir.join("Dockerfile"), "FROM python\n").unwrap();
    fs::write(dir.join("Makefile"), "all:\n\techo hi\n").unwrap();
    let conn = mem();
    let r = inspect_local_project(
        &conn,
        &InspectLocalRequest {
            project_root: dir.to_string_lossy().into(),
            entry_file: None,
        },
    )
    .unwrap();
    for m in ["python", "dockerfile", "makefile"] {
        assert!(
            r.extra_manifests.iter().any(|e| e == m),
            "missing manifest evidence: {m}"
        );
    }
    assert!(r.rule_plan.is_none());
    let _ = fs::remove_dir_all(&dir);
}

/// Batch 8: dry-run is proven from the config projection, never returned.
#[test]
fn dry_run_projection_proves_safe_mode() {
    let dir = temp_dir();
    fs::create_dir_all(dir.join("user_data")).unwrap();
    fs::write(
        dir.join("user_data/config.json"),
        r#"{"dry_run": true, "api_server": {"enabled": true}}"#,
    )
    .unwrap();
    assert!(
        config_proves_dry_run(&dir),
        "dry_run:true must prove safe mode"
    );
    fs::write(
        dir.join("user_data/config.json"),
        r#"{"dry_run": false, "exchange": {"key": "SECRET"}}"#,
    )
    .unwrap();
    assert!(
        !config_proves_dry_run(&dir),
        "dry_run:false must not prove safe mode"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn html_import_respects_user_selected_entry_even_without_index() {
    // 审计收口 #13：目录无 index.html、用户显式选择 demo.html → plan 用 demo.html。
    let dir = temp_dir();
    fs::write(dir.join("demo.html"), "<html><body>demo</body></html>").unwrap();
    let conn = mem();
    let r = inspect_local_project(
        &conn,
        &InspectLocalRequest {
            project_root: dir.to_string_lossy().into(),
            entry_file: Some("demo.html".into()),
        },
    )
    .unwrap();
    assert_eq!(r.project_kind, LocalProjectKind::Html);
    assert_eq!(
        r.rule_plan.as_ref().unwrap().entry_file.as_deref(),
        Some("demo.html")
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn html_import_user_entry_wins_over_index_html() {
    // 审计收口 #13：目录同时有 index.html 与用户所选 demo.html → 用户所选优先。
    let dir = temp_dir();
    fs::write(dir.join("index.html"), "<html/>").unwrap();
    fs::write(dir.join("demo.html"), "<html><body>demo</body></html>").unwrap();
    let conn = mem();
    let r = inspect_local_project(
        &conn,
        &InspectLocalRequest {
            project_root: dir.to_string_lossy().into(),
            entry_file: Some("demo.html".into()),
        },
    )
    .unwrap();
    assert_eq!(
        r.rule_plan.as_ref().unwrap().entry_file.as_deref(),
        Some("demo.html")
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn html_import_rejects_traversal_entry() {
    // 审计收口 #13：`../escape.html` 越界 → 拒绝，rule_plan 不生成假 plan。
    let dir = temp_dir();
    fs::write(dir.join("index.html"), "<html/>").unwrap();
    let conn = mem();
    let r = inspect_local_project(
        &conn,
        &InspectLocalRequest {
            project_root: dir.to_string_lossy().into(),
            entry_file: Some("../escape.html".into()),
        },
    )
    .unwrap();
    assert!(r.rule_plan.is_none(), "越界 entry 不得生成 plan");
    assert!(r
        .risks
        .iter()
        .any(|risk| risk.contains("escape") || risk.contains("unsafe")));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn html_import_rejects_missing_entry() {
    // 审计收口 #13：用户所选文件不存在 → 拒绝，不伪造 index 回退。
    let dir = temp_dir();
    let conn = mem();
    let r = inspect_local_project(
        &conn,
        &InspectLocalRequest {
            project_root: dir.to_string_lossy().into(),
            entry_file: Some("missing.html".into()),
        },
    )
    .unwrap();
    assert!(r.rule_plan.is_none(), "不存在 entry 不得生成 plan");
    assert!(r.risks.iter().any(|risk| risk.contains("not found")));
    let _ = fs::remove_dir_all(&dir);
}
