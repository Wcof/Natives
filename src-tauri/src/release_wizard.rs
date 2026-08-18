use crate::{log_sanitizer::sanitize, Error, Result};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

const MAX_DIAGNOSTIC_BYTES: usize = 8 * 1024;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInspection {
    pub name: String,
    pub version: String,
    pub has_changelog: bool,
    pub has_package_json: bool,
    pub has_cargo_toml: bool,
    pub git_dirty: bool,
    pub git_branch: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReleaseAction {
    UpdateVersion,
    NpmInstall,
    NpmBuild,
    CargoBuildRelease,
    GitCommit,
    GitTag,
    GitPushBranch,
    GitPushTags,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseStep {
    pub action: ReleaseAction,
    pub label: String,
    pub display: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleasePlan {
    pub version: String,
    pub steps: Vec<ReleaseStep>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleasePreparation {
    pub version: String,
    pub updated_files: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseExecution {
    pub action: ReleaseAction,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
    pub already_complete: bool,
}

impl ReleaseAction {
    fn parse(id: &str) -> Option<Self> {
        match id {
            "update-version" => Some(Self::UpdateVersion),
            "npm-install" => Some(Self::NpmInstall),
            "npm-build" => Some(Self::NpmBuild),
            "cargo-build-release" => Some(Self::CargoBuildRelease),
            "git-commit" => Some(Self::GitCommit),
            "git-tag" => Some(Self::GitTag),
            "git-push-branch" => Some(Self::GitPushBranch),
            "git-push-tags" => Some(Self::GitPushTags),
            _ => None,
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::UpdateVersion => "update-version",
            Self::NpmInstall => "npm-install",
            Self::NpmBuild => "npm-build",
            Self::CargoBuildRelease => "cargo-build-release",
            Self::GitCommit => "git-commit",
            Self::GitTag => "git-tag",
            Self::GitPushBranch => "git-push-branch",
            Self::GitPushTags => "git-push-tags",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::UpdateVersion => "Update version",
            Self::NpmInstall => "Install npm dependencies",
            Self::NpmBuild => "Build npm project",
            Self::CargoBuildRelease => "Build Rust release",
            Self::GitCommit => "Commit release",
            Self::GitTag => "Create release tag",
            Self::GitPushBranch => "Push branch",
            Self::GitPushTags => "Push tags",
        }
    }

    fn display(self, version: &str) -> String {
        match self {
            Self::UpdateVersion => "Update package version files".to_string(),
            Self::NpmInstall => "npm install".to_string(),
            Self::NpmBuild => "npm run build".to_string(),
            Self::CargoBuildRelease => "cargo build --release".to_string(),
            Self::GitCommit => {
                format!("git add -- version files; git commit -m release: v{version}")
            }
            Self::GitTag => format!("git tag v{version}"),
            Self::GitPushBranch => "git push".to_string(),
            Self::GitPushTags => "git push --tags".to_string(),
        }
    }

    fn argv(self, version: &str) -> Option<(&'static str, Vec<String>)> {
        match self {
            Self::UpdateVersion => None,
            Self::NpmInstall => Some(("npm", vec!["install".to_string()])),
            Self::NpmBuild => Some(("npm", vec!["run".to_string(), "build".to_string()])),
            Self::CargoBuildRelease => {
                Some(("cargo", vec!["build".to_string(), "--release".to_string()]))
            }
            Self::GitCommit => None,
            Self::GitTag => Some(("git", vec!["tag".to_string(), tag_name(version)])),
            Self::GitPushBranch => Some(("git", vec!["push".to_string()])),
            Self::GitPushTags => Some(("git", vec!["push".to_string(), "--tags".to_string()])),
        }
    }
}

/// Inspect a project for release readiness.
pub fn inspect_project(project_path: &str) -> Result<ProjectInspection> {
    let path = validate_project_dir(project_path)?;
    let has_package_json = path.join("package.json").is_file();
    let has_cargo_toml = path.join("Cargo.toml").is_file();
    let has_changelog = path.join("CHANGELOG.md").is_file() || path.join("changelog.md").is_file();
    let version = if has_package_json {
        read_package_version(&path)?
    } else if has_cargo_toml {
        read_cargo_version(&path)?
    } else {
        "0.0.0".to_string()
    };
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown")
        .to_string();
    let git_dirty = git_output(&path, &["status", "--porcelain"])
        .map(|output| !output.stdout.is_empty())
        .unwrap_or(false);
    let git_branch = git_output(&path, &["rev-parse", "--abbrev-ref", "HEAD"])
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|branch| !branch.is_empty())
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

/// Write all release version files atomically. Any failed write is an error;
/// when multiple files are affected, earlier writes are restored before return.
pub fn prepare_release(project_path: &str, version: &str) -> Result<ReleasePreparation> {
    let path = validate_release_input(project_path, version)?;
    prepare_release_at(&path, version, |file, content| {
        crate::module_manager::atomic_write(file, content)
    })
}

fn prepare_release_at<F>(path: &Path, version: &str, mut write: F) -> Result<ReleasePreparation>
where
    F: FnMut(&Path, &str) -> Result<()>,
{
    let mut updates = Vec::new();
    let package_path = path.join("package.json");
    if package_path.is_file() {
        let original = std::fs::read_to_string(&package_path).map_err(Error::Io)?;
        updates.push((
            package_path,
            original.clone(),
            update_package_version(&original, version)?,
        ));
    }
    let cargo_path = path.join("Cargo.toml");
    if cargo_path.is_file() {
        let original = std::fs::read_to_string(&cargo_path).map_err(Error::Io)?;
        updates.push((
            cargo_path,
            original.clone(),
            update_cargo_version(&original, version)?,
        ));
    }
    if updates.is_empty() {
        return Err(Error::InvalidInput(
            "release project has no package.json or Cargo.toml".into(),
        ));
    }

    let mut written: Vec<(&Path, &str)> = Vec::new();
    for (file, original, updated) in &updates {
        if original == updated {
            continue;
        }
        if let Err(error) = write(file, updated) {
            let rollback_errors = written
                .iter()
                .rev()
                .filter_map(|(written_file, previous)| {
                    write(written_file, previous)
                        .err()
                        .map(|rollback| rollback.to_string())
                })
                .collect::<Vec<_>>();
            if rollback_errors.is_empty() {
                return Err(error);
            }
            return Err(Error::Internal(format!(
                "release version write failed: {error}; rollback failed: {}",
                rollback_errors.join("; ")
            )));
        }
        written.push((file.as_path(), original.as_str()));
    }

    Ok(ReleasePreparation {
        version: version.to_string(),
        updated_files: updates
            .into_iter()
            .map(|(file, _, _)| file.to_string_lossy().to_string())
            .collect(),
    })
}

/// Generate the exact allowlisted action plan. The action ID, never this
/// display text, is what the executor accepts.
pub fn get_sequence(project_path: &str, version: &str) -> Result<ReleasePlan> {
    let path = validate_release_input(project_path, version)?;
    let mut actions = vec![ReleaseAction::UpdateVersion];
    if path.join("package.json").is_file() {
        actions.extend([ReleaseAction::NpmInstall, ReleaseAction::NpmBuild]);
    }
    if path.join("Cargo.toml").is_file() {
        actions.push(ReleaseAction::CargoBuildRelease);
    }
    actions.extend([
        ReleaseAction::GitCommit,
        ReleaseAction::GitTag,
        ReleaseAction::GitPushBranch,
        ReleaseAction::GitPushTags,
    ]);
    Ok(ReleasePlan {
        version: version.to_string(),
        steps: actions
            .into_iter()
            .map(|action| ReleaseStep {
                action,
                label: action.label().to_string(),
                display: action.display(version),
            })
            .collect(),
    })
}

/// Execute only one action from the release allowlist. There is deliberately
/// no shell string or user-controlled executable/argument surface.
pub fn execute_action(
    project_path: &str,
    version: &str,
    action_id: &str,
) -> Result<ReleaseExecution> {
    let path = validate_release_input(project_path, version)?;
    let action = ReleaseAction::parse(action_id).ok_or_else(|| {
        Error::InvalidInput(format!("release_execute: unsupported action {action_id:?}"))
    })?;
    match action {
        ReleaseAction::UpdateVersion => {
            prepare_release_at(&path, version, |file, content| {
                crate::module_manager::atomic_write(file, content)
            })?;
            Ok(successful_execution(action, false))
        }
        ReleaseAction::GitCommit => execute_commit(&path, version),
        ReleaseAction::GitTag => execute_tag(&path, version),
        _ => {
            let (program, args) = action
                .argv(version)
                .ok_or_else(|| Error::Internal("missing release action argv".into()))?;
            execute_argv(&path, action, program, &args)
        }
    }
}

fn execute_tag(path: &Path, version: &str) -> Result<ReleaseExecution> {
    let tag = tag_name(version);
    if let Some(target) = git_ref(path, &["rev-list", "-n", "1", &tag])? {
        let head = git_ref(path, &["rev-parse", "HEAD"])?
            .ok_or_else(|| Error::Internal("git HEAD is unavailable".into()))?;
        if target == head {
            return Ok(successful_execution(ReleaseAction::GitTag, true));
        }
        return Err(Error::Conflict(format!(
            "release tag conflict: {tag} already points to {target}, not HEAD {head}"
        )));
    }
    let (program, args) = ReleaseAction::GitTag
        .argv(version)
        .expect("GitTag has fixed argv");
    execute_argv(path, ReleaseAction::GitTag, program, &args)
}

fn execute_commit(path: &Path, version: &str) -> Result<ReleaseExecution> {
    if release_commit_is_current(path, version)? {
        return Ok(successful_execution(ReleaseAction::GitCommit, true));
    }
    let version_files = release_version_files(path)?;
    let staged_before = git_ref(path, &["diff", "--cached", "--name-only"])?.unwrap_or_default();
    if let Some(file) = staged_before
        .lines()
        .find(|file| !version_files.iter().any(|allowed| file == allowed))
    {
        return Err(Error::Conflict(format!(
            "release commit blocked by pre-existing staged file: {file}"
        )));
    }
    let mut add_args = vec!["add".to_string(), "--".to_string()];
    add_args.extend(version_files.iter().map(|file| (*file).to_string()));
    let staged = execute_argv(path, ReleaseAction::GitCommit, "git", &add_args)?;
    if !staged.success {
        return Ok(staged);
    }
    execute_argv(
        path,
        ReleaseAction::GitCommit,
        "git",
        &[
            "commit".to_string(),
            "-m".to_string(),
            format!("release: v{version}"),
        ],
    )
}

fn release_version_files(path: &Path) -> Result<Vec<&'static str>> {
    let mut files = Vec::new();
    if path.join("package.json").is_file() {
        files.push("package.json");
    }
    if path.join("Cargo.toml").is_file() {
        files.push("Cargo.toml");
    }
    if files.is_empty() {
        return Err(Error::InvalidInput(
            "release project has no package.json or Cargo.toml".into(),
        ));
    }
    Ok(files)
}

fn execute_argv(
    path: &Path,
    action: ReleaseAction,
    program: &str,
    args: &[String],
) -> Result<ReleaseExecution> {
    let output = Command::new(program)
        .args(args)
        .current_dir(path)
        .output()
        .map_err(|error| {
            Error::Internal(format!("release {} failed to start: {error}", action.id()))
        })?;
    Ok(ReleaseExecution {
        action,
        exit_code: output.status.code(),
        stdout: bounded_diagnostic(&output.stdout),
        stderr: bounded_diagnostic(&output.stderr),
        success: output.status.success(),
        already_complete: false,
    })
}

fn successful_execution(action: ReleaseAction, already_complete: bool) -> ReleaseExecution {
    ReleaseExecution {
        action,
        exit_code: Some(0),
        stdout: String::new(),
        stderr: String::new(),
        success: true,
        already_complete,
    }
}

fn validate_release_input(project_path: &str, version: &str) -> Result<PathBuf> {
    validate_semver(version)?;
    validate_project_dir(project_path)
}

fn validate_project_dir(project_path: &str) -> Result<PathBuf> {
    if project_path.trim().is_empty() {
        return Err(Error::InvalidInput("project path is required".into()));
    }
    let path = Path::new(project_path).canonicalize().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            Error::NotFound(project_path.to_string())
        } else {
            Error::Io(error)
        }
    })?;
    if !path.is_dir() {
        return Err(Error::InvalidInput(
            "project path must be a directory".into(),
        ));
    }
    Ok(path)
}

fn validate_semver(version: &str) -> Result<()> {
    let valid_identifier = |identifier: &str, numeric: bool| {
        !identifier.is_empty()
            && identifier
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            && (!numeric || identifier == "0" || !identifier.starts_with('0'))
    };
    let (core_and_pre, build) = match version.split_once('+') {
        Some((left, right)) => (left, Some(right)),
        None => (version, None),
    };
    if build.is_some_and(|value| value.split('.').any(|part| !valid_identifier(part, false))) {
        return Err(Error::InvalidInput(format!(
            "invalid semantic version: {version}"
        )));
    }
    let (core, prerelease) = match core_and_pre.split_once('-') {
        Some((left, right)) => (left, Some(right)),
        None => (core_and_pre, None),
    };
    let core_valid = core.split('.').count() == 3
        && core.split('.').all(|part| {
            !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && (part == "0" || !part.starts_with('0'))
        });
    let prerelease_valid = prerelease.is_none_or(|value| {
        value
            .split('.')
            .all(|part| valid_identifier(part, part.bytes().all(|byte| byte.is_ascii_digit())))
    });
    if core_valid && prerelease_valid {
        Ok(())
    } else {
        Err(Error::InvalidInput(format!(
            "invalid semantic version: {version}"
        )))
    }
}

fn update_package_version(content: &str, version: &str) -> Result<String> {
    let mut package: serde_json::Value = serde_json::from_str(content)?;
    let object = package
        .as_object_mut()
        .ok_or_else(|| Error::InvalidInput("package.json must be an object".into()))?;
    if object
        .get("version")
        .and_then(serde_json::Value::as_str)
        .is_none()
    {
        return Err(Error::InvalidInput(
            "package.json version is missing or invalid".into(),
        ));
    }
    object.insert(
        "version".into(),
        serde_json::Value::String(version.to_string()),
    );
    serde_json::to_string_pretty(&package).map_err(Error::Json)
}

fn update_cargo_version(content: &str, version: &str) -> Result<String> {
    let mut in_package = false;
    let mut changed = false;
    let mut lines = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_package = trimmed == "[package]";
        }
        if in_package && !changed && trimmed.starts_with("version") {
            let Some((key, _)) = line.split_once('=') else {
                return Err(Error::InvalidInput(
                    "Cargo.toml package version is invalid".into(),
                ));
            };
            if key.trim() == "version" {
                lines.push(format!("{key}= \"{version}\""));
                changed = true;
                continue;
            }
        }
        lines.push(line.to_string());
    }
    if !changed {
        return Err(Error::InvalidInput(
            "Cargo.toml package version is missing".into(),
        ));
    }
    let mut updated = lines.join("\n");
    if content.ends_with('\n') {
        updated.push('\n');
    }
    Ok(updated)
}

fn read_package_version(dir: &Path) -> Result<String> {
    let content = std::fs::read_to_string(dir.join("package.json")).map_err(Error::Io)?;
    let json: serde_json::Value = serde_json::from_str(&content)?;
    json.get("version")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| Error::InvalidInput("package.json version is missing or invalid".into()))
}

fn read_cargo_version(dir: &Path) -> Result<String> {
    let content = std::fs::read_to_string(dir.join("Cargo.toml")).map_err(Error::Io)?;
    let mut in_package = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_package = trimmed == "[package]";
        }
        if in_package {
            if let Some((key, value)) = trimmed.split_once('=') {
                if key.trim() == "version" {
                    return Ok(value.trim().trim_matches('"').to_string());
                }
            }
        }
    }
    Err(Error::InvalidInput(
        "Cargo.toml package version is missing".into(),
    ))
}

fn release_commit_is_current(path: &Path, version: &str) -> Result<bool> {
    let Some(subject) = git_ref(path, &["log", "-1", "--format=%s"])? else {
        return Ok(false);
    };
    Ok(subject == format!("release: v{version}"))
}

fn git_ref(path: &Path, args: &[&str]) -> Result<Option<String>> {
    let output = Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .map_err(|error| {
            Error::Internal(format!("git {} failed to start: {error}", args.join(" ")))
        })?;
    if !output.status.success() {
        return Ok(None);
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok((!value.is_empty()).then_some(value))
}

fn git_output(path: &Path, args: &[&str]) -> Option<std::process::Output> {
    Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .ok()
        .filter(|output| output.status.success())
}

fn tag_name(version: &str) -> String {
    format!("v{version}")
}

fn bounded_diagnostic(output: &[u8]) -> String {
    let sanitized = sanitize(&String::from_utf8_lossy(output));
    let start = sanitized
        .char_indices()
        .rev()
        .nth(MAX_DIAGNOSTIC_BYTES)
        .map(|(index, _)| index)
        .unwrap_or(0);
    sanitized[start..].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::TempDir;

    fn project(files: &[(&str, &str)]) -> TempDir {
        let dir = TempDir::new().unwrap();
        for (name, content) in files {
            std::fs::write(dir.path().join(name), content).unwrap();
        }
        dir
    }

    fn git(project: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(project)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?}: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn every_plan_action_round_trips_through_the_allowlist() {
        let project = project(&[
            ("package.json", r#"{"name":"demo","version":"1.0.0"}"#),
            (
                "Cargo.toml",
                "[package]\nname = \"demo\"\nversion = \"1.0.0\"\n",
            ),
        ]);
        let plan = get_sequence(project.path().to_str().unwrap(), "1.2.3").unwrap();
        assert_eq!(plan.steps.len(), 8);
        for step in plan.steps {
            assert_eq!(ReleaseAction::parse(step.action.id()), Some(step.action));
            if let Some((program, args)) = step.action.argv("1.2.3") {
                assert!(!program.contains("sh"));
                assert!(!args.iter().any(|arg| arg.contains("&&")));
            }
        }
    }

    #[test]
    fn unknown_and_free_form_actions_fail_closed() {
        let project = project(&[("package.json", r#"{"name":"demo","version":"1.0.0"}"#)]);
        for action in [
            "git push && git push --tags",
            "git push",
            "sh -c whoami",
            "npm install --ignore-scripts",
        ] {
            assert!(execute_action(project.path().to_str().unwrap(), "1.2.3", action).is_err());
        }
    }

    #[test]
    fn tag_uses_requested_version_and_existing_conflict_is_explicit() {
        let project = project(&[
            ("package.json", r#"{"name":"demo","version":"1.0.0"}"#),
            ("README.md", "one\n"),
        ]);
        git(project.path(), &["init"]);
        git(project.path(), &["config", "user.name", "Natives Test"]);
        git(
            project.path(),
            &["config", "user.email", "natives@example.com"],
        );
        git(project.path(), &["add", "."]);
        git(project.path(), &["commit", "-m", "initial"]);
        assert!(
            execute_action(project.path().to_str().unwrap(), "1.2.3", "update-version")
                .unwrap()
                .success
        );
        let committed =
            execute_action(project.path().to_str().unwrap(), "1.2.3", "git-commit").unwrap();
        assert!(committed.success);
        assert_eq!(
            git_ref(project.path(), &["log", "-1", "--format=%s"])
                .unwrap()
                .as_deref(),
            Some("release: v1.2.3")
        );
        let tagged = execute_action(project.path().to_str().unwrap(), "1.2.3", "git-tag").unwrap();
        assert!(tagged.success);
        assert_eq!(
            git_ref(project.path(), &["tag", "--list", "v1.2.3"])
                .unwrap()
                .as_deref(),
            Some("v1.2.3")
        );
        std::fs::write(project.path().join("README.md"), "two\n").unwrap();
        git(project.path(), &["add", "."]);
        git(project.path(), &["commit", "-m", "next"]);
        let conflict =
            execute_action(project.path().to_str().unwrap(), "1.2.3", "git-tag").unwrap_err();
        assert!(matches!(conflict, Error::Conflict(_)));
    }

    #[test]
    fn preparation_error_rolls_back_earlier_file_and_returns_err() {
        let project = project(&[
            ("package.json", r#"{"name":"demo","version":"1.0.0"}"#),
            (
                "Cargo.toml",
                "[package]\nname = \"demo\"\nversion = \"1.0.0\"\n",
            ),
        ]);
        let writes = AtomicUsize::new(0);
        let result = prepare_release_at(project.path(), "1.2.3", |file, content| {
            if writes.fetch_add(1, Ordering::SeqCst) == 1 {
                return Err(Error::Io(std::io::Error::other(
                    "simulated Cargo write failure",
                )));
            }
            crate::module_manager::atomic_write(file, content)
        });
        assert!(result.is_err());
        assert!(std::fs::read_to_string(project.path().join("package.json"))
            .unwrap()
            .contains("1.0.0"));
        assert!(std::fs::read_to_string(project.path().join("Cargo.toml"))
            .unwrap()
            .contains("1.0.0"));
    }

    #[test]
    fn invalid_version_or_non_directory_is_rejected_before_mutation() {
        let project = project(&[("package.json", r#"{"name":"demo","version":"1.0.0"}"#)]);
        assert!(prepare_release(project.path().to_str().unwrap(), "1.02.3").is_err());
        assert!(get_sequence(
            project.path().join("package.json").to_str().unwrap(),
            "1.2.3"
        )
        .is_err());
        assert!(std::fs::read_to_string(project.path().join("package.json"))
            .unwrap()
            .contains("1.0.0"));
    }
}
