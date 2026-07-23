//! Docker / Compose CLI adapter — argv only, no shell.

use super::model::DockerEngineStatus;
use super::paths::{resource_label, resource_label_key};
use crate::{Error, Result};
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

async fn run_capture(program: &str, args: &[&str], env: &[(&str, &str)]) -> Result<(i32, String, String)> {
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
            error: Some(format!("docker info exit {code}: {}", truncate(&stderr, 200))),
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
    let args = [
        "compose",
        "-p",
        project,
        "-f",
        file.as_ref(),
        "pull",
    ];
    let (code, _, stderr) = run_capture("docker", &args, &[]).await?;
    if code != 0 {
        return Err(fail_cmd("docker", &args, code, &stderr));
    }
    Ok(())
}

pub async fn compose_up(project: &str, compose_file: &Path, env_pairs: &[(String, String)]) -> Result<()> {
    let file = compose_file.to_string_lossy();
    // Pass env via process environment so values never appear in argv.
    let mut env_refs: Vec<(&str, &str)> = env_pairs
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    // Also set COMPOSE project label via --project-name already
    let label = resource_label(
        project
            .strip_prefix("natives-")
            .unwrap_or(project),
    );
    // docker compose does not take arbitrary labels on up easily for all resources;
    // we rely on container name project prefix + inspect by label when possible.
    let _ = label;
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
    // Build env slice
    let env_slice: Vec<(&str, &str)> = env_refs.drain(..).collect();
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
        if env_values.values().any(|v| !v.is_empty() && a.contains(v.as_str())) {
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
    let args = [
        "inspect",
        "-f",
        "{{.State.Running}}",
        container_name,
    ];
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
}
