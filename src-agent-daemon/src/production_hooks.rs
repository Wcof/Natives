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

struct UnsupportedNativeHook;

#[async_trait::async_trait]
impl HookHandler for UnsupportedNativeHook {
    async fn handle(&self, _request: HookRequest) -> HookResponse {
        HookResponse {
            decision: HookDecision::Deny {
                reason: "unsupported Native Hook adapter".into(),
            },
        }
    }
}

struct NativeMcpHook {
    server_id: String,
    tool_name: String,
    input_template: Option<String>,
}

#[async_trait::async_trait]
impl HookHandler for NativeMcpHook {
    async fn handle(&self, request: HookRequest) -> HookResponse {
        self.handle_outcome(request).await.into_response()
    }

    async fn handle_outcome(&self, request: HookRequest) -> HookOutcome {
        let Some(run) = crate::global_run_manager().get_run(&request.run_id) else {
            return HookOutcome::Failed {
                reason: "parent Run not found".into(),
            };
        };
        let selected = run
            .capability_snapshot
            .as_ref()
            .and_then(|value| value.get("mcpServers"))
            .and_then(Value::as_array)
            .is_some_and(|servers| {
                servers
                    .iter()
                    .any(|id| id.as_str() == Some(&self.server_id))
            });
        if !selected {
            return HookOutcome::Failed {
                reason: format!(
                    "MCP server {} is not selected by the parent Run",
                    self.server_id
                ),
            };
        }
        let arguments = match self.input_template.as_deref() {
            Some(template) => match serde_json::from_str::<Value>(template) {
                Ok(mut value) => {
                    substitute_hook_input(&mut value, &request.input);
                    value
                }
                Err(error) => {
                    return HookOutcome::Failed {
                        reason: format!("invalid MCP input template: {error}"),
                    }
                }
            },
            None => request.input,
        };
        let cancel = match crate::global_run_manager()
            .runtime
            .ensure_execution_token(&request.run_id, run.parent_run_id.as_deref())
            .await
        {
            Ok(token) => token,
            Err(reason) => return HookOutcome::Failed { reason },
        };
        match crate::runtime::mcp_invocation::invoke_mcp_tool(
            &self.server_id,
            &self.tool_name,
            arguments,
            &cancel,
            Some(&request.run_id),
        )
        .await
        {
            Ok(_) => HookOutcome::Decided(HookResponse {
                decision: HookDecision::Allow,
            }),
            Err(reason) => HookOutcome::Failed { reason },
        }
    }
}

fn substitute_hook_input(value: &mut Value, input: &Value) {
    match value {
        Value::String(text) if text == "${input}" => *value = input.clone(),
        Value::Array(items) => {
            for item in items {
                substitute_hook_input(item, input);
            }
        }
        Value::Object(object) => {
            for item in object.values_mut() {
                substitute_hook_input(item, input);
            }
        }
        _ => {}
    }
}

struct NativePromptHook {
    template: String,
    model_override: Option<String>,
    timeout_ms: u64,
}

#[async_trait::async_trait]
impl HookHandler for NativePromptHook {
    async fn handle(&self, request: HookRequest) -> HookResponse {
        self.handle_outcome(request).await.into_response()
    }

    async fn handle_outcome(&self, request: HookRequest) -> HookOutcome {
        let Some(run) = crate::global_run_manager().get_run(&request.run_id) else {
            return HookOutcome::Failed {
                reason: "parent Run not found".into(),
            };
        };
        let controls = crate::production::run_request_controls(run.effort.as_deref());
        let provider = crate::routing::RoutedProvider::new(crate::routing::load_plan(
            run.provider_id,
            run.key_id,
            run.model_id.clone(),
        ))
        .with_controls(controls);
        let model = self.model_override.as_deref().unwrap_or(&run.model_id);
        let input = assistant_protocol::v2::redact_secrets(&request.input.to_string());
        let cancel = match crate::global_run_manager()
            .runtime
            .ensure_execution_token(&request.run_id, run.parent_run_id.as_deref())
            .await
        {
            Ok(token) => token,
            Err(reason) => return HookOutcome::Failed { reason },
        };
        let deadline =
            tokio::time::Instant::now() + std::time::Duration::from_millis(self.timeout_ms);
        let stream = provider.stream_turn(
            ProviderTurnRequest {
                context: EngineProviderContext {
                    run_id: request.run_id.clone(),
                    attempt: 0,
                },
                model: model.to_string(),
                system_prompt: Some(self.template.clone()),
                messages: vec![AgentMessage::User(UserMessage {
                    message_id: MessageId::new(),
                    content: vec![ContentBlock::Text { text: input }],
                })],
                tools: Vec::new(),
            },
            cancel,
        );
        let Ok(Ok(mut events)) = tokio::time::timeout_at(deadline, stream).await else {
            return HookOutcome::Failed {
                reason: "Prompt Hook provider request failed or timed out".into(),
            };
        };
        let mut text = String::new();
        loop {
            let event = tokio::select! {
                _ = tokio::time::sleep_until(deadline) => {
                    return HookOutcome::Failed { reason: "Prompt Hook provider stream timed out".into() };
                }
                event = events.next() => event,
            };
            let Some(event) = event else { break };
            match event {
                EngineProviderEvent::TextDelta(delta) => text.push_str(&delta),
                EngineProviderEvent::Error { message, .. } => {
                    return HookOutcome::Failed { reason: message }
                }
                EngineProviderEvent::Completed
                | EngineProviderEvent::CompletedWithReason { .. } => break,
                _ => {}
            }
        }
        prompt_decision(&text)
    }
}

fn prompt_decision(text: &str) -> HookOutcome {
    let parsed = serde_json::from_str::<Value>(text.trim()).ok();
    let decision = parsed
        .as_ref()
        .and_then(|value| value.get("decision"))
        .and_then(Value::as_str)
        .unwrap_or(text)
        .trim()
        .to_ascii_lowercase();
    let reason = parsed
        .as_ref()
        .and_then(|value| value.get("reason"))
        .and_then(Value::as_str)
        .unwrap_or("Prompt Hook decision")
        .to_string();
    match decision.as_str() {
        "allow" => HookOutcome::Decided(HookResponse {
            decision: HookDecision::Allow,
        }),
        "deny" => HookOutcome::Decided(HookResponse {
            decision: HookDecision::Deny { reason },
        }),
        _ => HookOutcome::Failed {
            reason: "Prompt Hook must return structured allow/deny decision".into(),
        },
    }
}

struct NativeAgentHook {
    prompt: String,
    model_override: Option<String>,
    max_steps: u32,
    readonly_tools: Vec<String>,
    timeout_ms: u64,
}

#[async_trait::async_trait]
impl HookHandler for NativeAgentHook {
    async fn handle(&self, request: HookRequest) -> HookResponse {
        self.handle_outcome(request).await.into_response()
    }

    async fn handle_outcome(&self, request: HookRequest) -> HookOutcome {
        let Some(parent) = crate::global_run_manager().get_run(&request.run_id) else {
            return HookOutcome::Failed {
                reason: "parent Run not found".into(),
            };
        };
        if parent.parent_run_id.is_some() {
            return HookOutcome::Failed {
                reason: "Agent Hook recursion depth limit exceeded".into(),
            };
        }
        let Some(key_id) = parent.key_id.clone() else {
            return HookOutcome::Failed {
                reason: "parent Run has no credential lease reference".into(),
            };
        };
        let model_id = self
            .model_override
            .clone()
            .unwrap_or(parent.model_id.clone());
        let binding = crate::subagent_store::RouteBinding {
            provider_id: parent.provider_id.clone(),
            key_id: key_id.clone(),
            model_id: model_id.clone(),
        };
        let input = assistant_protocol::v2::redact_secrets(&request.input.to_string());
        let task = format!("Harness hook event:\n{input}");
        let (session_id, conversation_id) = match crate::subagent_store::create_hidden_child_session(
            &parent.conversation_id,
            Some(&parent.id),
            None,
            "Harness Agent Hook",
            &task,
            &binding,
            Some("readonly"),
            parent.project_id.as_deref(),
        ) {
            Ok(value) => value,
            Err(reason) => return HookOutcome::Failed { reason },
        };
        let created =
            match crate::global_run_manager().create_run(assistant_protocol::v2::CreateRunRequest {
                capability_selection: None,
                disabled_tools: None,
                conversation_id,
                provider_id: parent.provider_id,
                model_id,
                key_id: Some(key_id),
                agent_profile_id: None,
                permission_profile: Some("readonly".into()),
                content: Some(task),
                attachments: None,
                max_steps: Some(self.max_steps.clamp(1, 32)),
                parent_run_id: Some(parent.id.clone()),
                project_path: parent.project_path.clone(),
                idempotency_key: None,
                effort: parent.effort,
                runtime_id: Some("native".into()),
            }) {
                Ok(run) => run,
                Err(reason) => {
                    let _ = crate::subagent_store::close_subagent_session(
                        &session_id,
                        "failed",
                        Some(&reason),
                    );
                    return HookOutcome::Failed { reason };
                }
            };
        crate::global_run_manager()
            .runtime
            .set_run_tool_allowlist(&created.id, self.readonly_tools.clone())
            .await;
        crate::global_run_manager()
            .runtime
            .set_run_agent_directive(&created.id, self.prompt.clone())
            .await;
        crate::global_run_manager().runtime.events.append(
            &parent.id,
            RunEventKind::SubagentCreated {
                sub_run_id: created.id.clone(),
                agent_profile_id: None,
                task: "Harness Agent Hook".into(),
            },
        );
        if let Err(reason) = crate::run_manager::RunManager::start_detached_global(
            assistant_protocol::v2::StartRunRequest {
                run_id: Some(created.id.clone()),
                conversation_id: Some(created.conversation_id.clone()),
                provider_id: Some(created.provider_id.clone()),
                model_id: Some(created.model_id.clone()),
                key_id: created.key_id.clone(),
                content: None,
                attachments: None,
                trigger_message_id: None,
                permission_profile: Some("readonly".into()),
                max_steps: Some(created.max_steps),
                project_path: created.project_path.clone(),
                idempotency_key: None,
                effort: created.effort.clone(),
                runtime_id: Some("native".into()),
                agent_profile_id: None,
                capability_selection: None,
            },
        ) {
            let _ =
                crate::subagent_store::close_subagent_session(&session_id, "failed", Some(&reason));
            return HookOutcome::Failed { reason };
        }
        let deadline =
            tokio::time::Instant::now() + std::time::Duration::from_millis(self.timeout_ms);
        loop {
            if tokio::time::Instant::now() >= deadline {
                let _ = crate::global_run_manager()
                    .cancel(assistant_protocol::v2::CancelRunRequest {
                        run_id: created.id.clone(),
                    })
                    .await;
                let _ = crate::subagent_store::close_subagent_session(
                    &session_id,
                    "failed",
                    Some("Agent Hook child Run timed out"),
                );
                return HookOutcome::Failed {
                    reason: "Agent Hook child Run timed out".into(),
                };
            }
            if let Some(run) = crate::global_run_manager().get_run(&created.id) {
                if run.status.is_terminal() {
                    let status = run.status.as_str();
                    let _ = crate::subagent_store::close_subagent_session(
                        &session_id,
                        status,
                        run.error_code.as_deref(),
                    );
                    return if status == "completed" {
                        HookOutcome::Decided(HookResponse {
                            decision: HookDecision::Allow,
                        })
                    } else {
                        HookOutcome::Failed {
                            reason: format!("Agent Hook child Run ended with {status}"),
                        }
                    };
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
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

// ── Run-level frozen Hook Dispatcher (NE-P0-05) ──────────────────────────────

/// A Run's Hook plan, frozen at Run start and shared by every dispatch path.
///
/// One Run resolves and compiles its [`HookRegistry`] once; this value makes
/// that registry available, read-only, to every dispatch site in the process —
/// the engine loop, the permission gate, the notification hook, and the
/// subagent lifecycle hooks. A `hooks.json` edit made while the Run is in
/// flight never reaches this dispatcher, because discovery and resolution
/// happened at Run start. Mid-Run Hook modifications therefore only affect the
/// *next* Run, which resolves again and freezes a new plan.
///
/// Dispatch is a thin delegation to the underlying registry, which already
/// owns the `HookInvocationStarted` / `HookInvocationCompleted` telemetry
/// through the [`EventSequencer`] attached at freeze time. There is
/// deliberately no second execution engine and no second trace authority here:
/// the run event sequencer stays the single durable source for Hook traces.
#[derive(Clone)]
pub struct FrozenHookDispatcher {
    run_id: String,
    plan_hash: String,
    registry: Arc<HookRegistry>,
}

impl FrozenHookDispatcher {
    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    /// The frozen plan hash this dispatcher was bound to at Run start. In the
    /// production seam this is the Run snapshot's canonical hash — the same
    /// value the persisted snapshot, the compiled Provider prompt, and the
    /// HookInvocation trace metadata all reference.
    pub fn plan_hash(&self) -> &str {
        &self.plan_hash
    }

    /// Every Hook in the frozen registry, canonical event order then dispatch
    /// order. This is the catalog the control plane renders, so what a user
    /// sees a Run "attached" is provably what it dispatches.
    pub fn describe(&self) -> Vec<HookDefinition> {
        self.registry.describe()
    }

    pub fn describe_event(&self, event: HookEvent) -> Vec<HookDefinition> {
        self.registry.describe_event(event)
    }

    pub fn events_covered(&self) -> Vec<HookEvent> {
        self.registry.events_covered()
    }

    pub fn fail_closed_security(&self) -> bool {
        self.registry.fail_closed_security
    }

    /// Dispatch, resolving each Hook's failure through its
    /// [`HookFailurePolicy`]. Security-sensitive events stay fail-closed
    /// regardless of policy.
    ///
    /// HookInvocationStarted/Completed telemetry flows through the same
    /// [`EventSequencer`] the registry was frozen with — a single event source.
    pub async fn dispatch_outcomes(&self, request: HookRequest) -> Vec<HookOutcome> {
        self.registry.dispatch_outcomes(request).await
    }

    pub async fn dispatch(&self, request: HookRequest) -> Vec<HookResponse> {
        self.registry.dispatch(request).await
    }

    pub async fn observe(&self, request: HookRequest) -> Result<(), agent_core::ObserveResult> {
        self.registry.observe(request).await
    }

    /// Fold Hook outcomes into a single permission verdict, fail-closed.
    pub fn aggregate_permission(outcomes: &[HookOutcome]) -> agent_core::PermissionAggregate {
        HookRegistry::aggregate_permission(outcomes)
    }

    /// Aggregate allow/deny for a subagent gate: any Deny wins.
    pub fn aggregate_allow(responses: &[HookResponse]) -> Result<(), String> {
        HookRegistry::aggregate_allow(responses)
    }
}

/// Process-wide, Run-scoped frozen dispatchers, keyed by `run_id`.
///
/// Two writers converge on this table: the Run-start seam
/// ([`freeze_run_hooks`]) and the lazy fallback ([`resolve_frozen_dispatcher`]).
/// Insertion is idempotent — the first registration for a `run_id` wins and
/// later calls are no-ops — which is exactly what makes a dispatcher frozen:
/// a mid-Run `hooks.json` change can never replace it.
fn frozen_dispatchers() -> &'static Mutex<HashMap<String, FrozenHookDispatcher>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, FrozenHookDispatcher>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Freeze a Run's compiled registry as its shared read-only dispatcher.
///
/// Idempotent per `run_id`: the first freeze wins, so a second call with a
/// registry compiled from a mid-Run configuration change is ignored.
///
/// `events` is attached to the registry at freeze time so every dispatch from
/// the tool paths emits HookInvocation telemetry through the same
/// [`EventSequencer`] the engine uses.
pub fn freeze_run_hooks(
    run_id: &str,
    plan_hash: &str,
    registry: HookRegistry,
    events: EventSequencer,
) -> FrozenHookDispatcher {
    let mut map = frozen_dispatchers()
        .lock()
        .expect("frozen hook dispatcher registry poisoned");
    if let Some(existing) = map.get(run_id) {
        return existing.clone();
    }
    let frozen = FrozenHookDispatcher {
        run_id: run_id.to_string(),
        plan_hash: plan_hash.to_string(),
        registry: Arc::new(registry.with_events(events)),
    };
    map.insert(run_id.to_string(), frozen.clone());
    frozen
}

/// The Run's frozen dispatcher, when the Run has been frozen.
pub fn frozen_dispatcher_for_run(run_id: &str) -> Option<FrozenHookDispatcher> {
    frozen_dispatchers()
        .lock()
        .ok()
        .and_then(|map| map.get(run_id).cloned())
}

/// Resolve the Run's frozen dispatcher, compiling once when it is not frozen.
///
/// This is the seam the permission and notification tool paths call. It is the
/// single source of truth for the whole process: the first call for a `run_id`
/// compiles the HookRegistry (discovery + resolution) exactly once and caches
/// it; every later call — however many tool invocations, subagent spawns, or
/// notifications — reuses that same frozen registry and never re-scans
/// `hooks.json`.
///
/// The plan hash prefers the Run snapshot's canonical hash, so the Dispatcher,
/// the Snapshot, the Provider Prompt and the HookInvocation trace all agree on
/// the same plan. When no snapshot is persisted (hermetic tests, pre-seam
/// runs) it falls back to a deterministic hash of the compiled definitions.
pub fn resolve_frozen_dispatcher(
    run_id: &str,
    events: EventSequencer,
    project: Option<&Path>,
) -> FrozenHookDispatcher {
    if let Some(existing) = frozen_dispatcher_for_run(run_id) {
        return existing;
    }
    let definitions = discover_production_hooks(project);
    let plan_hash =
        snapshot_plan_hash_for_run(run_id).unwrap_or_else(|| definitions_plan_hash(&definitions));
    let registry = compile_production_hooks(&definitions, project);
    freeze_run_hooks(run_id, &plan_hash, registry, events)
}

/// Best-effort canonical plan hash from the Run's persisted Harness snapshot.
///
/// Same source as `harness.run.getSnapshot`: the snapshot's canonical hash.
/// Any persistence or DB failure degrades to `None`; the caller then uses the
/// deterministic definitions hash instead.
///
/// Test builds never touch the harness store: hermetic tests have no snapshot
/// row and must not depend on ambient DB env vars or the store lock.
#[cfg(not(test))]
fn snapshot_plan_hash_for_run(run_id: &str) -> Option<String> {
    crate::rpc::harness::repository::with_conn(|conn| {
        Ok(
            crate::rpc::harness::repository::get_run_snapshot(conn, run_id)?
                .map(|snapshot| snapshot.canonical_hash()),
        )
    })
    .ok()
    .flatten()
}

#[cfg(test)]
fn snapshot_plan_hash_for_run(_run_id: &str) -> Option<String> {
    None
}

/// Deterministic plan hash for a definition set — the lazy fallback used when
/// no Run snapshot is persisted.
fn definitions_plan_hash(definitions: &[HookDefinition]) -> String {
    harness_core::sha256_hex(&serde_json::to_string(definitions).unwrap_or_default())
}

/// Map a `HookInvocationStarted` run event into the [`HookInvocationTrace`]
/// metadata model.
///
/// This is the daemon side of the contract the model specifies in
/// `harness-core`: the single mapping between the durable event payload and
/// the model, so a trace can be attributed to the frozen plan it ran under.
pub fn trace_from_started_event(
    event: &assistant_protocol::v2::RunEventV2,
    plan_hash: &str,
) -> Option<HookInvocationTrace> {
    let assistant_protocol::v2::RunEventKind::HookInvocationStarted {
        invocation_id,
        hook_id,
        hook_event,
        source,
        ordinal,
        input_summary,
        input_truncated,
    } = &event.payload
    else {
        return None;
    };
    Some(HookInvocationTrace::started(
        event.run_id.clone(),
        plan_hash,
        invocation_id.clone(),
        hook_id.clone(),
        hook_event.clone(),
        source.clone(),
        *ordinal,
        input_summary.clone(),
        *input_truncated,
    ))
}

/// Map a `HookInvocationCompleted` run event into the [`HookInvocationTrace`]
/// metadata model.
pub fn trace_from_completed_event(
    event: &assistant_protocol::v2::RunEventV2,
    plan_hash: &str,
) -> Option<HookInvocationTrace> {
    let assistant_protocol::v2::RunEventKind::HookInvocationCompleted {
        invocation_id,
        hook_id,
        hook_event,
        source,
        ordinal,
        effective_decision,
        error_category,
        duration_ms,
        output_summary,
        output_truncated,
        ..
    } = &event.payload
    else {
        return None;
    };
    Some(
        HookInvocationTrace::started(
            event.run_id.clone(),
            plan_hash,
            invocation_id.clone(),
            hook_id.clone(),
            hook_event.clone(),
            source.clone(),
            *ordinal,
            String::new(),
            false,
        )
        .completed(
            *duration_ms,
            effective_decision.clone(),
            error_category.as_deref().and_then(parse_error_category),
            output_summary.clone(),
            *output_truncated,
        ),
    )
}

/// Map the registry's error-category string onto the structured model enum.
fn parse_error_category(category: &str) -> Option<HookErrorCategory> {
    match category {
        "handler_failure" => Some(HookErrorCategory::HandlerFailure),
        "spawn_failure" => Some(HookErrorCategory::SpawnFailure),
        "timeout" => Some(HookErrorCategory::Timeout),
        "persistence_failure" => Some(HookErrorCategory::PersistenceFailure),
        "policy_resolution" => Some(HookErrorCategory::PolicyResolution),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{HookDecision, HookRegistry, HookRequest, HookResponse};
    use harness_core::hooks::HookInvocationStatus;
    // Only the assertions need a concrete `Duration`; production code goes
    // through `HookDefinition::timeout()`.
    use std::time::Duration;

    /// Behaviour snapshot for the pre-`discover`/`compile` split (task T0).
    ///
    /// The registry stores opaque `Box<dyn HookHandler>`, so these tests pin
    /// observable dispatch behaviour rather than internal structure. The
    /// refactor must keep every assertion below byte-identical.
    const ALL_EVENTS: [HookEvent; 16] = [
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
    ];

    /// A loopback URL is rejected by `validate_http_hook_url` before any socket
    /// is opened, so an `http` hook is a fast, hermetic handler-counting probe.
    const PROBE_URL: &str = "http://127.0.0.1:1/hook";

    struct TempProject {
        root: std::path::PathBuf,
    }

    impl TempProject {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("natives-hooks-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&root).unwrap();
            Self { root }
        }

        fn write(&self, rel: &str, body: &str) -> &Self {
            let path = self.root.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
            self
        }

        fn hooks(&self) -> HookRegistry {
            build_production_hooks_for_project(Some(&self.root))
        }
    }

    impl Drop for TempProject {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    async fn dispatch(
        hooks: &HookRegistry,
        event: HookEvent,
        tool: Option<&str>,
    ) -> Vec<HookResponse> {
        hooks
            .dispatch(HookRequest {
                event,
                run_id: "run".into(),
                tool_name: tool.map(str::to_string),
                input: serde_json::json!({}),
            })
            .await
    }

    /// `build_production_hooks_for_project` reads two ambient env vars. Tests
    /// must account for them rather than assume a clean environment.
    fn env_hook_count(var: &str) -> usize {
        usize::from(
            std::env::var(var)
                .ok()
                .is_some_and(|v| !v.trim().is_empty()),
        )
    }

    fn probe_group(event: &str, command: &str) -> String {
        format!(r#"{{"hooks":{{"{event}":[{{"hooks":[{{"type":"http","url":"{command}"}}]}}]}}}}"#)
    }

    #[tokio::test]
    async fn builtin_defaults_allow_every_event_without_project_hooks() {
        let hooks = build_production_hooks_for_project(None);
        for event in ALL_EVENTS {
            let extra = match event {
                HookEvent::PreToolUse => env_hook_count("NATIVES_HOOK_CMD"),
                HookEvent::PostToolUse => env_hook_count("NATIVES_HOOK_HTTP"),
                _ => 0,
            };
            let responses = dispatch(&hooks, event, Some("read_file")).await;
            assert_eq!(
                responses.len(),
                1 + extra,
                "event {event:?} should have exactly one builtin default handler"
            );
            assert!(
                matches!(responses[0].decision, HookDecision::Allow),
                "event {event:?} builtin default must Allow"
            );
        }
    }

    #[tokio::test]
    async fn security_fail_closed_is_enabled_after_build() {
        assert!(build_production_hooks_for_project(None).fail_closed_security);
    }

    #[tokio::test]
    async fn builtin_default_dispatches_before_project_hooks() {
        let project = TempProject::new();
        project.write(".claude/hooks.json", &probe_group("PreToolUse", PROBE_URL));
        let responses = dispatch(&project.hooks(), HookEvent::PreToolUse, Some("read_file")).await;

        assert_eq!(responses.len(), 2 + env_hook_count("NATIVES_HOOK_CMD"));
        assert!(
            matches!(responses[0].decision, HookDecision::Allow),
            "builtin allow-all must be registered before project hooks"
        );
        assert!(matches!(responses[1].decision, HookDecision::Deny { .. }));
    }

    #[tokio::test]
    async fn every_candidate_project_file_is_loaded() {
        let project = TempProject::new();
        for rel in [
            ".claude/hooks.json",
            ".claude/settings.json",
            ".claude/settings.local.json",
            ".agents/hooks.json",
            ".agents/settings.json",
            ".grok/hooks.json",
            ".natives/hooks.json",
        ] {
            project.write(rel, &probe_group("Notification", PROBE_URL));
        }
        let responses = dispatch(&project.hooks(), HookEvent::Notification, None).await;
        assert_eq!(
            responses.len(),
            8,
            "one builtin default plus seven candidate project files"
        );
    }

    #[tokio::test]
    async fn unknown_event_names_are_ignored() {
        let project = TempProject::new();
        project.write(
            ".natives/hooks.json",
            &probe_group("BogusEventName", PROBE_URL),
        );
        for event in ALL_EVENTS {
            let extra = match event {
                HookEvent::PreToolUse => env_hook_count("NATIVES_HOOK_CMD"),
                HookEvent::PostToolUse => env_hook_count("NATIVES_HOOK_HTTP"),
                _ => 0,
            };
            assert_eq!(
                dispatch(&project.hooks(), event, None).await.len(),
                1 + extra,
                "unknown event name must not register a handler on {event:?}"
            );
        }
    }

    #[tokio::test]
    async fn snake_case_event_aliases_are_accepted() {
        let project = TempProject::new();
        project.write(
            ".natives/hooks.json",
            &probe_group("pre_compact", PROBE_URL),
        );
        assert_eq!(
            dispatch(&project.hooks(), HookEvent::PreCompact, None)
                .await
                .len(),
            2
        );
    }

    #[tokio::test]
    async fn bare_object_without_hooks_wrapper_is_parsed() {
        let project = TempProject::new();
        project.write(
            ".natives/hooks.json",
            &format!(r#"{{"Notification":[{{"hooks":[{{"type":"http","url":"{PROBE_URL}"}}]}}]}}"#),
        );
        assert_eq!(
            dispatch(&project.hooks(), HookEvent::Notification, None)
                .await
                .len(),
            2
        );
    }

    #[tokio::test]
    async fn group_without_inner_hooks_array_is_a_single_handler() {
        let project = TempProject::new();
        project.write(
            ".natives/hooks.json",
            &format!(r#"{{"hooks":{{"Notification":[{{"type":"http","url":"{PROBE_URL}"}}]}}}}"#),
        );
        assert_eq!(
            dispatch(&project.hooks(), HookEvent::Notification, None)
                .await
                .len(),
            2
        );
    }

    #[tokio::test]
    async fn malformed_and_incomplete_entries_are_skipped() {
        let project = TempProject::new();
        project
            .write(".claude/hooks.json", "{ not json at all")
            .write(
                ".claude/settings.json",
                r#"{"hooks":{"Notification":[{"hooks":[{"type":"command"}]}]}}"#,
            )
            .write(
                ".agents/hooks.json",
                r#"{"hooks":{"Notification":[{"hooks":[{"type":"command","command":"   "}]}]}}"#,
            )
            .write(
                ".grok/hooks.json",
                r#"{"hooks":{"Notification":[{"hooks":[{"type":"http"}]}]}}"#,
            )
            .write(
                ".natives/hooks.json",
                r#"{"hooks":{"Notification":[{"hooks":[{"type":"websocket","url":"ws://x"}]}]}}"#,
            );
        assert_eq!(
            dispatch(&project.hooks(), HookEvent::Notification, None)
                .await
                .len(),
            1,
            "invalid JSON, missing command, blank command, missing url, and \
             unknown handler type must all be skipped"
        );
    }

    #[tokio::test]
    async fn matcher_scopes_handlers_to_tool_names() {
        let project = TempProject::new();
        project.write(
            ".natives/hooks.json",
            &format!(
                r#"{{"hooks":{{"Notification":[
                    {{"matcher":"Bash|run_*","hooks":[{{"type":"http","url":"{PROBE_URL}"}}]}},
                    {{"matcher":"*","hooks":[{{"type":"http","url":"{PROBE_URL}"}}]}}
                ]}}}}"#
            ),
        );
        let hooks = project.hooks();
        assert_eq!(
            dispatch(&hooks, HookEvent::Notification, Some("Bash"))
                .await
                .len(),
            3
        );
        assert_eq!(
            dispatch(&hooks, HookEvent::Notification, Some("run_command"))
                .await
                .len(),
            3
        );
        assert_eq!(
            dispatch(&hooks, HookEvent::Notification, Some("read_file"))
                .await
                .len(),
            2,
            "only the wildcard matcher applies to an unmatched tool"
        );
    }

    // ── Discovery layer (task T3) ──────────────────────────────────────────
    //
    // These assert facts the pre-split code could not express at all, because a
    // registered Hook was an opaque `Box<dyn HookHandler>`.

    fn definitions_from(project: &TempProject) -> Vec<HookDefinition> {
        discover_production_hooks(Some(&project.root))
    }

    fn file_definitions(project: &TempProject) -> Vec<HookDefinition> {
        definitions_from(project)
            .into_iter()
            .filter(|d| d.source.scope == HookScope::Project)
            .collect()
    }

    #[test]
    fn discovery_yields_one_builtin_default_per_event_first() {
        let defs = discover_production_hooks(None);
        let builtins: Vec<_> = defs
            .iter()
            .filter(|d| d.source.scope == HookScope::Builtin)
            .collect();

        assert_eq!(builtins.len(), 16);
        assert_eq!(
            builtins.iter().map(|d| d.event).collect::<Vec<_>>(),
            HookEvent::ALL.to_vec(),
            "builtin defaults must be discovered in canonical event order"
        );
        for def in builtins {
            assert_eq!(def.order, 0, "builtin default must dispatch first");
            assert_eq!(
                def.id.as_str(),
                format!("builtin/allow-all#{}", def.event.as_str())
            );
        }
    }

    #[test]
    fn discovery_records_file_group_and_entry_provenance() {
        let project = TempProject::new();
        project.write(
            ".claude/settings.json",
            &format!(
                r#"{{"hooks":{{"PostToolUse":[
                    {{"matcher":"Edit","hooks":[
                        {{"type":"http","url":"{PROBE_URL}"}},
                        {{"type":"http","url":"{PROBE_URL}"}}
                    ]}},
                    {{"matcher":"Write","hooks":[{{"type":"http","url":"{PROBE_URL}"}}]}}
                ]}}}}"#
            ),
        );
        let defs = file_definitions(&project);

        assert_eq!(defs.len(), 3);
        assert_eq!(
            defs.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(),
            vec![
                "project/.claude/settings.json#PostToolUse[0]/0",
                "project/.claude/settings.json#PostToolUse[0]/1",
                "project/.claude/settings.json#PostToolUse[1]/0",
            ]
        );
        assert_eq!(
            defs.iter()
                .map(|d| d.matcher.as_deref())
                .collect::<Vec<_>>(),
            vec![Some("Edit"), Some("Edit"), Some("Write")]
        );
        // The builtin default already claimed ordinal 0 for this event.
        assert_eq!(
            defs.iter().map(|d| d.order).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
    }

    #[test]
    fn timeout_is_clamped_between_one_and_six_hundred_seconds() {
        let project = TempProject::new();
        project.write(
            ".natives/hooks.json",
            &format!(
                r#"{{"hooks":{{"Notification":[{{"hooks":[
                    {{"type":"http","url":"{PROBE_URL}","timeout":0}},
                    {{"type":"http","url":"{PROBE_URL}","timeout":9999}},
                    {{"type":"http","url":"{PROBE_URL}","timeout":30}},
                    {{"type":"http","url":"{PROBE_URL}"}}
                ]}}]}}}}"#
            ),
        );
        assert_eq!(
            file_definitions(&project)
                .iter()
                .map(|d| d.timeout_ms)
                .collect::<Vec<_>>(),
            vec![1_000, 600_000, 30_000, 10_000],
            "clamp to [1s, 600s] with a 10s default"
        );
    }

    #[test]
    fn command_hooks_are_shell_wrapped_unless_args_are_explicit() {
        let project = TempProject::new();
        project.write(
            ".natives/hooks.json",
            r#"{"hooks":{"Notification":[{"hooks":[
                {"type":"command","command":"echo hi"},
                {"type":"command","command":"/usr/bin/echo","args":["hi"]}
            ]}]}}"#,
        );
        let defs = file_definitions(&project);
        assert_eq!(defs.len(), 2);

        let expected_wrapper: (&str, Vec<String>) = if cfg!(windows) {
            ("cmd.exe", vec!["/C".into(), "echo hi".into()])
        } else {
            ("/bin/sh", vec!["-lc".into(), "echo hi".into()])
        };
        assert_eq!(
            defs[0].kind,
            HookKind::Command {
                program: expected_wrapper.0.to_string(),
                args: expected_wrapper.1,
                trusted: true,
            }
        );
        assert_eq!(
            defs[1].kind,
            HookKind::Command {
                program: "/usr/bin/echo".into(),
                args: vec!["hi".into()],
                trusted: true,
            }
        );
    }

    #[test]
    fn discovery_covers_all_seven_project_candidates_in_load_order() {
        let project = TempProject::new();
        for [dir, file] in PROJECT_CANDIDATES {
            project.write(
                &format!("{dir}/{file}"),
                &probe_group("Notification", PROBE_URL),
            );
        }
        assert_eq!(
            file_definitions(&project)
                .iter()
                .map(|d| d.source.origin.clone())
                .collect::<Vec<_>>(),
            PROJECT_CANDIDATES
                .iter()
                .map(|[dir, file]| format!("{dir}/{file}"))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn every_discovered_hook_has_a_unique_identity() {
        let project = TempProject::new();
        for [dir, file] in PROJECT_CANDIDATES {
            project.write(
                &format!("{dir}/{file}"),
                &format!(
                    r#"{{"hooks":{{"Notification":[{{"hooks":[
                        {{"type":"http","url":"{PROBE_URL}"}},
                        {{"type":"http","url":"{PROBE_URL}"}}
                    ]}}]}}}}"#
                ),
            );
        }
        let ids: Vec<_> = definitions_from(&project)
            .into_iter()
            .map(|d| d.id)
            .collect();
        let unique: std::collections::BTreeSet<_> = ids.iter().cloned().collect();
        assert_eq!(ids.len(), unique.len(), "hook identities must not collide");
    }

    #[tokio::test]
    async fn discovery_order_is_dispatch_order() {
        let project = TempProject::new();
        project.write(
            ".natives/hooks.json",
            &format!(
                r#"{{"hooks":{{"Notification":[{{"hooks":[
                    {{"type":"http","url":"{PROBE_URL}"}},
                    {{"type":"http","url":"ftp://example.test/hook"}}
                ]}}]}}}}"#
            ),
        );
        let defs = definitions_from(&project);
        let notification: Vec<_> = defs
            .iter()
            .filter(|d| d.event == HookEvent::Notification)
            .collect();
        assert_eq!(notification.len(), 3);

        let responses = dispatch(
            &compile_production_hooks(&defs, Some(&project.root)),
            HookEvent::Notification,
            None,
        )
        .await;
        assert_eq!(responses.len(), notification.len());
        assert!(matches!(responses[0].decision, HookDecision::Allow));
        // Distinct denial reasons prove the two file hooks kept their order.
        let reason_of = |i: usize| match &responses[i].decision {
            HookDecision::Deny { reason } => reason.clone(),
            other => panic!("expected Deny, got {other:?}"),
        };
        assert!(reason_of(1).contains("private/loopback"));
        assert!(reason_of(2).contains("only http/https"));
    }

    #[tokio::test]
    async fn compiling_discovered_definitions_matches_the_direct_build() {
        let project = TempProject::new();
        project
            .write(".claude/hooks.json", &probe_group("PreToolUse", PROBE_URL))
            .write(
                ".natives/hooks.json",
                &probe_group("Notification", PROBE_URL),
            );

        let direct = build_production_hooks_for_project(Some(&project.root));
        let composed = compile_production_hooks(&definitions_from(&project), Some(&project.root));

        for event in HookEvent::ALL {
            assert_eq!(
                dispatch(&direct, event, Some("Bash")).await.len(),
                dispatch(&composed, event, Some("Bash")).await.len(),
                "handler count diverged on {event:?}"
            );
        }
        assert_eq!(direct.fail_closed_security, composed.fail_closed_security);
    }

    #[test]
    fn unknown_builtin_names_compile_to_nothing_rather_than_panicking() {
        let source = HookSource::builtin("from-a-newer-daemon");
        let definition = HookDefinition {
            id: HookId::new(&source, HookEvent::Notification),
            event: HookEvent::Notification,
            source,
            order: 0,
            matcher: None,
            conditions: Vec::new(),
            timeout_ms: 0,
            failure_policy: HookFailurePolicy::Fail,
            kind: HookKind::Builtin {
                name: "from-a-newer-daemon".into(),
            },
        };
        let hooks = compile_production_hooks(&[definition], None);
        assert!(hooks.fail_closed_security);
    }

    /// Phase 1 acceptance: a built registry can answer, for any project, which
    /// Hooks are attached, where each came from, in what order, what it matches,
    /// how long it may run, and whether it is trusted.
    #[test]
    fn built_registry_answers_the_full_provenance_question() {
        let project = TempProject::new();
        project
            .write(
                ".claude/settings.json",
                r#"{"hooks":{"PreToolUse":[{"matcher":"Bash|run_*","hooks":[
                    {"type":"command","command":"./scripts/audit.sh","timeout":45}
                ]}]}}"#,
            )
            .write(
                ".natives/hooks.json",
                &format!(
                    r#"{{"hooks":{{"PreToolUse":[{{"hooks":[
                        {{"type":"http","url":"{PROBE_URL}"}}
                    ]}}]}}}}"#
                ),
            );

        let described = build_production_hooks_for_project(Some(&project.root))
            .describe_event(HookEvent::PreToolUse);

        let env_extra = env_hook_count("NATIVES_HOOK_CMD");
        assert_eq!(described.len(), 3 + env_extra);

        // 1. builtin default, first
        assert_eq!(described[0].source.scope, HookScope::Builtin);
        assert_eq!(described[0].order, 0);

        // 2. the project command hook, with file, group, and entry provenance
        let audit = &described[1];
        assert_eq!(
            audit.id.as_str(),
            "project/.claude/settings.json#PreToolUse[0]/0"
        );
        assert_eq!(audit.source.scope, HookScope::Project);
        assert_eq!(audit.source.origin, ".claude/settings.json");
        assert_eq!(audit.source.group_index, Some(0));
        assert_eq!(audit.source.entry_index, Some(0));
        assert_eq!(audit.order, 1);
        assert_eq!(audit.matcher.as_deref(), Some("Bash|run_*"));
        assert_eq!(audit.timeout(), Duration::from_secs(45));
        assert_eq!(audit.failure_policy, HookFailurePolicy::Fail);
        assert!(
            matches!(&audit.kind, HookKind::Command { trusted, .. } if *trusted),
            "project command hooks are trusted"
        );

        // 3. the second file's http hook, ordered after it
        assert_eq!(described[2].source.origin, ".natives/hooks.json");
        assert_eq!(described[2].order, 2);
        assert!(matches!(described[2].kind, HookKind::Http { .. }));
    }

    #[test]
    fn describe_covers_every_event_that_has_a_handler() {
        let hooks = build_production_hooks_for_project(None);
        let described = hooks.describe();
        assert_eq!(
            described.iter().map(|d| d.event).collect::<Vec<_>>(),
            HookEvent::ALL.to_vec(),
            "the sixteen builtin defaults must all be describable"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn loads_claude_settings_matchers_and_permission_request() {
        let project = std::env::temp_dir().join(format!("natives-hooks-{}", uuid::Uuid::new_v4()));
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

    #[test]
    fn mcp_template_substitutes_structured_event_input() {
        let mut value = serde_json::json!({"payload": "${input}", "literal": "keep"});
        substitute_hook_input(&mut value, &serde_json::json!({"command": "cargo test"}));
        assert_eq!(value["payload"]["command"], "cargo test");
        assert_eq!(value["literal"], "keep");
    }

    #[test]
    fn prompt_hook_requires_a_structured_allow_or_deny() {
        assert!(matches!(
            prompt_decision(r#"{"decision":"allow","reason":"safe"}"#),
            HookOutcome::Decided(HookResponse {
                decision: HookDecision::Allow
            })
        ));
        assert!(matches!(
            prompt_decision(r#"{"decision":"deny","reason":"unsafe"}"#),
            HookOutcome::Decided(HookResponse {
                decision: HookDecision::Deny { .. }
            })
        ));
        assert!(matches!(
            prompt_decision("maybe"),
            HookOutcome::Failed { .. }
        ));
    }

    // ── Run-level frozen Hook Dispatcher (NE-P0-05) ─────────────────────────

    /// A Hook that always fails, so failure-policy resolution can be exercised
    /// without spawning processes or opening sockets.
    struct FixedFailHook;

    #[async_trait::async_trait]
    impl HookHandler for FixedFailHook {
        async fn handle(&self, _request: HookRequest) -> HookResponse {
            HookOutcome::Failed {
                reason: "boom".into(),
            }
            .into_response()
        }

        async fn handle_outcome(&self, _request: HookRequest) -> HookOutcome {
            HookOutcome::Failed {
                reason: "boom".into(),
            }
        }
    }

    fn failing_registry(event: HookEvent, policy: HookFailurePolicy) -> HookRegistry {
        let mut registry = HookRegistry::new();
        registry.enable_security_fail_closed();
        let source = HookSource::builtin("fixed-fail");
        let definition = HookDefinition {
            id: HookId::new(&source, event),
            event,
            source,
            order: 0,
            matcher: None,
            conditions: Vec::new(),
            timeout_ms: 0,
            failure_policy: policy,
            kind: HookKind::Builtin {
                name: "fixed-fail".into(),
            },
        };
        registry.register_defined(definition, Box::new(FixedFailHook));
        registry
    }

    /// The freeze is idempotent per run: the first registration wins and a
    /// later registration compiled from a mid-Run change is ignored.
    #[test]
    fn frozen_dispatcher_freezes_a_run_and_ignores_later_registrations() {
        let project = TempProject::new();
        project.write(
            ".natives/hooks.json",
            &probe_group("Notification", PROBE_URL),
        );
        let events = EventSequencer::memory_only();

        let first = freeze_run_hooks("run-freeze", "plan-v1", project.hooks(), events.clone());
        assert_eq!(first.plan_hash(), "plan-v1");
        assert_eq!(
            frozen_dispatcher_for_run("run-freeze")
                .expect("run frozen")
                .plan_hash(),
            "plan-v1"
        );

        let second = freeze_run_hooks("run-freeze", "plan-v2", project.hooks(), events);
        assert_eq!(
            second.plan_hash(),
            "plan-v1",
            "a second freeze for the same run must be a no-op"
        );
        assert_eq!(
            frozen_dispatcher_for_run("run-freeze")
                .expect("run frozen")
                .plan_hash(),
            "plan-v1"
        );
    }

    /// Every dispatched Hook emits exactly one HookInvocationStarted and one
    /// HookInvocationCompleted, through the same EventSequencer the registry
    /// was frozen with — a single event source, and no dangling Started.
    #[tokio::test]
    async fn frozen_dispatcher_emits_started_and_completed_trace_through_event_sequencer() {
        let project = TempProject::new();
        project.write(
            ".natives/hooks.json",
            &probe_group("Notification", PROBE_URL),
        );
        let events = EventSequencer::memory_only();
        let frozen = freeze_run_hooks("run-trace", "plan-trace", project.hooks(), events.clone());

        let responses = frozen
            .dispatch(HookRequest {
                event: HookEvent::Notification,
                run_id: "run-trace".into(),
                tool_name: None,
                input: serde_json::json!({}),
            })
            .await;
        // allow-all builtin + the loopback-rejected probe both dispatch.
        assert_eq!(responses.len(), 2);

        let replayed = events.replay_after("run-trace", 0);
        let started: Vec<_> = replayed
            .iter()
            .filter(|e| matches!(&e.payload, RunEventKind::HookInvocationStarted { .. }))
            .collect();
        let completed: Vec<_> = replayed
            .iter()
            .filter(|e| matches!(&e.payload, RunEventKind::HookInvocationCompleted { .. }))
            .collect();
        assert_eq!(
            started.len(),
            2,
            "every dispatched hook must emit HookInvocationStarted"
        );
        assert_eq!(
            completed.len(),
            2,
            "every dispatched hook must emit HookInvocationCompleted"
        );

        for start in &started {
            let start_trace = trace_from_started_event(start, "plan-trace").expect("started maps");
            assert_eq!(start_trace.plan_hash, "plan-trace");
            assert_eq!(start_trace.status, HookInvocationStatus::Started);
            assert!(
                completed
                    .iter()
                    .any(|c| trace_from_completed_event(c, "plan-trace")
                        .is_some_and(|t| t.invocation_id == start_trace.invocation_id)),
                "started invocation {} must have a completed twin",
                start_trace.invocation_id
            );
        }
    }

    /// Resolving through the tool-path seam compiles exactly once per run and
    /// is immune to mid-Run `hooks.json` edits — the edited file only reaches a
    /// *new* Run.
    #[tokio::test]
    async fn resolve_frozen_dispatcher_compiles_once_and_is_immune_to_mid_run_edits() {
        let project = TempProject::new();
        project.write(
            ".natives/hooks.json",
            &probe_group("Notification", PROBE_URL),
        );
        let run_id = format!("run-resolve-{}", uuid::Uuid::new_v4());
        let events = EventSequencer::memory_only();

        let first =
            resolve_frozen_dispatcher(&run_id, events.clone(), Some(project.root.as_path()));
        let plan_hash = first.plan_hash().to_string();
        assert_eq!(
            frozen_dispatcher_for_run(&run_id).unwrap().plan_hash(),
            plan_hash
        );

        // Mid-Run edit: add a second probe hook to the same file.
        project.write(
            ".natives/hooks.json",
            &format!(
                r#"{{"hooks":{{"Notification":[{{"hooks":[
                    {{"type":"http","url":"{PROBE_URL}"}},
                    {{"type":"http","url":"{PROBE_URL}"}}
                ]}}]}}}}"#
            ),
        );

        let second = resolve_frozen_dispatcher(&run_id, events, Some(project.root.as_path()));
        assert_eq!(
            second.plan_hash(),
            plan_hash,
            "a mid-Run edit must not change the frozen plan hash"
        );
        let responses = second
            .dispatch(HookRequest {
                event: HookEvent::Notification,
                run_id: run_id.clone(),
                tool_name: None,
                input: serde_json::json!({}),
            })
            .await;
        assert_eq!(
            responses.len(),
            2,
            "the frozen dispatcher still dispatches the pre-edit handler set"
        );
    }

    /// `failurePolicy` / `failure_policy` is read from `hooks.json`; unknown
    /// or missing values stay fail-closed `Fail`.
    #[test]
    fn failure_policy_field_is_parsed_from_hooks_json() {
        let project = TempProject::new();
        project.write(
            ".natives/hooks.json",
            &format!(
                r#"{{"hooks":{{"Notification":[{{"hooks":[
                    {{"type":"http","url":"{PROBE_URL}","failurePolicy":"skip"}},
                    {{"type":"http","url":"{PROBE_URL}","failure_policy":"default"}},
                    {{"type":"http","url":"{PROBE_URL}"}},
                    {{"type":"http","url":"{PROBE_URL}","failurePolicy":"bogus"}}
                ]}}]}}}}"#
            ),
        );
        let policies: Vec<_> = file_definitions(&project)
            .iter()
            .map(|d| d.failure_policy)
            .collect();
        assert_eq!(
            policies,
            vec![
                HookFailurePolicy::Skip,
                HookFailurePolicy::Default,
                HookFailurePolicy::Fail,
                HookFailurePolicy::Fail,
            ]
        );
    }

    /// The frozen dispatcher resolves a handler failure through the Hook's
    /// failure policy: Skip drops it, Default allows it, and a security event
    /// stays fail-closed no matter what the policy says.
    #[tokio::test]
    async fn frozen_dispatcher_applies_failure_policy_skip_default_and_security_override() {
        let events = EventSequencer::memory_only();

        let frozen = freeze_run_hooks(
            "run-skip",
            "plan-skip",
            failing_registry(HookEvent::Notification, HookFailurePolicy::Skip),
            events.clone(),
        );
        let responses = frozen
            .dispatch(HookRequest {
                event: HookEvent::Notification,
                run_id: "run-skip".into(),
                tool_name: None,
                input: serde_json::json!({}),
            })
            .await;
        assert!(
            responses.is_empty(),
            "Skip must drop the failed hook: {responses:?}"
        );

        let frozen = freeze_run_hooks(
            "run-default",
            "plan-default",
            failing_registry(HookEvent::Notification, HookFailurePolicy::Default),
            events.clone(),
        );
        let responses = frozen
            .dispatch(HookRequest {
                event: HookEvent::Notification,
                run_id: "run-default".into(),
                tool_name: None,
                input: serde_json::json!({}),
            })
            .await;
        assert_eq!(responses.len(), 1);
        assert!(
            matches!(responses[0].decision, HookDecision::Allow),
            "Default must resolve a failure to Allow"
        );

        let frozen = freeze_run_hooks(
            "run-sec",
            "plan-sec",
            failing_registry(HookEvent::PreToolUse, HookFailurePolicy::Default),
            events.clone(),
        );
        let responses = frozen
            .dispatch(HookRequest {
                event: HookEvent::PreToolUse,
                run_id: "run-sec".into(),
                tool_name: Some("write_file".into()),
                input: serde_json::json!({}),
            })
            .await;
        assert!(
            matches!(responses[0].decision, HookDecision::Deny { .. }),
            "a security event must stay fail-closed even with Default policy"
        );
    }

    /// The trace mapping round-trips the durable run-event payloads into the
    /// metadata model, including the structured error category.
    #[test]
    fn trace_mapping_round_trips_run_event_payloads() {
        let events = EventSequencer::memory_only();
        let started = events.append(
            "run-map",
            RunEventKind::HookInvocationStarted {
                invocation_id: "inv-1".into(),
                hook_id: "builtin/allow-all#Notification".into(),
                hook_event: "Notification".into(),
                source: "allow-all".into(),
                ordinal: 0,
                input_summary: "{\"tool\":\"notification\"}".into(),
                input_truncated: false,
            },
        );
        let trace = trace_from_started_event(&started, "plan-hash").expect("started maps");
        assert_eq!(trace.plan_hash, "plan-hash");
        assert_eq!(trace.status, HookInvocationStatus::Started);
        assert_eq!(trace.hook_event, "Notification");
        assert_eq!(trace.hook_id, "builtin/allow-all#Notification");
        assert!(!trace.input_truncated);

        let completed = events.append(
            "run-map",
            RunEventKind::HookInvocationCompleted {
                invocation_id: "inv-1".into(),
                hook_id: "builtin/allow-all#Notification".into(),
                hook_event: "Notification".into(),
                source: "allow-all".into(),
                ordinal: 0,
                status: "completed".into(),
                effective_decision: Some("Allow".into()),
                error_category: Some("timeout".into()),
                duration_ms: 7,
                output_summary: "decision".into(),
                output_truncated: false,
            },
        );
        let trace = trace_from_completed_event(&completed, "plan-hash").expect("completed maps");
        assert_eq!(trace.status, HookInvocationStatus::Completed);
        assert_eq!(trace.duration_ms, 7);
        assert_eq!(trace.effective_decision.as_deref(), Some("Allow"));
        assert_eq!(
            trace.error_category,
            Some(HookErrorCategory::Timeout),
            "the structured error category must survive the mapping"
        );
        assert_eq!(
            parse_error_category("handler_failure"),
            Some(HookErrorCategory::HandlerFailure)
        );
        assert_eq!(parse_error_category("unknown"), None);
    }
}
