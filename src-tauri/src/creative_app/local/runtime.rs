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
use tauri::{AppHandle, Emitter};
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
        let registry = leases.unwrap_or_else(|| Arc::new(crate::creative_app::port_lease::PortLeaseRegistry::new()));
        let op_key = format!("start:{app_id}:{runtime_id}");
        let port = match plan.port.mode {
            crate::creative_app::model::LaunchPortMode::Fixed => {
                let p = plan
                    .port
                    .value
                    .ok_or_else(|| Error::InvalidInput("fixed port missing".into()))?;
                // Registry acquire also verifies the port is free at OS level.
                registry.acquire(p, &op_key).map_err(|_| {
                    Error::InvalidInput(format!("port {p} is already in use"))
                })?
            }
            crate::creative_app::model::LaunchPortMode::Auto => {
                let p = preferred_port.unwrap_or(0);
                if p != 0 {
                    registry.acquire(p, &op_key).map_err(|_| {
                        Error::InvalidInput(format!("port {p} is already in use"))
                    })?
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

fn emit_log<R: tauri::Runtime>(app: &tauri::AppHandle<R>, runtime_id: &str, app_id: &str, line: &LogLine) {
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

fn emit_progress<R: tauri::Runtime>(app: &tauri::AppHandle<R>, app_id: &str, stage: &str, message: &str) {
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
    // Managed-process profile (batch 10 CR-1002): Python/Binary WebUI.
    // Dispatch on the profile before the legacy program map.
    if let Some(profile) = &plan.process_profile {
        use crate::creative_app::model::ProcessProfile;
        return match profile {
            ProcessProfile::Python(p) => {
                // T06/T09: use the Host-trusted interpreter. Re-resolve at
                // start (canonical path + python identity) so a swapped/shell
                // interpreter is refused — the agent's stored string is never
                // trusted at the spawn point.
                let interpreter =
                    crate::creative_app::process_driver::resolve_python_interpreter(
                        &p.interpreter,
                    )?;
                let mut args = vec![p.entry.clone()];
                args.extend(p.args.iter().cloned());
                Ok((interpreter, args))
            }
            ProcessProfile::Binary(b) => {
                // T09: recompute identity at launch. A content change since
                // approval invalidates the authorization and refuses to spawn.
                let canonical =
                    crate::creative_app::process_driver::verify_binary_identity(
                        &b.executable_path,
                        &b.executable_hash,
                    )?;
                Ok((canonical, b.args.clone()))
            }
        };
    }
    match plan.program {
        crate::creative_app::model::LaunchProgram::Npm => {
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

/// True when at least one process still exists in the given process group.
/// POSIX: `kill(-pgid, 0)` returns 0 if any member is alive, -1/ESRCH if none.
#[cfg(unix)]
pub fn process_group_exists(pgid: i32) -> bool {
    unsafe { libc::kill(-pgid, 0) == 0 }
}

#[cfg(not(unix))]
pub fn process_group_exists(_pgid: i32) -> bool {
    false
}

/// Poll until the port stops accepting TCP connections or `timeout` elapses.
/// A port that was never bound reports released immediately.
pub fn wait_port_released(port: u16, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if !port_listening(port) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
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
pub fn static_open_url(
    host_http_port: u16,
    creative_id: &str,
    runtime_id: &str,
    open_path: &str,
) -> String {
    let path = if open_path.starts_with('/') {
        open_path.to_string()
    } else {
        format!("/{open_path}")
    };
    format!("http://127.0.0.1:{host_http_port}/local-projects/{runtime_id}/{creative_id}{path}")
}

/// A preview URL candidate with its provenance (batch 6). The runtime probes
/// candidates in order and picks the first healthy one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewUrlCandidate {
    pub url: String,
    pub source: &'static str,
}

/// Determinable URL candidates in priority order (batch 6):
/// explicit plan port → framework default. Compose adds an inspect-resolved
/// candidate at runtime (the actual published host port), which is async and
/// not computable here.
pub fn resolve_preview_urls(
    plan: &LaunchPlan,
    host_http_port: u16,
    app_id: &str,
    runtime_id: &str,
) -> Vec<PreviewUrlCandidate> {
    let mut out = Vec::new();
    let open = |p: u16| -> String {
        let path = if plan.open_path.starts_with('/') {
            plan.open_path.clone()
        } else {
            format!("/{}", plan.open_path)
        };
        format!("http://127.0.0.1:{p}{path}")
    };
    match plan.runtime {
        LocalLaunchRuntime::StaticHttp => {
            out.push(PreviewUrlCandidate {
                url: static_open_url(host_http_port, app_id, runtime_id, &plan.open_path),
                source: "framework_default",
            });
        }
        LocalLaunchRuntime::NodeDevServer => {
            if let Some(p) = plan.port.value {
                out.push(PreviewUrlCandidate {
                    url: open(p),
                    source: "explicit_plan",
                });
            }
        }
        LocalLaunchRuntime::DockerCompose => {
            if let Some(d) = &plan.compose {
                if let Some(p) = d.host_port {
                    out.push(PreviewUrlCandidate {
                        url: open(p),
                        source: "explicit_plan",
                    });
                }
                // framework default port (8080) as a last-resort candidate;
                // compose_inspect is appended at runtime.
                out.push(PreviewUrlCandidate {
                    url: open(8080),
                    source: "framework_default",
                });
            }
        }
    }
    out
}

/// Rewrite an `0.0.0.0`-hosted URL to loopback (batch 6) — a server that binds
/// all interfaces must still be previewed on 127.0.0.1.
pub fn normalize_loopback(url: &str) -> String {
    if let Some(rest) = url.strip_prefix("http://0.0.0.0") {
        format!("http://127.0.0.1{rest}")
    } else if let Some(rest) = url.strip_prefix("https://0.0.0.0") {
        format!("https://127.0.0.1{rest}")
    } else {
        url.to_string()
    }
}

pub fn plan_is_static(plan: &LaunchPlan) -> bool {
    matches!(plan.runtime, LocalLaunchRuntime::StaticHttp)
}

/// Stable, unique Compose project name for a local creative app (batch 5).
/// `natives-{seed}-{id-suffix}` — two Natives apps never share a project, and
/// the name is deterministic across restarts so stop/down find the same project.
pub fn compose_project_name(app_id: &str, seed: &str) -> String {
    let seed = if seed.trim().is_empty() {
        "compose"
    } else {
        seed
    };
    // First 8 hex chars of the app id as a collision-resistant suffix.
    let mut h = std::collections::hash_map::DefaultHasher::new();
    use std::hash::{Hash, Hasher};
    app_id.hash(&mut h);
    let suffix = format!("{:08x}", h.finish());
    format!("natives-{seed}-{suffix}")
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
        let u = static_open_url(1234, "abc", "run-1", "/");
        assert_eq!(u, "http://127.0.0.1:1234/local-projects/run-1/abc/");
    }

    /// Batch 5: the Compose project name is stable per app and unique across apps.
    #[test]
    fn compose_project_name_is_stable_and_unique() {
        let a = compose_project_name("app-1", "freq");
        let a2 = compose_project_name("app-1", "freq");
        let b = compose_project_name("app-2", "freq");
        assert_eq!(a, a2, "same app + seed must derive the same project");
        assert_ne!(a, b, "different apps must never share a compose project");
        assert!(a.starts_with("natives-freq-"), "unexpected prefix: {a}");
        assert_eq!(
            compose_project_name("x", ""),
            compose_project_name("x", "compose")
        );
    }

    /// Batch 6: URL candidates follow the documented priority (explicit plan
    /// port first, framework default last) and always target loopback.
    #[test]
    fn preview_url_priority_and_loopback() {
        let mut node = LaunchPlan {
            schema_version: 1,
            source: crate::creative_app::model::LaunchPlanSource::Rule,
            project_kind: crate::creative_app::model::LocalProjectKind::Vite,
            runtime: LocalLaunchRuntime::NodeDevServer,
            program: crate::creative_app::model::LaunchProgram::Npm,
            cwd_relative: ".".into(),
            script: Some("dev".into()),
            entry_file: None,
            script_runner: Some(crate::creative_app::model::ScriptRunner::Vite),
            args: vec![],
            environment_keys: vec![],
            port: crate::creative_app::model::LaunchPort {
                mode: crate::creative_app::model::LaunchPortMode::Fixed,
                value: Some(5173),
            },
            open_path: "/".into(),
            health_path: "/".into(),
            startup_timeout_ms: 60_000,
            auto_open: true,
            confidence: None,
            reason: "t".into(),
            compose: None,
            trade_approval: None,
            process_profile: None,
        };
        let cands = resolve_preview_urls(&node, 1234, "app", "run-1");
        assert_eq!(cands[0].source, "explicit_plan");
        assert_eq!(cands[0].url, "http://127.0.0.1:5173/");

        node.port.value = None;
        let cands = resolve_preview_urls(&node, 1234, "app", "run-1");
        assert!(
            cands.is_empty(),
            "no fixed port → no deterministic candidate"
        );

        // 0.0.0.0 host must normalize to loopback for preview.
        assert_eq!(
            normalize_loopback("http://0.0.0.0:8080/api"),
            "http://127.0.0.1:8080/api"
        );
        assert_eq!(
            normalize_loopback("http://127.0.0.1:8080/"),
            "http://127.0.0.1:8080/"
        );
    }

    /// P0: a process that ignores SIGTERM must still be killed (KILL follows the
    /// grace window), its process group verified gone, its port verified released,
    /// and the direct child reaped — never left as a zombie.
    #[cfg(unix)]
    #[tokio::test]
    async fn term_timeout_kills_group_and_releases_port() {
        use std::process::Stdio;
        use tokio::process::Command;

        let Ok(_v) = std::process::Command::new("node").arg("--version").output() else {
            eprintln!("[skip] node not available; cannot verify group kill");
            return;
        };

        let port = pick_free_port();
        let js = format!(
            "process.on('SIGTERM', () => {{}}); \
             require('http').createServer((_q,s)=>s.end('ok')).listen({port}, '127.0.0.1');"
        );

        let mut cmd = Command::new("node");
        cmd.arg("-e")
            .arg(&js)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // SAFETY: pre_exec runs in the forked child before exec; setpgid is the
        // only libc call and its use here is the standard new-process-group pattern.
        unsafe {
            cmd.pre_exec(|| {
                if libc::setpgid(0, 0) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = cmd.spawn().expect("spawn node");

        let pid = child.id().expect("node pid") as i32;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !port_listening(port) {
            assert!(
                std::time::Instant::now() < deadline,
                "node never bound port {port}"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        // node ignores SIGTERM, so the grace window must end in SIGKILL.
        terminate_tree_with_grace(&mut child, Some(pid), Duration::from_millis(300)).await;

        // Direct child must be reaped.
        let _status = child.wait().await.expect("reap node child");

        assert!(
            !process_group_exists(pid),
            "process group {pid} still has members after kill"
        );
        assert!(
            wait_port_released(port, Duration::from_secs(2)),
            "port {port} still accepting connections after kill"
        );
    }

    /// Batch 2: stop must set the cancel flag (preempting health/readers) and
    /// drain the instance's tracked tasks instead of leaving them running.
    #[tokio::test]
    async fn stop_sets_cancel_flag_and_drains_tasks() {
        use std::future::pending;

        let mgr = LocalRuntimeManager::new();
        let cancelled = Arc::new(AtomicBool::new(false));
        let task_handle = tokio::spawn(async move {
            let _ = pending::<()>().await;
        });
        {
            let mut map = mgr.procs.lock().await;
            map.insert(
                "a-run".into(),
                LiveLocalProcess {
                    runtime_id: "a-run".into(),
                    app_id: "a".into(),
                    child: None,
                    identity: ProcessIdentity::default(),
                    plan_fingerprint: String::new(),
                    started_at: Instant::now(),
                    port: None,
                    open_url: None,
                    program: "x".into(),
                    cwd: PathBuf::from("/"),
                    log: mgr.logs.get_or_open("a", "a-run"),
                    lease: None,
                    cancelled: cancelled.clone(),
                },
            );
            mgr.task_handles
                .lock()
                .await
                .insert("a-run".into(), vec![task_handle]);
        }

        mgr.stop::<tauri::Wry>("a-run", None).await.expect("stop succeeds");
        assert!(
            cancelled.load(Ordering::SeqCst),
            "stop must set the cancel flag before reaping"
        );
        let handles = mgr.task_handles.lock().await;
        assert!(
            !handles.contains_key("a-run"),
            "stop must drain tracked reader/health tasks"
        );
    }

    /// CR-301 (#05/#22): two runs of the same app live in separate runtime slots.
    /// live_runtime_ids / stop are keyed by runtime id, so a late event from run 1
    /// can never touch run 2's resources.
    #[tokio::test]
    async fn two_runs_of_same_app_are_isolated() {
        let mgr = LocalRuntimeManager::new();
        let mk = |rt: &str, pid: u32| LiveLocalProcess {
            runtime_id: rt.into(),
            app_id: "app-iso".into(),
            child: None,
            identity: ProcessIdentity {
                pid: Some(pid),
                ..ProcessIdentity::default()
            },
            plan_fingerprint: String::new(),
            started_at: Instant::now(),
            port: None,
            open_url: None,
            program: "x".into(),
            cwd: PathBuf::from("/"),
            log: mgr.logs.get_or_open("app-iso", rt),
            lease: None,
            cancelled: Arc::new(AtomicBool::new(false)),
        };
        {
            let mut map = mgr.procs.lock().await;
            map.insert("run-1".into(), mk("run-1", 101));
            map.insert("run-2".into(), mk("run-2", 202));
        }
        let mut ids = mgr.live_runtime_ids().await;
        ids.sort();
        assert_eq!(ids, vec!["run-1".to_string(), "run-2".to_string()]);
        // Stopping run 1 must not touch run 2.
        mgr.stop::<tauri::Wry>("run-1", None).await.expect("stop run-1");
        assert_eq!(mgr.live_runtime_ids().await, vec!["run-2".to_string()]);
        mgr.purge_app_logs("app-iso");
    }

    /// CR-302: stop must return Err (never claim released) when the port stays
    /// bound after the tree is gone. The caller preserves the identity so a retry
    /// stop stays possible and never writes stopped.
    #[tokio::test]
    async fn stop_fails_when_port_not_released() {
        let mgr = LocalRuntimeManager::new();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        {
            let mut map = mgr.procs.lock().await;
            map.insert(
                "run-1".into(),
                LiveLocalProcess {
                    runtime_id: "run-1".into(),
                    app_id: "app-p".into(),
                    child: None,
                    identity: ProcessIdentity::default(),
                    plan_fingerprint: String::new(),
                    started_at: Instant::now(),
                    port: Some(port),
                    open_url: None,
                    program: "x".into(),
                    cwd: PathBuf::from("/"),
                    log: mgr.logs.get_or_open("app-p", "run-1"),
                    lease: None,
                    cancelled: Arc::new(AtomicBool::new(false)),
                },
            );
        }
        let err = mgr.stop::<tauri::Wry>("run-1", None).await.unwrap_err();
        assert!(err.to_string().contains("stop incomplete"), "{err}");
        assert!(err.to_string().contains("port"), "{err}");
        drop(listener);
        mgr.purge_app_logs("app-p");
    }

    /// CR-302: repeated stop is idempotent — a second stop on an already-stopped
    /// runtime is Ok and drains nothing.
    #[tokio::test]
    async fn repeated_stop_is_idempotent() {
        let mgr = LocalRuntimeManager::new();
        {
            let mut map = mgr.procs.lock().await;
            map.insert(
                "run-1".into(),
                LiveLocalProcess {
                    runtime_id: "run-1".into(),
                    app_id: "app-r".into(),
                    child: None,
                    identity: ProcessIdentity::default(),
                    plan_fingerprint: String::new(),
                    started_at: Instant::now(),
                    port: None,
                    open_url: None,
                    program: "x".into(),
                    cwd: PathBuf::from("/"),
                    log: mgr.logs.get_or_open("app-r", "run-1"),
                    lease: None,
                    cancelled: Arc::new(AtomicBool::new(false)),
                },
            );
        }
        mgr.stop::<tauri::Wry>("run-1", None).await.expect("first stop");
        assert!(
            mgr.stop::<tauri::Wry>("run-1", None).await.is_ok(),
            "second stop is idempotent"
        );
        mgr.purge_app_logs("app-r");
    }

    /// CR-302: a reused PID (different start time) must NOT match the persisted
    /// identity — an unknown process is never killed by stop/reconcile (#10).
    #[cfg(unix)]
    #[tokio::test]
    async fn pid_reuse_is_rejected_by_strict_identity() {
        let Ok(_) = std::process::Command::new("node").arg("--version").output() else {
            eprintln!("[skip] node not available");
            return;
        };
        let mut cmd = Command::new("node");
        cmd.arg("-e")
            .arg("setInterval(()=>{},1000)")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        // SAFETY: pre_exec runs in the forked child before exec; setpgid is the
        // standard new-process-group pattern used elsewhere in this module.
        unsafe {
            cmd.pre_exec(|| {
                if libc::setpgid(0, 0) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = cmd.spawn().expect("spawn node");
        let pid = child.id().expect("pid");
        let started = process_start_time_unix(Some(pid)).expect("start time");

        let good = ProcessIdentity {
            pid: Some(pid),
            started_at_unix: Some(started),
            executable: Some("node".into()),
            cwd: Some(
                std::env::current_dir()
                    .unwrap()
                    .to_string_lossy()
                    .to_string(),
            ),
            plan_fingerprint: Some("fp".into()),
            process_group_id: Some(pid as i32),
        };
        assert!(
            identity_matches_live_strict(&good),
            "the correct identity must match the live process"
        );
        // A different process now owns the same PID (reuse): start time differs.
        let reused = ProcessIdentity {
            started_at_unix: Some(started + 10_000),
            ..good.clone()
        };
        assert!(
            !identity_matches_live_strict(&reused),
            "a reused PID with a different start time must be rejected"
        );
        // Unknown PID / missing fingerprint are also rejected (fail closed).
        assert!(!identity_matches_live_strict(&ProcessIdentity {
            pid: Some(pid),
            started_at_unix: Some(started),
            executable: Some("node".into()),
            cwd: Some(
                std::env::current_dir()
                    .unwrap()
                    .to_string_lossy()
                    .to_string()
            ),
            plan_fingerprint: None,
            process_group_id: None,
        }));

        unsafe {
            let _ = libc::kill(-(pid as i32), libc::SIGKILL);
        }
        let _ = child.wait().await;
    }
}
