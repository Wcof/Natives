//! TASK-001 (N01): Checkpoint must only accept Gateway-authorized canonical
//! project-relative paths. Escaping paths (absolute / symlink) are rejected
//! before any checkpoint or handler I/O, and rejected calls produce zero
//! snapshots.
//!
//! Run: `cargo test -p natives-agent-daemon checkpoint_path -- --test-threads=2`

use agent_core::EngineToolRuntime;
use capability_gateway::TrustedPath;
use natives_agent_daemon::checkpoint::CheckpointManager;
use natives_agent_daemon::{PermissionGatedTools, ProductionRuntime};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

fn trusted(canonical: PathBuf, project_relative: &str) -> TrustedPath {
    TrustedPath::new(canonical, PathBuf::from(project_relative))
}

/// Production binds project roots from verified identity (canonical). Mirror
/// that here so macOS `/var` → `/private/var` symlinks stay consistent between
/// `begin_run` and the canonical file paths.
fn canonical_root(dir: &tempfile::TempDir) -> PathBuf {
    dir.path()
        .canonicalize()
        .unwrap_or_else(|_| dir.path().to_path_buf())
}

// ---------------------------------------------------------------------------
// CheckpointManager: raw/escaping paths are refused, trusted paths captured.
// ---------------------------------------------------------------------------

#[test]
fn checkpoint_path_rejects_absolute_escape_before_io() {
    let root = tempfile::tempdir().unwrap();
    let mgr = CheckpointManager::new();
    mgr.begin_run("run-abs", "conv-1", root.path()).unwrap();
    let err = mgr
        .capture_before(
            "run-abs",
            &trusted(PathBuf::from("/etc/passwd"), "etc/passwd"),
        )
        .unwrap_err();
    assert!(err.contains("escapes project root"), "got: {err}");
    // No snapshot may have been recorded for the escaped path.
    let cp = mgr.checkpoint_for_run_public("run-abs").unwrap();
    assert!(
        cp.files.is_empty(),
        "escaped path must not produce a snapshot"
    );
}

#[test]
fn checkpoint_path_rejects_symlink_escape() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("secret.txt");
    std::fs::write(&target, "TOP-SECRET").unwrap();
    let link = root.path().join("evil-link");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let mgr = CheckpointManager::new();
    mgr.begin_run("run-sym", "conv-1", root.path()).unwrap();
    // canonical resolves to the outside target; the relative form alone is not
    // enough to authorize it.
    let err = mgr
        .capture_before(
            "run-sym",
            &trusted(link.canonicalize().unwrap(), "evil-link"),
        )
        .unwrap_err();
    assert!(err.contains("escapes project root"), "got: {err}");
    let cp = mgr.checkpoint_for_run_public("run-sym").unwrap();
    assert!(cp.files.is_empty());
}

#[test]
fn checkpoint_path_accepts_trusted_project_relative() {
    let dir = tempfile::tempdir().unwrap();
    let root = canonical_root(&dir);
    let file = root.join("a.txt");
    std::fs::write(&file, "before").unwrap();
    let mgr = CheckpointManager::new();
    mgr.begin_run("run-ok", "conv-1", &root).unwrap();
    let trusted = trusted(file.canonicalize().unwrap(), "a.txt");
    mgr.capture_before("run-ok", &trusted).unwrap();
    std::fs::write(&file, "after").unwrap();
    mgr.capture_after("run-ok", &trusted).unwrap();
    let cp = mgr.checkpoint_for_run_public("run-ok").unwrap();
    assert_eq!(cp.files.len(), 1);
    let snap = &cp.files[0];
    assert_eq!(snap.before_content.as_deref(), Some("before"));
    assert_eq!(snap.after_content.as_deref(), Some("after"));
    assert_eq!(snap.path, "a.txt");
}

#[test]
fn checkpoint_path_capture_after_rejects_escape() {
    let root = tempfile::tempdir().unwrap();
    let mgr = CheckpointManager::new();
    mgr.begin_run("run-after", "conv-1", root.path()).unwrap();
    let err = mgr
        .capture_after(
            "run-after",
            &trusted(PathBuf::from("/etc/passwd"), "etc/passwd"),
        )
        .unwrap_err();
    assert!(err.contains("escapes project root"), "got: {err}");
    let cp = mgr.checkpoint_for_run_public("run-after").unwrap();
    assert!(cp.files.is_empty());
}

// ---------------------------------------------------------------------------
// Production chain: an escaping write is rejected by Gateway path preflight
// before it can reach checkpoint, ledger, or the handler (zero I/O).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn checkpoint_path_production_chain_rejects_absolute_before_handler_io() {
    let project = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let secret = outside.path().join("secret.txt");
    std::fs::write(&secret, "TOP-SECRET").unwrap();

    let rt = Arc::new(ProductionRuntime::new());
    let mut gateway = capability_gateway::CapabilityGateway::new();
    gateway.set_project_root(project.path().to_string_lossy().into_owned());
    let _ = gateway.register_builtins();
    let tools = PermissionGatedTools {
        gateway: Arc::new(gateway),
        permissions: rt.permissions.clone(),
        events: rt.events.clone(),
        interactions: rt.interactions.clone(),
        subagents: rt.subagents.clone(),
        task_outputs: rt.task_outputs.clone(),
        engines: rt.engines.clone(),
        runtime: Some(rt.clone()),
        provider_id: "test".into(),
        key_id: None,
        parent_run_id: "run-cp-esc".into(),
        conversation_id: "conv-cp".into(),
        model_id: "model-x".into(),
        permission_profile: "autonomous".into(),
        tool_allowlist: None,
        team: None,
        mcp_tool_schemas: Vec::new(),
        selected_mcp_servers: None,
    };

    let cancel = CancellationToken::new();
    let res = tools
        .execute_tool_with_call_id(
            "write_file",
            serde_json::json!({"path": secret.to_string_lossy(), "content": "OWNED"}),
            &cancel,
            Some("call-esc-1"),
        )
        .await;

    // The call must be rejected by PATH validation — never by the checkpoint
    // ("no checkpoint for run" would surface as PERSISTENCE_FAILED) and never
    // by a successful write.
    assert!(
        res.is_error,
        "escaped write must be rejected: {:?}",
        res.output
    );
    let code = res.output.get("error_code").and_then(Value::as_str);
    assert_ne!(
        code,
        Some("PERSISTENCE_FAILED"),
        "path rejection must happen before checkpoint I/O: {:?}",
        res.output
    );
    // The handler must never run for an escaped path.
    assert_eq!(
        std::fs::read_to_string(&secret).unwrap(),
        "TOP-SECRET",
        "handler must not write an escaped path"
    );
}
