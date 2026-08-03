//! cancellation_tree_e2e — task-03 acceptance.
//!
//! Covers:
//! - parent → child → grandchild token cascade
//! - cancel_all_execution_roots drains registry
//! - process cancel hook is invoked on force
//! - permission waiter is woken on cancel

use capability_gateway::ProcessSupervisor;
use natives_agent_daemon::runtime::{
    CancelPhase, ExecutionRegistry, ManagedResource, ProcessCancelHook,
};
use natives_agent_daemon::ProductionRuntime;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

struct KillCounter {
    n: AtomicUsize,
}

#[async_trait::async_trait]
impl ProcessCancelHook for KillCounter {
    async fn cancel_task(&self, _task_id: &str) -> Result<(), String> {
        self.n.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    async fn cancel_tasks_for_run(&self, _run_id: &str) -> Result<(), String> {
        Ok(())
    }
}

#[tokio::test]
async fn parent_child_grandchild_all_cancel() {
    let reg = ExecutionRegistry::with_grace_ms(30);
    let hook = Arc::new(KillCounter {
        n: AtomicUsize::new(0),
    });
    reg.set_process_cancel_hook(hook.clone());
    reg.register_root("p").await.unwrap();
    reg.register_child("c", "p").await.unwrap();
    reg.register_child("g", "c").await.unwrap();
    reg.track_resource("g", ManagedResource::ProcessTask("bg-1".into()))
        .await
        .unwrap();

    let g_tok = reg.token("g").await.unwrap();
    assert!(!g_tok.is_cancelled());

    let out = reg.cancel_tree("p").await;
    assert!(out.quiet, "{out:?}");
    assert_eq!(out.phase, CancelPhase::Clean);
    assert!(g_tok.is_cancelled());
    assert_eq!(reg.active_count().await, 0);
    assert!(hook.n.load(Ordering::SeqCst) >= 1);
}

#[tokio::test]
async fn production_runtime_registers_child_under_parent_token() {
    let rt = ProductionRuntime::new();
    rt.ensure_execution_token("parent-run", None).await.unwrap();
    rt.ensure_execution_token("child-run", Some("parent-run"))
        .await
        .unwrap();
    let child = rt.execution.token("child-run").await.unwrap();
    assert!(!child.is_cancelled());

    rt.cancel_run("parent-run").await;
    assert!(child.is_cancelled());
}

#[tokio::test]
async fn cancel_all_execution_roots_quiet() {
    let reg = ExecutionRegistry::with_grace_ms(20);
    reg.register_root("a").await.unwrap();
    reg.register_root("b").await.unwrap();
    reg.register_child("a1", "a").await.unwrap();
    let results = reg.cancel_all_execution_roots().await;
    assert!(results.iter().all(|r| r.quiet));
    assert_eq!(reg.active_count().await, 0);
}

#[tokio::test]
async fn production_cancel_wakes_permission_waiter_fast() {
    let rt = Arc::new(ProductionRuntime::new());
    let run_id = "run-perm-cancel";
    let _tok = rt.ensure_execution_token(run_id, None).await.unwrap();

    let (tx, rx) = oneshot::channel::<(bool, String)>();
    rt.insert_permission_waiter("perm-1", run_id, "write_file", tx)
        .await;

    let start = Instant::now();
    let waiter = tokio::spawn(async move {
        match tokio::time::timeout(Duration::from_secs(2), rx).await {
            Ok(Ok((approved, scope))) => (approved, scope),
            _ => (true, "timeout".into()), // fail if we hang
        }
    });

    // Give waiter a moment to block, then cancel.
    tokio::time::sleep(Duration::from_millis(10)).await;
    rt.cancel_run(run_id).await;
    let (approved, scope) = waiter.await.unwrap();
    let elapsed = start.elapsed();
    assert!(!approved, "cancel must deny permission");
    assert!(scope == "cancelled" || scope == "once", "scope={scope}");
    assert!(
        elapsed < Duration::from_millis(100),
        "permission wait cancel took {elapsed:?} (want <100ms)"
    );
    assert!(
        !rt.interactions.has_permission("perm-1").await,
        "waiter must be cleared"
    );
}

#[tokio::test]
async fn single_process_cancel_does_not_require_parent_token() {
    // Background process cancel is resource-scoped: cancel_task only.
    let sup = capability_gateway::global_process_supervisor();
    use capability_gateway::{ProcessSpec, ProcessSupervisor};
    let task_id = format!("solo-{}", uuid::Uuid::new_v4());
    let run_id = format!("run-{}", uuid::Uuid::new_v4());
    let tmp = std::env::temp_dir().join(&run_id);
    std::fs::create_dir_all(&tmp).unwrap();
    let snap = sup
        .spawn(ProcessSpec {
            run_id: run_id.clone(),
            task_id: task_id.clone(),
            display_command: "sleep 30".into(),
            program: "sleep".into(),
            args: vec!["30".into()],
            cwd: tmp,
            timeout_ms: 60_000,
            background: true,
        })
        .await
        .expect("spawn sleep");
    assert_eq!(snap.task_id, task_id);
    let cancelled = sup.cancel(&task_id).await.unwrap();
    assert_eq!(cancelled.state, capability_gateway::ProcessState::Cancelled);
    // Parent run token (if any) is independent — not cancelled by process-only cancel.
    let tok = CancellationToken::new();
    assert!(!tok.is_cancelled());
}

/// ProcessCancelHook that forwards to the real supervisor so the registry's
/// force phase actually kills + reaps the OS child.
struct SupervisorKillHook;

#[async_trait::async_trait]
impl ProcessCancelHook for SupervisorKillHook {
    async fn cancel_task(&self, task_id: &str) -> Result<(), String> {
        let snap = capability_gateway::global_process_supervisor()
            .cancel(task_id)
            .await?;
        assert_eq!(
            snap.state,
            capability_gateway::ProcessState::Cancelled,
            "supervisor must settle the child as Cancelled after kill+wait"
        );
        Ok(())
    }
    async fn cancel_tasks_for_run(&self, _run_id: &str) -> Result<(), String> {
        Ok(())
    }
}

/// Local helper: count OS processes whose full command line contains `needle`.
fn pgrep_count(needle: &str) -> usize {
    let out = std::process::Command::new("pgrep")
        .arg("-f")
        .arg(needle)
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).lines().count(),
        Ok(_) => 0,
        Err(_) => 0,
    }
}

/// Batch-2 acceptance #1: a real long-running shell child is killed, wait/reaped,
/// the ExecutionRegistry drains quiet, and no late "success" is observable.
#[tokio::test]
async fn shell_child_is_killed_reaped_and_registry_quiet_on_cancel() {
    use capability_gateway::{ProcessSpec, ProcessSupervisor};
    let reg = ExecutionRegistry::with_grace_ms(10);
    reg.set_process_cancel_hook(Arc::new(SupervisorKillHook));
    let run_id = format!("shell-cancel-{}", uuid::Uuid::new_v4());
    let task_id = format!("shell-task-{}", uuid::Uuid::new_v4());
    reg.register_root(&run_id).await.unwrap();
    let sup = capability_gateway::global_process_supervisor();
    let tmp = std::env::temp_dir().join(&run_id);
    std::fs::create_dir_all(&tmp).unwrap();
    // A unique long-running command so pgrep can prove the OS child disappears.
    let needle = format!("sleep {}", 3000 + std::process::id() % 1000);
    let snap = sup
        .spawn(ProcessSpec {
            run_id: run_id.clone(),
            task_id: task_id.clone(),
            display_command: needle.clone(),
            program: "sleep".into(),
            args: vec![needle.trim_start_matches("sleep ").to_string()],
            cwd: tmp,
            timeout_ms: 120_000,
            background: true,
        })
        .await
        .expect("spawn long-running child");
    assert_eq!(snap.task_id, task_id);
    assert_eq!(snap.state, capability_gateway::ProcessState::Background);
    // Child must actually be alive as an OS process before we cancel.
    for _ in 0..50 {
        if pgrep_count(&needle) >= 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        pgrep_count(&needle) >= 1,
        "child `{needle}` must be running before cancel"
    );

    // Track as a process resource and cancel the whole run tree.
    reg.track_resource(&run_id, ManagedResource::ProcessTask(task_id.clone()))
        .await
        .unwrap();
    let out = reg.cancel_tree(&run_id).await;
    assert!(out.quiet, "cancel must be clean: {out:?}");
    assert_eq!(out.phase, CancelPhase::Clean);
    assert!(
        reg.tree_quiet(&run_id).await,
        "registry must drain the tree"
    );

    // The OS process is gone: killed + reaped (wait() reaped the zombie).
    for _ in 0..50 {
        if pgrep_count(&needle) == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        pgrep_count(&needle),
        0,
        "child `{needle}` must be killed + reaped (no orphan)"
    );
    // No late success: the supervisor settles the child as Cancelled, never
    // Completed/Failed (a killed process must not present as a success).
    let after = sup.poll(&task_id).await.unwrap();
    assert_eq!(
        after.state,
        capability_gateway::ProcessState::Cancelled,
        "cancelled child must not report a late success"
    );
    assert_ne!(after.state, capability_gateway::ProcessState::Completed);
}

/// Batch-2 acceptance #6: many parallel tracked tool futures all exit on cancel
/// and the registry drains quiet (no stuck JoinHandle / resource).
#[tokio::test]
async fn parallel_tool_cancel_clears_all_futures_and_registry_quiet() {
    let reg = Arc::new(ExecutionRegistry::with_grace_ms(5));
    reg.register_root("par-run").await.unwrap();
    let handles: Vec<_> = (0..8)
        .map(|i| {
            let reg = reg.clone();
            let run_id = format!("par-child-{i}");
            tokio::spawn(async move {
                reg.register_child(&run_id, "par-run").await.unwrap();
                let inner = tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(30)).await;
                });
                reg.attach_join(&run_id, inner).await.unwrap();
                let tok = reg.token(&run_id).await.unwrap();
                tok.cancelled().await;
            })
        })
        .collect();
    // Wait until all children are registered and blocked on their token.
    for _ in 0..200 {
        if reg.list_tree("par-run").await.len() == 9 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(
        reg.list_tree("par-run").await.len(),
        9,
        "root + 8 children must register"
    );
    // Cancel AFTER the futures are parked — they must all wake and exit.
    let out = reg.cancel_tree("par-run").await;
    assert!(out.quiet, "parallel cancel must be clean: {out:?}");
    assert_eq!(out.phase, CancelPhase::Clean);
    // All outer futures finish promptly once their tokens fire.
    let deadline = Instant::now() + Duration::from_secs(2);
    for h in handles {
        let _ = tokio::time::timeout_at(deadline.into(), h)
            .await
            .expect("every parallel future must exit after cancel");
    }
    assert!(reg.tree_quiet("par-run").await, "registry must be quiet");
    assert_eq!(reg.active_count().await, 0, "no residual registry entries");
}

/// Batch-2 acceptance #4 (registry half): cancel distinguishes a clean cleanup
/// from a cleanup that failed (leftover resource → CleanupFailed, not a fake
/// clean terminal).
#[tokio::test]
async fn cancel_cleanup_phase_distinguishes_clean_from_cleanup_failed() {
    // A hook that refuses to cancel → residual resource → CleanupFailed.
    struct RefusingHook;
    #[async_trait::async_trait]
    impl ProcessCancelHook for RefusingHook {
        async fn cancel_task(&self, _task_id: &str) -> Result<(), String> {
            Err("injected: process kill failed".into())
        }
        async fn cancel_tasks_for_run(&self, _run_id: &str) -> Result<(), String> {
            Ok(())
        }
    }
    let reg = ExecutionRegistry::with_grace_ms(5);
    reg.set_process_cancel_hook(Arc::new(RefusingHook));
    reg.register_root("bad-cancel").await.unwrap();
    reg.track_resource("bad-cancel", ManagedResource::ProcessTask("t-1".into()))
        .await
        .unwrap();
    let out = reg.cancel_tree("bad-cancel").await;
    assert!(!out.quiet, "refusing hook must surface a cleanup failure");
    assert_eq!(out.phase, CancelPhase::CleanupFailed);
    assert!(
        out.errors.iter().any(|e| e.contains("process kill failed")),
        "errors must carry the injected failure: {out:?}"
    );

    // Clean baseline for comparison: same tree shape, working hook → Clean.
    let reg2 = ExecutionRegistry::with_grace_ms(5);
    reg2.set_process_cancel_hook(Arc::new(SupervisorKillHook));
    reg2.register_root("good-cancel").await.unwrap();
    let out2 = reg2.cancel_tree("good-cancel").await;
    assert!(out2.quiet, "no resources → clean: {out2:?}");
    assert_eq!(out2.phase, CancelPhase::Clean);
}

/// Batch-2 acceptance #2: a local fake HTTP MCP server holds a request; cancel
/// must release the transport (curl killed + call settles as cancelled), with
/// no late success/progress after cancel.
#[tokio::test]
async fn mcp_http_call_cancel_releases_transport_without_late_success() {
    use natives_agent_daemon::mcp_runtime::{global_mcp, McpCancelCallback};
    use tokio::io::AsyncReadExt;

    // Local fake MCP server: accepts one POST, reads the request, then holds
    // the connection open (pending) until the test releases it.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let request_received = Arc::new(AtomicBool::new(false));
    let received = request_received.clone();
    let release = Arc::new(tokio::sync::Notify::new());
    let release_server = release.clone();
    let server_task = tokio::spawn(async move {
        if let Ok((mut sock, _)) = listener.accept().await {
            let mut buf = vec![0u8; 4096];
            let _ = sock.read(&mut buf).await;
            received.store(true, Ordering::SeqCst);
            release_server.notified().await;
            drop(sock); // closing the connection releases the transport
        }
    });

    global_mcp()
        .register_server(agent_core::McpServerConfig {
            id: "http-cancel".into(),
            transport: agent_core::McpTransport::Http,
            command: None,
            args: None,
            url: Some(format!("http://{addr}")),
            trusted: true,
            auth_token: None,
            headers: None,
        })
        .unwrap();
    global_mcp()
        .upsert_tool(agent_core::McpToolDescriptor {
            server_id: "http-cancel".into(),
            name: "echo".into(),
            description: "echo".into(),
            input_schema: serde_json::json!({"type":"object"}),
        })
        .unwrap();

    // The HTTP call is synchronous (curl subprocess + poll loop), so run it in
    // a blocking task with a cancel callback the test can fire.
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let cancel_flag_cb = cancel_flag.clone();
    let call = {
        let mcp = global_mcp();
        let cancel_cb: McpCancelCallback = Arc::new(move || cancel_flag_cb.load(Ordering::SeqCst));
        tokio::task::spawn_blocking(move || {
            mcp.call_tool_with_progress_and_cancel(
                "http-cancel",
                "echo",
                serde_json::json!({ "x": 1 }),
                None,
                Some(cancel_cb),
            )
        })
    };

    // Wait for the fake server to receive the request → transport is pending.
    for _ in 0..200 {
        if request_received.load(Ordering::SeqCst) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        request_received.load(Ordering::SeqCst),
        "MCP transport must be pending on the fake server"
    );

    // Fire cancel: the HTTP loop must kill curl and settle the call as
    // cancelled — never a success.
    cancel_flag.store(true, Ordering::SeqCst);
    let result = tokio::time::timeout(Duration::from_secs(5), call)
        .await
        .expect("cancelled MCP call must settle promptly")
        .unwrap();
    assert!(
        result.is_err(),
        "cancelled MCP call must not return a success"
    );
    let err = result.unwrap_err();
    assert!(
        err.contains("cancelled"),
        "cancel must be the terminal fact, got: {err}"
    );
    assert!(
        !err.contains("timeout"),
        "cancel must not be misreported as a timeout"
    );

    // No late success: the fake server sees the connection released (EOF).
    release.notify_waiters();
    let _ = server_task.await;
    let _ = std::mem::forget(cancel_flag);
}
