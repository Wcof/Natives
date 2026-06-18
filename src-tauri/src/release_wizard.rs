use crate::{Error, Result};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct ProjectInspection {
    pub name: String,
    pub version: String,
    pub has_changelog: bool,
    pub has_package_json: bool,
    pub has_cargo_toml: bool,
    pub git_dirty: bool,
    pub git_branch: String,
}

/// Inspect a project for release readiness
pub fn inspect_project(project_path: &str) -> Result<ProjectInspection> {
    let path = Path::new(project_path);
    if !path.exists() {
        return Err(Error::NotFound(project_path.to_string()));
    }

    let has_package_json = path.join("package.json").exists();
    let has_cargo_toml = path.join("Cargo.toml").exists();
    let has_changelog = path.join("CHANGELOG.md").exists() || path.join("changelog.md").exists();

    // Get version from package.json or Cargo.toml
    let version = if has_package_json {
        read_package_version(path).unwrap_or_else(|| "0.0.0".to_string())
    } else if has_cargo_toml {
        read_cargo_version(path).unwrap_or_else(|| "0.0.0".to_string())
    } else {
        "0.0.0".to_string()
    };

    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    // Check git status
    let git_output = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(path)
        .output()
        .ok();
    let git_dirty = git_output
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false);

    let branch_output = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(path)
        .output()
        .ok();
    let git_branch = branch_output
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    Ok(ProjectInspection {
        name,
        version,
        has_changelog,
        has_package_json,
        has_cargo_toml,
        git_dirty,
        git_branch,
    })
}

/// Prepare a release (update version, create tag, etc.)
pub fn prepare_release(project_path: &str, version: &str) -> Result<serde_json::Value> {
    let path = Path::new(project_path);
    let has_package_json = path.join("package.json").exists();
    let has_cargo_toml = path.join("Cargo.toml").exists();

    let mut steps = Vec::new();

    // Update version in package.json
    if has_package_json {
        if let Ok(content) = std::fs::read_to_string(path.join("package.json")) {
            let updated = content.replace(
                &format!("\"version\": \"{}\"", read_package_version(path).unwrap_or_default()),
                &format!("\"version\": \"{version}\""),
            );
            if let Err(e) = std::fs::write(path.join("package.json"), updated) {
                steps.push(serde_json::json!({"step": "update package.json", "error": e.to_string()}));
            } else {
                steps.push(serde_json::json!({"step": "update package.json", "status": "ok"}));
            }
        }
    }

    // Update version in Cargo.toml
    if has_cargo_toml {
        if let Ok(content) = std::fs::read_to_string(path.join("Cargo.toml")) {
            let old_version = read_cargo_version(path).unwrap_or_default();
            let updated = content.replace(
                &format!("version = \"{old_version}\""),
                &format!("version = \"{version}\""),
            );
            if let Err(e) = std::fs::write(path.join("Cargo.toml"), updated) {
                steps.push(serde_json::json!({"step": "update Cargo.toml", "error": e.to_string()}));
            } else {
                steps.push(serde_json::json!({"step": "update Cargo.toml", "status": "ok"}));
            }
        }
    }

    Ok(serde_json::json!({
        "version": version,
        "steps": steps,
    }))
}

/// Get release sequence (list of steps to execute)
pub fn get_sequence(project_path: &str, version: &str) -> Result<serde_json::Value> {
    let path = Path::new(project_path);
    let has_package_json = path.join("package.json").exists();
    let has_cargo_toml = path.join("Cargo.toml").exists();

    let mut steps = vec![
        serde_json::json!({"id": "version", "label": "Update version", "command": "update-version"}),
    ];

    if has_package_json {
        steps.push(serde_json::json!({"id": "npm", "label": "npm install", "command": "npm install"}));
        steps.push(serde_json::json!({"id": "build", "label": "npm run build", "command": "npm run build"}));
    }
    if has_cargo_toml {
        steps.push(serde_json::json!({"id": "cargo", "label": "cargo build --release", "command": "cargo build --release"}));
    }

    steps.push(serde_json::json!({"id": "git", "label": "git commit + tag", "command": "git commit && git tag"}));
    steps.push(serde_json::json!({"id": "push", "label": "git push", "command": "git push && git push --tags"}));

    Ok(serde_json::json!({
        "version": version,
        "steps": steps,
    }))
}

/// Execute a release command
pub fn execute_command(project_path: &str, command: &str) -> Result<serde_json::Value> {
    let output = std::process::Command::new("sh")
        .args(["-c", command])
        .current_dir(project_path)
        .output()
        .map_err(|e| Error::Internal(format!("command failed: {e}")))?;

    Ok(serde_json::json!({
        "exitCode": output.status.code(),
        "stdout": String::from_utf8_lossy(&output.stdout),
        "stderr": String::from_utf8_lossy(&output.stderr),
        "success": output.status.success(),
    }))
}

fn read_package_version(dir: &Path) -> Option<String> {
    let content = std::fs::read_to_string(dir.join("package.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;
    json.get("version")?.as_str().map(|s| s.to_string())
}

fn read_cargo_version(dir: &Path) -> Option<String> {
    let content = std::fs::read_to_string(dir.join("Cargo.toml")).ok()?;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("version") && trimmed.contains('=') {
            let value = trimmed.split('=').nth(1)?.trim().trim_matches('"');
            return Some(value.to_string());
        }
    }
    None
}
