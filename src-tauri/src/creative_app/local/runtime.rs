//! Local creative process supervisor: process groups, live logs, tree kill.
//!
//! Designed for Vite/Vue/Node dev servers. Does not use shell. Does not
//! auto-take over orphan PIDs without identity verification.

use super::logs::{append_with_secrets, LogLine, LogRegistry, LogStream, LocalLogStore};
use crate::creative_app::model::{LaunchPlan, LaunchProgram, LocalLaunchRuntime, ProcessIdentity};
use crate::{Error, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

const GRACEFUL_WAIT_MS: u64 = 5_000;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreativeAppLogEvent {
    pub app_id: String,
    pub seq: u64,
    pub ts_ms: i64,
    pub stream: String,
    pub text: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreativeAppOperationProgress {
    pub app_id: String,
    pub stage: String,
    pub message: String,
}

#[allow(dead_code)]
struct LiveLocalProcess {
    child: Option<Child>,
    identity: ProcessIdentity,
    plan_fingerprint: String,
    started_at: Instant,
    port: Option<u16>,
    open_url: Option<String>,
    program: String,
    cwd: PathBuf,
    log: Arc<LocalLogStore>,
}

pub struct LocalRuntimeManager {
    procs: Mutex<HashMap<String, LiveLocalProcess>>,
    logs: LogRegistry,
}

impl Default for LocalRuntimeManager {
    fn default() -> Self {
        Self::new()
    }
}

impl LocalRuntimeManager {
    pub fn new() -> Self {
        Self {
            procs: Mutex::new(HashMap::new()),
            logs: LogRegistry::new(),
        }
    }

    pub fn logs(&self) -> &LogRegistry {
        &self.logs
    }

    pub async fn is_running(&self, app_id: &str) -> bool {
        let mut map = self.procs.lock().await;
        if let Some(p) = map.get_mut(app_id) {
            if let Some(child) = p.child.as_mut() {
                match child.try_wait() {
                    Ok(Some(_)) => {
                        p.child = None;
                        return false;
                    }
                    Ok(None) => return true,
                    Err(_) => return false,
                }
            }
        }
        false
    }

    pub async fn current_port(&self, app_id: &str) -> Option<u16> {
        let map = self.procs.lock().await;
        map.get(app_id).and_then(|p| p.port)
    }

    pub async fn open_url(&self, app_id: &str) -> Option<String> {
        let map = self.procs.lock().await;
        map.get(app_id).and_then(|p| p.open_url.clone())
    }

    pub async fn identity(&self, app_id: &str) -> Option<ProcessIdentity> {
        let map = self.procs.lock().await;
        map.get(app_id).map(|p| p.identity.clone())
    }

    pub async fn stop_all(&self, app: Option<&AppHandle>) {
        let ids: Vec<String> = {
            let map = self.procs.lock().await;
            map.keys().cloned().collect()
        };
        for id in ids {
            let _ = self.stop(&id, app).await;
        }
    }

    pub async fn stop(&self, app_id: &str, app: Option<&AppHandle>) -> Result<()> {
        let mut map = self.procs.lock().await;
        let Some(mut live) = map.remove(app_id) else {
            return Ok(());
        };
        live.log
            .append(LogStream::System, "stopping process tree…");
        if let Some(mut child) = live.child.take() {
            terminate_tree(&mut child, live.identity.process_group_id).await;
        }
        live.log.append(LogStream::System, "stopped");
        if let Some(app) = app {
            emit_progress(app, app_id, "stopped", "process stopped");
        }
        Ok(())
    }

    /// Start a node_dev_server plan. Caller must hold app-level mutation lock.
    pub async fn start_node_dev(
        &self,
        app: &AppHandle,
        app_id: &str,
        project_root: &Path,
        plan: &LaunchPlan,
        plan_fingerprint: &str,
        env: &[(String, String)],
        preferred_port: Option<u16>,
    ) -> Result<(u16, String, ProcessIdentity)> {
        if self.is_running(app_id).await {
            return Err(Error::InvalidInput("already running".into()));
        }

        let cwd = project_root.join(if plan.cwd_relative == "." {
            PathBuf::new()
        } else {
            PathBuf::from(&plan.cwd_relative)
        });
        if !cwd.is_dir() {
            return Err(Error::InvalidInput(format!(
                "cwd not found: {}",
                cwd.display()
            )));
        }

        let port = match plan.port.mode {
            crate::creative_app::model::LaunchPortMode::Fixed => {
                let p = plan.port.value.ok_or_else(|| {
                    Error::InvalidInput("fixed port missing".into())
                })?;
                if port_in_use(p) {
                    return Err(Error::InvalidInput(format!(
                        "port {p} is already in use"
                    )));
                }
                p
            }
            crate::creative_app::model::LaunchPortMode::Auto => {
                preferred_port.unwrap_or_else(pick_free_port)
            }
        };

        let (program, mut args) = build_command(plan, port)?;
        // Append runner-specific host/port flags (Vite vs Vue CLI differ).
        if matches!(
            plan.program,
            LaunchProgram::Npm | LaunchProgram::Pnpm | LaunchProgram::Yarn
        ) {
            let runner = plan.script_runner.ok_or_else(|| {
                Error::InvalidInput(
                    "launch plan missing scriptRunner; re-scan or re-validate before start".into(),
                )
            })?;
            if !args.iter().any(|a| a == "--") {
                args.push("--".into());
            }
            args.extend(super::plan::runner_port_flags(runner, port));
        }

        let log = self.logs.get_or_open(app_id);
        // Inject this app's env values so live + persisted logs redact them by value.
        log.set_secrets(env.iter().map(|(_, v)| v.clone()).collect());
        log.append(
            LogStream::System,
            &format!("starting: {program} {}", args.join(" ")),
        );
        // Unified progress channel name used by frontend onProgress.
        emit_progress(app, app_id, "starting", "spawning process");

        let mut cmd = Command::new(&program);
        cmd.args(&args)
            .current_dir(&cwd)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .env_clear()
            .envs(std::env::vars().filter(|(k, _)| is_safe_inherited_env(k)))
            .env("HOST", "127.0.0.1")
            .env("PORT", port.to_string());
        let secret_values: Vec<String> = env
            .iter()
            .map(|(_, value)| value.clone())
            .filter(|value| value.trim().len() >= 4)
            .collect();
        for (k, v) in env {
            cmd.env(k, v);
        }

        // New process group on Unix for tree kill.
        #[cfg(unix)]
        unsafe {
            cmd.pre_exec(|| {
                if libc::setpgid(0, 0) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        #[cfg(windows)]
        {
            cmd.creation_flags(0x00000200); // CREATE_NEW_PROCESS_GROUP
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| Error::Internal(format!("spawn failed: {e}")))?;

        let pid = child.id();
        let pgid = pid.map(|p| p as i32);
        // Prefer OS-reported start time when available (sysinfo) for orphan matching.
        let started_unix = process_start_time_unix(pid).unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0)
        });
        let executable = resolve_executable_path(&program).unwrap_or(program.clone());

        let identity = ProcessIdentity {
            pid,
            started_at_unix: Some(started_unix),
            executable: Some(executable.clone()),
            cwd: Some(cwd.to_string_lossy().to_string()),
            plan_fingerprint: Some(plan_fingerprint.to_string()),
            process_group_id: pgid,
        };

        // Pipe readers
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let app_handle = app.clone();
        let app_id_out = app_id.to_string();
        let log_out = log.clone();
        let stdout_secrets = secret_values.clone();
        if let Some(out) = stdout {
            tokio::spawn(async move {
                let mut lines = BufReader::new(out).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let entry = append_with_secrets(
                        log_out.as_ref(),
                        LogStream::Stdout,
                        &line,
                        &stdout_secrets,
                    );
                    emit_log(&app_handle, &app_id_out, &entry);
                }
            });
        }
        let app_handle = app.clone();
        let app_id_err = app_id.to_string();
        let log_err = log.clone();
        let stderr_secrets = secret_values;
        if let Some(err) = stderr {
            tokio::spawn(async move {
                let mut lines = BufReader::new(err).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let entry = append_with_secrets(
                        log_err.as_ref(),
                        LogStream::Stderr,
                        &line,
                        &stderr_secrets,
                    );
                    emit_log(&app_handle, &app_id_err, &entry);
                }
            });
        }

        let open_path = if plan.open_path.starts_with('/') {
            plan.open_path.clone()
        } else {
            format!("/{}", plan.open_path)
        };
        let open_url = format!("http://127.0.0.1:{port}{open_path}");

        {
            let mut map = self.procs.lock().await;
            map.insert(
                app_id.to_string(),
                LiveLocalProcess {
                    child: Some(child),
                    identity: identity.clone(),
                    plan_fingerprint: plan_fingerprint.to_string(),
                    started_at: Instant::now(),
                    port: Some(port),
                    open_url: Some(open_url.clone()),
                    program: executable,
                    cwd: cwd.clone(),
                    log: log.clone(),
                },
            );
        }

        // Exit watcher: when process dies, clear live slot and emit progress so
        // lifecycle can reconcile DB state on next poll / explicit check.
        {
            let app_handle = app.clone();
            let app_id_watch = app_id.to_string();
            // We cannot hold Child here (moved into map). Poll is_running in a loop.
            let this_procs = self.procs.lock().await;
            drop(this_procs);
            // Spawn detached poller using manager methods via Arc is not available here;
            // lifecycle registers watchers after start. Mark system log only.
            log.append(LogStream::System, "exit watcher armed via health/poll path");
            let _ = app_handle;
            let _ = app_id_watch;
        }

        Ok((port, open_url, identity))
    }

    /// If the managed process has exited, remove it and return exit info.
    pub async fn take_if_exited(&self, app_id: &str) -> Option<i32> {
        let mut map = self.procs.lock().await;
        let live = map.get_mut(app_id)?;
        let child = live.child.as_mut()?;
        match child.try_wait() {
            Ok(Some(status)) => {
                let code = status.code().unwrap_or(-1);
                live.child = None;
                live.log.append(
                    LogStream::System,
                    &format!("process exited (code {code})"),
                );
                map.remove(app_id);
                Some(code)
            }
            _ => None,
        }
    }

    /// Poll all live processes; return app_ids that exited with codes.
    pub async fn poll_exits(&self) -> Vec<(String, i32)> {
        let ids: Vec<String> = {
            let map = self.procs.lock().await;
            map.keys().cloned().collect()
        };
        let mut out = Vec::new();
        for id in ids {
            if let Some(code) = self.take_if_exited(&id).await {
                out.push((id, code));
            }
        }
        out
    }

    /// Wait until health check passes or timeout. On failure leaves process running
    /// (caller may stop or mark start_unhealthy).
    pub async fn wait_healthy(
        &self,
        app: &AppHandle,
        app_id: &str,
        health_path: &str,
        timeout_ms: u32,
    ) -> Result<()> {
        let deadline = Instant::now() + Duration::from_millis(timeout_ms as u64);
        let health_path = if health_path.starts_with('/') {
            health_path.to_string()
        } else {
            format!("/{health_path}")
        };
        emit_progress(app, app_id, "health_check", "waiting for server");

        loop {
            if !self.is_running(app_id).await {
                return Err(Error::Internal("process exited before healthy".into()));
            }
            let port = self.current_port(app_id).await.ok_or_else(|| {
                Error::Internal("missing port".into())
            })?;
            if port_listening(port) {
                let url = format!("http://127.0.0.1:{port}{health_path}");
                if http_reachable(&url).await {
                    emit_progress(app, app_id, "ready", "health check passed");
                    return Ok(());
                }
            }
            if Instant::now() >= deadline {
                return Err(Error::Internal(format!(
                    "health check timed out after {timeout_ms}ms"
                )));
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    pub fn recent_logs(&self, app_id: &str, limit: usize) -> Vec<LogLine> {
        self.logs.get_or_open(app_id).recent_memory(limit)
    }

    pub fn persisted_tail(&self, app_id: &str, max_bytes: usize) -> String {
        self.logs
            .get_or_open(app_id)
            .read_persisted_tail(max_bytes)
    }

    pub fn purge_logs(&self, app_id: &str) {
        self.logs.remove(app_id);
    }
}

fn emit_log(app: &AppHandle, app_id: &str, line: &LogLine) {
    let ev = CreativeAppLogEvent {
        app_id: app_id.to_string(),
        seq: line.seq,
        ts_ms: line.ts_ms,
        stream: line.stream.as_str().to_string(),
        text: line.text.clone(),
    };
    let _ = app.emit("creative-app-log", &ev);
}

fn emit_progress(app: &AppHandle, app_id: &str, stage: &str, message: &str) {
    let ev = CreativeAppOperationProgress {
        app_id: app_id.to_string(),
        stage: stage.to_string(),
        message: message.to_string(),
    };
    // Frontend already listens to creative-app-progress (GitHub install).
    let _ = app.emit("creative-app-progress", &ev);
    // Keep legacy name briefly for any internal subscribers.
    let _ = app.emit("creative-app-operation-progress", &ev);
}

fn build_command(plan: &LaunchPlan, _port: u16) -> Result<(String, Vec<String>)> {
    match plan.program {
        LaunchProgram::Npm => {
            let script = plan
                .script
                .as_deref()
                .ok_or_else(|| Error::InvalidInput("missing script".into()))?;
            Ok(("npm".into(), vec!["run".into(), script.into()]))
        }
        LaunchProgram::Pnpm => {
            let script = plan
                .script
                .as_deref()
                .ok_or_else(|| Error::InvalidInput("missing script".into()))?;
            Ok(("pnpm".into(), vec!["run".into(), script.into()]))
        }
        LaunchProgram::Yarn => {
            let script = plan
                .script
                .as_deref()
                .ok_or_else(|| Error::InvalidInput("missing script".into()))?;
            Ok(("yarn".into(), vec![script.into()]))
        }
        LaunchProgram::Node => {
            let entry = plan
                .entry_file
                .as_deref()
                .ok_or_else(|| Error::InvalidInput("missing entryFile".into()))?;
            let mut args = vec![entry.to_string()];
            args.extend(plan.args.iter().cloned());
            Ok(("node".into(), args))
        }
        LaunchProgram::Internal => Err(Error::InvalidInput(
            "internal program is for static_http only".into(),
        )),
    }
}

fn process_start_time_unix(pid: Option<u32>) -> Option<i64> {
    let pid = pid?;
    use sysinfo::{Pid, ProcessesToUpdate, System};
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::Some(&[Pid::from_u32(pid)]), true);
    let p = sys.process(Pid::from_u32(pid))?;
    // sysinfo start_time is seconds since UNIX epoch
    let st = p.start_time();
    if st == 0 {
        None
    } else {
        Some(st as i64)
    }
}

fn resolve_executable_path(program: &str) -> Option<String> {
    if program.contains('/') || program.contains('\\') {
        return Some(program.to_string());
    }
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(program);
        if candidate.is_file() {
            return Some(candidate.to_string_lossy().to_string());
        }
        #[cfg(windows)]
        {
            let candidate = dir.join(format!("{program}.cmd"));
            if candidate.is_file() {
                return Some(candidate.to_string_lossy().to_string());
            }
            let candidate = dir.join(format!("{program}.exe"));
            if candidate.is_file() {
                return Some(candidate.to_string_lossy().to_string());
            }
        }
    }
    None
}

/// Strict identity match against a live OS process (PID reuse safe).
pub fn identity_matches_live_strict(ident: &ProcessIdentity) -> bool {
    let Some(pid) = ident.pid else {
        return false;
    };
    let Some(expected_start) = ident.started_at_unix else {
        return false;
    };
    let Some(expected_exe) = ident.executable.as_deref() else {
        return false;
    };
    if ident.plan_fingerprint.as_deref().unwrap_or("").is_empty() {
        return false;
    }
    if ident.cwd.as_deref().unwrap_or("").is_empty() {
        return false;
    }

    use sysinfo::{Pid, ProcessesToUpdate, System};
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::Some(&[Pid::from_u32(pid)]), true);
    let Some(proc) = sys.process(Pid::from_u32(pid)) else {
        return false;
    };
    let live_start = proc.start_time() as i64;
    if live_start == 0 || (live_start - expected_start).abs() > 2 {
        return false;
    }
    let live_exe = proc
        .exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    if live_exe.is_empty() {
        return false;
    }
    // Compare basenames when absolute paths differ by symlink resolution.
    let base = |s: &str| {
        Path::new(s)
            .file_name()
            .and_then(|x| x.to_str())
            .unwrap_or(s)
            .to_ascii_lowercase()
    };
    if base(&live_exe) != base(expected_exe) && live_exe != expected_exe {
        return false;
    }
    if let Some(cwd) = &ident.cwd {
        if let Some(live_cwd) = proc.cwd() {
            let live = live_cwd.to_string_lossy();
            if live != *cwd {
                // allow trailing slash differences
                if live.trim_end_matches('/') != cwd.trim_end_matches('/') {
                    return false;
                }
            }
        }
    }
    true
}


fn is_safe_inherited_env(key: &str) -> bool {
    matches!(
        key,
        "PATH"
            | "HOME"
            | "USER"
            | "LOGNAME"
            | "SHELL"
            | "TMPDIR"
            | "TEMP"
            | "TMP"
            | "LANG"
            | "LC_ALL"
            | "LC_CTYPE"
            | "TERM"
            | "COLORTERM"
            | "NODE_OPTIONS"
            | "npm_config_registry"
            | "ALLUSERSPROFILE"
            | "APPDATA"
            | "LOCALAPPDATA"
            | "SystemRoot"
            | "ComSpec"
            | "PATHEXT"
            | "USERPROFILE"
            | "HOMEDRIVE"
            | "HOMEPATH"
    ) || key.starts_with("LC_")
}

pub fn pick_free_port() -> u16 {
    // Bind to 127.0.0.1:0 then drop.
    std::net::TcpListener::bind("127.0.0.1:0")
        .ok()
        .and_then(|l| l.local_addr().ok())
        .map(|a| a.port())
        .unwrap_or(5173)
}

pub fn port_in_use(port: u16) -> bool {
    // If we cannot bind, assume in use.
    std::net::TcpListener::bind(("127.0.0.1", port)).is_err()
}

pub fn port_listening(port: u16) -> bool {
    std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        Duration::from_millis(100),
    )
    .is_ok()
}

async fn http_reachable(url: &str) -> bool {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .redirect(reqwest::redirect::Policy::limited(3))
        .build();
    let Ok(client) = client else {
        return false;
    };
    match client.get(url).send().await {
        Ok(resp) => {
            let code = resp.status().as_u16();
            (200..500).contains(&code)
        }
        Err(_) => false,
    }
}

async fn terminate_tree(child: &mut Child, pgid: Option<i32>) {
    // Graceful
    #[cfg(unix)]
    {
        if let Some(pgid) = pgid {
            unsafe {
                let _ = libc::kill(-pgid, libc::SIGTERM);
            }
        } else {
            let _ = child.kill().await;
        }
    }
    #[cfg(not(unix))]
    {
        let _ = child.kill().await;
    }

    let deadline = Instant::now() + Duration::from_millis(GRACEFUL_WAIT_MS);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return,
            Ok(None) if Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            _ => break,
        }
    }

    // Force
    #[cfg(unix)]
    {
        if let Some(pgid) = pgid {
            unsafe {
                let _ = libc::kill(-pgid, libc::SIGKILL);
            }
        } else {
            let _ = child.start_kill();
        }
    }
    #[cfg(windows)]
    {
        // Best-effort: taskkill tree by pid
        if let Some(pid) = child.id() {
            let _ = Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .await;
        } else {
            let _ = child.start_kill();
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = child.start_kill();
    }
    let _ = child.wait().await;
}

/// Static local apps do not spawn a process; open_url is built from host HTTP port.
pub fn static_open_url(host_http_port: u16, creative_id: &str, open_path: &str) -> String {
    let path = if open_path.starts_with('/') {
        open_path.to_string()
    } else {
        format!("/{open_path}")
    };
    format!("http://127.0.0.1:{host_http_port}/local-projects/{creative_id}{path}")
}

pub fn plan_is_static(plan: &LaunchPlan) -> bool {
    matches!(plan.runtime, LocalLaunchRuntime::StaticHttp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_port_is_nonzero() {
        let p = pick_free_port();
        assert!(p > 0);
    }

    #[test]
    fn static_url_shape() {
        let u = static_open_url(1234, "abc", "/");
        assert_eq!(u, "http://127.0.0.1:1234/local-projects/abc/");
    }
}
