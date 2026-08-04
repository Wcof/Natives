//! Whitelist dependency install for local creative projects.
//! User must confirm in UI; only fixed package-manager install commands are allowed.

use super::logs::{LogRegistry, LogStream};
use super::store;
use crate::creative_app::model::*;
use crate::{Error, Result};
use rusqlite::Connection;
use std::path::Path;
use std::process::Stdio;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

/// Build the exact install argv for a project. Never accepts free-form user commands.
pub fn resolve_install_command(root: &Path, pm: PackageManager) -> Result<(String, Vec<String>)> {
    match pm {
        PackageManager::Npm => {
            if root.join("package-lock.json").is_file() {
                Ok(("npm".into(), vec!["ci".into()]))
            } else {
                Ok(("npm".into(), vec!["install".into()]))
            }
        }
        PackageManager::Pnpm => {
            if root.join("pnpm-lock.yaml").is_file() {
                Ok((
                    "pnpm".into(),
                    vec!["install".into(), "--frozen-lockfile".into()],
                ))
            } else {
                Ok(("pnpm".into(), vec!["install".into()]))
            }
        }
        PackageManager::Yarn => {
            // Detect yarn berry via yarn.lock + .yarnrc.yml presence roughly.
            if root.join(".yarnrc.yml").is_file() || root.join(".yarnrc.yaml").is_file() {
                Ok(("yarn".into(), vec!["install".into(), "--immutable".into()]))
            } else {
                Ok((
                    "yarn".into(),
                    vec!["install".into(), "--frozen-lockfile".into()],
                ))
            }
        }
    }
}

/// Preview exact whitelist install argv for UI confirmation (never free-form).
pub fn preview_install_command(
    conn: &Connection,
    id: &str,
) -> Result<(String, Vec<String>, PackageManager)> {
    let rec = store::get_app(conn, id)?.ok_or_else(|| Error::NotFound(id.into()))?;
    let root = Path::new(&rec.canonical_project_root);
    if !root.is_dir() {
        return Err(Error::InvalidInput("project root missing".into()));
    }
    let plan = LaunchPlan::from_json(&rec.launch_plan_json)
        .map_err(|e| Error::InvalidInput(format!("launch_plan: {e}")))?;
    let pm = detect_pm_from_plan_or_locks(root, &plan)?;
    let (program, args) = resolve_install_command(root, pm)?;
    Ok((program, args, pm))
}

fn detect_pm_from_plan_or_locks(root: &Path, plan: &LaunchPlan) -> Result<PackageManager> {
    match plan.program {
        LaunchProgram::Npm => Ok(PackageManager::Npm),
        LaunchProgram::Pnpm => Ok(PackageManager::Pnpm),
        LaunchProgram::Yarn => Ok(PackageManager::Yarn),
        _ => {
            if root.join("pnpm-lock.yaml").is_file() {
                Ok(PackageManager::Pnpm)
            } else if root.join("yarn.lock").is_file() {
                Ok(PackageManager::Yarn)
            } else {
                Ok(PackageManager::Npm)
            }
        }
    }
}

pub async fn install_dependencies(
    conn: &Connection,
    app: &AppHandle,
    logs: &LogRegistry,
    id: &str,
) -> Result<CreativeAppSummary> {
    let rec = store::get_app(conn, id)?.ok_or_else(|| Error::NotFound(id.into()))?;
    let root = Path::new(&rec.canonical_project_root);
    if !root.is_dir() {
        return Err(Error::InvalidInput("project root missing".into()));
    }
    let plan = LaunchPlan::from_json(&rec.launch_plan_json)
        .map_err(|e| Error::InvalidInput(format!("launch_plan: {e}")))?;
    if plan.runtime == LocalLaunchRuntime::StaticHttp {
        return Err(Error::InvalidInput(
            "static_http projects do not install node dependencies".into(),
        ));
    }
    let pm = detect_pm_from_plan_or_locks(root, &plan)?;
    let (program, args) = resolve_install_command(root, pm)?;

    let log = logs.get_or_open(id, &format!("deps-{id}"));
    // Redact this app's env values from install logs (by value, not just pattern).
    log.set_secrets(store::secret_values(conn, id));
    log.append(
        LogStream::System,
        &format!("install deps: {program} {}", args.join(" ")),
    );
    let _ = app.emit(
        "creative-app-operation-progress",
        serde_json::json!({
            "appId": id,
            "stage": "installing_dependencies",
            "message": format!("{program} {}", args.join(" ")),
        }),
    );

    let mut cmd = Command::new(&program);
    cmd.args(&args)
        .current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .env_clear()
        .envs(std::env::vars().filter(|(k, _)| {
            matches!(
                k.as_str(),
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
                    | "SystemRoot"
                    | "ComSpec"
                    | "PATHEXT"
                    | "USERPROFILE"
                    | "HOMEDRIVE"
                    | "HOMEPATH"
                    | "APPDATA"
                    | "LOCALAPPDATA"
                    | "ALLUSERSPROFILE"
            ) || k.starts_with("LC_")
        }));

    // New process group so a package manager that spawns children (node-gyp,
    // postinstall scripts) is cleaned up as a tree on kill_on_drop / app exit.
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
        .map_err(|e| Error::Internal(format!("install spawn failed: {e}")))?;

    if let Some(out) = child.stdout.take() {
        let log = log.clone();
        let app = app.clone();
        let id = id.to_string();
        tokio::spawn(async move {
            let mut lines = BufReader::new(out).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let entry = log.append(LogStream::Stdout, &line);
                let _ = app.emit(
                    "creative-app-log",
                    serde_json::json!({
                        "appId": id,
                        "seq": entry.seq,
                        "tsMs": entry.ts_ms,
                        "stream": "stdout",
                        "text": entry.text,
                    }),
                );
            }
        });
    }
    if let Some(err) = child.stderr.take() {
        let log = log.clone();
        let app = app.clone();
        let id = id.to_string();
        tokio::spawn(async move {
            let mut lines = BufReader::new(err).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let entry = log.append(LogStream::Stderr, &line);
                let _ = app.emit(
                    "creative-app-log",
                    serde_json::json!({
                        "appId": id,
                        "seq": entry.seq,
                        "tsMs": entry.ts_ms,
                        "stream": "stderr",
                        "text": entry.text,
                    }),
                );
            }
        });
    }

    let status = child
        .wait()
        .await
        .map_err(|e| Error::Internal(format!("install wait: {e}")))?;
    if !status.success() {
        let msg = format!("dependency install failed (exit {:?})", status.code());
        log.append(LogStream::System, &msg);
        let detail = CreativeAppStatusDetail {
            code: LocalCreativeIssueCode::DependenciesMissing,
            message: msg.clone(),
            recovery_actions: vec!["install_dependencies".into(), "open_terminal".into()],
        };
        store::set_state(
            conn,
            id,
            CreativeAppState::InstalledStopped,
            Some(&msg),
            Some(&serde_json::to_string(&detail).unwrap_or_default()),
            &chrono::Utc::now().to_rfc3339(),
        )?;
        return Err(Error::Internal(msg));
    }

    log.append(LogStream::System, "dependency install finished");
    // Re-check node_modules
    let missing = !root.join("node_modules").is_dir();
    let detail = if missing {
        Some(CreativeAppStatusDetail {
            code: LocalCreativeIssueCode::DependenciesMissing,
            message: "node_modules still missing after install".into(),
            recovery_actions: vec!["install_dependencies".into()],
        })
    } else {
        None
    };
    store::set_state(
        conn,
        id,
        CreativeAppState::InstalledStopped,
        None,
        detail
            .as_ref()
            .map(|d| serde_json::to_string(d).unwrap_or_default())
            .as_deref(),
        &chrono::Utc::now().to_rfc3339(),
    )?;
    let rec = store::get_app(conn, id)?.unwrap();
    Ok(store::summary_from_local(&rec))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn npm_ci_when_lock_present() {
        let dir = std::env::temp_dir().join(format!(
            "natives-dep-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("package-lock.json"), "{}").unwrap();
        let (p, a) = resolve_install_command(&dir, PackageManager::Npm).unwrap();
        assert_eq!(p, "npm");
        assert_eq!(a, vec!["ci".to_string()]);
        let _ = fs::remove_dir_all(&dir);
    }
}
