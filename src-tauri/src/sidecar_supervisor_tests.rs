use super::*;
use std::sync::Mutex;

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
        bootstrap_path: dir.join("boot"),
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
        bootstrap_path: dir.join("boot"),
        daemon_bin: PathBuf::from("/unused"),
        natives_db_path: dir.join("natives.db"),
        assistant_db_path: dir.join("assistant.db"),
        require_uds: true,
        health_timeout: Duration::from_millis(1),
        shutdown_grace: Duration::from_millis(50),
        max_restarts: 1,
    };
    let err = SidecarSupervisor::new(cfg)
        .wait_for_readiness("bootstrap", Duration::from_millis(1))
        .unwrap_err();
    assert!(err.contains("daemon readiness failed"));
    let _ = std::fs::remove_dir_all(&dir);
}

fn spawn_lifeline_fixture(ignore_eof: bool) -> (Child, PathBuf) {
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
    let mut cmd = Command::new(&script);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("NATIVES_PARENT_LIFELINE", "stdio");
    if ignore_eof {
        cmd.env("NATIVES_FIXTURE_IGNORE_EOF", "1");
    }
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
    let child = cmd.spawn().expect("spawn fixture");
    (child, script)
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
        bootstrap_path: dir.join("boot"),
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
