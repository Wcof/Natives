//! Production Hook assembly — discovery and compilation.
//!
//! Split into two halves so Hooks can be inspected as data before they become
//! opaque handlers:
//!
//! - [`discover_production_hooks`] reads builtin defaults, project and user
//!   files, and environment configuration into [`HookDefinition`] values that
//!   carry identity, provenance, order, and policy.
//! - [`compile_production_hooks`] turns those definitions into an executable
//!   [`HookRegistry`].
//!
//! [`build_production_hooks_for_project`] is their composition and is what the
//! run path calls. Discovery order is dispatch order; the two must never drift.
//!
//! Rust lifecycle hooks are always present (fail-open allow defaults +
//! fail-closed security), plus optional trusted command/HTTP hooks from env and
//! project `.claude|grok|natives/hooks.json` command hooks.

use agent_core::{
    AgentMessage, AllowAllHook, CommandHook, ContentBlock, EngineProvider, EngineProviderContext,
    EngineProviderEvent, EventSequencer, HookDecision, HookHandler, HookOutcome, HookRegistry,
    HookRequest, HookResponse, HttpHook, MessageId, ProviderTurnRequest, UserMessage,
};
use assistant_protocol::v2::RunEventKind;
use futures_util::StreamExt;
use harness_core::blueprint::{CommandWorkingDirPolicy, HookAdapterSpecV3, NativeHookSpecV3};
use harness_core::hooks::{
    HookDefinition, HookErrorCategory, HookEvent, HookFailurePolicy, HookId, HookInvocationTrace,
    HookKind, HookScope, HookSource,
};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

/// Name of the built-in allow-all default, as it appears in Hook identities.
const BUILTIN_ALLOW_ALL: &str = "allow-all";

/// Project-root-relative candidate files, in load order.
const PROJECT_CANDIDATES: [[&str; 2]; 7] = [
    [".claude", "hooks.json"],
    [".claude", "settings.json"],
    [".claude", "settings.local.json"],
    [".agents", "hooks.json"],
    [".agents", "settings.json"],
    [".grok", "hooks.json"],
    [".natives", "hooks.json"],
];

/// Home-relative candidate files, in load order.
#[cfg_attr(test, allow(dead_code))]
const USER_CANDIDATES: [[&str; 2]; 5] = [
    [".natives", "hooks.json"],
    [".agents", "hooks.json"],
    [".agents", "settings.json"],
    [".claude", "settings.json"],
    [".claude", "settings.local.json"],
];

/// Build the production HookRegistry using the current working directory for
/// project hook discovery. Prefer [`build_production_hooks_for_project`] with an
/// explicit path on the main run path.
pub fn build_production_hooks() -> HookRegistry {
    build_production_hooks_for_project(std::env::current_dir().ok().as_deref())
}

/// Same as [`build_production_hooks`] with an explicit project path for hook discovery.
pub fn build_production_hooks_for_project(project: Option<&Path>) -> HookRegistry {
    compile_production_hooks(&discover_production_hooks(project), project)
}

/// Enumerate every Hook that would be registered for `project`, as data.
///
/// The returned order is the dispatch order: builtin defaults first, then
/// project files, then user files, then environment hooks. `order` is the
/// per-event ordinal, so it always agrees with actual dispatch sequence.
pub fn discover_production_hooks(project: Option<&Path>) -> Vec<HookDefinition> {
    let mut out = Vec::new();
    let mut ordinals: HashMap<HookEvent, i32> = HashMap::new();

    // Built-in allow defaults; project/user hooks may still Deny (aggregate fail-closed).
    for event in HookEvent::ALL {
        let source = HookSource::builtin(BUILTIN_ALLOW_ALL);
        out.push(HookDefinition {
            id: HookId::new(&source, event),
            event,
            source,
            order: next_ordinal(&mut ordinals, event),
            matcher: None,
            conditions: Vec::new(),
            timeout_ms: 0,
            failure_policy: HookFailurePolicy::Fail,
            kind: HookKind::Builtin {
                name: BUILTIN_ALLOW_ALL.to_string(),
            },
        });
    }

    if let Some(root) = project {
        for [dir, file] in PROJECT_CANDIDATES {
            let path = root.join(dir).join(file);
            let origin = format!("{dir}/{file}");
            collect_file_hooks(&path, HookScope::Project, &origin, &mut ordinals, &mut out);
        }
        // User-level hooks (optional). Unit tests must not execute the
        // developer's real ~/.claude hook commands.
        #[cfg(not(test))]
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            let home = std::path::PathBuf::from(home);
            for [dir, file] in USER_CANDIDATES {
                let path = home.join(dir).join(file);
                let origin = format!("{dir}/{file}");
                collect_file_hooks(&path, HookScope::User, &origin, &mut ordinals, &mut out);
            }
        }
    }

    // Trusted command hook: NATIVES_HOOK_CMD=/path/to/binary (argv only, never shell).
    if let Ok(program) = std::env::var("NATIVES_HOOK_CMD") {
        if !program.trim().is_empty() {
            let source = HookSource::env("NATIVES_HOOK_CMD");
            out.push(HookDefinition {
                id: HookId::new(&source, HookEvent::PreToolUse),
                event: HookEvent::PreToolUse,
                source,
                order: next_ordinal(&mut ordinals, HookEvent::PreToolUse),
                matcher: None,
                conditions: Vec::new(),
                timeout_ms: 10_000,
                failure_policy: HookFailurePolicy::Fail,
                kind: HookKind::Command {
                    program,
                    args: std::env::var("NATIVES_HOOK_CMD_ARGS")
                        .ok()
                        .map(|s| s.split_whitespace().map(str::to_string).collect())
                        .unwrap_or_default(),
                    trusted: true,
                },
            });
        }
    }
    // HTTP hook with host allowlist: NATIVES_HOOK_HTTP=https://hooks.example/pre
    if let Ok(url) = std::env::var("NATIVES_HOOK_HTTP") {
        if !url.trim().is_empty() {
            let source = HookSource::env("NATIVES_HOOK_HTTP");
            out.push(HookDefinition {
                id: HookId::new(&source, HookEvent::PostToolUse),
                event: HookEvent::PostToolUse,
                source,
                order: next_ordinal(&mut ordinals, HookEvent::PostToolUse),
                matcher: None,
                conditions: Vec::new(),
                timeout_ms: 5_000,
                failure_policy: HookFailurePolicy::Fail,
                kind: HookKind::Http {
                    url,
                    allow_hosts: std::env::var("NATIVES_HOOK_HTTP_ALLOW")
                        .ok()
                        .map(|s| s.split(',').map(|h| h.trim().to_string()).collect())
                        .unwrap_or_default(),
                },
            });
        }
    }
    out
}

/// Compile definitions into an executable registry, preserving their order.
///
/// `project` supplies the working directory handed to command hooks; it is not
/// re-read for discovery.
pub fn compile_production_hooks(
    definitions: &[HookDefinition],
    project: Option<&Path>,
) -> HookRegistry {
    compile_production_hooks_with_native(definitions, &[], project)
}

/// Compile the resolved definitions and preserve the complete v3 adapter for
/// Natives-owned Hooks. Imported definitions continue through the legacy
/// Command/HTTP path.
pub fn compile_production_hooks_with_native(
    definitions: &[HookDefinition],
    native_hooks: &[NativeHookSpecV3],
    project: Option<&Path>,
) -> HookRegistry {
    let mut hooks = HookRegistry::new();
    for definition in definitions {
        let native = native_hooks
            .iter()
            .find(|hook| HookId::native(&hook.id, hook.event) == definition.id);
        let handler: Box<dyn HookHandler> = if let Some(native) = native {
            match &native.adapter {
                HookAdapterSpecV3::Command {
                    program,
                    args,
                    working_dir_policy,
                    secret_env_refs,
                    trusted,
                    mode,
                    ..
                } if secret_env_refs.is_empty()
                    && *mode == harness_core::blueprint::CommandMode::Exec =>
                {
                    Box::new(CommandHook {
                        program: program.clone(),
                        args: args.clone(),
                        timeout: definition.timeout(),
                        trusted: *trusted,
                        cwd: match working_dir_policy {
                            CommandWorkingDirPolicy::ProjectRoot => project.map(Path::to_path_buf),
                            CommandWorkingDirPolicy::None => None,
                        },
                        tool_pattern: definition.matcher.clone(),
                    })
                }
                HookAdapterSpecV3::Http {
                    url,
                    allow_hosts,
                    headers,
                    secret_header_refs,
                } if headers.is_empty() && secret_header_refs.is_empty() => Box::new(HttpHook {
                    url: url.clone(),
                    timeout: definition.timeout(),
                    allow_hosts: allow_hosts.clone(),
                    tool_pattern: definition.matcher.clone(),
                }),
                HookAdapterSpecV3::McpTool {
                    server_id,
                    tool_name,
                    input_template,
                } => Box::new(NativeMcpHook {
                    server_id: server_id.clone(),
                    tool_name: tool_name.clone(),
                    input_template: input_template.clone(),
                }),
                HookAdapterSpecV3::Prompt {
                    template,
                    model_override,
                } => Box::new(NativePromptHook {
                    template: template.clone(),
                    model_override: model_override.clone(),
                    timeout_ms: native.timeout_ms,
                }),
                HookAdapterSpecV3::Agent {
                    prompt,
                    model_override,
                    max_steps,
                    readonly_tools,
                } => Box::new(NativeAgentHook {
                    prompt: prompt.clone(),
                    model_override: model_override.clone(),
                    max_steps: *max_steps,
                    readonly_tools: readonly_tools.clone(),
                    timeout_ms: native.timeout_ms,
                }),
                HookAdapterSpecV3::Command { .. } | HookAdapterSpecV3::Http { .. } => {
                    Box::new(UnsupportedNativeHook)
                }
            }
        } else {
            compile_standard_hook(definition, project)
        };
        hooks.register_defined(definition.clone(), handler);
    }
    hooks.enable_security_fail_closed();
    hooks
}

fn compile_standard_hook(
    definition: &HookDefinition,
    project: Option<&Path>,
) -> Box<dyn HookHandler> {
    match &definition.kind {
        HookKind::Builtin { name } if name == BUILTIN_ALLOW_ALL => Box::new(AllowAllHook),
        // Unknown builtins are inert rather than fatal: an older Daemon
        // must not crash on a definition a newer one wrote.
        HookKind::Builtin { .. } => Box::new(UnsupportedNativeHook),
        HookKind::Command {
            program,
            args,
            trusted,
        } => Box::new(CommandHook {
            program: program.clone(),
            args: args.clone(),
            timeout: definition.timeout(),
            trusted: *trusted,
            cwd: project.map(Path::to_path_buf),
            tool_pattern: definition.matcher.clone(),
        }),
        HookKind::Http { url, allow_hosts } => Box::new(HttpHook {
            url: url.clone(),
            timeout: definition.timeout(),
            allow_hosts: allow_hosts.clone(),
            tool_pattern: definition.matcher.clone(),
        }),
    }
}

fn next_ordinal(ordinals: &mut HashMap<HookEvent, i32>, event: HookEvent) -> i32 {
    let slot = ordinals.entry(event).or_insert(0);
    let current = *slot;
    *slot += 1;
    current
}

/// Parse one candidate file into definitions, appending in file order.
///
/// Unreadable, malformed, or structurally unexpected files are skipped
/// silently — a broken editor config must not stop a Run from starting.
fn collect_file_hooks(
    path: &Path,
    scope: HookScope,
    origin: &str,
    ordinals: &mut HashMap<HookEvent, i32>,
    out: &mut Vec<HookDefinition>,
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
        let Some(event) = HookEvent::parse(event_name) else {
            continue;
        };
        let Some(groups) = groups.as_array() else {
            continue;
        };
        for (group_index, group) in groups.iter().enumerate() {
            let matcher = group
                .get("matcher")
                .and_then(Value::as_str)
                .map(str::to_string);
            if let Some(handlers) = group.get("hooks").and_then(Value::as_array) {
                for (entry_index, handler) in handlers.iter().enumerate() {
                    push_handler_definition(
                        event,
                        matcher.clone(),
                        handler,
                        HookSource::file(scope, origin, group_index, entry_index),
                        ordinals,
                        out,
                    );
                }
            } else {
                push_handler_definition(
                    event,
                    matcher,
                    group,
                    HookSource::file(scope, origin, group_index, 0),
                    ordinals,
                    out,
                );
            }
        }
    }
}

/// Read a Hook's `failure_policy` from its handler entry.
///
/// Both the snake_case (`failure_policy`) and camelCase (`failurePolicy`)
/// spellings are accepted. Unknown or missing values default to `Fail`
/// (`HookFailurePolicy::Fail`) — the same fail-closed default the runtime's
/// `resolve_failure` applies, so discovery and dispatch can never disagree
/// about what "unspecified" means.
fn parse_failure_policy(handler: &Value) -> HookFailurePolicy {
    let raw = handler
        .get("failure_policy")
        .and_then(Value::as_str)
        .or_else(|| handler.get("failurePolicy").and_then(Value::as_str))
        .unwrap_or("fail");
    match raw {
        "skip" => HookFailurePolicy::Skip,
        "default" | "continue" | "allow" => HookFailurePolicy::Default,
        _ => HookFailurePolicy::Fail,
    }
}

fn push_handler_definition(
    event: HookEvent,
    matcher: Option<String>,
    handler: &Value,
    source: HookSource,
    ordinals: &mut HashMap<HookEvent, i32>,
    out: &mut Vec<HookDefinition>,
) {
    let timeout_ms = handler
        .get("timeout")
        .and_then(Value::as_u64)
        .unwrap_or(10)
        .clamp(1, 600)
        * 1_000;

    let kind = match handler
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("command")
    {
        "command" => {
            let Some(command) = handler.get("command").and_then(Value::as_str) else {
                return;
            };
            if command.trim().is_empty() {
                return;
            }
            let explicit_args = handler.get("args").and_then(Value::as_array).map(|args| {
                args.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            });
            let (program, args) = if let Some(args) = explicit_args {
                (command.to_string(), args)
            } else if cfg!(windows) {
                (
                    "cmd.exe".to_string(),
                    vec!["/C".into(), command.to_string()],
                )
            } else {
                (
                    "/bin/sh".to_string(),
                    vec!["-lc".into(), command.to_string()],
                )
            };
            HookKind::Command {
                program,
                args,
                trusted: true,
            }
        }
        "http" => {
            let Some(url) = handler.get("url").and_then(Value::as_str) else {
                return;
            };
            HookKind::Http {
                url: url.to_string(),
                allow_hosts: Vec::new(),
            }
        }
        _ => return,
    };

    out.push(HookDefinition {
        id: HookId::new(&source, event),
        event,
        source,
        order: next_ordinal(ordinals, event),
        matcher,
        conditions: Vec::new(),
        timeout_ms,
        failure_policy: parse_failure_policy(handler),
        kind,
    });
}

// Split implementation lives in same-directory modules by responsibility
// (task-01 structure); public paths below are preserved via re-exports.
mod production_hooks_native;
mod production_hooks_frozen;
mod production_hooks_trace;

use self::production_hooks_native::{
    NativeAgentHook, NativeMcpHook, NativePromptHook, UnsupportedNativeHook,
};
pub use self::production_hooks_frozen::{
    FrozenHookDispatcher, freeze_run_hooks, frozen_dispatcher_for_run, resolve_frozen_dispatcher,
};
pub use self::production_hooks_trace::{trace_from_completed_event, trace_from_started_event};
#[cfg(test)]
use self::production_hooks_native::{prompt_decision, substitute_hook_input};
#[cfg(test)]
use self::production_hooks_trace::parse_error_category;

#[cfg(test)]
mod production_hooks_discovery_tests;
#[cfg(test)]
mod production_hooks_native_tests;
#[cfg(test)]
mod production_hooks_frozen_tests;
#[cfg(test)]
mod production_hooks_trace_tests;
