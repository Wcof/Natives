//! Real Host–Daemon lifecycle e2e (Phase 5 / task-13).
//!
//! Uses the real `natives-agent-daemon` binary when available in PATH / target/.
//! Skips cleanly when the binary is not built.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

fn find_daemon_binary() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("NATIVES_DAEMON_BIN") {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return Some(pb);
        }
    }
    let candidates = [
        "target/debug/natives-agent-daemon",
        "target/release/natives-agent-daemon",
        "../target/debug/natives-agent-daemon",
        "../target/release/natives-agent-daemon",
    ];
    for c in candidates {
        let p = PathBuf::from(c);
        if p.is_file() {
            return Some(p);
        }
    }
    which("natives-agent-daemon")
}

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let c = dir.join(name);
        if c.is_file() {
            return Some(c);
        }
    }
    None
}

fn wait_with_timeout(
    child: &mut std::process::Child,
    timeout: Duration,
) -> Option<std::process::ExitStatus> {
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(s)) => return Some(s),
            Ok(None) if start.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Err(_) => return None,
        }
    }
}

#[test]
fn daemon_exits_on_stdin_eof_lifeline() {
    let Some(bin) = find_daemon_binary() else {
        eprintln!("skip: natives-agent-daemon binary not found");
        return;
    };
    let tmp = tempfile::tempdir().expect("tempdir");
    let socket = tmp.path().join("daemon.sock");
    let mut child = Command::new(&bin)
        .env("NATIVES_PARENT_LIFELINE", "stdio")
        .env(
            "NATIVES_DAEMON_SOCKET",
            socket.to_string_lossy().to_string(),
        )
        .env("NATIVES_DAEMON_INSTANCE_ID", "e2e-lifeline")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn daemon");
    // Drop stdin → EOF should trigger parent_lifeline exit.
    drop(child.stdin.take());
    let status = wait_with_timeout(&mut child, Duration::from_secs(8));
    assert!(
        status.is_some(),
        "daemon must exit after stdin EOF within grace"
    );
}
