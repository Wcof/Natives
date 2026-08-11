//! Local creative process supervisor: process groups, live logs, tree kill.
//!
//! Designed for Vite/Vue/Node dev servers. Does not use shell. Does not
//! auto-take over orphan PIDs without identity verification.

use super::logs::{append_with_secrets, LocalLogStore, LogLine, LogRegistry, LogStream};
use crate::creative_app::model::{LaunchPlan, LaunchProgram, LocalLaunchRuntime, ProcessIdentity};
use crate::creative_app::port_lease::{PortLease, PortLeaseRegistryHandle};
use crate::{Error, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::Emitter;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

const GRACEFUL_WAIT_MS: u64 = 5_000;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreativeAppLogEvent {
    /// Runtime instance id owning this log line (CR-301). Empty for legacy /
    /// aggregate lines that predate runtime-scoped logs.
    pub runtime_id: String,
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
    runtime_id: String,
    app_id: String,
    child: Option<Child>,
    identity: ProcessIdentity,
    plan_fingerprint: String,
    started_at: Instant,
    port: Option<u16>,
    open_url: Option<String>,
    program: String,
    cwd: PathBuf,
    log: Arc<LocalLogStore>,
    /// Port lease held until the child proves it bound the port (T09). Kept
    /// with the live process so the reservation spans spawn → health and is
    /// released on exit/stop.
    lease: Option<PortLease>,
    /// Set when the instance is being stopped. Reader / health tasks observe it
    /// so stop can preempt an in-flight start (P0 / batch 2).
    cancelled: Arc<AtomicBool>,
}

/// Live local process registry, keyed by RUNTIME INSTANCE id (CR-301). Two
/// consecutive runs of the same app never share a slot, so a late exit/health
/// event from run 1 cannot land on run 2's resources.
pub struct LocalRuntimeManager {
    procs: Mutex<HashMap<String, LiveLocalProcess>>,
    logs: LogRegistry,
    /// Tasks spawned on behalf of an instance (log readers, health wait). Kept
    /// so stop can end them deterministically instead of relying on pipe EOF.
    task_handles: Mutex<HashMap<String, Vec<JoinHandle<()>>>>,
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
            task_handles: Mutex::new(HashMap::new()),
        }
    }

    pub fn logs(&self) -> &LogRegistry {
        &self.logs
    }

    /// Runtime ids with a live managed child (used to heartbeat each instance).
    pub async fn live_runtime_ids(&self) -> Vec<String> {
        let map = self.procs.lock().await;
        map.keys().cloned().collect()
    }

    /// True while a managed child is still alive.
    pub async fn is_running(&self, runtime_id: &str) -> bool {
        let mut map = self.procs.lock().await;
        if let Some(p) = map.get_mut(runtime_id) {
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

    /// Register a task owned by an instance so stop can end it.
    async fn track_task(&self, runtime_id: &str, handle: JoinHandle<()>) {
        let mut map = self.task_handles.lock().await;
        map.entry(runtime_id.to_string()).or_default().push(handle);
    }

    /// Abort all tracked tasks for an instance (log readers / health). Called
    /// after the process tree is gone so a reader can never linger on a dead pipe.
    async fn end_tracked_tasks(&self, runtime_id: &str) {
        let handles = {
            let mut map = self.task_handles.lock().await;
            map.remove(runtime_id).unwrap_or_default()
        };
        for h in handles {
            h.abort();
        }
    }

    pub async fn current_port(&self, runtime_id: &str) -> Option<u16> {
        let map = self.procs.lock().await;
        map.get(runtime_id).and_then(|p| p.port)
    }

    pub async fn open_url(&self, runtime_id: &str) -> Option<String> {
        let map = self.procs.lock().await;
        map.get(runtime_id).and_then(|p| p.open_url.clone())
    }

    pub async fn identity(&self, runtime_id: &str) -> Option<ProcessIdentity> {
        let map = self.procs.lock().await;
        map.get(runtime_id).map(|p| p.identity.clone())
    }

    pub async fn stop_all<R: tauri::Runtime>(&self, app: Option<&tauri::AppHandle<R>>) {
        let ids: Vec<String> = {
            let map = self.procs.lock().await;
            map.keys().cloned().collect()
        };
        for id in ids {
            let _ = self.stop(&id, app).await;
        }
    }

    /// Stop the managed process tree and verify release (P0).
    ///
    /// Returns `Err` when the process group still has members or the app port is
    /// still accepting connections after TERM→grace→KILL→reap. Callers must NOT
    /// write `stopped` on `Err` — the identity/port must be preserved so a retry
    /// stop stays possible.
    pub async fn stop<R: tauri::Runtime>(
        &self,
        runtime_id: &str,
        app: Option<&tauri::AppHandle<R>>,
    ) -> Result<()> {
        let mut map = self.procs.lock().await;
        let Some(mut live) = map.remove(runtime_id) else {
            // Nothing live to stop; make sure no tracked tasks linger either.
            drop(map);
            self.end_tracked_tasks(runtime_id).await;
            return Ok(());
        };
        let pgid = live.identity.process_group_id;
        let port = live.port;
        // Signal readers / health first so stop preempts an in-flight start.
        live.cancelled.store(true, Ordering::SeqCst);
        live.log.append(LogStream::System, "stopping process tree…");
        let mut problems: Vec<String> = Vec::new();

        if let Some(mut child) = live.child.take() {
            terminate_tree(&mut child, pgid).await;
            // Reap the direct child — a reaped child must be waited, never left
            // behind as a zombie. tokio caches the status, so a second wait here
            // is safe when terminate_tree already reaped it.
            if let Err(e) = child.wait().await {
                problems.push(format!("child wait failed: {e}"));
            }
        }
        if let Some(pgid) = pgid {
            if process_group_exists(pgid) {
                problems.push(format!("process group {pgid} still has members after kill"));
            }
        }
        if let Some(port) = port {
            if !wait_port_released(port, Duration::from_secs(3)) {
                problems.push(format!(
                    "port {port} still accepting connections after stop"
                ));
            }
        }

        drop(map);
        self.end_tracked_tasks(runtime_id).await;

        if !problems.is_empty() {
            live.log.append(
                LogStream::System,
                &format!("stop incomplete: {}", problems.join("; ")),
            );
            if let Some(app) = app {
                emit_progress(app, &live.app_id, "stop_failed", &problems.join("; "));
            }
            return Err(Error::Internal(format!(
                "stop incomplete: {}",
                problems.join("; ")
            )));
        }

        live.log.append(LogStream::System, "stopped");
        if let Some(app) = app {
            emit_progress(app, &live.app_id, "stopped", "process stopped");
        }
        Ok(())
    }

    /// Start a node_dev_server plan. Caller must hold app-level mutation lock.
    /// The runtime is keyed by `runtime_id` (CR-301) so a restart of the same app
    /// gets a fresh slot and old exit/health events cannot reach the new run.
    #[allow(clippy::too_many_arguments)] // pre-existing parameter list
    pub async fn start_node_dev<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        runtime_id: &str,
        app_id: &str,
        project_root: &Path,
        plan: &LaunchPlan,
        plan_fingerprint: &str,
        env: &[(String, String)],
        preferred_port: Option<u16>,
        leases: Option<PortLeaseRegistryHandle>,
    ) -> Result<(u16, String, ProcessIdentity)> {
        if self.is_running(runtime_id).await {
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

        // Port selection through a held lease (T09). A bound listener reserves
        // the port at the OS level until we release the hold right before
        // spawn, closing the pick-free-port → spawn TOCTOU window.
        let registry = leases
            .unwrap_or_else(|| Arc::new(crate::creative_app::port_lease::PortLeaseRegistry::new()));
        let op_key = format!("start:{app_id}:{runtime_id}");
        let port = match plan.port.mode {
            crate::creative_app::model::LaunchPortMode::Fixed => {
                let p = plan
                    .port
                    .value
                    .ok_or_else(|| Error::InvalidInput("fixed port missing".into()))?;
                // Registry acquire also verifies the port is free at OS level.
                registry
                    .acquire(p, &op_key)
                    .map_err(|_| Error::InvalidInput(format!("port {p} is already in use")))?
            }
            crate::creative_app::model::LaunchPortMode::Auto => {
                let p = preferred_port.unwrap_or(0);
                if p != 0 {
                    registry
                        .acquire(p, &op_key)
                        .map_err(|_| Error::InvalidInput(format!("port {p} is already in use")))?
                } else {
                    registry.acquire_auto(&op_key)?
                }
            }
        };
        let mut lease = port;
        let port = lease.port();

        let (program, mut args) = build_command(plan, port)?;
        // P0: never spawn a command that can place real trades without explicit
        // authorization. The plan validator already gates registration; this is
        // defense-in-depth at the spawn point.
        if super::risk::classify_command(&program, &args) == super::risk::CommandRisk::Block {
            return Err(Error::InvalidInput(
                "start blocked: command may place real trades; refusing to auto-start".into(),
            ));
        }
        // Append runner-specific host/port flags (Vite vs Vue CLI differ).
        if matches!(
            plan.program,
            crate::creative_app::model::LaunchProgram::Npm
                | LaunchProgram::Pnpm
                | LaunchProgram::Yarn
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

        let log = self.logs.get_or_open(app_id, runtime_id);
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

        // The child must be able to bind the port: drop the OS-level hold at
        // the LAST moment before spawn. The registry reservation stays alive in
        // `lease` (stored with the live process) until health proves the bind.
        lease.release_hold();

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
        let cancelled = Arc::new(AtomicBool::new(false));
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let app_handle = app.clone();
        let app_id_out = app_id.to_string();
        let runtime_id_out = runtime_id.to_string();
        let log_out = log.clone();
        let stdout_secrets = secret_values.clone();
        if let Some(out) = stdout {
            let handle = tokio::spawn(async move {
                let mut lines = BufReader::new(out).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let entry = append_with_secrets(
                        log_out.as_ref(),
                        LogStream::Stdout,
                        &line,
                        &stdout_secrets,
                    );
                    emit_log(&app_handle, &runtime_id_out, &app_id_out, &entry);
                }
            });
            self.track_task(runtime_id, handle).await;
        }
        let app_handle = app.clone();
        let app_id_err = app_id.to_string();
        let runtime_id_err = runtime_id.to_string();
        let log_err = log.clone();
        let stderr_secrets = secret_values;
        if let Some(err) = stderr {
            let handle = tokio::spawn(async move {
                let mut lines = BufReader::new(err).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let entry = append_with_secrets(
                        log_err.as_ref(),
                        LogStream::Stderr,
                        &line,
                        &stderr_secrets,
                    );
                    emit_log(&app_handle, &runtime_id_err, &app_id_err, &entry);
                }
            });
            self.track_task(runtime_id, handle).await;
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
                runtime_id.to_string(),
                LiveLocalProcess {
                    runtime_id: runtime_id.to_string(),
                    app_id: app_id.to_string(),
                    child: Some(child),
                    identity: identity.clone(),
                    plan_fingerprint: plan_fingerprint.to_string(),
                    started_at: Instant::now(),
                    port: Some(port),
                    open_url: Some(open_url.clone()),
                    program: executable,
                    cwd: cwd.clone(),
                    log: log.clone(),
                    lease: Some(lease),
                    cancelled,
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

    /// Confirm the port lease for a runtime after health proved the child bound
    /// it (T09). Releases the reservation — the child's own socket is now the
    /// durable protection.
    pub async fn confirm_port_lease(&self, runtime_id: &str) {
        let mut map = self.procs.lock().await;
        if let Some(live) = map.get_mut(runtime_id) {
            if let Some(lease) = live.lease.take() {
                lease.confirm_bound();
            }
        }
    }

    /// If the managed process has exited, remove it and return exit info.
    pub async fn take_if_exited(&self, runtime_id: &str) -> Option<i32> {
        let mut map = self.procs.lock().await;
        let live = map.get_mut(runtime_id)?;
        let child = live.child.as_mut()?;
        match child.try_wait() {
            Ok(Some(status)) => {
                let code = status.code().unwrap_or(-1);
                live.child = None;
                live.log
                    .append(LogStream::System, &format!("process exited (code {code})"));
                map.remove(runtime_id);
                Some(code)
            }
            _ => None,
        }
    }

    /// Poll all live processes; return `(runtime_id, exit_code)` pairs so a late
    /// exit can be attributed to the exact run that produced it (CR-301 #22).
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
    /// (caller may stop or mark start_unhealthy). Cancellable: a stop preempts the
    /// wait via the instance's cancelled flag (batch 2).
    pub async fn wait_healthy<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        runtime_id: &str,
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
        let cancelled = {
            let map = self.procs.lock().await;
            map.get(runtime_id).map(|p| p.cancelled.clone())
        };
        emit_progress(app, app_id, "health_check", "waiting for server");

        loop {
            if cancelled
                .as_ref()
                .map(|c| c.load(Ordering::SeqCst))
                .unwrap_or(false)
            {
                return Err(Error::Cancelled("start cancelled by stop".into()));
            }
            if !self.is_running(runtime_id).await {
                return Err(Error::Internal("process exited before healthy".into()));
            }
            let port = self
                .current_port(runtime_id)
                .await
                .ok_or_else(|| Error::Internal("missing port".into()))?;
            if port_listening(port) {
                let url = format!("http://127.0.0.1:{port}{health_path}");
                if http_reachable(&url).await {
                    emit_progress(app, app_id, "ready", "health check passed");
                    // The child proved it bound the port — release the lease.
                    self.confirm_port_lease(runtime_id).await;
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

    /// Per-runtime memory lines newer than `cursor`, limited to `limit`.
    pub fn recent_logs(
        &self,
        app_id: &str,
        runtime_id: &str,
        cursor: u64,
        limit: usize,
    ) -> Vec<LogLine> {
        self.logs
            .get_or_open(app_id, runtime_id)
            .recent_memory_after(cursor, limit)
    }

    /// Per-runtime persisted tail.
    pub fn persisted_tail(&self, app_id: &str, runtime_id: &str, max_bytes: usize) -> String {
        self.logs
            .get_or_open(app_id, runtime_id)
            .read_persisted_tail(max_bytes)
    }

    /// App-level aggregate tail (legacy app log + every per-runtime run), for the
    /// read-only "old app logs" view (CR-301 dual-read).
    pub fn app_aggregate_tail(&self, app_id: &str, max_bytes: usize) -> String {
        self.logs.app_aggregate_tail(app_id, max_bytes)
    }

    /// Drop a runtime's live log store (no purge — logs stay on disk).
    pub fn purge_logs(&self, runtime_id: &str) {
        self.logs.remove(runtime_id);
    }

    /// Drop every log store of an app and purge its whole log directory.
    pub fn purge_app_logs(&self, app_id: &str) {
        self.logs.remove_app(app_id);
    }
}

fn emit_log<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    runtime_id: &str,
    app_id: &str,
    line: &LogLine,
) {
    let ev = CreativeAppLogEvent {
        runtime_id: runtime_id.to_string(),
        app_id: app_id.to_string(),
        seq: line.seq,
        ts_ms: line.ts_ms,
        stream: line.stream.as_str().to_string(),
        text: line.text.clone(),
    };
    let _ = app.emit("creative-app-log", &ev);
}

fn emit_progress<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    app_id: &str,
    stage: &str,
    message: &str,
) {
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

// Command building / executable resolution / process identity utilities moved
// to `runtime_utils` (W3); the compatibility shims below re-export the items
// that callers still reach via `runtime::`. The `pub use` list is the single
// re-export; the forwarding fns below exist for items with cfg/type wrappers.
pub use super::runtime_utils::{
    build_command, compose_project_name, http_reachable, identity_matches_live_strict,
    is_safe_inherited_env, normalize_loopback, plan_is_static, port_in_use,
    process_start_time_unix, resolve_executable_path, resolve_preview_urls, static_open_url,
    PreviewUrlCandidate,
};

pub fn pick_free_port() -> u16 {
    super::runtime_utils::pick_free_port()
}

pub fn port_listening(port: u16) -> bool {
    super::runtime_utils::port_listening(port)
}

#[cfg(unix)]
pub fn process_group_exists(pgid: i32) -> bool {
    super::runtime_utils::process_group_exists(pgid)
}

#[cfg(not(unix))]
pub fn process_group_exists(_pgid: i32) -> bool {
    false
}

pub fn wait_port_released(port: u16, timeout: Duration) -> bool {
    super::runtime_utils::wait_port_released(port, timeout)
}

async fn terminate_tree(child: &mut Child, pgid: Option<i32>) {
    terminate_tree_with_grace(child, pgid, Duration::from_millis(GRACEFUL_WAIT_MS)).await;
}

async fn terminate_tree_with_grace(child: &mut Child, pgid: Option<i32>, grace: Duration) {
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

    let deadline = Instant::now() + grace;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            _ => break,
        }
    }

    // Force: kill any remaining group members even when the direct child already
    // exited gracefully (a detached grandchild keeps the group + port alive).
    #[cfg(unix)]
    {
        if let Some(pgid) = pgid {
            if process_group_exists(pgid) {
                unsafe {
                    let _ = libc::kill(-pgid, libc::SIGKILL);
                }
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
    // Always reap the direct child so it never becomes a zombie.
    let _ = child.wait().await;
}

/// Static local apps do not spawn a process; open_url is built from host HTTP port.
///
/// CR-303: the URL is instance-scoped — the runtime instance id is part of the
/// path (`/local-projects/{runtimeId}/{creativeId}/…`), so a stopped (or
/// superseded) run's URL is no longer servable and the HTTP route can validate
/// the active runtime before serving any file.
// URL / preview utilities are re-exported above via the single `pub use`.

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod runtime_tests;
