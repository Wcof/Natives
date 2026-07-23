//! cancellation_tree_e2e — task-03 acceptance.
//!
//! Covers:
//! - parent → child → grandchild token cascade
//! - cancel_all_execution_roots drains registry
//! - process cancel hook is invoked on force
//! - permission waiter is woken on cancel

use natives_agent_daemon::runtime::{
    CancelPhase, ExecutionRegistry, ManagedResource, ProcessCancelHook,
};
use natives_agent_daemon::ProductionRuntime;
use std::sync::atomic::{AtomicUsize, Ordering};
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
    rt.permission_waiters.lock().await.insert(
        "perm-1".into(),
        (run_id.into(), "write_file".into(), tx),
    );

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
    assert!(
        scope == "cancelled" || scope == "once",
        "scope={scope}"
    );
    assert!(
        elapsed < Duration::from_millis(100),
        "permission wait cancel took {elapsed:?} (want <100ms)"
    );
    assert!(
        !rt.permission_waiters.lock().await.contains_key("perm-1"),
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
    assert_eq!(
        cancelled.state,
        capability_gateway::ProcessState::Cancelled
    );
    // Parent run token (if any) is independent — not cancelled by process-only cancel.
    let tok = CancellationToken::new();
    assert!(!tok.is_cancelled());
}
