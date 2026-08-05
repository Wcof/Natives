//! LaunchPlan validation and fingerprinting.
//!
//! All rule / user / AI plans must pass through `validate_launch_plan` before persistence
//! or execution. AI output is never executed directly.

use super::path::resolve_under;
use crate::creative_app::model::*;
use crate::{Error, Result};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;

const MIN_TIMEOUT_MS: u32 = 5_000;
const MAX_TIMEOUT_MS: u32 = 300_000;
const DEFAULT_TIMEOUT_MS: u32 = 60_000;

/// Shell metacharacters / constructs that must never appear in scripts or args.
const FORBIDDEN_SCRIPT_MARKERS: &[&str] = &[
    "&&", "||", ";", "|", ">", "<", "`", "$(", "${", "\n", "\r", "\0", "&",
];

/// Validate and normalize a LaunchPlan against a project root.
pub fn validate_launch_plan(root: &Path, mut plan: LaunchPlan) -> Result<LaunchPlan> {
    if plan.schema_version != 1 {
        return Err(Error::InvalidInput(format!(
            "unsupported LaunchPlan schemaVersion: {}",
            plan.schema_version
        )));
    }

    // cwd
    let cwd_rel = normalize_rel(&plan.cwd_relative)?;
    let cwd = resolve_under(root, &cwd_rel)?;
    if !cwd.is_dir() {
        return Err(Error::InvalidInput(format!(
            "cwdRelative is not a directory: {cwd_rel}"
        )));
    }
    plan.cwd_relative = cwd_rel;

    // timeout
    if plan.startup_timeout_ms == 0 {
        plan.startup_timeout_ms = DEFAULT_TIMEOUT_MS;
    }
    if plan.startup_timeout_ms < MIN_TIMEOUT_MS || plan.startup_timeout_ms > MAX_TIMEOUT_MS {
        return Err(Error::InvalidInput(format!(
            "startupTimeoutMs must be between {MIN_TIMEOUT_MS} and {MAX_TIMEOUT_MS}"
        )));
    }

    // open / health paths (URL path, not filesystem)
    plan.open_path = normalize_url_path(&plan.open_path)?;
    plan.health_path = normalize_url_path(&plan.health_path)?;

    // port
    match plan.port.mode {
        LaunchPortMode::Auto => {
            plan.port.value = None;
        }
        LaunchPortMode::Fixed => {
            let p = plan
                .port
                .value
                .ok_or_else(|| Error::InvalidInput("fixed port requires value".into()))?;
            if p == 0 {
                return Err(Error::InvalidInput("invalid fixed port".into()));
            }
        }
    }

    // environment keys — names only
    for k in &plan.environment_keys {
        validate_env_key(k)?;
    }

    // Managed-process profile (Python/Binary, batch 10 CR-1002): the command is
    // carried by the profile and validated by process_driver, so the legacy
    // program map (package.json / node entry / static index.html) does not apply.
    // cwd / open / health / port were already validated above.
    if plan.process_profile.is_some() {
        return Ok(plan);
    }

    match plan.runtime {
        LocalLaunchRuntime::StaticHttp => {
            if plan.program != LaunchProgram::Internal {
                return Err(Error::InvalidInput(
                    "static_http runtime requires program=internal".into(),
                ));
            }
            if plan.script.is_some() {
                return Err(Error::InvalidInput(
                    "static_http must not set script".into(),
                ));
            }
            let entry = plan.entry_file.as_deref().unwrap_or("index.html").trim();
            let entry_n = normalize_rel(entry)?;
            let entry_path = resolve_under(root, &join_rel(&plan.cwd_relative, &entry_n))?;
            if !entry_path.is_file() {
                return Err(Error::InvalidInput(format!(
                    "entryFile not found: {entry_n}"
                )));
            }
            plan.entry_file = Some(entry_n);
            plan.script_runner = None;
            plan.args.clear();
        }
        LocalLaunchRuntime::NodeDevServer => {
            if matches!(plan.program, LaunchProgram::Internal) {
                return Err(Error::InvalidInput(
                    "node_dev_server requires npm/pnpm/yarn/node program".into(),
                ));
            }
            match plan.program {
                LaunchProgram::Npm | LaunchProgram::Pnpm | LaunchProgram::Yarn => {
                    let script = plan
                        .script
                        .as_deref()
                        .ok_or_else(|| {
                            Error::InvalidInput("package manager program requires script".into())
                        })?
                        .trim();
                    validate_script_name(script)?;
                    let pkg = resolve_under(root, &join_rel(&plan.cwd_relative, "package.json"))?;
                    if !pkg.is_file() {
                        return Err(Error::InvalidInput(
                            "package.json missing for node_dev_server".into(),
                        ));
                    }
                    // Read the REAL script body — name-only checks are not enough.
                    let body = read_package_script_body(&pkg, script)?;
                    if !script_body_is_safe(&body) {
                        return Err(Error::InvalidInput(format!(
                            "script '{script}' is not auto-executable (shell constructs or unsupported runner)"
                        )));
                    }
                    let runner = detect_script_runner(&body).ok_or_else(|| {
                        Error::InvalidInput(format!(
                            "script '{script}' runner not supported (only vite / vue-cli-service / node relative entry)"
                        ))
                    })?;
                    plan.script = Some(script.to_string());
                    plan.entry_file = None;
                    plan.script_runner = Some(runner);
                    for a in &plan.args {
                        validate_arg(a)?;
                    }
                }
                LaunchProgram::Node => {
                    let entry = plan.entry_file.as_deref().ok_or_else(|| {
                        Error::InvalidInput("node program requires entryFile".into())
                    })?;
                    let entry_n = normalize_rel(entry)?;
                    validate_node_entry(&entry_n)?;
                    let entry_path = resolve_under(root, &join_rel(&plan.cwd_relative, &entry_n))?;
                    if !entry_path.is_file() {
                        return Err(Error::InvalidInput(format!(
                            "entryFile not found: {entry_n}"
                        )));
                    }
                    plan.entry_file = Some(entry_n);
                    plan.script = None;
                    plan.script_runner = Some(ScriptRunner::Node);
                    for a in &plan.args {
                        validate_arg(a)?;
                    }
                }
                LaunchProgram::Internal => unreachable!(),
            }
        }
        LocalLaunchRuntime::DockerCompose => {
            if plan.program != LaunchProgram::Internal {
                return Err(Error::InvalidInput(
                    "docker_compose runtime requires program=internal (the compose file drives execution)".into(),
                ));
            }
            let detail = plan.compose.take().ok_or_else(|| {
                Error::InvalidInput("docker_compose runtime requires a compose plan".into())
            })?;
            // The compose file must exist inside the project root (absolute
            // resolution happens at start; we validate reachability here).
            let compose_rel = normalize_rel(&detail.compose_file)?;
            let compose_path = resolve_under(root, &join_rel(&plan.cwd_relative, &compose_rel))?;
            if !compose_path.is_file() {
                return Err(Error::InvalidInput(format!(
                    "compose file not found: {compose_rel}"
                )));
            }
            if detail.project_seed.trim().is_empty() || detail.project_seed.len() > 64 {
                return Err(Error::InvalidInput("invalid compose project seed".into()));
            }
            for c in detail.project_seed.chars() {
                if !(c.is_ascii_alphanumeric() || c == '-') {
                    return Err(Error::InvalidInput(
                        "compose project seed may only contain letters, digits and '-'".into(),
                    ));
                }
            }
            for a in &detail.command {
                validate_arg(a)?;
            }
            plan.health_path = normalize_url_path(&detail.health_path)?;
            if let Some(p) = detail.host_port {
                if p == 0 {
                    return Err(Error::InvalidInput("invalid compose host port".into()));
                }
            }
            plan.compose = Some(ComposePlanDetail {
                compose_file: compose_rel,
                project_seed: detail.project_seed,
                service: detail.service,
                command: detail.command,
                health_path: detail.health_path,
                host_port: detail.host_port,
            });
            plan.script = None;
            plan.entry_file = None;
            plan.script_runner = None;
            plan.args.clear();
        }
    }

    if plan.reason.trim().is_empty() {
        plan.reason = "validated".into();
    }

    // P0 dangerous-command gate: a plan whose effective command can place real
    // trades is blocked by default. Only an explicit user authorization (added
    // with the Compose plan in later batches) may relax this — never auto-start.
    if super::risk::plan_command_risk(&plan) == super::risk::CommandRisk::Block {
        return Err(Error::InvalidInput(
            "launch plan blocked: command may place real trades; refusing to auto-start".into(),
        ));
    }

    Ok(plan)
}

/// Fingerprint of plan + key project files for orphan/process identity.
pub fn fingerprint_plan(root: &Path, plan: &LaunchPlan) -> String {
    let mut h = DefaultHasher::new();
    plan.schema_version.hash(&mut h);
    format!("{:?}", plan.source).hash(&mut h);
    plan.project_kind.as_str().hash(&mut h);
    format!("{:?}", plan.runtime).hash(&mut h);
    plan.program.as_str().hash(&mut h);
    plan.cwd_relative.hash(&mut h);
    plan.script.hash(&mut h);
    plan.entry_file.hash(&mut h);
    plan.script_runner.map(|r| r.as_str()).hash(&mut h);
    // Hash actual script body so replaced package.json scripts change fingerprint.
    if let (Some(script), LaunchProgram::Npm | LaunchProgram::Pnpm | LaunchProgram::Yarn) =
        (plan.script.as_deref(), plan.program)
    {
        let pkg = root.join(if plan.cwd_relative == "." {
            "package.json".into()
        } else {
            format!("{}/package.json", plan.cwd_relative)
        });
        if let Ok(body) = read_package_script_body(&pkg, script) {
            body.hash(&mut h);
        }
    }
    plan.args.hash(&mut h);
    plan.environment_keys.hash(&mut h);
    format!("{:?}", plan.port.mode).hash(&mut h);
    plan.port.value.hash(&mut h);
    plan.open_path.hash(&mut h);
    plan.health_path.hash(&mut h);
    plan.startup_timeout_ms.hash(&mut h);

    for name in [
        "package.json",
        "vite.config.ts",
        "vite.config.js",
        "vite.config.mjs",
        "vue.config.js",
        "index.html",
        "package-lock.json",
        "pnpm-lock.yaml",
        "yarn.lock",
    ] {
        let p = root.join(name);
        if let Ok(meta) = std::fs::metadata(&p) {
            name.hash(&mut h);
            meta.len().hash(&mut h);
            if let Ok(mtime) = meta.modified() {
                if let Ok(d) = mtime.duration_since(std::time::UNIX_EPOCH) {
                    d.as_secs().hash(&mut h);
                }
            }
        }
    }
    format!("{:016x}", h.finish())
}

fn normalize_rel(s: &str) -> Result<String> {
    let t = s.trim().trim_start_matches("./");
    if t.is_empty() {
        return Ok(".".into());
    }
    if t.starts_with('/') || t.contains("..") {
        return Err(Error::InvalidInput(format!("unsafe relative path: {s}")));
    }
    // backslash on unix is unusual — normalize
    let n = t.replace('\\', "/");
    if n.split('/').any(|p| p == ".." || p.is_empty() && n != ".") {
        // allow single "." only
        if n != "." {
            // empty segment from double slash
            if n.contains("//") {
                return Err(Error::InvalidInput(format!("invalid path: {s}")));
            }
        }
    }
    for part in n.split('/') {
        if part == ".." {
            return Err(Error::InvalidInput(format!("path traversal: {s}")));
        }
    }
    Ok(n)
}

fn join_rel(cwd: &str, child: &str) -> String {
    if cwd == "." || cwd.is_empty() {
        child.to_string()
    } else if child == "." {
        cwd.to_string()
    } else {
        format!("{cwd}/{child}")
    }
}

fn normalize_url_path(s: &str) -> Result<String> {
    let t = s.trim();
    if t.is_empty() {
        return Ok("/".into());
    }
    if t.contains("://") || t.contains("..") {
        return Err(Error::InvalidInput(format!(
            "invalid open/health path: {s}"
        )));
    }
    if t.starts_with('/') {
        Ok(t.to_string())
    } else {
        Ok(format!("/{t}"))
    }
}

fn validate_env_key(k: &str) -> Result<()> {
    let t = k.trim();
    if t.is_empty() || t.len() > 128 {
        return Err(Error::InvalidInput("invalid environment key".into()));
    }
    if !t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(Error::InvalidInput(format!(
            "environment key must be [A-Za-z0-9_]: {t}"
        )));
    }
    Ok(())
}

fn validate_script_name(script: &str) -> Result<()> {
    if script.is_empty() || script.len() > 64 {
        return Err(Error::InvalidInput("invalid script name".into()));
    }
    for m in FORBIDDEN_SCRIPT_MARKERS {
        if script.contains(m) {
            return Err(Error::InvalidInput(format!(
                "script contains forbidden shell construct: {m}"
            )));
        }
    }
    // npm script names are typically [a-z0-9:_\-]
    if !script
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '_' | '-' | '.'))
    {
        return Err(Error::InvalidInput(format!(
            "unsupported script name: {script}"
        )));
    }
    Ok(())
}

fn validate_arg(arg: &str) -> Result<()> {
    if arg.is_empty() || arg.len() > 256 {
        return Err(Error::InvalidInput("invalid arg".into()));
    }
    for m in FORBIDDEN_SCRIPT_MARKERS {
        if arg.contains(m) {
            return Err(Error::InvalidInput(format!(
                "arg contains forbidden shell construct: {m}"
            )));
        }
    }
    if arg.starts_with('-') {
        // flags ok
        return Ok(());
    }
    // relative entry-like args only
    if Path::new(arg).is_absolute() {
        return Err(Error::InvalidInput(
            "absolute path args are not allowed".into(),
        ));
    }
    Ok(())
}

/// Reject package.json script bodies that are not auto-executable.
pub fn script_body_is_safe(body: &str) -> bool {
    detect_script_runner(body).is_some()
}

/// Detect a supported single-command runner from a package.json script body.
/// Returns None when the body uses shell metacharacters or unsupported wrappers.
pub fn detect_script_runner(body: &str) -> Option<ScriptRunner> {
    let t = body.trim();
    if t.is_empty() {
        return None;
    }
    for m in FORBIDDEN_SCRIPT_MARKERS {
        if t.contains(m) {
            return None;
        }
    }
    // Reject env assignment prefixes: FOO=bar vite
    let first = t.split_whitespace().next()?;
    if first.contains('=') {
        return None;
    }
    for banned in [
        "sudo",
        "sh",
        "bash",
        "zsh",
        "fish",
        "cmd",
        "powershell",
        "pwsh",
        "npx",
        "yarn",
        "pnpm",
        "npm",
        "bun",
        "deno",
        "cross-env",
        "env",
        "exec",
    ] {
        if first.eq_ignore_ascii_case(banned) {
            return None;
        }
    }

    // Normalize path-like binaries: ./node_modules/.bin/vite → vite
    let bin = first
        .rsplit('/')
        .next()
        .unwrap_or(first)
        .rsplit('\\')
        .next()
        .unwrap_or(first)
        .trim_end_matches(".cmd")
        .trim_end_matches(".exe");

    if bin.eq_ignore_ascii_case("vite") {
        // Remaining tokens must be plain args (no shell)
        if !rest_args_safe(t) {
            return None;
        }
        return Some(ScriptRunner::Vite);
    }
    if bin.eq_ignore_ascii_case("vue-cli-service") {
        // Expect subcommand serve/dev
        let parts: Vec<&str> = t.split_whitespace().collect();
        if parts.len() < 2 {
            return None;
        }
        let sub = parts[1];
        if !(sub.eq_ignore_ascii_case("serve") || sub.eq_ignore_ascii_case("dev")) {
            return None;
        }
        if !rest_args_safe(t) {
            return None;
        }
        return Some(ScriptRunner::VueCli);
    }
    if bin.eq_ignore_ascii_case("node") {
        // node relative-entry.js [args]
        let parts: Vec<&str> = t.split_whitespace().collect();
        if parts.len() < 2 {
            return None;
        }
        if validate_node_entry(parts[1]).is_err() {
            return None;
        }
        if !rest_args_safe(t) {
            return None;
        }
        return Some(ScriptRunner::Node);
    }
    None
}

fn rest_args_safe(body: &str) -> bool {
    for tok in body.split_whitespace().skip(1) {
        if validate_arg(tok).is_err() {
            return false;
        }
    }
    true
}

fn validate_node_entry(entry: &str) -> Result<()> {
    let n = normalize_rel(entry)?;
    let lower = n.to_ascii_lowercase();
    if !(lower.ends_with(".js") || lower.ends_with(".mjs") || lower.ends_with(".cjs")) {
        return Err(Error::InvalidInput(
            "node entryFile must be a relative .js/.mjs/.cjs file".into(),
        ));
    }
    Ok(())
}

fn read_package_script_body(pkg_path: &Path, script: &str) -> Result<String> {
    let meta = std::fs::metadata(pkg_path).map_err(Error::Io)?;
    if meta.len() > 1024 * 1024 {
        return Err(Error::InvalidInput("package.json too large".into()));
    }
    let data = std::fs::read_to_string(pkg_path).map_err(Error::Io)?;
    let v: serde_json::Value = serde_json::from_str(&data)
        .map_err(|e| Error::InvalidInput(format!("package.json: {e}")))?;
    let body = v
        .get("scripts")
        .and_then(|s| s.get(script))
        .and_then(|x| x.as_str())
        .ok_or_else(|| {
            Error::InvalidInput(format!("script '{script}' not found in package.json"))
        })?;
    Ok(body.to_string())
}

/// Build extra CLI flags based on detected runner. Does not include package-manager args.
pub fn runner_port_flags(runner: ScriptRunner, port: u16) -> Vec<String> {
    match runner {
        ScriptRunner::Vite => vec![
            "--host".into(),
            "127.0.0.1".into(),
            "--port".into(),
            port.to_string(),
            "--strictPort".into(),
        ],
        ScriptRunner::VueCli => vec![
            "--host".into(),
            "127.0.0.1".into(),
            "--port".into(),
            port.to_string(),
        ],
        ScriptRunner::Node => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> PathBuf {
        let mut p = std::env::temp_dir();
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        p.push(format!("natives-local-plan-test-{n}"));
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn html_plan() -> LaunchPlan {
        LaunchPlan {
            schema_version: 1,
            source: LaunchPlanSource::Rule,
            project_kind: LocalProjectKind::Html,
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
            confidence: Some(1.0),
            reason: "test".into(),
            compose: None,
            trade_approval: None,
            process_profile: None,
        }
    }

    #[test]
    fn validates_static_html() {
        let dir = temp_dir();
        fs::write(dir.join("index.html"), "<html/>").unwrap();
        let p = validate_launch_plan(&dir, html_plan()).unwrap();
        assert_eq!(p.program, LaunchProgram::Internal);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_shell_in_script() {
        assert!(!script_body_is_safe("vite && echo hi"));
        assert!(!script_body_is_safe("node $(cat x)"));
        assert!(script_body_is_safe("vite"));
        assert!(script_body_is_safe("vue-cli-service serve"));
    }

    #[test]
    fn rejects_bad_port() {
        let dir = temp_dir();
        fs::write(dir.join("index.html"), "<html/>").unwrap();
        let mut plan = html_plan();
        plan.port = LaunchPort {
            mode: LaunchPortMode::Fixed,
            value: None,
        };
        assert!(validate_launch_plan(&dir, plan).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_npm_script_body_with_shell() {
        assert!(detect_script_runner("vite && echo hi").is_none());
        assert!(detect_script_runner("npx vite").is_none());
        assert!(detect_script_runner("cross-env NODE_ENV=dev vite").is_none());
        assert_eq!(
            detect_script_runner("vite"),
            Some(crate::creative_app::model::ScriptRunner::Vite)
        );
        assert_eq!(
            detect_script_runner("vue-cli-service serve"),
            Some(crate::creative_app::model::ScriptRunner::VueCli)
        );
    }

    #[test]
    fn vite_and_vue_flags_differ() {
        let v = runner_port_flags(crate::creative_app::model::ScriptRunner::Vite, 5173);
        assert!(v.iter().any(|a| a == "--strictPort"));
        let c = runner_port_flags(crate::creative_app::model::ScriptRunner::VueCli, 8080);
        assert!(!c.iter().any(|a| a == "--strictPort"));
        assert!(c.iter().any(|a| a == "--port"));
    }

    fn compose_plan(file: &str, command: Vec<String>) -> LaunchPlan {
        let mut plan = html_plan();
        plan.runtime = LocalLaunchRuntime::DockerCompose;
        plan.program = LaunchProgram::Internal;
        plan.compose = Some(ComposePlanDetail {
            compose_file: file.into(),
            project_seed: "proj".into(),
            service: None,
            command,
            health_path: "/".into(),
            host_port: None,
        });
        plan
    }

    /// Batch 5: a Compose plan validates only when the compose file exists under
    /// root and the effective command is not a real-trading override.
    #[test]
    fn compose_plan_validates_and_gates_trade_override() {
        let dir = temp_dir();
        fs::write(
            dir.join("docker-compose.yml"),
            "services:\n  web:\n    image: x\n",
        )
        .unwrap();
        // Explicit trade override is blocked by the risk gate.
        let bad = compose_plan(
            "docker-compose.yml",
            vec!["trade".into(), "--config".into()],
        );
        assert!(
            validate_launch_plan(&dir, bad).is_err(),
            "a trade override must never validate"
        );
        // Safe compose plan validates with runtime preserved.
        let ok = compose_plan("docker-compose.yml", vec![]);
        let v = validate_launch_plan(&dir, ok).unwrap();
        assert_eq!(v.runtime, LocalLaunchRuntime::DockerCompose);
        assert_eq!(v.creative_runtime(), CreativeAppRuntime::DockerCompose);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn compose_plan_requires_file_under_root() {
        let dir = temp_dir();
        let plan = compose_plan("nope.yml", vec![]);
        assert!(validate_launch_plan(&dir, plan).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    /// Batch 8: an explicit webserver approval relaxes the gate only when the
    /// command is a webserver command; a mismatch stays blocked.
    #[test]
    fn trade_approval_only_relaxes_matching_safe_mode() {
        let dir = temp_dir();
        fs::write(
            dir.join("docker-compose.yml"),
            "services:\n  bot:\n    image: t\n",
        )
        .unwrap();

        // A trade command override with a webserver approval must STILL block.
        let mut mismatch = compose_plan(
            "docker-compose.yml",
            vec!["trade".into(), "--config".into()],
        );
        mismatch.trade_approval = Some(TradeApproval::Webserver);
        assert!(
            validate_launch_plan(&dir, mismatch).is_err(),
            "a webserver approval can never run trade"
        );

        // A webserver command with a webserver approval validates.
        let mut ok = compose_plan(
            "docker-compose.yml",
            vec!["webserver".into(), "--config".into()],
        );
        ok.trade_approval = Some(TradeApproval::Webserver);
        assert!(
            validate_launch_plan(&dir, ok).is_ok(),
            "matching webserver approval must validate"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
