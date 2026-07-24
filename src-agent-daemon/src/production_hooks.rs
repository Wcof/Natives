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
                    cwd: project.map(std::path::Path::to_path_buf),
                    tool_pattern: None,
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
                    tool_pattern: None,
                }),
            );
        }
    }
    hooks
}

/// Load project/user hooks from native files and Claude-compatible settings.
fn load_project_hooks(project: &std::path::Path, hooks: &mut HookRegistry) {
    let candidates = [
        project.join(".claude").join("hooks.json"),
        project.join(".claude").join("settings.json"),
        project.join(".claude").join("settings.local.json"),
        project.join(".agents").join("hooks.json"),
        project.join(".agents").join("settings.json"),
        project.join(".grok").join("hooks.json"),
        project.join(".natives").join("hooks.json"),
    ];
    for path in candidates {
        load_hooks_file(&path, project, hooks);
    }
    // User-level hooks (optional). Unit tests must not execute the developer's
    // real ~/.claude hook commands.
    #[cfg(not(test))]
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        let home = std::path::PathBuf::from(home);
        for path in [
            home.join(".natives").join("hooks.json"),
            home.join(".agents").join("hooks.json"),
            home.join(".agents").join("settings.json"),
            home.join(".claude").join("settings.json"),
            home.join(".claude").join("settings.local.json"),
        ] {
            load_hooks_file(&path, project, hooks);
        }
    }
}

fn load_hooks_file(
    path: &std::path::Path,
    project: &std::path::Path,
    hooks: &mut HookRegistry,
) {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return;
    };
    let Ok(value) = serde_json::from_str::<Value>(&raw) else {
        return;
    };
    let Some(obj) = value
        .get("hooks")
        .and_then(Value::as_object)
        .or_else(|| value.as_object())
    else {
        return;
    };
    for (event_name, groups) in obj {
        let Some(event) = parse_hook_event(event_name) else {
            continue;
        };
        let Some(groups) = groups.as_array() else {
            continue;
        };
        for group in groups {
            let matcher = group
                .get("matcher")
                .and_then(Value::as_str)
                .map(str::to_string);
            if let Some(handlers) = group.get("hooks").and_then(Value::as_array) {
                for handler in handlers {
                    register_handler(event, matcher.clone(), handler, project, hooks);
                }
            } else {
                register_handler(event, matcher, group, project, hooks);
            }
        }
    }
}

fn parse_hook_event(name: &str) -> Option<HookEvent> {
    Some(match name {
        "PreToolUse" | "pre_tool_use" => HookEvent::PreToolUse,
        "PostToolUse" | "post_tool_use" => HookEvent::PostToolUse,
        "PostToolUseFailure" | "post_tool_use_failure" => HookEvent::PostToolUseFailure,
        "Stop" | "stop" => HookEvent::Stop,
        "StopFailure" | "stop_failure" => HookEvent::StopFailure,
        "SessionStart" | "session_start" => HookEvent::SessionStart,
        "SessionEnd" | "session_end" => HookEvent::SessionEnd,
        "UserPromptSubmit" | "user_prompt_submit" => HookEvent::UserPromptSubmit,
        "PermissionRequest" | "permission_request" => HookEvent::PermissionRequest,
        "PermissionDenied" | "permission_denied" => HookEvent::PermissionDenied,
        "SubagentStart" | "subagent_start" => HookEvent::SubagentStart,
        "SubagentStop" | "SubagentEnd" | "subagent_stop" => HookEvent::SubagentStop,
        "CompactStart" | "PreCompact" | "pre_compact" => HookEvent::PreCompact,
        "CompactEnd" | "PostCompact" | "post_compact" => HookEvent::PostCompact,
        "Notification" | "notification" => HookEvent::Notification,
        "Error" | "error" => HookEvent::Error,
        _ => return None,
    })
}

fn register_handler(
    event: HookEvent,
    matcher: Option<String>,
    handler: &Value,
    project: &std::path::Path,
    hooks: &mut HookRegistry,
) {
    let timeout = Duration::from_secs(
        handler
            .get("timeout")
            .and_then(Value::as_u64)
            .unwrap_or(10)
            .clamp(1, 600),
    );
    match handler.get("type").and_then(Value::as_str).unwrap_or("command") {
        "command" => {
            let Some(command) = handler.get("command").and_then(Value::as_str) else {
                return;
            };
            if command.trim().is_empty() {
                return;
            }
            let explicit_args = handler
                .get("args")
                .and_then(Value::as_array)
                .map(|args| {
                    args.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                });
            let (program, args) = if let Some(args) = explicit_args {
                (command.to_string(), args)
            } else if cfg!(windows) {
                ("cmd.exe".to_string(), vec!["/C".into(), command.to_string()])
            } else {
                (
                    "/bin/sh".to_string(),
                    vec!["-lc".into(), command.to_string()],
                )
            };
            hooks.register(
                event,
                Box::new(CommandHook {
                    program,
                    args,
                    timeout,
                    trusted: true,
                    cwd: Some(project.to_path_buf()),
                    tool_pattern: matcher,
                }),
            );
        }
        "http" => {
            let Some(url) = handler.get("url").and_then(Value::as_str) else {
                return;
            };
            hooks.register(
                event,
                Box::new(HttpHook {
                    url: url.to_string(),
                    timeout,
                    allow_hosts: Vec::new(),
                    tool_pattern: matcher,
                }),
            );
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{HookRequest, HookRegistry};

    #[cfg(unix)]
    #[tokio::test]
    async fn loads_claude_settings_matchers_and_permission_request() {
        let project =
            std::env::temp_dir().join(format!("natives-hooks-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(project.join(".claude")).unwrap();
        std::fs::write(
            project.join(".claude").join("settings.json"),
            r#"{
              "hooks": {
                "PreToolUse": [{
                  "matcher": "Bash|run_command",
                  "hooks": [{ "type": "command", "command": "/usr/bin/false || /usr/bin/true" }]
                }],
                "PermissionRequest": [{
                  "hooks": [{ "type": "command", "command": "/usr/bin/true" }]
                }]
              }
            }"#,
        )
        .unwrap();
        let hooks = build_production_hooks_for_project(Some(&project));

        let unmatched = hooks
            .dispatch(HookRequest {
                event: HookEvent::PreToolUse,
                run_id: "run".into(),
                tool_name: Some("read_file".into()),
                input: serde_json::json!({}),
            })
            .await;
        let matched = hooks
            .dispatch(HookRequest {
                event: HookEvent::PreToolUse,
                run_id: "run".into(),
                tool_name: Some("run_command".into()),
                input: serde_json::json!({}),
            })
            .await;
        let permission = hooks
            .dispatch(HookRequest {
                event: HookEvent::PermissionRequest,
                run_id: "run".into(),
                tool_name: Some("write_file".into()),
                input: serde_json::json!({}),
            })
            .await;

        let _ = std::fs::remove_dir_all(&project);
        assert_eq!(unmatched.len(), 1);
        assert_eq!(matched.len(), 2);
        assert_eq!(permission.len(), 2);
        assert!(HookRegistry::aggregate_allow(&matched).is_ok());
    }
}
