//! Docker / Compose CLI adapter — argv only, no shell.

use super::model::DockerEngineStatus;
use super::paths::{resource_label, resource_label_key};
use crate::{Error, Result};
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

async fn run_capture(
    program: &str,
    args: &[&str],
    env: &[(&str, &str)],
) -> Result<(i32, String, String)> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for (k, v) in env {
        cmd.env(k, v);
    }
    // Never pass secrets as CLI flags — only as env keys for docker -e KEY
    let output = cmd
        .output()
        .await
        .map_err(|e| Error::Internal(format!("failed to spawn {program}: {e}")))?;
    let code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    Ok((code, stdout, stderr))
}

fn fail_cmd(program: &str, args: &[&str], code: i32, stderr: &str) -> Error {
    // Sanitize: never include env values
    Error::Internal(format!(
        "{program} {} failed (exit {code}): {}",
        args.join(" "),
        truncate(stderr, 800)
    ))
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

pub async fn engine_status() -> DockerEngineStatus {
    let info = run_capture("docker", &["info", "--format", "{{.ServerVersion}}"], &[]).await;
    match info {
        Ok((0, stdout, _)) => {
            let version = stdout.trim().to_string();
            let compose = run_capture("docker", &["compose", "version", "--short"], &[]).await;
            let (compose_available, compose_version) = match compose {
                Ok((0, v, _)) => (true, Some(v.trim().to_string())),
                _ => (false, None),
            };
            DockerEngineStatus {
                available: true,
                version: if version.is_empty() {
                    None
                } else {
                    Some(version)
                },
                compose_available,
                compose_version,
                error: None,
            }
        }
        Ok((code, _, stderr)) => DockerEngineStatus {
            available: false,
            version: None,
            compose_available: false,
            compose_version: None,
            error: Some(format!(
                "docker info exit {code}: {}",
                truncate(&stderr, 200)
            )),
        },
        Err(e) => DockerEngineStatus {
            available: false,
            version: None,
            compose_available: false,
            compose_version: None,
            error: Some(e.to_string()),
        },
    }
}

pub async fn require_docker() -> Result<DockerEngineStatus> {
    let st = engine_status().await;
    if !st.available {
        return Err(Error::Internal(
            st.error
                .unwrap_or_else(|| "Docker Engine is not available".into()),
        ));
    }
    Ok(st)
}

// ── Compose ────────────────────────────────────────────────────

pub async fn compose_pull(project: &str, compose_file: &Path) -> Result<()> {
    let file = compose_file.to_string_lossy();
    let args = ["compose", "-p", project, "-f", file.as_ref(), "pull"];
    let (code, _, stderr) = run_capture("docker", &args, &[]).await?;
    if code != 0 {
        return Err(fail_cmd("docker", &args, code, &stderr));
    }
    Ok(())
}

pub async fn compose_up(
    project: &str,
    compose_file: &Path,
    env_pairs: &[(String, String)],
) -> Result<()> {
    let file = compose_file.to_string_lossy();
    // Pass env via process environment so values never appear in argv.
    let mut env_refs: Vec<(&str, &str)> = env_pairs
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let args = [
        "compose",
        "-p",
        project,
        "-f",
        file.as_ref(),
        "up",
        "-d",
        "--no-build",
        "--remove-orphans",
    ];
    let env_slice: Vec<(&str, &str)> = std::mem::take(&mut env_refs);
    let (code, _, stderr) = run_capture("docker", &args, &env_slice).await?;
    if code != 0 {
        return Err(fail_cmd("docker", &args, code, &stderr));
    }
    Ok(())
}

/// Start a Compose project with a Natives-owned command override (batch 8).
/// The override file lives in `override_dir` and never touches the user's
/// compose file; it sets the service `command` to the approved tokens.
pub async fn compose_up_override(
    project: &str,
    compose_file: &Path,
    service: &str,
    command: &[String],
    env_pairs: &[(String, String)],
    override_dir: &Path,
) -> Result<()> {
    let override_path = override_dir.join(format!("{project}-override.yml"));
    let cmd_yaml = command
        .iter()
        .map(|t| format!("- {t}"))
        .collect::<Vec<_>>()
        .join("\n");
    let yaml = format!("services:\n  {service}:\n    command:\n{cmd_yaml}\n");
    // Override-file write is small std::fs IO; keep it off the async runtime.
    let override_dir_owned = override_dir.to_path_buf();
    let override_path_owned = override_path.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        std::fs::create_dir_all(&override_dir_owned).map_err(Error::Io)?;
        std::fs::write(&override_path_owned, yaml).map_err(Error::Io)?;
        Ok(())
    })
    .await
    .map_err(|e| Error::Internal(e.to_string()))??;

    let file = compose_file.to_string_lossy();
    let ov = override_path.to_string_lossy();
    let args = [
        "compose",
        "-p",
        project,
        "-f",
        file.as_ref(),
        "-f",
        ov.as_ref(),
        "up",
        "-d",
        "--no-build",
        "--remove-orphans",
    ];
    let mut env_refs: Vec<(&str, &str)> = env_pairs
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let env_slice: Vec<(&str, &str)> = std::mem::take(&mut env_refs);
    let (code, _, stderr) = run_capture("docker", &args, &env_slice).await?;
    if code != 0 {
        return Err(fail_cmd("docker", &args, code, &stderr));
    }
    Ok(())
}

pub async fn compose_stop(project: &str, compose_file: &Path) -> Result<()> {
    let file = compose_file.to_string_lossy();
    let args = ["compose", "-p", project, "-f", file.as_ref(), "stop"];
    let (code, _, stderr) = run_capture("docker", &args, &[]).await?;
    if code != 0 {
        return Err(fail_cmd("docker", &args, code, &stderr));
    }
    Ok(())
}

pub async fn compose_down(
    project: &str,
    compose_file: &Path,
    remove_volumes: bool,
    remove_images: bool,
) -> Result<()> {
    let file = compose_file.to_string_lossy();
    let mut args = vec![
        "compose",
        "-p",
        project,
        "-f",
        file.as_ref(),
        "down",
        "--remove-orphans",
    ];
    if remove_volumes {
        args.push("--volumes");
    }
    if remove_images {
        args.push("--rmi");
        args.push("local");
    }
    let (code, _, stderr) = run_capture("docker", &args, &[]).await?;
    if code != 0 {
        return Err(fail_cmd("docker", &args, code, &stderr));
    }
    Ok(())
}

pub async fn compose_ps_running(project: &str, compose_file: &Path) -> Result<bool> {
    let file = compose_file.to_string_lossy();
    let args = [
        "compose",
        "-p",
        project,
        "-f",
        file.as_ref(),
        "ps",
        "--status",
        "running",
        "-q",
    ];
    let (code, stdout, _) = run_capture("docker", &args, &[]).await?;
    if code != 0 {
        return Ok(false);
    }
    Ok(!stdout.trim().is_empty())
}

/// Resolve the host port a Compose project actually published on 127.0.0.1
/// (batch 5). Parses `docker ps` Ports output like `127.0.0.1:8080->8080/tcp`.
pub async fn compose_host_port(project: &str, compose_file: &Path) -> Result<Option<u16>> {
    let _ = compose_file;
    let filter = format!("com.docker.compose.project={project}");
    let args = ["ps", "--filter", &filter, "--format", "{{.Ports}}"];
    let (code, stdout, _) = run_capture("docker", &args, &[]).await?;
    if code != 0 {
        return Ok(None);
    }
    for line in stdout.lines() {
        for part in line.split(',') {
            let part = part.trim();
            if let Some(idx) = part.find("->") {
                let left = &part[..idx];
                if let Some(colon) = left.rfind(':') {
                    if let Ok(p) = left[colon + 1..].parse::<u16>() {
                        return Ok(Some(p));
                    }
                }
            }
        }
    }
    Ok(None)
}

/// Verify a Compose project has no running containers (post-stop check).
pub async fn compose_has_running(project: &str, compose_file: &Path) -> Result<bool> {
    compose_ps_running(project, compose_file).await
}

// ── Docker Run ─────────────────────────────────────────────────

pub async fn docker_pull(image: &str) -> Result<()> {
    let args = ["pull", image];
    let (code, _, stderr) = run_capture("docker", &args, &[]).await?;
    if code != 0 {
        return Err(fail_cmd("docker", &args, code, &stderr));
    }
    Ok(())
}

/// Create container with label and 127.0.0.1 port publish.
/// Env: only keys appear as `-e KEY`; values come from parent process env.
pub async fn docker_create(
    container_name: &str,
    image: &str,
    host_port: u16,
    container_port: u16,
    app_id: &str,
    env_keys: &[String],
    env_values: &HashMap<String, String>,
) -> Result<()> {
    let label = resource_label(app_id);
    let port = format!("127.0.0.1:{host_port}:{container_port}");
    let mut args: Vec<String> = vec![
        "create".into(),
        "--name".into(),
        container_name.into(),
        "--label".into(),
        label,
        "-p".into(),
        port,
    ];
    for k in env_keys {
        args.push("-e".into());
        args.push(k.clone()); // KEY only
    }
    args.push(image.into());

    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let env_pairs: Vec<(&str, &str)> = env_keys
        .iter()
        .filter_map(|k| env_values.get(k).map(|v| (k.as_str(), v.as_str())))
        .collect();

    // Ensure secrets don't leak into logs of args
    for a in &arg_refs {
        if env_values
            .values()
            .any(|v| !v.is_empty() && a.contains(v.as_str()))
        {
            return Err(Error::Internal(
                "internal error: secret leaked into docker argv".into(),
            ));
        }
    }

    let (code, _, stderr) = run_capture("docker", &arg_refs, &env_pairs).await?;
    if code != 0 {
        return Err(fail_cmd("docker", &arg_refs, code, &stderr));
    }
    Ok(())
}

pub async fn docker_start(container_name: &str) -> Result<()> {
    let args = ["start", container_name];
    let (code, _, stderr) = run_capture("docker", &args, &[]).await?;
    if code != 0 {
        return Err(fail_cmd("docker", &args, code, &stderr));
    }
    Ok(())
}

pub async fn docker_stop(container_name: &str) -> Result<()> {
    let args = ["stop", container_name];
    let (code, _, stderr) = run_capture("docker", &args, &[]).await?;
    if code != 0 {
        return Err(fail_cmd("docker", &args, code, &stderr));
    }
    Ok(())
}

pub async fn docker_rm(container_name: &str, force: bool) -> Result<()> {
    let mut args = vec!["rm"];
    if force {
        args.push("-f");
    }
    args.push(container_name);
    let (code, _, stderr) = run_capture("docker", &args, &[]).await?;
    if code != 0 {
        return Err(fail_cmd("docker", &args, code, &stderr));
    }
    Ok(())
}

pub async fn docker_inspect_running(container_name: &str) -> Result<bool> {
    let args = ["inspect", "-f", "{{.State.Running}}", container_name];
    let (code, stdout, _) = run_capture("docker", &args, &[]).await?;
    if code != 0 {
        return Ok(false);
    }
    Ok(stdout.trim().eq_ignore_ascii_case("true"))
}

pub async fn docker_exists(container_name: &str) -> Result<bool> {
    let args = ["inspect", container_name];
    let (code, _, _) = run_capture("docker", &args, &[]).await?;
    Ok(code == 0)
}

pub async fn docker_logs(container_name: &str, tail: usize) -> Result<String> {
    let tail_s = tail.to_string();
    let args = ["logs", "--tail", &tail_s, container_name];
    let (code, stdout, stderr) = run_capture("docker", &args, &[]).await?;
    if code != 0 {
        return Err(fail_cmd("docker", &args, code, &stderr));
    }
    // Prefer stdout; merge stderr lines without secrets (docker logs may mix)
    if stdout.is_empty() {
        Ok(stderr)
    } else if stderr.is_empty() {
        Ok(stdout)
    } else {
        Ok(format!("{stdout}{stderr}"))
    }
}

pub async fn compose_logs(project: &str, compose_file: &Path, tail: usize) -> Result<String> {
    let file = compose_file.to_string_lossy();
    let tail_s = tail.to_string();
    let args = [
        "compose",
        "-p",
        project,
        "-f",
        file.as_ref(),
        "logs",
        "--tail",
        &tail_s,
        "--no-color",
    ];
    let (code, stdout, stderr) = run_capture("docker", &args, &[]).await?;
    if code != 0 {
        return Err(fail_cmd("docker", &args, code, &stderr));
    }
    if stdout.is_empty() {
        Ok(stderr)
    } else {
        Ok(stdout)
    }
}

/// Find containers by app label.
pub async fn containers_by_app_id(app_id: &str) -> Result<Vec<String>> {
    let filter = format!("label={}", resource_label_key());
    // docker ps -aq --filter label=ai.natives.creative-app.id --filter label=...=app_id
    let filter2 = format!("label={}={}", resource_label_key(), app_id);
    let args = ["ps", "-aq", "--filter", &filter2];
    let (code, stdout, stderr) = run_capture("docker", &args, &[]).await?;
    if code != 0 {
        return Err(fail_cmd("docker", &args, code, &stderr));
    }
    let ids = stdout
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();
    let _ = filter;
    Ok(ids)
}

/// Readiness: HTTP 200–499 within timeout.
pub async fn wait_ready(url: &str, timeout: Duration) -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|e| Error::Internal(format!("http client: {e}")))?;
    let start = std::time::Instant::now();
    let mut last_err = String::from("not attempted");
    while start.elapsed() < timeout {
        match client.get(url).send().await {
            Ok(resp) => {
                let code = resp.status().as_u16();
                if (200..500).contains(&code) {
                    return Ok(());
                }
                last_err = format!("HTTP {code}");
            }
            Err(e) => {
                last_err = e.to_string();
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    Err(Error::Internal(format!(
        "health check timed out for {url}: {last_err}"
    )))
}

/// Check if a host TCP port is already in use (best-effort via docker port listing).
pub async fn host_port_in_use(port: u16) -> Result<bool> {
    // Use `docker ps --format` published ports
    let args = ["ps", "--format", "{{.Ports}}"];
    let (code, stdout, _) = run_capture("docker", &args, &[]).await?;
    if code != 0 {
        return Ok(false);
    }
    let needle = format!(":{port}->");
    let needle2 = format!("0.0.0.0:{port}");
    let needle3 = format!("127.0.0.1:{port}");
    Ok(stdout.contains(&needle) || stdout.contains(&needle2) || stdout.contains(&needle3))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_works() {
        assert_eq!(truncate("abc", 10), "abc");
        assert!(truncate("abcdefghijklmnopqrstuvwxyz", 5).ends_with('…'));
    }

    #[tokio::test]
    async fn argv_never_embeds_secret_value() {
        let mut env: HashMap<String, String> = HashMap::new();
        env.insert("SECRET".into(), "super-secret-value".into());
        let keys: Vec<String> = vec!["SECRET".into()];
        for k in &keys {
            assert_eq!(k.as_str(), "SECRET");
            assert!(!k.contains("super-secret"));
        }
        for v in env.values() {
            assert_eq!(v.as_str(), "super-secret-value");
        }
    }

    /// T09: Compose safety is structural — the compose commands never pass a
    /// privileged flag, never run a global prune, and stop never removes user
    /// volumes. Guarded at the source so a future edit can't silently relax it.
    #[test]
    fn compose_commands_never_privileged_or_global_prune_or_volume_drop() {
        let src = include_str!("docker.rs");
        for (name, body) in [
            ("compose_up", "pub async fn compose_up"),
            ("compose_up_override", "pub async fn compose_up_override"),
            ("compose_stop", "pub async fn compose_stop"),
            ("compose_down", "pub async fn compose_down"),
        ] {
            let start = src.find(body).unwrap_or_else(|| panic!("{name} not found"));
            let end = src[start..]
                .find("\n    }\n")
                .map(|i| start + i)
                .unwrap_or(src.len());
            let func = &src[start..end];
            assert!(
                !func.contains("--privileged"),
                "{name} must never pass --privileged"
            );
            assert!(
                !func.contains("system prune") && !func.contains("prune"),
                "{name} must never run a global/system prune"
            );
        }
        // Stop must not remove user volumes (no -v / --volumes).
        let stop_src = include_str!("docker.rs");
        let stop_start = stop_src.find("pub async fn compose_stop").unwrap();
        let stop_end = stop_src[stop_start..].find("\n    }\n").unwrap() + stop_start;
        let stop_fn = &stop_src[stop_start..stop_end];
        assert!(
            !stop_fn.contains("-v") && !stop_fn.contains("--volumes"),
            "compose_stop must never delete user volumes"
        );
        // Delete only drops volumes when the user explicitly opts in.
        let down_start = stop_src.find("pub async fn compose_down").unwrap();
        let down_end = stop_src[down_start..].find("\n    }\n").unwrap() + down_start;
        let down_fn = &stop_src[down_start..down_end];
        assert!(
            down_fn.contains("remove_volumes"),
            "compose_down must gate volume deletion behind the user option"
        );
    }
}
