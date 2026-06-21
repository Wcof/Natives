//! Driver — Local CLI driver for Claude / Codex headless mode
//!
//! Reference: fanbox/electron/wechat/driver.js (187L)

#![allow(dead_code)]

use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};

const LOGIN_SHELL: &str = "/bin/zsh";

/// Run a command with stdin input, returning { ok, out, err, ms }
pub fn run(cmd: &str, stdin_text: &str, cwd: Option<&str>, _idle_ms: u64, max_ms: u64) -> Value {
    let started = std::time::Instant::now();
    let mut child = match Command::new(LOGIN_SHELL)
        .arg("-lc")
        .arg(cmd)
        .current_dir(cwd.unwrap_or("."))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return json!({ "ok": false, "err": e.to_string(), "ms": started.elapsed().as_millis() }),
    };

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(stdin_text.as_bytes());
    }

    let output = match child.wait_with_output() {
        Ok(o) => o,
        Err(e) => return json!({ "ok": false, "err": e.to_string(), "ms": started.elapsed().as_millis() }),
    };

    let elapsed = started.elapsed().as_millis();
    if elapsed > max_ms as u128 {
        return json!({ "ok": false, "err": "max timeout", "timedOut": true, "timeoutReason": "max", "ms": elapsed });
    }

    json!({
        "ok": output.status.success(),
        "code": output.status.code(),
        "out": String::from_utf8_lossy(&output.stdout),
        "err": String::from_utf8_lossy(&output.stderr),
        "ms": elapsed
    })
}
pub fn which(bin: &str) -> bool {
    Command::new(LOGIN_SHELL)
        .arg("-lc")
        .arg(format!("command -v {} || true", bin))
        .output()
        .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
        .unwrap_or(false)
}

/// Detect availability of both claude and codex CLIs
pub fn detect_agents() -> Value {
    json!({
        "claude": which("claude"),
        "codex": which("codex")
    })
}

/// Launch an agent in headless mode with the given input
pub fn launch(agent: &str, input: &str, cwd: &str) -> Value {
    let cmd = match agent {
        "claude" => "claude --print --output-format json",
        "codex" => "codex --print --output-format json",
        _ => return json!({ "ok": false, "err": "unknown agent" }),
    };
    run(cmd, input, Some(cwd), 120000, 1800000)
}
