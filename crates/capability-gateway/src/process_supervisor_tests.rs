//! Process supervisor tests (extracted from process_supervisor.rs).

use super::*;

use super::*;

#[tokio::test]
async fn fake_supervisor_run_isolation() {
    let sup = FakeProcessSupervisor::new();
    let a = sup
        .spawn(ProcessSpec {
            run_id: "run-a".into(),
            task_id: "t1".into(),
            display_command: "echo a".into(),
            program: "echo".into(),
            args: vec![],
            cwd: PathBuf::from("."),
            timeout_ms: 1000,
            background: false,
        })
        .await
        .unwrap();
    assert_eq!(a.run_id, "run-a");
    let list_b = sup.list_for_run("run-b").await;
    assert!(list_b.is_empty());
    let list_a = sup.list_for_run("run-a").await;
    assert_eq!(list_a.len(), 1);
}

#[tokio::test]
async fn fake_cancel() {
    let sup = FakeProcessSupervisor::new();
    sup.spawn(ProcessSpec {
        run_id: "r".into(),
        task_id: "t".into(),
        display_command: "x".into(),
        program: "x".into(),
        args: vec![],
        cwd: PathBuf::from("."),
        timeout_ms: 1000,
        background: true,
    })
    .await
    .unwrap();
    let snap = sup.cancel("t").await.unwrap();
    assert_eq!(snap.state, ProcessState::Cancelled);
}

#[test]
fn cwd_must_stay_in_project() {
    let tmp = std::env::temp_dir().join(format!("natives-cwd-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(tmp.join("sub")).unwrap();
    let ok = resolve_cwd(&tmp, Some("sub")).unwrap();
    let root = tmp.canonicalize().unwrap();
    assert!(ok.starts_with(&root));
    assert!(resolve_cwd(&tmp, Some("../x")).is_err());
    let _ = std::fs::remove_dir_all(&tmp);
}

// -----------------------------------------------------------------------
// TASK-002 (J02): bounded concurrent drain, Unicode-safe truncation, and
// cancel/timeout reaping. These fail against the pre-fix supervisor
// (post-exit-only drain deadlocks on >64KB output; byte-slicing panics on
// multi-byte characters).
// -----------------------------------------------------------------------

fn empty_proc() -> LiveProcess {
    LiveProcess {
        run_id: "run-shell".into(),
        display_command: "test".into(),
        child: None,
        state: ProcessState::Running,
        exit_code: None,
        stdout: String::new(),
        stderr: String::new(),
        pending: Vec::new(),
        truncated: false,
        background: false,
        persisted_bytes: 0,
        readers: Vec::new(),
    }
}

fn sh_spec(task_id: &str, command: &str, background: bool) -> ProcessSpec {
    ProcessSpec {
        run_id: "run-shell".into(),
        task_id: task_id.into(),
        display_command: command.into(),
        program: "/bin/sh".into(),
        args: vec!["-lc".into(), command.into()],
        cwd: std::env::temp_dir(),
        timeout_ms: 60_000,
        background,
    }
}

#[tokio::test]
async fn shell_10mb_stdout_completes_without_deadlock() {
    let sup = LocalProcessSupervisor::with_budget(2_000);
    let snap = sup
        .spawn(sh_spec("t-10mb", "head -c 10485760 /dev/zero; true", false))
        .await
        .unwrap();
    assert!(
        matches!(snap.state, ProcessState::Completed | ProcessState::Failed),
        "10MB stdout must not deadlock the child, got {:?}",
        snap.state
    );
    assert_eq!(snap.exit_code, Some(0));
}

#[tokio::test]
async fn shell_10mb_both_streams_interleaved_completes() {
    let sup = LocalProcessSupervisor::with_budget(3_000);
    // 10MB on stdout and 10MB on stderr, produced concurrently.
    let cmd = "(head -c 10485760 /dev/zero) & (head -c 10485760 /dev/zero >&2) & wait";
    let snap = sup.spawn(sh_spec("t-2x10mb", cmd, false)).await.unwrap();
    assert!(
        matches!(snap.state, ProcessState::Completed | ProcessState::Failed),
        "interleaved 10MB streams must not deadlock, got {:?}",
        snap.state
    );
    assert_eq!(snap.exit_code, Some(0));
}

#[test]
fn shell_unicode_tail_never_panics() {
    let s = "你好世界🌍🎉日本語テキスト";
    for max in 0..=s.len() {
        let _ = tail(s, max);
    }
    // A cap smaller than a multi-byte char must not slice mid-character.
    let _ = tail("你", 1);
}

#[test]
fn shell_unicode_append_capped_never_panics() {
    let text = "你好世界🌍🎉日本語テキスト";
    // Remaining budgets of 1..4 bytes must never split a 3/4-byte char.
    for rem in 1..=4usize {
        let mut proc = empty_proc();
        proc.persisted_bytes = 1_048_576 - rem;
        append_capped(&mut proc, "stdout", text);
        assert!(proc.stdout.len() <= 1_048_576, "persist cap exceeded");
    }
}

#[tokio::test]
async fn shell_cancel_kills_waits_and_reaps() {
    let sup = LocalProcessSupervisor::new();
    sup.spawn(sh_spec("t-cancel", "sleep 30", false))
        .await
        .unwrap();
    let snap = sup.cancel("t-cancel").await.unwrap();
    assert_eq!(snap.state, ProcessState::Cancelled);
    let after = sup.poll("t-cancel").await.unwrap();
    assert_eq!(after.state, ProcessState::Cancelled, "child must be reaped");
}

#[tokio::test]
async fn shell_cancel_unknown_task_errors_gracefully() {
    let sup = LocalProcessSupervisor::new();
    let err = sup.cancel("no-such-task").await.unwrap_err();
    assert!(err.contains("unknown task"), "got: {err}");
}

#[tokio::test]
async fn shell_background_output_captured_after_completion() {
    let sup = LocalProcessSupervisor::new();
    sup.spawn(sh_spec("t-bg", "printf 'bg-out-你好'", true))
        .await
        .unwrap();
    let snap = sup.wait("t-bg", 5_000).await.unwrap();
    assert!(matches!(
        snap.state,
        ProcessState::Completed | ProcessState::Failed
    ));
    assert!(
        snap.stdout_tail.contains("bg-out-你好"),
        "background output must be captured, got {:?}",
        snap.stdout_tail
    );
}

// -----------------------------------------------------------------------
// T04: real-process cancel evidence — TERM-trap, 1GB output, parallel
// cancel, and process-tree kill. These fail against a supervisor that only
// SIGKILLs the direct child or drains output after exit.
// -----------------------------------------------------------------------

/// A real child that records SIGTERM and keeps running (ignores it) must
/// receive TERM first and then be escalated to KILL within the budget.
#[cfg(unix)]
#[tokio::test]
async fn shell_trap_term_child_is_terminated_within_budget() {
    let dir = std::env::temp_dir().join(format!("ps-term-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let marker = dir.join("term.txt");
    let script = dir.join("trap_term.py");
    std::fs::write(
        &script,
        format!(
            r#"import signal, time, sys
def handler(signum, frame):
    open({marker:?}, "w").write("term")
signal.signal(signal.SIGTERM, handler)
print("ready", flush=True)
while True:
    time.sleep(1)
"#
        ),
    )
    .unwrap();

    let sup = LocalProcessSupervisor::new();
    let task_id = format!("t-term-{}", uuid::Uuid::new_v4());
    let _snap = sup
        .spawn(ProcessSpec {
            run_id: "run-shell".into(),
            task_id: task_id.clone(),
            display_command: "trap_term.py".into(),
            program: "python3".into(),
            args: vec![script.to_string_lossy().to_string()],
            cwd: dir.clone(),
            timeout_ms: 60_000,
            background: true,
        })
        .await
        .expect("spawn trap_term.py (python3 required)");

    // Give the child a moment to install its handler.
    tokio::time::sleep(Duration::from_millis(300)).await;

    let started = Instant::now();
    let cancelled = sup.cancel(&task_id).await.unwrap();
    assert_eq!(cancelled.state, ProcessState::Cancelled);
    assert!(
        started.elapsed() < Duration::from_secs(8),
        "TERM→KILL escalation took too long: {:?}",
        started.elapsed()
    );

    // TERM must have been attempted (the handler ran) before KILL reaped it.
    assert!(
        marker.exists(),
        "child must have received SIGTERM before escalation, but {} does not exist",
        marker.display()
    );
    // Child must be reaped (not left as a zombie/runner).
    let after = sup.poll(&task_id).await.unwrap();
    assert_eq!(after.state, ProcessState::Cancelled);
    assert_eq!(
        after.exit_code, None,
        "cancelled child has no exit code yet"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// 1GB stdout must complete without deadlocking and must never grow the
/// persisted buffer past the 1MiB cap.
#[tokio::test]
async fn shell_1gb_stdout_bounded_memory_and_no_deadlock() {
    let sup = LocalProcessSupervisor::with_budget(2_000);
    let snap = tokio::time::timeout(
        Duration::from_secs(120),
        sup.spawn(sh_spec(
            "t-1gb",
            "head -c 1073741824 /dev/zero; true",
            false,
        )),
    )
    .await
    .expect("1GB producer must not hang")
    .unwrap();
    assert!(
        matches!(snap.state, ProcessState::Completed | ProcessState::Failed),
        "1GB stdout must not deadlock, got {:?}",
        snap.state
    );
    assert_eq!(snap.exit_code, Some(0));
    // The supervisor caps persisted output at 1MiB per stream.
    let final_snap = sup.wait("t-1gb", 30_000).await.unwrap();
    assert!(final_snap.stdout_tail.len() <= 8_192, "tail bounded");
    assert!(final_snap.truncated, "1GB must be flagged truncated");
}

/// Two concurrent cancels of the same task must both settle, with the
/// child reaped exactly once and no registry lock deadlock.
#[tokio::test]
async fn parallel_cancel_same_task_is_safe() {
    let sup = Arc::new(LocalProcessSupervisor::new());
    let task_id = format!("t-par-{}", uuid::Uuid::new_v4());
    sup.spawn(sh_spec(&task_id, "sleep 30", true))
        .await
        .unwrap();

    let a = sup.clone();
    let b = sup.clone();
    let tid_a = task_id.clone();
    let tid_b = task_id.clone();
    let (ra, rb) = tokio::join!(async move { a.cancel(&tid_a).await }, async move {
        b.cancel(&tid_b).await
    },);
    assert_eq!(ra.unwrap().state, ProcessState::Cancelled);
    assert_eq!(rb.unwrap().state, ProcessState::Cancelled);
    let after = sup.poll(&task_id).await.unwrap();
    assert_eq!(after.state, ProcessState::Cancelled);
}

/// Cancelling a shell that spawned a background child must kill the whole
/// process group, not just the direct shell.
#[cfg(unix)]
#[tokio::test]
async fn parent_cancel_kills_process_tree() {
    let dir = std::env::temp_dir().join(format!("ps-tree-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let pidfile = dir.join("child.pid");
    let cmd = format!("sleep 30 & echo $! > {}; wait", pidfile.to_string_lossy());
    let sup = LocalProcessSupervisor::new();
    let task_id = format!("t-tree-{}", uuid::Uuid::new_v4());
    sup.spawn(ProcessSpec {
        run_id: "run-shell".into(),
        task_id: task_id.clone(),
        display_command: cmd.clone(),
        program: "/bin/sh".into(),
        args: vec!["-lc".into(), cmd],
        cwd: dir.clone(),
        timeout_ms: 60_000,
        background: true,
    })
    .await
    .unwrap();

    // Wait until the background child pid is recorded, then cancel.
    let child_pid = {
        let mut deadline = 0;
        loop {
            if pidfile.exists() {
                let raw = std::fs::read_to_string(&pidfile).unwrap_or_default();
                if let Ok(pid) = raw.trim().parse::<i32>() {
                    break pid;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
            deadline += 1;
            assert!(deadline < 100, "background child pid never recorded");
        }
    };

    let cancelled = sup.cancel(&task_id).await.unwrap();
    assert_eq!(cancelled.state, ProcessState::Cancelled);

    // The grandchild must be gone too (same process group, TERM/KILL).
    let mut gone = false;
    for _ in 0..50 {
        // kill(pid, 0) probes liveness without signalling.
        let alive = unsafe { libc::kill(child_pid, 0) } == 0;
        if !alive {
            gone = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(gone, "grandchild {child_pid} survived parent cancel");
    let _ = std::fs::remove_dir_all(&dir);
}
