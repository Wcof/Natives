use super::*;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier, Mutex};

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn config_from_env_defaults() {
    let _g = ENV_LOCK.lock().unwrap();
    let prev_runtime = std::env::var("NATIVES_RUNTIME_DIR").ok();
    let prev_socket = std::env::var("NATIVES_DAEMON_SOCKET").ok();
    // from_env() prefers NATIVES_DAEMON_SOCKET over runtime_dir — clear it
    // so this test deterministically asserts the runtime-dir-derived path
    // regardless of what other tests may have left in the environment.
    std::env::set_var("NATIVES_RUNTIME_DIR", "/tmp/natives-sup-test");
    std::env::remove_var("NATIVES_DAEMON_SOCKET");
    let cfg = SupervisorConfig::from_env();
    assert!(cfg.socket_path.starts_with("/tmp/natives-sup-test"));
    if let Some(v) = prev_runtime {
        std::env::set_var("NATIVES_RUNTIME_DIR", v);
    } else {
        std::env::remove_var("NATIVES_RUNTIME_DIR");
    }
    if let Some(v) = prev_socket {
        std::env::set_var("NATIVES_DAEMON_SOCKET", v);
    } else {
        std::env::remove_var("NATIVES_DAEMON_SOCKET");
    }
}

#[test]
fn default_daemon_binary_is_resolved_to_an_existing_path() {
    let _g = ENV_LOCK.lock().unwrap();
    let previous = std::env::var("NATIVES_DAEMON_BIN").ok();
    std::env::remove_var("NATIVES_DAEMON_BIN");
    let daemon_bin = SupervisorConfig::from_env().daemon_bin;
    assert!(
        daemon_bin.is_absolute() && daemon_bin.is_file(),
        "unresolved daemon binary: {}",
        daemon_bin.display()
    );
    if let Some(value) = previous {
        std::env::set_var("NATIVES_DAEMON_BIN", value);
    } else {
        std::env::remove_var("NATIVES_DAEMON_BIN");
    }
}

#[test]
fn require_uds_fault_without_binary() {
    let dir = tempfile_path();
    let _ = std::fs::create_dir_all(&dir);
    let cfg = SupervisorConfig {
        runtime_dir: dir.clone(),
        socket_path: dir.join("t.sock"),
        pid_path: dir.join("t.pid"),
        daemon_bin: PathBuf::from("/nonexistent/natives-agent-daemon-xyz"),
        natives_db_path: dir.join("natives.db"),
        assistant_db_path: dir.join("assistant.db"),
        require_uds: true,
        health_timeout: Duration::from_millis(100),
        shutdown_grace: Duration::from_millis(50),
        max_restarts: 1,
    };
    let sup = SidecarSupervisor::new(cfg);
    let err = sup.ensure_started().unwrap_err();
    assert!(err.contains("no embedded fallback") || err.contains("not found"));
    assert!(matches!(
        sup.status().state,
        SupervisorState::Faulted { .. }
    ));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn validate_db_path_empty() {
    assert!(validate_natives_db_path(Path::new("")).is_err());
}

#[test]
fn readiness_requires_rpc_handshake_not_just_missing_socket() {
    let dir = tempfile_path();
    let _ = std::fs::create_dir_all(&dir);
    let cfg = SupervisorConfig {
        runtime_dir: dir.clone(),
        socket_path: dir.join("t.sock"),
        pid_path: dir.join("t.pid"),
        daemon_bin: PathBuf::from("/unused"),
        natives_db_path: dir.join("natives.db"),
        assistant_db_path: dir.join("assistant.db"),
        require_uds: true,
        health_timeout: Duration::from_millis(1),
        shutdown_grace: Duration::from_millis(50),
        max_restarts: 1,
    };
    let err = SidecarSupervisor::new(cfg)
        .wait_for_readiness("bootstrap", "instance", Duration::from_millis(1))
        .unwrap_err();
    assert!(err.contains("daemon readiness failed"));
    let _ = std::fs::remove_dir_all(&dir);
}

fn spawn_lifeline_fixture(ignore_eof: bool) -> (Child, PathBuf) {
    let script = lifeline_fixture_script();
    let mut cmd = Command::new(&script);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("NATIVES_PARENT_LIFELINE", "stdio");
    if ignore_eof {
        cmd.env("NATIVES_FIXTURE_IGNORE_EOF", "1");
    }
    configure_fixture_process_group(&mut cmd);
    let child = cmd.spawn().expect("spawn fixture");
    (child, script)
}

fn lifeline_fixture_script() -> PathBuf {
    let script = std::env::temp_dir().join(format!("natives-lifeline-fixture-{}.sh", uuid_like()));
    let body = concat!(
        "#!/bin/sh\n",
        "if [ \"${NATIVES_FIXTURE_IGNORE_EOF:-0}\" = \"1\" ]; then\n",
        "  while true; do sleep 0.05; done\n",
        "fi\n",
        "while IFS= read -r _line || [ -n \"$_line\" ]; do\n",
        "  :\n",
        "done\n",
        "exit 0\n",
    );
    std::fs::write(&script, body).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    script
}

fn ignoring_lifeline_fixture_script() -> PathBuf {
    let script = std::env::temp_dir().join(format!("natives-lifeline-ignore-{}.sh", uuid_like()));
    std::fs::write(&script, "#!/bin/sh\nwhile true; do sleep 0.05; done\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    script
}

fn configure_fixture_process_group(cmd: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                if libc::setpgid(0, 0) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
}

fn fixture_config(dir: &Path, daemon_bin: PathBuf, max_restarts: u32) -> SupervisorConfig {
    SupervisorConfig {
        runtime_dir: dir.to_path_buf(),
        socket_path: dir.join("t.sock"),
        pid_path: dir.join("t.pid"),
        daemon_bin,
        natives_db_path: dir.join("natives.db"),
        assistant_db_path: dir.join("assistant.db"),
        require_uds: false,
        health_timeout: Duration::from_millis(100),
        shutdown_grace: Duration::from_millis(200),
        max_restarts,
    }
}

fn wait_timeout(
    child: &mut Child,
    dur: Duration,
) -> std::io::Result<Option<std::process::ExitStatus>> {
    let start = Instant::now();
    loop {
        if let Some(st) = child.try_wait()? {
            return Ok(Some(st));
        }
        if start.elapsed() >= dur {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn lifeline_eof_exits_fixture_without_kill() {
    let (mut child, script) = spawn_lifeline_fixture(false);
    drop(child.stdin.take());
    let status = wait_timeout(&mut child, Duration::from_secs(2))
        .expect("wait")
        .expect("fixture should exit on EOF");
    assert!(status.code().is_some());
    let _ = std::fs::remove_file(script);
}

#[test]
fn force_kill_when_fixture_ignores_graceful() {
    let (mut child, script) = spawn_lifeline_fixture(true);
    drop(child.stdin.take());
    assert!(child.try_wait().unwrap().is_none());
    force_kill_child_tree(&mut child).unwrap();
    let _ = child.wait().unwrap();
    let _ = std::fs::remove_file(script);
}

#[test]
fn shutdown_is_idempotent() {
    let dir = tempfile_path();
    let _ = std::fs::create_dir_all(&dir);
    let (mut child, script) = spawn_lifeline_fixture(false);
    let lifeline = child.stdin.take();
    let cfg = SupervisorConfig {
        runtime_dir: dir.clone(),
        socket_path: dir.join("t.sock"),
        pid_path: dir.join("t.pid"),
        daemon_bin: script.clone(),
        natives_db_path: dir.join("natives.db"),
        assistant_db_path: dir.join("assistant.db"),
        require_uds: false,
        health_timeout: Duration::from_millis(100),
        shutdown_grace: Duration::from_millis(200),
        max_restarts: 1,
    };
    let sup = SidecarSupervisor::new(cfg);
    {
        let mut inner = sup.state.lock().unwrap();
        let pid = child.id();
        inner.child = Some(child);
        inner.lifeline_stdin = lifeline;
        inner.status.state = SupervisorState::Healthy;
        inner.status.pid = Some(pid);
    }
    sup.shutdown().unwrap();
    assert_eq!(sup.status().state, SupervisorState::Stopped);
    sup.shutdown().unwrap();
    assert_eq!(sup.status().state, SupervisorState::Stopped);
    let _ = std::fs::remove_file(script);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn exited_child_is_reaped_and_restarted_with_new_pid() {
    let _peer_lock = crate::credential_broker::credential_broker_uds::lock_broker_peer_for_test();
    let dir = tempfile_path();
    std::fs::create_dir_all(&dir).unwrap();
    let script = lifeline_fixture_script();
    let sup = SidecarSupervisor::new(fixture_config(&dir, script.clone(), 2));
    sup.bypass_readiness_for_test();
    let started = sup.ensure_started().unwrap();
    let old_pid = started.pid.unwrap();
    let old_generation = sup.state.lock().unwrap().broker_peer_generation.unwrap();
    std::thread::sleep(Duration::from_millis(20));
    assert_eq!(sup.poll_child_health().state, SupervisorState::Healthy);

    {
        let mut inner = sup.state.lock().unwrap();
        let child = inner.child.as_mut().unwrap();
        force_kill_child_tree(child).unwrap();
        wait_timeout(child, Duration::from_secs(2))
            .unwrap()
            .expect("killed child should exit");
    }
    let restarted = sup.ensure_healthy_or_restart().unwrap();
    assert_eq!(restarted.state, SupervisorState::Healthy);
    assert_eq!(restarted.restart_count, 1);
    assert_ne!(restarted.pid, Some(old_pid));
    let restarted_generation = sup.state.lock().unwrap().broker_peer_generation.unwrap();
    assert_ne!(
        old_generation, restarted_generation,
        "restart must replace broker peer identity"
    );
    #[cfg(unix)]
    assert_ne!(
        unsafe { libc::kill(old_pid as i32, 0) },
        0,
        "old child was not reaped"
    );

    sup.shutdown().unwrap();
    let _ = std::fs::remove_file(script);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn fresh_start_does_not_consume_restart_budget() {
    let _peer_lock = crate::credential_broker::credential_broker_uds::lock_broker_peer_for_test();
    let dir = tempfile_path();
    std::fs::create_dir_all(&dir).unwrap();
    let script = lifeline_fixture_script();
    let sup = SidecarSupervisor::new(fixture_config(&dir, script.clone(), 2));
    sup.bypass_readiness_for_test();

    let started = sup.ensure_started().unwrap();
    assert_eq!(started.state, SupervisorState::Healthy);
    assert_eq!(started.restart_count, 0);
    assert!(started.pid.is_some());
    assert!(!dir.join("bootstrap.token").exists());

    sup.shutdown().unwrap();
    let _ = std::fs::remove_file(script);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn healthy_child_probe_does_not_restart() {
    let _peer_lock = crate::credential_broker::credential_broker_uds::lock_broker_peer_for_test();
    let dir = tempfile_path();
    std::fs::create_dir_all(&dir).unwrap();
    let script = lifeline_fixture_script();
    let sup = SidecarSupervisor::new(fixture_config(&dir, script.clone(), 1));
    sup.bypass_readiness_for_test();
    sup.bypass_health_probe_for_test();
    let started = sup.ensure_started().unwrap();

    let checked = sup.ensure_healthy_or_restart().unwrap();
    assert_eq!(checked.pid, started.pid);
    assert_eq!(checked.restart_count, 0);

    sup.shutdown().unwrap();
    let _ = std::fs::remove_file(script);
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn alive_unresponsive_child_is_restarted_once_with_bounded_probe() {
    use std::os::unix::net::UnixListener;
    use std::sync::mpsc;

    let _peer_lock = crate::credential_broker::credential_broker_uds::lock_broker_peer_for_test();
    let dir = tempfile_path();
    std::fs::create_dir_all(&dir).unwrap();
    let script = lifeline_fixture_script();
    let sup = SidecarSupervisor::new(fixture_config(&dir, script.clone(), 2));
    sup.bypass_readiness_for_test();
    let started = sup.ensure_started().unwrap();
    let old_pid = started.pid.unwrap();

    let listener = UnixListener::bind(sup.config().socket_path.clone()).unwrap();
    let (accepted_sender, accepted_receiver) = mpsc::channel();
    let (release_sender, release_receiver) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let (_stream, _) = listener.accept().unwrap();
        accepted_sender.send(()).unwrap();
        let _ = release_receiver.recv_timeout(Duration::from_secs(5));
    });

    let started_at = Instant::now();
    let restarted = sup.ensure_healthy_or_restart().unwrap();
    assert!(started_at.elapsed() < Duration::from_secs(3));
    accepted_receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap();
    assert_eq!(restarted.state, SupervisorState::Healthy);
    assert_eq!(restarted.restart_count, 1);
    assert_ne!(restarted.pid, Some(old_pid));

    release_sender.send(()).unwrap();
    server.join().unwrap();
    sup.shutdown().unwrap();
    let _ = std::fs::remove_file(script);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn failed_restart_attempts_exhaust_budget_without_unbounded_retry() {
    let dir = tempfile_path();
    std::fs::create_dir_all(&dir).unwrap();
    let sup = SidecarSupervisor::new(fixture_config(
        &dir,
        PathBuf::from("/nonexistent/natives-agent-daemon-budget"),
        1,
    ));

    let first = sup.ensure_healthy_or_restart().unwrap();
    assert!(matches!(first.state, SupervisorState::Faulted { .. }));
    assert_eq!(first.restart_count, 1);
    let second = sup.ensure_healthy_or_restart().unwrap_err();
    assert!(second.contains("restart budget exhausted (1/1)"));
    assert_eq!(sup.status().restart_count, 1);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn concurrent_restart_callers_claim_only_one_spawn() {
    let _peer_lock = crate::credential_broker::credential_broker_uds::lock_broker_peer_for_test();
    let dir = tempfile_path();
    std::fs::create_dir_all(&dir).unwrap();
    let script = lifeline_fixture_script();
    let sup = Arc::new(SidecarSupervisor::new(fixture_config(
        &dir,
        script.clone(),
        3,
    )));
    sup.bypass_readiness_for_test();
    let barrier = Arc::new(Barrier::new(5));
    let mut threads = Vec::new();
    for _ in 0..4 {
        let sup = Arc::clone(&sup);
        let barrier = Arc::clone(&barrier);
        threads.push(std::thread::spawn(move || {
            barrier.wait();
            sup.ensure_healthy_or_restart()
        }));
    }
    barrier.wait();
    let results: Vec<_> = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect();
    assert!(results.iter().any(Result::is_ok));
    assert!(results.iter().all(|result| {
        result.is_ok() || result.as_ref().unwrap_err().contains("already in progress")
    }));

    let status = sup.status();
    assert_eq!(status.state, SupervisorState::Healthy);
    assert_eq!(status.restart_count, 1);
    assert!(status.pid.is_some());
    sup.shutdown().unwrap();
    let _ = std::fs::remove_file(script);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn shutdown_stops_watchdog_and_refuses_future_restart() {
    let _peer_lock = crate::credential_broker::credential_broker_uds::lock_broker_peer_for_test();
    let dir = tempfile_path();
    std::fs::create_dir_all(&dir).unwrap();
    let script = lifeline_fixture_script();
    let sup = SidecarSupervisor::new(fixture_config(&dir, script.clone(), 2));
    sup.bypass_readiness_for_test();
    sup.ensure_started().unwrap();
    sup.shutdown().unwrap();

    assert!(!sup.watchdog_should_run());
    assert!(sup.ensure_healthy_or_restart().is_err());
    assert_eq!(sup.status().state, SupervisorState::Stopped);
    let _ = std::fs::remove_file(script);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn shutdown_claim_clears_broker_peer_before_waiting_for_child_exit() {
    use std::sync::mpsc;

    let _peer_lock = crate::credential_broker::credential_broker_uds::lock_broker_peer_for_test();
    let dir = tempfile_path();
    std::fs::create_dir_all(&dir).unwrap();
    let script = ignoring_lifeline_fixture_script();
    let sup = Arc::new(SidecarSupervisor::new(fixture_config(
        &dir,
        script.clone(),
        1,
    )));
    sup.bypass_readiness_for_test();
    let pid = sup.ensure_started().unwrap().pid.unwrap();
    assert!(crate::credential_broker::credential_broker_uds::broker_peer_matches(pid));

    let (sender, receiver) = mpsc::channel();
    let shutdown_supervisor = Arc::clone(&sup);
    std::thread::spawn(move || {
        sender
            .send(shutdown_supervisor.shutdown_with_grace(Duration::from_millis(200)))
            .unwrap();
    });

    let deadline = Instant::now() + Duration::from_millis(100);
    while crate::credential_broker::credential_broker_uds::broker_peer_matches(pid)
        && Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        !crate::credential_broker::credential_broker_uds::broker_peer_matches(pid),
        "shutdown claim must revoke broker identity before grace wait"
    );
    assert!(
        receiver.try_recv().is_err(),
        "shutdown should still be in its grace wait"
    );
    receiver
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .unwrap();

    let _ = std::fs::remove_file(script);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn continuous_lifecycle_no_orphan_pids() {
    for _ in 0..20 {
        let (mut child, script) = spawn_lifeline_fixture(false);
        let pid = child.id();
        drop(child.stdin.take());
        let _ = wait_timeout(&mut child, Duration::from_secs(2))
            .unwrap()
            .expect("exit");
        #[cfg(unix)]
        {
            let alive = unsafe { libc::kill(pid as i32, 0) == 0 };
            assert!(!alive, "orphan pid {pid} still alive");
        }
        let _ = std::fs::remove_file(script);
    }
}

#[test]
fn stale_ownership_never_auto_kills_unknown_pid() {
    let record = serde_json::json!({
        "daemon_pid": 1,
        "daemon_bin": "/usr/bin/true",
        "instance_id": "other",
    });
    assert!(!SidecarSupervisor::should_reap_stale_ownership(
        &record,
        Path::new("/usr/bin/true"),
        Some("mine")
    ));
    assert!(!SidecarSupervisor::should_reap_stale_ownership(
        &record,
        Path::new("/usr/bin/true"),
        Some("other")
    ));
}

fn tempfile_path() -> PathBuf {
    std::env::temp_dir().join(format!("natives-sup-{}", uuid_like()))
}
