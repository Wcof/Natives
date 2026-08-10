//! Local runtime process / port / URL utilities (W3 split from runtime.rs).
//!
//! Pure helpers shared by the local runtime manager: command building,
//! executable resolution, port probing, preview URL derivation. No manager
//! state lives here — the manager composes these.

use crate::creative_app::model::{LaunchPlan, LaunchProgram, LocalLaunchRuntime, ProcessIdentity};
use crate::{Error, Result};
use std::path::Path;
use std::time::{Duration, Instant};

/// Build the spawn command + args for a managed-process launch profile
/// (batch 10 CR-1002) or the legacy program map.
pub fn build_command(plan: &LaunchPlan, _port: u16) -> Result<(String, Vec<String>)> {
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
                let interpreter = crate::creative_app::process_driver::resolve_python_interpreter(
                    &p.interpreter,
                )?;
                let mut args = vec![p.entry.clone()];
                args.extend(p.args.iter().cloned());
                Ok((interpreter, args))
            }
            ProcessProfile::Binary(b) => {
                // T09: recompute identity at launch. A content change since
                // approval invalidates the authorization and refuses to spawn.
                let canonical = crate::creative_app::process_driver::verify_binary_identity(
                    &b.executable_path,
                    &b.executable_hash,
                )?;
                Ok((canonical, b.args.clone()))
            }
        };
    }
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

/// UNIX epoch seconds when a process started (PID reuse safe).
pub fn process_start_time_unix(pid: Option<u32>) -> Option<i64> {
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

/// Resolve an executable by absolute path or PATH lookup.
pub fn resolve_executable_path(program: &str) -> Option<String> {
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

/// Env keys safe to inherit into a spawned child (never secrets).
pub fn is_safe_inherited_env(key: &str) -> bool {
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

/// Bind to 127.0.0.1:0 then drop to find a free port.
pub fn pick_free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .ok()
        .and_then(|l| l.local_addr().ok())
        .map(|a| a.port())
        .unwrap_or(5173)
}

/// If we cannot bind, assume in use.
pub fn port_in_use(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_err()
}

/// True when something accepts TCP on the port.
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

/// Best-effort HTTP reachability probe (2s timeout, limited redirects).
pub async fn http_reachable(url: &str) -> bool {
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
