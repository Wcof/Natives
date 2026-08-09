use super::*;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir() -> PathBuf {
    let mut p = std::env::temp_dir();
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    p.push(format!("natives-local-plan-test-{n}"));
    fs::create_dir_all(&p).unwrap();
    p
}

fn html_plan() -> LaunchPlan {
    LaunchPlan {
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
        auto_open: true,
        confidence: Some(1.0),
        reason: "test".into(),
        compose: None,
        trade_approval: None,
        process_profile: None,
    }
}

#[test]
fn validates_static_html() {
    let dir = temp_dir();
    fs::write(dir.join("index.html"), "<html/>").unwrap();
    let p = validate_launch_plan(&dir, html_plan()).unwrap();
    assert_eq!(p.program, LaunchProgram::Internal);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn rejects_shell_in_script() {
    assert!(!script_body_is_safe("vite && echo hi"));
    assert!(!script_body_is_safe("node $(cat x)"));
    assert!(script_body_is_safe("vite"));
    assert!(script_body_is_safe("vue-cli-service serve"));
}

#[test]
fn rejects_bad_port() {
    let dir = temp_dir();
    fs::write(dir.join("index.html"), "<html/>").unwrap();
    let mut plan = html_plan();
    plan.port = LaunchPort {
        mode: LaunchPortMode::Fixed,
        value: None,
    };
    assert!(validate_launch_plan(&dir, plan).is_err());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn rejects_npm_script_body_with_shell() {
    assert!(detect_script_runner("vite && echo hi").is_none());
    assert!(detect_script_runner("npx vite").is_none());
    assert!(detect_script_runner("cross-env NODE_ENV=dev vite").is_none());
    assert_eq!(
        detect_script_runner("vite"),
        Some(crate::creative_app::model::ScriptRunner::Vite)
    );
    assert_eq!(
        detect_script_runner("vue-cli-service serve"),
        Some(crate::creative_app::model::ScriptRunner::VueCli)
    );
}

#[test]
fn vite_and_vue_flags_differ() {
    let v = runner_port_flags(crate::creative_app::model::ScriptRunner::Vite, 5173);
    assert!(v.iter().any(|a| a == "--strictPort"));
    let c = runner_port_flags(crate::creative_app::model::ScriptRunner::VueCli, 8080);
    assert!(!c.iter().any(|a| a == "--strictPort"));
    assert!(c.iter().any(|a| a == "--port"));
}

fn compose_plan(file: &str, command: Vec<String>) -> LaunchPlan {
    let mut plan = html_plan();
    plan.runtime = LocalLaunchRuntime::DockerCompose;
    plan.program = LaunchProgram::Internal;
    plan.compose = Some(ComposePlanDetail {
        compose_file: file.into(),
        project_seed: "proj".into(),
        service: None,
        command,
        health_path: "/".into(),
        host_port: None,
    });
    plan
}

/// Batch 5: a Compose plan validates only when the compose file exists under
/// root and the effective command is not a real-trading override.
#[test]
fn compose_plan_validates_and_gates_trade_override() {
    let dir = temp_dir();
    fs::write(
        dir.join("docker-compose.yml"),
        "services:\n  web:\n    image: x\n",
    )
    .unwrap();
    // Explicit trade override is blocked by the risk gate.
    let bad = compose_plan(
        "docker-compose.yml",
        vec!["trade".into(), "--config".into()],
    );
    assert!(
        validate_launch_plan(&dir, bad).is_err(),
        "a trade override must never validate"
    );
    // Safe compose plan validates with runtime preserved.
    let ok = compose_plan("docker-compose.yml", vec![]);
    let v = validate_launch_plan(&dir, ok).unwrap();
    assert_eq!(v.runtime, LocalLaunchRuntime::DockerCompose);
    assert_eq!(v.creative_runtime(), CreativeAppRuntime::DockerCompose);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn compose_plan_requires_file_under_root() {
    let dir = temp_dir();
    let plan = compose_plan("nope.yml", vec![]);
    assert!(validate_launch_plan(&dir, plan).is_err());
    let _ = fs::remove_dir_all(&dir);
}

/// Batch 8: an explicit webserver approval relaxes the gate only when the
/// command is a webserver command; a mismatch stays blocked.
#[test]
fn trade_approval_only_relaxes_matching_safe_mode() {
    let dir = temp_dir();
    fs::write(
        dir.join("docker-compose.yml"),
        "services:\n  bot:\n    image: t\n",
    )
    .unwrap();

    // A trade command override with a webserver approval must STILL block.
    let mut mismatch = compose_plan(
        "docker-compose.yml",
        vec!["trade".into(), "--config".into()],
    );
    mismatch.trade_approval = Some(TradeApproval::Webserver);
    assert!(
        validate_launch_plan(&dir, mismatch).is_err(),
        "a webserver approval can never run trade"
    );

    // A webserver command with a webserver approval validates.
    let mut ok = compose_plan(
        "docker-compose.yml",
        vec!["webserver".into(), "--config".into()],
    );
    ok.trade_approval = Some(TradeApproval::Webserver);
    assert!(
        validate_launch_plan(&dir, ok).is_ok(),
        "matching webserver approval must validate"
    );
    let _ = fs::remove_dir_all(&dir);
}
