//! Local project directory scanner and rule-based LaunchPlan builder.
//!
//! Limits: depth ≤ 3, ≤ 2000 entries, config files ≤ 1 MiB, summary ≤ 256 KiB.
//! Never reads .env*, secrets, or files outside the project root.

use super::path::{canonical_project_root, resolve_under};
use super::plan::{script_body_is_safe, validate_launch_plan};
use super::store;
use crate::creative_app::model::*;
use crate::{Error, Result};
use rusqlite::Connection;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const MAX_DEPTH: usize = 3;
const MAX_ENTRIES: usize = 2000;
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
const MAX_SUMMARY_BYTES: usize = 256 * 1024;

const IGNORE_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "dist",
    "build",
    "coverage",
    ".cache",
    ".next",
    ".nuxt",
    ".output",
    ".turbo",
    ".vercel",
    ".idea",
    ".vscode",
    "__pycache__",
    "target",
];

const SECRET_NAME_MARKERS: &[&str] = &[
    ".env",
    ".npmrc",
    ".netrc",
    "id_rsa",
    "id_ed25519",
    ".pem",
    ".key",
    "credentials",
    "secret",
    "token",
    "cert",
];

pub fn inspect_local_project(
    conn: &Connection,
    req: &InspectLocalRequest,
) -> Result<LocalProjectScanResult> {
    let root = canonical_project_root(&req.project_root)?;
    let root_s = root.to_string_lossy().to_string();

    let existing_id = store::get_app_by_root(conn, &root_s)?.map(|r| r.id);

    let mut entries = 0usize;
    let mut tree_sample: Vec<String> = Vec::new();
    let mut has_index_html = false;
    let mut has_vue_file = false;
    let mut vite_config = false;
    let mut vue_config = false;
    let mut package_json_path: Option<PathBuf> = None;
    let mut lock_npm = false;
    let mut lock_pnpm = false;
    let mut lock_yarn = false;
    let mut summary_budget = MAX_SUMMARY_BYTES;
    let mut compose_files: Vec<PathBuf> = Vec::new();
    let mut has_compose = false;
    let mut has_dockerfile = false;
    let mut has_python = false;
    let mut has_makefile = false;

    walk(
        &root,
        &root,
        0,
        &mut entries,
        &mut tree_sample,
        &mut summary_budget,
        &mut |path, rel, is_dir| {
            if is_dir {
                return;
            }
            let name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if name == "index.html" && (rel == "index.html" || !rel.contains('/')) {
                has_index_html = true;
            }
            if name == "index.html" {
                has_index_html = true;
            }
            if name.ends_with(".vue") {
                has_vue_file = true;
            }
            if name.starts_with("vite.config.") {
                vite_config = true;
            }
            if name.starts_with("vue.config.") {
                vue_config = true;
            }
            if matches!(
                name.as_str(),
                "docker-compose.yml" | "docker-compose.yaml" | "compose.yml" | "compose.yaml"
            ) {
                has_compose = true;
                compose_files.push(path.to_path_buf());
            }
            if name == "dockerfile" {
                has_dockerfile = true;
            }
            if name == "pyproject.toml"
                || (name.starts_with("requirements") && name.ends_with(".txt"))
                || (name.starts_with("requirements") && name.ends_with(".pip"))
            {
                has_python = true;
            }
            if name == "makefile" {
                has_makefile = true;
            }
            if name == "package.json" && package_json_path.is_none() {
                package_json_path = Some(path.to_path_buf());
            }
            if name == "package-lock.json" {
                lock_npm = true;
            }
            if name == "pnpm-lock.yaml" {
                lock_pnpm = true;
            }
            if name == "yarn.lock" {
                lock_yarn = true;
            }
        },
    )?;

    // Prefer root package.json explicitly
    let root_pkg = root.join("package.json");
    if root_pkg.is_file() {
        package_json_path = Some(root_pkg);
    }

    let tool_versions = detect_tool_versions();
    let mut risks = Vec::new();
    let mut blockers = Vec::new();
    let mut scripts: Vec<String> = Vec::new();
    let mut preferred_script: Option<String> = None;
    let mut package_manager: Option<PackageManager> = None;
    let mut package_manager_choices: Vec<PackageManager> = Vec::new();
    let mut has_node_modules = root.join("node_modules").is_dir();
    let mut deps_from_pkg = false;
    let mut package_manager_field: Option<String> = None;
    let mut script_bodies: BTreeMap<String, String> = BTreeMap::new();

    if let Some(ref pkg_path) = package_json_path {
        match read_json_limited(pkg_path) {
            Ok(v) => {
                deps_from_pkg = true;
                if let Some(pm) = v.get("packageManager").and_then(|x| x.as_str()) {
                    package_manager_field = Some(pm.to_string());
                }
                if let Some(obj) = v.get("scripts").and_then(|x| x.as_object()) {
                    for (k, val) in obj {
                        scripts.push(k.clone());
                        if let Some(body) = val.as_str() {
                            script_bodies.insert(k.clone(), body.to_string());
                        }
                    }
                    scripts.sort();
                    for cand in ["dev", "serve", "start"] {
                        if obj.contains_key(cand) {
                            preferred_script = Some(cand.to_string());
                            break;
                        }
                    }
                }
                let deps = merge_dep_names(&v);
                if deps.iter().any(|d| d == "vue" || d.starts_with("@vue/")) {
                    has_vue_file = true; // treat as vue project signal
                }
                if deps
                    .iter()
                    .any(|d| d == "vite" || d.starts_with("@vitejs/"))
                {
                    vite_config = true;
                }
            }
            Err(e) => {
                risks.push(format!("package.json unreadable: {e}"));
            }
        }
    }

    // package manager resolution
    if let Some(ref pmf) = package_manager_field {
        let lower = pmf.to_ascii_lowercase();
        if lower.starts_with("pnpm") {
            package_manager = Some(PackageManager::Pnpm);
        } else if lower.starts_with("yarn") {
            package_manager = Some(PackageManager::Yarn);
        } else if lower.starts_with("npm") {
            package_manager = Some(PackageManager::Npm);
        }
    }
    let lock_count = [lock_npm, lock_pnpm, lock_yarn]
        .iter()
        .filter(|x| **x)
        .count();
    if package_manager.is_none() {
        if lock_count > 1 {
            if lock_npm {
                package_manager_choices.push(PackageManager::Npm);
            }
            if lock_pnpm {
                package_manager_choices.push(PackageManager::Pnpm);
            }
            if lock_yarn {
                package_manager_choices.push(PackageManager::Yarn);
            }
            risks.push("multiple lockfiles detected; choose a package manager".into());
        } else if lock_pnpm {
            package_manager = Some(PackageManager::Pnpm);
        } else if lock_yarn {
            package_manager = Some(PackageManager::Yarn);
        } else if lock_npm {
            package_manager = Some(PackageManager::Npm);
        } else if package_json_path.is_some() {
            package_manager = Some(PackageManager::Npm);
            risks.push("no lockfile; default suggestion is npm (confirm required)".into());
        }
    }

    let project_kind = classify_kind(
        package_json_path.is_some(),
        has_index_html,
        vite_config,
        vue_config,
        has_vue_file,
    );

    if package_json_path.is_some() && !has_node_modules {
        // also check cwd relative node_modules next to package.json
        if let Some(ref p) = package_json_path {
            if let Some(parent) = p.parent() {
                has_node_modules = parent.join("node_modules").is_dir();
            }
        }
    }

    let dependencies_missing = package_json_path.is_some() && !has_node_modules;
    if dependencies_missing {
        risks.push("node_modules missing; install dependencies before start".into());
    }

    // tool availability
    if package_json_path.is_some() {
        if tool_versions.node.is_none() {
            blockers.push("node is not available on PATH".into());
        }
        if let Some(pm) = package_manager {
            let missing = match pm {
                PackageManager::Npm => tool_versions.npm.is_none(),
                PackageManager::Pnpm => tool_versions.pnpm.is_none(),
                PackageManager::Yarn => tool_versions.yarn.is_none(),
            };
            if missing {
                blockers.push(format!("{} is not available on PATH", pm.as_str()));
            }
        }
    }

    let rule_plan = build_rule_plan(
        &root,
        project_kind,
        package_manager,
        preferred_script.as_deref(),
        &script_bodies,
        has_index_html,
        &mut risks,
    );
    // A Compose-only project gets a Docker Compose plan (batch 5) unless its
    // default command is trade-blocked.
    let rule_plan = match rule_plan {
        Some(p) => Some(p),
        None if has_compose => build_compose_plan(&root, &compose_files),
        None => None,
    };

    // existing registration note
    if existing_id.is_some() {
        risks.push("this path is already registered as a local creative app".into());
    }

    // Non-web manifest evidence (batch 4). These runtimes are not yet available
    // as drivers, so detection is honest evidence + risks, never a fake plan.
    let mut extra_manifests = Vec::new();
    if has_compose {
        extra_manifests.push("docker-compose".into());
        risks.push(
            "Compose project detected; the Docker Compose runner is not yet available".into(),
        );
        for cf in &compose_files {
            if let Some(msg) = compose_command_risk(cf) {
                blockers.push(msg);
            }
        }
    }
    if has_dockerfile {
        extra_manifests.push("dockerfile".into());
        risks.push("Dockerfile detected; local container runtime is pending".into());
    }
    if has_python {
        extra_manifests.push("python".into());
        risks.push("Python project detected; a Python runner is not yet available".into());
    }
    if has_makefile {
        extra_manifests.push("makefile".into());
        risks.push("Makefile detected; it is never auto-executable".into());
    }

    let _ = deps_from_pkg; // silence

    Ok(LocalProjectScanResult {
        project_root: root_s,
        project_kind,
        package_manager,
        package_manager_choices,
        scripts,
        preferred_script,
        has_node_modules,
        dependencies_missing,
        tool_versions,
        risks,
        blockers,
        rule_plan,
        extra_manifests,
        tree_sample,
        existing_id,
    })
}

/// Prove the project config has `dry_run: true` (batch 8). Reads only the
/// dry_run projection from common config locations — never logs or returns
/// config content or secrets.
pub fn config_proves_dry_run(root: &Path) -> bool {
    for cand in [
        root.join("user_data/config.json"),
        root.join("config.json"),
        root.join("config/config.json"),
    ] {
        if let Ok(meta) = fs::metadata(&cand) {
            if meta.len() > MAX_CONFIG_BYTES {
                continue;
            }
        }
        let Ok(data) = fs::read_to_string(&cand) else {
            continue;
        };
        if let Ok(v) = serde_json::from_str::<Value>(&data) {
            if v.get("dry_run").and_then(|x| x.as_bool()) == Some(true) {
                return true;
            }
        }
    }
    false
}

/// Inspect a Compose file's `command:` lines and block any that can place real
/// trades (P0 dangerous-command gate). Only the inline form is parsed here; the
/// Compose runtime batch parses full service topology.
pub fn compose_command_risk(path: &Path) -> Option<String> {
    let meta = fs::metadata(path).ok()?;
    if meta.len() > MAX_CONFIG_BYTES {
        return None;
    }
    let data = fs::read_to_string(path).ok()?;
    let mut lines = data.lines().peekable();
    while let Some(line) = lines.next() {
        let t = line.trim();
        let Some(rest) = t.strip_prefix("command:") else {
            continue;
        };
        let rest = rest.trim();
        if rest.is_empty() {
            // List form:
            //   command:
            //     - trade
            //     - --config
            let mut tokens: Vec<String> = Vec::new();
            while let Some(next) = lines.peek() {
                let n = next.trim_start();
                if n.starts_with('-') {
                    tokens.push(n.trim_start_matches('-').trim().to_string());
                    lines.next();
                } else {
                    break;
                }
            }
            if !tokens.is_empty()
                && super::risk::classify_command("", &tokens) == super::risk::CommandRisk::Block
            {
                return Some(format!(
                    "compose command may place real trades: {}",
                    tokens.join(" ")
                ));
            }
            continue;
        }
        let tokens: Vec<String> = rest.split_whitespace().map(str::to_string).collect();
        if !tokens.is_empty()
            && super::risk::classify_command("", &tokens) == super::risk::CommandRisk::Block
        {
            return Some(format!("compose command may place real trades: {rest}"));
        }
    }
    None
}

/// Build a Docker Compose rule plan for a Compose-only project (batch 5).
/// Returns None when the compose default command can place real trades — the
/// scan already recorded that as a blocker, so no plan is offered.
fn build_compose_plan(root: &Path, compose_files: &[PathBuf]) -> Option<LaunchPlan> {
    let file = compose_files
        .iter()
        .find(|f| f.parent() == Some(root))
        .or_else(|| compose_files.first())?;
    if compose_command_risk(file).is_some() {
        return None;
    }
    let rel = file
        .strip_prefix(root)
        .ok()?
        .to_string_lossy()
        .replace('\\', "/");
    let seed = root
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| {
            s.chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
                .take(32)
                .collect::<String>()
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "compose".into());
    let plan = LaunchPlan {
        schema_version: 1,
        source: LaunchPlanSource::Rule,
        project_kind: LocalProjectKind::Unknown,
        runtime: LocalLaunchRuntime::DockerCompose,
        program: LaunchProgram::Internal,
        cwd_relative: ".".into(),
        script: None,
        entry_file: None,
        script_runner: None,
        args: vec![],
        environment_keys: vec![],
        port: LaunchPort {
            mode: LaunchPortMode::Auto,
            value: None,
        },
        open_path: "/".into(),
        health_path: "/".into(),
        startup_timeout_ms: 120_000,
        auto_open: true,
        confidence: Some(0.7),
        reason: "docker compose detected".into(),
        compose: Some(ComposePlanDetail {
            compose_file: rel,
            project_seed: seed,
            service: None,
            command: vec![],
            health_path: "/".into(),
            host_port: None,
        }),
        trade_approval: None,
        process_profile: None,
    };
    validate_launch_plan(root, plan).ok()
}

fn classify_kind(
    has_pkg: bool,
    has_index: bool,
    vite: bool,
    vue_cfg: bool,
    has_vue: bool,
) -> LocalProjectKind {
    if !has_pkg {
        if has_index {
            return LocalProjectKind::Html;
        }
        return LocalProjectKind::Unknown;
    }
    if vite && has_vue {
        return LocalProjectKind::VueVite;
    }
    if vite {
        return LocalProjectKind::Vite;
    }
    if vue_cfg || has_vue {
        return LocalProjectKind::Vue;
    }
    if has_index {
        // package.json but looks static-ish
        return LocalProjectKind::ViteOther;
    }
    LocalProjectKind::Unknown
}

fn build_rule_plan(
    root: &Path,
    kind: LocalProjectKind,
    pm: Option<PackageManager>,
    preferred_script: Option<&str>,
    script_bodies: &BTreeMap<String, String>,
    has_index_html: bool,
    risks: &mut Vec<String>,
) -> Option<LaunchPlan> {
    match kind {
        LocalProjectKind::Html => {
            if !has_index_html {
                return None;
            }
            let plan = LaunchPlan {
                schema_version: 1,
                source: LaunchPlanSource::Rule,
                project_kind: kind,
                runtime: LocalLaunchRuntime::StaticHttp,
                program: LaunchProgram::Internal,
                cwd_relative: ".".into(),
                script: None,
                entry_file: Some("index.html".into()),
                script_runner: None,
                args: vec![],
                environment_keys: vec![],
                port: LaunchPort {
                    mode: LaunchPortMode::Auto,
                    value: None,
                },
                open_path: "/".into(),
                health_path: "/".into(),
                startup_timeout_ms: 60_000,
                auto_open: true,
                confidence: Some(0.95),
                reason: "root index.html detected".into(),
                compose: None,
                trade_approval: None,
                process_profile: None,
            };
            validate_launch_plan(root, plan).ok()
        }
        LocalProjectKind::Vite
        | LocalProjectKind::VueVite
        | LocalProjectKind::Vue
        | LocalProjectKind::ViteOther => {
            let pm = pm?;
            let script = preferred_script?;
            if let Some(body) = script_bodies.get(script) {
                if !script_body_is_safe(body) {
                    risks.push(format!(
                        "script '{script}' is not auto-executable (shell constructs); configure manually"
                    ));
                    return None;
                }
            }
            let program = match pm {
                PackageManager::Npm => LaunchProgram::Npm,
                PackageManager::Pnpm => LaunchProgram::Pnpm,
                PackageManager::Yarn => LaunchProgram::Yarn,
            };
            let plan = LaunchPlan {
                schema_version: 1,
                source: LaunchPlanSource::Rule,
                project_kind: kind,
                runtime: LocalLaunchRuntime::NodeDevServer,
                program,
                cwd_relative: ".".into(),
                script: Some(script.to_string()),
                entry_file: None,
                script_runner: None,
                args: vec![],
                environment_keys: vec![],
                port: LaunchPort {
                    mode: LaunchPortMode::Auto,
                    value: None,
                },
                open_path: "/".into(),
                health_path: "/".into(),
                startup_timeout_ms: 60_000,
                auto_open: true,
                confidence: Some(0.8),
                reason: format!("rule: {} run {script}", pm.as_str()),
                compose: None,
                trade_approval: None,
                process_profile: None,
            };
            match validate_launch_plan(root, plan) {
                Ok(p) => Some(p),
                Err(e) => {
                    risks.push(format!("rule plan invalid: {e}"));
                    None
                }
            }
        }
        LocalProjectKind::Unknown => None,
    }
}

fn walk(
    root: &Path,
    dir: &Path,
    depth: usize,
    entries: &mut usize,
    tree_sample: &mut Vec<String>,
    summary_budget: &mut usize,
    on_file: &mut dyn FnMut(&Path, &str, bool),
) -> Result<()> {
    if depth > MAX_DEPTH || *entries >= MAX_ENTRIES {
        return Ok(());
    }
    let rd = match fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return Ok(()),
    };
    let mut items: Vec<_> = rd.filter_map(|e| e.ok()).collect();
    items.sort_by_key(|e| e.file_name());
    for ent in items {
        if *entries >= MAX_ENTRIES {
            break;
        }
        *entries += 1;
        let path = ent.path();
        // symlink policy: resolve and skip if outside root
        let meta = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.file_type().is_symlink() {
            match path.canonicalize() {
                Ok(real) => {
                    let root_c = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
                    if !real.starts_with(&root_c) {
                        continue;
                    }
                }
                Err(_) => continue,
            }
        }
        let name = ent.file_name();
        let name_s = name.to_string_lossy();
        if name_s.starts_with('.') && name_s != ".well-known" {
            // allow scanning non-secret dotfiles selectively; skip common secrets
            if is_secret_name(&name_s) {
                continue;
            }
        }
        let rel = path
            .strip_prefix(root)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        if is_secret_name(&name_s) || is_secret_name(&rel) {
            continue;
        }
        let is_dir = meta.is_dir() || (meta.file_type().is_symlink() && path.is_dir());
        if is_dir {
            if IGNORE_DIRS.iter().any(|d| name_s.eq_ignore_ascii_case(d)) {
                continue;
            }
            if *summary_budget > rel.len() + 2 {
                *summary_budget -= rel.len() + 2;
                if tree_sample.len() < 200 {
                    tree_sample.push(format!("{rel}/"));
                }
            }
            on_file(&path, &rel, true);
            walk(
                root,
                &path,
                depth + 1,
                entries,
                tree_sample,
                summary_budget,
                on_file,
            )?;
        } else {
            if *summary_budget > rel.len() + 1 {
                *summary_budget -= rel.len() + 1;
                if tree_sample.len() < 200 {
                    tree_sample.push(rel.clone());
                }
            }
            on_file(&path, &rel, false);
        }
    }
    Ok(())
}

fn is_secret_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if lower == ".env" || lower.starts_with(".env.") || lower.ends_with(".env") {
        return true;
    }
    SECRET_NAME_MARKERS.iter().any(|m| lower.contains(m))
}

fn read_json_limited(path: &Path) -> Result<Value> {
    let meta = fs::metadata(path).map_err(Error::Io)?;
    if meta.len() > MAX_CONFIG_BYTES {
        return Err(Error::InvalidInput(format!(
            "config file too large: {}",
            path.display()
        )));
    }
    let data = fs::read_to_string(path).map_err(Error::Io)?;
    serde_json::from_str(&data).map_err(|e| Error::InvalidInput(format!("invalid JSON: {e}")))
}

fn merge_dep_names(v: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for key in ["dependencies", "devDependencies"] {
        if let Some(obj) = v.get(key).and_then(|x| x.as_object()) {
            for k in obj.keys() {
                out.push(k.clone());
            }
        }
    }
    out
}

fn detect_tool_versions() -> LocalToolVersions {
    LocalToolVersions {
        node: version_of("node"),
        npm: version_of("npm"),
        pnpm: version_of("pnpm"),
        yarn: version_of("yarn"),
    }
}

fn version_of(bin: &str) -> Option<String> {
    let output = Command::new(bin).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&output.stdout)
        .trim()
        .trim_start_matches('v')
        .to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Used by create path to re-validate user-supplied roots quickly.
#[allow(dead_code)]
pub fn ensure_package_json(root: &Path) -> Result<PathBuf> {
    resolve_under(root, "package.json")
}

#[cfg(test)]
#[path = "scan_tests.rs"]
mod scan_tests;
