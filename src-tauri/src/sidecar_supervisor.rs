//! Sidecar Supervisor — production bootstrap for the Agent Daemon.
//!
//! Target lifecycle (Phase 2):
//! 1. Resolve runtime dir (socket, pid, lock)
//! 2. Clean stale socket / zombie pid
//! 3. Spawn agent-daemon sidecar
//! 4. Obtain bootstrap via secure channel (not normal logs)
//! 5. Wait for health/readiness
//! 6. Protocol v2 handshake
//! 7. Register credential broker path
//! 8. Recover unfinished runs / permissions
//! 9. Start UI event subscription
//! 10. Auto-restart on crash + re-handshake
//! 11. Graceful shutdown on Tauri exit
//!
//! Production rule: UDS failure must surface as an explicit fault state.
//! Never silently fall back to Embedded.

#[path = "sidecar_supervisor_config.rs"]
mod sidecar_supervisor_config;
#[path = "sidecar_supervisor_helpers.rs"]
mod sidecar_supervisor_helpers;

pub use sidecar_supervisor_config::*;
pub use sidecar_supervisor_helpers::*;

#[cfg(test)]
use sidecar_supervisor_helpers::uuid_like;
use sidecar_supervisor_helpers::{
    force_kill_child_tree, generate_bootstrap_token, which_in_path, write_broker_session,
};

use std::path::Path;
use std::process::{Child, Command, Stdio};
#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const DAEMON_RPC_PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// In-process supervisor (Phase 2 skeleton — spawn + health + no silent fallback).
pub struct SidecarSupervisor {
    config: SupervisorConfig,
    state: Mutex<InnerState>,
    #[cfg(test)]
    readiness_bypass: AtomicBool,
    #[cfg(test)]
    health_probe_bypass: AtomicBool,
}

struct InnerState {
    status: SupervisorStatus,
    child: Option<Child>,
    /// Host end of the inherited stdin lifeline. Dropping closes the pipe (EOF to Daemon).
    lifeline_stdin: Option<std::process::ChildStdin>,
    bootstrap_token: Option<String>,
    instance_id: Option<String>,
    /// Generation of the broker identity installed for this daemon lifecycle.
    broker_peer_generation: Option<u64>,
    /// Ensures concurrent window/app exit hooks only run shutdown once.
    shutdown_started: bool,
}

impl SidecarSupervisor {
    pub fn new(config: SupervisorConfig) -> Self {
        let mode = if config.require_uds { "uds" } else { "auto" };
        Self {
            state: Mutex::new(InnerState {
                status: SupervisorStatus {
                    state: SupervisorState::Stopped,
                    mode: mode.into(),
                    socket_path: Some(config.socket_path.display().to_string()),
                    pid: None,
                    restart_count: 0,
                    last_error: None,
                    production_ready: false,
                },
                child: None,
                lifeline_stdin: None,
                bootstrap_token: None,
                instance_id: None,
                broker_peer_generation: None,
                shutdown_started: false,
            }),
            config,
            #[cfg(test)]
            readiness_bypass: AtomicBool::new(false),
            #[cfg(test)]
            health_probe_bypass: AtomicBool::new(false),
        }
    }

    pub fn status(&self) -> SupervisorStatus {
        self.state
            .lock()
            .map(|g| g.status.clone())
            .unwrap_or_else(|_| SupervisorStatus {
                state: SupervisorState::Faulted {
                    reason: "supervisor lock poisoned".into(),
                },
                mode: "unknown".into(),
                socket_path: None,
                pid: None,
                restart_count: 0,
                last_error: Some("lock poisoned".into()),
                production_ready: false,
            })
    }

    /// Ensure runtime paths exist and clear stale sockets from dead processes.
    pub fn prepare_runtime_dir(&self) -> Result<(), String> {
        std::fs::create_dir_all(&self.config.runtime_dir)
            .map_err(|e| format!("create runtime dir: {e}"))?;
        if self.config.socket_path.exists() {
            // Stale socket: remove so bind can succeed after crash.
            let _ = std::fs::remove_file(&self.config.socket_path);
        }
        Ok(())
    }

    /// Start sidecar if not running. On require_uds, failures become Faulted.
    pub fn ensure_started(&self) -> Result<SupervisorStatus, String> {
        {
            let mut inner = self.state.lock().map_err(|e| e.to_string())?;
            if inner.shutdown_started {
                return Err("sidecar supervisor is shutting down; restart refused".into());
            }
            if matches!(inner.status.state, SupervisorState::Healthy) {
                return Ok(inner.status.clone());
            }
            if matches!(
                inner.status.state,
                SupervisorState::Starting | SupervisorState::Restarting
            ) {
                return Err("sidecar startup or restart already in progress".into());
            }
            inner.status.state = SupervisorState::Starting;
            inner.status.production_ready = false;
            inner.status.last_error = None;
        }

        self.start_claimed()
    }

    fn start_claimed(&self) -> Result<SupervisorStatus, String> {
        if let Err(error) = self
            .prepare_runtime_dir()
            .and_then(|_| validate_natives_db_path(&self.config.natives_db_path))
        {
            let mut inner = self.state.lock().map_err(|e| e.to_string())?;
            inner.status.state = if inner.shutdown_started {
                SupervisorState::Stopped
            } else {
                SupervisorState::Faulted {
                    reason: error.clone(),
                }
            };
            inner.status.production_ready = false;
            inner.status.last_error = Some(error.clone());
            inner.status.pid = None;
            return Err(error);
        }
        match self.spawn_child() {
            Ok((mut child, bootstrap, instance_id, broker_peer_generation)) => {
                let pid = child.id();
                let lifeline_stdin = child.stdin.take();
                // Write pid (best-effort)
                let _ = std::fs::write(&self.config.pid_path, pid.to_string());

                let readiness =
                    self.wait_for_readiness(&bootstrap, &instance_id, self.config.health_timeout);
                if let Err(reason) = readiness {
                    crate::credential_broker::credential_broker_uds::clear_broker_peer(
                        broker_peer_generation,
                    );
                    let _ = force_kill_child_tree(&mut child);
                    let _ = child.wait();
                    let mut inner = self.state.lock().map_err(|e| e.to_string())?;
                    inner.child = None;
                    inner.lifeline_stdin = None;
                    inner.bootstrap_token = None;
                    inner.instance_id = None;
                    inner.broker_peer_generation = None;
                    inner.status.state = if inner.shutdown_started {
                        SupervisorState::Stopped
                    } else {
                        SupervisorState::Faulted {
                            reason: reason.clone(),
                        }
                    };
                    inner.status.production_ready = false;
                    inner.status.last_error = Some(reason.clone());
                    inner.status.pid = None;
                    let _ = std::fs::remove_file(&self.config.socket_path);
                    let _ = std::fs::remove_file(&self.config.pid_path);
                    let _ = std::fs::remove_file(
                        self.config.runtime_dir.join("agent-daemon.ownership.json"),
                    );
                    if inner.shutdown_started {
                        return Err("sidecar supervisor shut down during startup".into());
                    }
                    if self.config.require_uds {
                        return Err(format!(
                            "UDS required but daemon not healthy: {reason} (no embedded fallback)"
                        ));
                    }
                    return Ok(inner.status.clone());
                }

                let mut inner = self.state.lock().map_err(|e| e.to_string())?;
                if inner.shutdown_started {
                    crate::credential_broker::credential_broker_uds::clear_broker_peer(
                        broker_peer_generation,
                    );
                    drop(inner);
                    drop(lifeline_stdin);
                    let _ = force_kill_child_tree(&mut child);
                    let _ = child.wait();
                    self.remove_runtime_artifacts();
                    return Err("sidecar supervisor shut down during startup".into());
                }
                inner.child = Some(child);
                inner.lifeline_stdin = lifeline_stdin;
                inner.bootstrap_token = Some(bootstrap.clone());
                inner.instance_id = Some(instance_id);
                inner.broker_peer_generation = Some(broker_peer_generation);
                std::env::set_var("NATIVES_DAEMON_SOCKET", &self.config.socket_path);
                std::env::set_var("NATIVES_DAEMON_BOOTSTRAP", &bootstrap);
                if self.config.require_uds {
                    std::env::set_var("NATIVES_DAEMON_MODE", "uds");
                }
                std::env::set_var("NATIVES_DB_PATH", &self.config.natives_db_path);
                std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &self.config.assistant_db_path);
                inner.status.state = SupervisorState::Healthy;
                inner.status.pid = Some(pid);
                inner.status.production_ready = self.config.require_uds;
                inner.status.last_error = None;
                Ok(inner.status.clone())
            }
            Err(e) => {
                let mut inner = self.state.lock().map_err(|err| err.to_string())?;
                if inner.shutdown_started {
                    inner.status.state = SupervisorState::Stopped;
                    inner.status.production_ready = false;
                    inner.status.pid = None;
                    return Err("sidecar supervisor shut down during startup".into());
                }
                inner.status.state = SupervisorState::Faulted { reason: e.clone() };
                inner.status.production_ready = false;
                inner.status.last_error = Some(e.clone());
                if self.config.require_uds {
                    Err(format!(
                        "UDS required; sidecar start failed: {e} (no embedded fallback)"
                    ))
                } else {
                    Ok(inner.status.clone())
                }
            }
        }
    }

    fn spawn_child(&self) -> Result<(Child, String, String, u64), String> {
        if !self.config.daemon_bin.exists()
            && which_in_path(self.config.daemon_bin.to_string_lossy().as_ref()).is_none()
        {
            return Err(format!(
                "daemon binary not found: {} (set NATIVES_DAEMON_BIN)",
                self.config.daemon_bin.display()
            ));
        }
        let bootstrap = generate_bootstrap_token();
        let instance_id = generate_bootstrap_token();
        let broker_session = assistant_protocol::v2::credential::CredentialBrokerSession {
            instance_id: instance_id.clone(),
            auth_token: generate_bootstrap_token(),
        };
        let mut cmd = Command::new(&self.config.daemon_bin);
        cmd.env("NATIVES_DAEMON_SOCKET", &self.config.socket_path)
            .env("NATIVES_DAEMON_BOOTSTRAP", &bootstrap)
            .env("NATIVES_DB_PATH", &self.config.natives_db_path)
            .env("NATIVES_ASSISTANT_DB_PATH", &self.config.assistant_db_path)
            .env("NATIVES_RUNTIME_DIR", &self.config.runtime_dir)
            // Host→Daemon lifeline: Host holds write end; Host exit closes pipe → Daemon EOF.
            .env("NATIVES_PARENT_LIFELINE", "stdio")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // Safety: runs in child before exec; establishes its own process group.
            unsafe {
                cmd.pre_exec(|| {
                    if libc::setpgid(0, 0) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("spawn {}: {e}", self.config.daemon_bin.display()))?;
        let broker_peer_generation =
            match crate::credential_broker::credential_broker_uds::install_broker_peer(
                child.id(),
                broker_session.clone(),
            ) {
                Ok(generation) => generation,
                Err(error) => {
                    let _ = force_kill_child_tree(&mut child);
                    let _ = child.wait();
                    return Err(error);
                }
            };
        if let Err(error) = write_broker_session(&mut child, &broker_session) {
            crate::credential_broker::credential_broker_uds::clear_broker_peer(
                broker_peer_generation,
            );
            let _ = force_kill_child_tree(&mut child);
            let _ = child.wait();
            return Err(error);
        }
        let ownership = serde_json::json!({
            "daemon_pid": child.id(),
            "host_pid": std::process::id(),
            "instance_id": instance_id,
            "daemon_bin": self.config.daemon_bin.display().to_string(),
            "started_at": chrono::Utc::now().to_rfc3339(),
            "runtime_dir": self.config.runtime_dir.display().to_string(),
            "socket": self.config.socket_path.display().to_string(),
        });
        let _ = std::fs::write(
            self.config.runtime_dir.join("agent-daemon.ownership.json"),
            ownership.to_string(),
        );
        Ok((child, bootstrap, instance_id, broker_peer_generation))
    }

    fn wait_for_readiness(
        &self,
        bootstrap: &str,
        instance_id: &str,
        timeout: Duration,
    ) -> Result<(), String> {
        #[cfg(test)]
        if self.readiness_bypass.load(Ordering::SeqCst) {
            return Ok(());
        }
        let start = Instant::now();
        let mut last_error = "daemon not ready".to_string();
        while start.elapsed() < timeout {
            if !self.watchdog_should_run() {
                return Err("sidecar supervisor shut down during readiness".into());
            }
            if self.config.socket_path.exists() {
                match self.readiness_probe(bootstrap, instance_id) {
                    Ok(()) => return Ok(()),
                    Err(error) => last_error = error,
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Err(format!(
            "daemon readiness failed within {:?}: {last_error}",
            timeout
        ))
    }

    fn readiness_probe(&self, bootstrap: &str, instance_id: &str) -> Result<(), String> {
        #[cfg(test)]
        if self.health_probe_bypass.load(Ordering::SeqCst) {
            return Ok(());
        }
        let socket = self.config.socket_path.clone();
        let bootstrap = bootstrap.to_string();
        let instance_id = instance_id.to_string();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| format!("readiness runtime: {e}"))?;
            rt.block_on(async {
                tokio::time::timeout(DAEMON_RPC_PROBE_TIMEOUT, async move {
                    let mut client = natives_agent_daemon::DaemonClient::connect(
                        socket,
                        &bootstrap,
                        natives_agent_daemon::client_protocol_version(),
                    )
                    .await
                    .map_err(|e| format!("handshake/ping connect: {e}"))?;
                    if client.protocol_version() != natives_agent_daemon::client_protocol_version()
                    {
                        return Err(format!(
                            "protocol mismatch: daemon={} client={}",
                            client.protocol_version(),
                            natives_agent_daemon::client_protocol_version()
                        ));
                    }
                    let ping = client
                        .call("daemon.ping", serde_json::json!({}))
                        .await
                        .map_err(|e| format!("daemon.ping: {e}"))?;
                    if ping.get("pong").and_then(|v| v.as_bool()) != Some(true) {
                        return Err(format!("daemon.ping missing pong: {ping}"));
                    }
                    let status = client
                        .call("daemon.getStatus", serde_json::json!({}))
                        .await
                        .map_err(|e| format!("daemon.getStatus: {e}"))?;
                    if status.get("protocol_version").and_then(|v| v.as_str())
                        != Some(natives_agent_daemon::client_protocol_version())
                    {
                        return Err(format!("daemon.getStatus protocol mismatch: {status}"));
                    }
                    if status.get("instance_id").and_then(|v| v.as_str())
                        != Some(instance_id.as_str())
                    {
                        return Err("daemon.getStatus instance mismatch".into());
                    }
                    // W3 P0-03: DaemonStatusV2 deliberately carries NO db path.
                    // Readiness is decomposed (instance/protocol/health/storage/
                    // broker); a missing legacy field must never kill a healthy
                    // sidecar, and the host must not demand a field the daemon no
                    // longer reports.
                    if status.get("storage_ready").and_then(|v| v.as_str()) != Some("ready") {
                        return Err(format!("daemon.getStatus storage not ready: {status}"));
                    }
                    if status
                        .get("credential_broker_ready")
                        .and_then(|v| v.as_str())
                        != Some("ready")
                    {
                        return Err(format!(
                            "daemon.getStatus credential broker not ready: {status}"
                        ));
                    }
                    let health = status
                        .get("health")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unavailable");
                    if health != "ready" && health != "degraded" {
                        return Err(format!("daemon.getStatus health not ready: {status}"));
                    }
                    Ok(())
                })
                .await
                .map_err(|_| {
                    format!(
                        "daemon readiness probe timed out after {:?}",
                        DAEMON_RPC_PROBE_TIMEOUT
                    )
                })?
            })
        })
        .join()
        .map_err(|_| "readiness probe panicked".to_string())?
    }

    pub fn poll_child_health(&self) -> SupervisorStatus {
        {
            let mut inner = match self.state.lock() {
                Ok(g) => g,
                Err(_) => return self.status(),
            };
            let mut exited_generation = None;
            if let Some(child) = inner.child.as_mut() {
                match child.try_wait() {
                    Ok(Some(exit_status)) => {
                        inner.child = None;
                        inner.lifeline_stdin = None;
                        inner.bootstrap_token = None;
                        inner.instance_id = None;
                        exited_generation = inner.broker_peer_generation.take();
                        let reason = format!("sidecar exited: {exit_status}");
                        inner.status.state = if inner.shutdown_started {
                            SupervisorState::Stopped
                        } else {
                            SupervisorState::Faulted {
                                reason: reason.clone(),
                            }
                        };
                        inner.status.production_ready = false;
                        inner.status.last_error = Some(reason);
                        inner.status.pid = None;
                    }
                    Ok(None) => {
                        if matches!(inner.status.state, SupervisorState::Healthy) {
                            inner.status.production_ready = self.config.require_uds;
                        }
                    }
                    Err(e) => {
                        let reason = format!("poll child: {e}");
                        inner.status.state = SupervisorState::Degraded {
                            reason: reason.clone(),
                        };
                        inner.status.production_ready = false;
                        inner.status.last_error = Some(reason);
                    }
                }
            }
            if let Some(generation) = exited_generation {
                crate::credential_broker::credential_broker_uds::clear_broker_peer(generation);
                self.remove_runtime_artifacts();
            }
            inner.status.clone()
        }
    }

    pub fn ensure_healthy_or_restart(&self) -> Result<SupervisorStatus, String> {
        self.poll_child_health();
        let bootstrap = {
            let inner = self.state.lock().map_err(|e| e.to_string())?;
            if inner.shutdown_started {
                return Err("sidecar supervisor is shutting down; restart refused".into());
            }
            if matches!(inner.status.state, SupervisorState::Healthy) {
                inner.bootstrap_token.clone()
            } else {
                None
            }
        };
        if let Some(bootstrap) = bootstrap {
            let instance_id = {
                let inner = self.state.lock().map_err(|e| e.to_string())?;
                inner.instance_id.clone()
            }
            .ok_or_else(|| "sidecar instance identity unavailable".to_string())?;
            if let Err(error) = self.readiness_probe(&bootstrap, &instance_id) {
                return self.restart_unresponsive_child(&bootstrap, error);
            }
            return Ok(self.status());
        }
        let restart_count = {
            let mut inner = self.state.lock().map_err(|e| e.to_string())?;
            if inner.shutdown_started {
                return Err("sidecar supervisor is shutting down; restart refused".into());
            }
            if matches!(inner.status.state, SupervisorState::Healthy) {
                return Ok(inner.status.clone());
            }
            if matches!(
                inner.status.state,
                SupervisorState::Starting | SupervisorState::Restarting
            ) {
                return Err("sidecar startup or restart already in progress".into());
            }
            if inner.child.is_some() {
                return Err("sidecar health is unknown; refusing duplicate child spawn".into());
            }
            if inner.status.restart_count >= self.config.max_restarts {
                let reason = format!(
                    "sidecar restart budget exhausted ({}/{})",
                    inner.status.restart_count, self.config.max_restarts
                );
                inner.status.state = SupervisorState::Faulted {
                    reason: reason.clone(),
                };
                inner.status.production_ready = false;
                inner.status.last_error = Some(reason.clone());
                return Err(reason);
            }
            inner.status.restart_count = inner.status.restart_count.saturating_add(1);
            inner.status.state = SupervisorState::Restarting;
            inner.status.production_ready = false;
            inner.status.pid = None;
            inner.status.restart_count
        };

        let result = self.start_claimed();
        if let Err(error) = &result {
            let mut inner = self.state.lock().map_err(|e| e.to_string())?;
            if !inner.shutdown_started {
                inner.status.last_error = Some(format!(
                    "restart attempt {restart_count}/{} failed: {error}",
                    self.config.max_restarts
                ));
            }
        }
        result
    }

    fn restart_unresponsive_child(
        &self,
        bootstrap: &str,
        probe_error: String,
    ) -> Result<SupervisorStatus, String> {
        let (restart_count, mut child, broker_peer_generation) = {
            let mut inner = self.state.lock().map_err(|e| e.to_string())?;
            if inner.shutdown_started {
                return Err("sidecar supervisor is shutting down; restart refused".into());
            }
            if !matches!(inner.status.state, SupervisorState::Healthy)
                || inner.bootstrap_token.as_deref() != Some(bootstrap)
            {
                return Ok(inner.status.clone());
            }
            if inner.status.restart_count >= self.config.max_restarts {
                let reason = format!(
                    "sidecar restart budget exhausted ({}/{})",
                    inner.status.restart_count, self.config.max_restarts
                );
                inner.status.state = SupervisorState::Faulted {
                    reason: reason.clone(),
                };
                inner.status.production_ready = false;
                inner.status.last_error = Some(reason.clone());
                return Err(reason);
            }
            let child = inner
                .child
                .take()
                .ok_or_else(|| "sidecar healthy state has no child".to_string())?;
            inner.status.restart_count = inner.status.restart_count.saturating_add(1);
            inner.status.state = SupervisorState::Restarting;
            inner.status.production_ready = false;
            inner.status.pid = None;
            inner.status.last_error = Some(format!("sidecar health probe failed: {probe_error}"));
            inner.bootstrap_token = None;
            inner.instance_id = None;
            drop(inner.lifeline_stdin.take());
            (
                inner.status.restart_count,
                child,
                inner.broker_peer_generation.take(),
            )
        };

        if let Some(generation) = broker_peer_generation {
            crate::credential_broker::credential_broker_uds::clear_broker_peer(generation);
        }
        if let Err(error) = force_kill_child_tree(&mut child) {
            let mut inner = self.state.lock().map_err(|e| e.to_string())?;
            if !inner.shutdown_started {
                inner.status.state = SupervisorState::Faulted {
                    reason: format!("restart could not stop unhealthy sidecar: {error}"),
                };
                inner.status.last_error = Some(error.clone());
            }
            return Err(format!("restart could not stop unhealthy sidecar: {error}"));
        }
        let _ = child.wait();
        self.remove_runtime_artifacts();

        let result = self.start_claimed();
        if let Err(error) = &result {
            let mut inner = self.state.lock().map_err(|e| e.to_string())?;
            if !inner.shutdown_started {
                inner.status.last_error = Some(format!(
                    "restart attempt {restart_count}/{} failed: {error}",
                    self.config.max_restarts
                ));
            }
        }
        result
    }

    /// Graceful stop with grace→force upgrade. Idempotent under concurrent hooks.
    pub fn shutdown(&self) -> Result<(), String> {
        self.shutdown_with_grace(self.config.shutdown_grace)
    }

    /// Same as [`Self::shutdown`] with an explicit grace window (tests / ops).
    pub fn shutdown_with_grace(&self, grace: Duration) -> Result<(), String> {
        let (mut child, _broker_peer_generation) = {
            let mut inner = self.state.lock().map_err(|e| e.to_string())?;
            if inner.shutdown_started && inner.child.is_none() {
                if matches!(inner.status.state, SupervisorState::Stopped) {
                    return Ok(());
                }
                if matches!(inner.status.state, SupervisorState::ShuttingDown) {
                    inner.status.state = SupervisorState::Stopped;
                    return Ok(());
                }
            }
            inner.shutdown_started = true;
            inner.status.state = SupervisorState::ShuttingDown;
            let broker_peer_generation = inner.broker_peer_generation.take();
            if let Some(generation) = broker_peer_generation {
                crate::credential_broker::credential_broker_uds::clear_broker_peer(generation);
            }
            // Closing the write end signals EOF to Daemon (primary graceful path).
            drop(inner.lifeline_stdin.take());
            (inner.child.take(), broker_peer_generation)
        };

        let mut kill_err: Option<String> = None;
        if let Some(ref mut child) = child {
            let deadline = Instant::now() + grace;
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) if Instant::now() >= deadline => {
                        if let Err(e) = force_kill_child_tree(child) {
                            kill_err = Some(e);
                        }
                        match child.wait() {
                            Ok(_) => break,
                            Err(e) => {
                                kill_err = Some(format!(
                                    "{}; wait: {e}",
                                    kill_err.clone().unwrap_or_default()
                                ));
                                break;
                            }
                        }
                    }
                    Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                    Err(e) => {
                        kill_err = Some(format!("try_wait: {e}"));
                        let _ = force_kill_child_tree(child);
                        let _ = child.wait();
                        break;
                    }
                }
            }
        }

        self.remove_runtime_artifacts();
        let mut inner = self.state.lock().map_err(|e| e.to_string())?;
        inner.child = None;
        inner.lifeline_stdin = None;
        inner.bootstrap_token = None;
        inner.instance_id = None;
        inner.broker_peer_generation = None;
        if let Some(err) = kill_err {
            inner.status.state = SupervisorState::Faulted {
                reason: format!("shutdown cleanup failed: {err}"),
            };
            inner.status.last_error = Some(err.clone());
            inner.status.production_ready = false;
            inner.status.pid = None;
            return Err(format!("shutdown cleanup failed: {err}"));
        }
        inner.status.state = SupervisorState::Stopped;
        inner.status.pid = None;
        inner.status.production_ready = false;
        Ok(())
    }
}

// Process-wide authority (existing singleton; watchdog reuses it).
/// Process-wide supervisor (lazy).

static GLOBAL_SUPERVISOR: std::sync::OnceLock<SidecarSupervisor> = std::sync::OnceLock::new();

pub fn global_supervisor() -> &'static SidecarSupervisor {
    GLOBAL_SUPERVISOR.get_or_init(|| SidecarSupervisor::new(SupervisorConfig::from_env()))
}

impl SidecarSupervisor {
    /// Never kill an arbitrary PID from a stale file. Always fail-closed without deeper proof.
    pub fn should_reap_stale_ownership(
        record: &serde_json::Value,
        expected_bin: &Path,
        expected_instance: Option<&str>,
    ) -> bool {
        let Some(bin) = record.get("daemon_bin").and_then(|v| v.as_str()) else {
            return false;
        };
        if Path::new(bin) != expected_bin {
            return false;
        }
        if let Some(want) = expected_instance {
            let got = record.get("instance_id").and_then(|v| v.as_str());
            if got != Some(want) {
                return false;
            }
        }
        false
    }

    pub fn bootstrap_token(&self) -> Option<String> {
        self.state
            .lock()
            .ok()
            .and_then(|g| g.bootstrap_token.clone())
    }

    pub fn config(&self) -> &SupervisorConfig {
        &self.config
    }

    /// The Host watchdog exits after any explicit application/supervisor shutdown.
    pub fn watchdog_should_run(&self) -> bool {
        self.state
            .lock()
            .map(|inner| !inner.shutdown_started)
            .unwrap_or(false)
    }

    fn remove_runtime_artifacts(&self) {
        let _ = std::fs::remove_file(&self.config.socket_path);
        let _ = std::fs::remove_file(&self.config.pid_path);
        let _ = std::fs::remove_file(self.config.runtime_dir.join("agent-daemon.ownership.json"));
    }

    #[cfg(test)]
    fn bypass_readiness_for_test(&self) {
        self.readiness_bypass.store(true, Ordering::SeqCst);
    }

    #[cfg(test)]
    fn bypass_health_probe_for_test(&self) {
        self.health_probe_bypass.store(true, Ordering::SeqCst);
    }
}

#[cfg(test)]
#[path = "sidecar_supervisor_tests.rs"]
mod sidecar_supervisor_tests;
