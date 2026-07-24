//! Production HookRegistry assembly (extracted from `production.rs`, task-01 structure).
//!
//! Rust lifecycle hooks are always present (fail-open allow defaults + fail-closed
//! security), plus optional trusted command/HTTP hooks from env and project
//! `.claude|grok|natives/hooks.json` command hooks.

use agent_core::{AllowAllHook, CommandHook, HookEvent, HookRegistry, HttpHook};
use serde_json::Value;
use std::time::Duration;

/// Build the production HookRegistry using the current working directory for
/// project hook discovery. Prefer [`build_production_hooks_for_project`] with an
/// explicit path on the main run path.
pub fn build_production_hooks() -> HookRegistry {
    build_production_hooks_for_project(std::env::current_dir().ok().as_deref())
}

/// Same as [`build_production_hooks`] with an explicit project path for hook discovery.
pub fn build_production_hooks_for_project(project: Option<&std::path::Path>) -> HookRegistry {
    let mut hooks = HookRegistry::new();
    // Full target event surface (fail-open allow-all defaults; project hooks may deny).
    for event in [
        HookEvent::SessionStart,
        HookEvent::SessionEnd,
        HookEvent::UserPromptSubmit,
        HookEvent::PreToolUse,
        HookEvent::PostToolUse,
        HookEvent::PostToolUseFailure,
        HookEvent::PermissionRequest,
        HookEvent::PermissionDenied,
        HookEvent::Notification,
        HookEvent::SubagentStart,
        HookEvent::SubagentStop,
        HookEvent::PreCompact,
        HookEvent::PostCompact,
        HookEvent::Stop,
        HookEvent::StopFailure,
        HookEvent::Error,
    ] {
        // Built-in allow defaults; project/user hooks may still Deny (aggregate fail-closed).
        hooks.register(event, Box::new(AllowAllHook));
    }
    // After defaults, enable fail-closed so removing all handlers cannot open tools.
    hooks.enable_security_fail_closed();

    if let Some(root) = project {
        load_project_hooks(root, &mut hooks);
    }

    // Trusted command hook: NATIVES_HOOK_CMD=/path/to/binary (argv only, never shell).
    if let Ok(program) = std::env::var("NATIVES_HOOK_CMD") {
        if !program.trim().is_empty() {
            hooks.register(
                HookEvent::PreToolUse,
                Box::new(CommandHook {
                    program,
                    args: std::env::var("NATIVES_HOOK_CMD_ARGS")
                        .ok()
                        .map(|s| s.split_whitespace().map(str::to_string).collect())
                        .unwrap_or_default(),
                    timeout: Duration::from_secs(10),
                    trusted: true,
                }),
            );
        }
    }
    // HTTP hook with host allowlist: NATIVES_HOOK_HTTP=https://hooks.example/pre
    if let Ok(url) = std::env::var("NATIVES_HOOK_HTTP") {
        if !url.trim().is_empty() {
            let allow = std::env::var("NATIVES_HOOK_HTTP_ALLOW")
                .ok()
                .map(|s| s.split(',').map(|h| h.trim().to_string()).collect())
                .unwrap_or_default();
            hooks.register(
                HookEvent::PostToolUse,
                Box::new(HttpHook {
                    url,
                    timeout: Duration::from_secs(5),
                    allow_hosts: allow,
                }),
            );
        }
    }
    hooks
}

/// Load project hooks from `.claude|grok|natives/hooks.json` (compatible shapes).
/// Supported: `{ "PreToolUse": [ { "type": "command", "command": "/path", "args": [] } ] }`
fn load_project_hooks(project: &std::path::Path, hooks: &mut HookRegistry) {
    let candidates = [
        project.join(".claude").join("hooks.json"),
        project.join(".grok").join("hooks.json"),
        project.join(".natives").join("hooks.json"),
    ];
    for path in candidates {
        load_hooks_file(&path, hooks);
    }
    // User-level hooks (optional): ~/.natives/hooks.json
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        load_hooks_file(
            &std::path::PathBuf::from(home)
                .join(".natives")
                .join("hooks.json"),
            hooks,
        );
    }
}

fn load_hooks_file(path: &std::path::Path, hooks: &mut HookRegistry) {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return;
    };
    let Ok(value) = serde_json::from_str::<Value>(&raw) else {
        return;
    };
    let Some(obj) = value.as_object() else {
        return;
    };
    for (event_name, handlers) in obj {
        let event = match event_name.as_str() {
            "PreToolUse" | "pre_tool_use" => HookEvent::PreToolUse,
            "PostToolUse" | "post_tool_use" => HookEvent::PostToolUse,
            "PostToolUseFailure" => HookEvent::PostToolUseFailure,
            "Stop" | "stop" => HookEvent::Stop,
            "StopFailure" => HookEvent::StopFailure,
            "SessionStart" => HookEvent::SessionStart,
            "SessionEnd" => HookEvent::SessionEnd,
            "UserPromptSubmit" => HookEvent::UserPromptSubmit,
            "PermissionRequest" | "PermissionDenied" => HookEvent::PermissionDenied,
            "SubagentStart" => HookEvent::SubagentStart,
            "SubagentStop" => HookEvent::SubagentStop,
            "CompactStart" | "PreCompact" => HookEvent::PreCompact,
            "CompactEnd" | "PostCompact" => HookEvent::PostCompact,
            "Notification" => HookEvent::Notification,
            "Error" => HookEvent::StopFailure,
            _ => continue,
        };
        let Some(arr) = handlers.as_array() else {
            continue;
        };
        for h in arr {
            let h_type = h.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if h_type != "command" {
                continue;
            }
            let Some(program) = h.get("command").and_then(|v| v.as_str()) else {
                continue;
            };
            if program.trim().is_empty() {
                continue;
            }
            let args = h
                .get("args")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            hooks.register(
                event,
                Box::new(CommandHook {
                    program: program.to_string(),
                    args,
                    timeout: Duration::from_secs(10),
                    trusted: true,
                }),
            );
        }
    }
}
