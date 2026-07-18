//! Production execution seams for the Agent Daemon Run Authority.
//!
//! Real providers (no Echo mock on production path), capability-gateway tools,
//! permission Ask/Allow/Deny, hooks, and subagent child runs.

use agent_core::{
    AgentEngine, EngineError, EngineMessage, EngineProvider, EngineProviderEvent,
    EngineProviderEventStream, EngineRunConfig, EngineToolRuntime, EventSequencer, HookEvent, HookRegistry,
    PermissionManager, PermissionProfile, SubAgentConfig, SubAgentManager, SubAgentStatus,
    ToolExecutionResult, ToolSchema, AllowAllHook, CommandHook, HttpHook,
};
use agent_core::assemble_context;
use assistant_protocol::v2::RunEventKind;
use capability_gateway::{CapabilityGateway, SideEffect};
use futures_util::StreamExt;
use provider_adapters::capabilities::{
    history_message_to_provider, Credential, HistoryMessage, HistoryToolCall, ProviderAdapter,
    ProviderRequest, ProviderTool,
};
use provider_adapters::stream::ProviderEvent;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{oneshot, Mutex};

/// Shared production runtime owned by the Daemon.
pub struct ProductionRuntime {
    pub events: EventSequencer,
    pub permissions: Arc<PermissionManager>,
    pub subagents: Arc<SubAgentManager>,
    pub hooks: Arc<Mutex<HookRegistry>>,
    /// permission_id → (run_id, resolver). run_id binding prevents cross-run responds.
    pub permission_waiters: Arc<Mutex<HashMap<String, (String, oneshot::Sender<bool>)>>>,
    /// task_id → child run status/output
    pub task_outputs: Arc<Mutex<HashMap<String, TaskRecord>>>,
    pub engines: Arc<Mutex<HashMap<String, Arc<AgentEngine>>>>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TaskRecord {
    pub run_id: String,
    pub status: String,
    pub output: Option<String>,
}

/// Build the production HookRegistry: Rust lifecycle hooks always present;
/// optional trusted CommandHook (argv) and HttpHook (SSRF-gated) from env;
/// optional project `.claude/hooks.json` command hooks (M3).
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

impl ProductionRuntime {
    pub fn new() -> Self {
        Self {
            events: EventSequencer::new(),
            permissions: Arc::new(PermissionManager::new(PermissionProfile::ConfirmEach)),
            subagents: Arc::new(SubAgentManager::new(SubAgentConfig::default())),
            hooks: Arc::new(Mutex::new(build_production_hooks())),
            permission_waiters: Arc::new(Mutex::new(HashMap::new())),
            // type: HashMap<permission_id, (run_id, oneshot)>
            task_outputs: Arc::new(Mutex::new(HashMap::new())),
            engines: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn set_permission_profile(&self, profile: &str) {
        let p = match profile {
            "full_access" | "autonomous" | "full" => PermissionProfile::Autonomous,
            _ => PermissionProfile::ConfirmEach,
        };
        self.permissions.set_profile(p).await;
    }

    pub async fn respond_permission(
        &self,
        request_id: &str,
        approved: bool,
        run_id: Option<&str>,
    ) -> Result<(), String> {
        if request_id.trim().is_empty() {
            return Err("request_id required".into());
        }
        let mut map = self.permission_waiters.lock().await;
        let Some((bound_run, tx)) = map.remove(request_id) else {
            return Err(format!("permission request not found: {request_id}"));
        };
        if let Some(rid) = run_id {
            if !rid.is_empty() && rid != bound_run {
                // Re-insert so legitimate owner can still respond.
                map.insert(request_id.to_string(), (bound_run.clone(), tx));
                return Err(format!(
                    "permission run_id mismatch: expected {bound_run}, got {rid}"
                ));
            }
        }
        let _ = tx.send(approved);
        Ok(())
    }

    pub async fn start_run(
        &self,
        run_id: String,
        conversation_id: String,
        provider_id: String,
        model_id: String,
        key_id: Option<String>,
        permission_profile: String,
        user_content: String,
        max_steps: u32,
        project_path: Option<std::path::PathBuf>,
    ) -> Result<(), String> {
        let project_root = project_path.ok_or_else(|| {
            "project_path is required for daemon runs; process cwd fallback is disabled".to_string()
        })?;
        // Full production hook set + project hooks for this workspace.
        let hooks = build_production_hooks_for_project(Some(&project_root));
        let engine = Arc::new(AgentEngine::new(self.events.clone()).with_hooks(hooks));
        self.engines.lock().await.insert(run_id.clone(), engine.clone());

        let provider = RealProvider {
            provider_id: provider_id.clone(),
            key_id: key_id.clone(),
        };
        let tools = PermissionGatedTools {
            gateway: {
                let mut g = CapabilityGateway::new();
                g.set_project_root(project_root.to_string_lossy().to_string());
                g.register_builtins();
                Arc::new(g)
            },
            permissions: self.permissions.clone(),
            events: self.events.clone(),
            waiters: self.permission_waiters.clone(),
            subagents: self.subagents.clone(),
            task_outputs: self.task_outputs.clone(),
            engines: self.engines.clone(),
            runtime: None,
            provider_id: provider_id.clone(),
            parent_run_id: run_id.clone(),
            conversation_id: conversation_id.clone(),
            model_id: model_id.clone(),
            permission_profile: permission_profile.clone(),
        };

        let assembled = assemble_context(None, Some(&project_root), None);
        let config = EngineRunConfig {
            run_id: run_id.clone(),
            conversation_id,
            model: model_id,
            system_prompt: if assembled.system_prompt.is_empty() {
                None
            } else {
                Some(assembled.system_prompt)
            },
            user_content,
            max_steps,
        };

        let status = engine
            .run(config, &provider, &tools)
            .await
            .map(|s| s.as_str().to_string())
            .unwrap_or_else(|_| "failed".into());
        self.events.append(
            &run_id,
            if status == "completed" {
                RunEventKind::Completed {
                    reason: "stop".into(),
                }
            } else if status == "interrupted" {
                RunEventKind::Interrupted {
                    reason: "cancelled".into(),
                }
            } else {
                RunEventKind::Failed {
                    error: "engine ended".into(),
                    code: status.clone(),
                }
            },
        );
        // Completed may already be emitted by engine; duplicate is ok for terminal detection.
        self.engines.lock().await.remove(&run_id);
        let _ = status;
        Ok(())
    }

    /// Sole production cancel API: cancel `run_id` and every nested descendant
    /// child run. For each id: request_cancel live engine → mark subagent
    /// metadata Cancelled → update task_outputs → append Interrupted.
    /// All cancel entry points (RPC, UI, kill_task, parent cancel) must call this.
    pub async fn cancel_run_tree(&self, run_id: &str) {
        let descendants = self.subagents.list_descendants(run_id).await;
        // Root first, then every child/grandchild run_id (engines are keyed by run_id).
        let mut run_ids: Vec<String> = vec![run_id.to_string()];
        for d in &descendants {
            if !run_ids.contains(&d.run_id) {
                run_ids.push(d.run_id.clone());
            }
        }

        {
            let engines = self.engines.lock().await;
            for rid in &run_ids {
                if let Some(engine) = engines.get(rid) {
                    engine.request_cancel();
                }
            }
        }

        // Metadata + task_outputs for every descendant task.
        for d in &descendants {
            let _ = self
                .subagents
                .update_status(&d.id, SubAgentStatus::Cancelled)
                .await;
            if let Some(rec) = self.task_outputs.lock().await.get_mut(&d.id) {
                rec.status = "cancelled".into();
            }
        }
        // Also mark any non-descendant metadata children (defensive).
        let _ = self.subagents.cascade_cancel_metadata(run_id).await;

        for rid in &run_ids {
        self.events.append(
                rid,
                RunEventKind::Cancelled {
                    reason: "cancelled".into(),
                },
            );
        }
    }

    /// Alias for tree cancel — never cancel a single node without descendants.
    pub async fn cancel_run(&self, run_id: &str) {
        self.cancel_run_tree(run_id).await;
    }

    pub async fn spawn_child_task(
        &self,
        parent_run_id: &str,
        prompt: String,
        provider_id: String,
        key_id: String,
        model_id: String,
        permission_profile: String,
    ) -> Result<String, String> {
        let child = self
            .subagents
            .spawn(
                parent_run_id,
                prompt.clone(),
                1,
                provider_id.clone(),
                key_id.clone(),
                model_id.clone(),
                permission_profile.clone(),
                vec!["read_file".into(), "list_dir".into(), "grep".into()],
                None,
                Some("none".into()),
                None,
            )
            .await?;
        self.subagents
            .update_status(&child.id, SubAgentStatus::Running)
            .await?;
        self.events.append(
            parent_run_id,
            RunEventKind::SubagentCreated {
                sub_run_id: child.run_id.clone(),
                agent_profile_id: child.agent_profile_id.clone(),
                task: prompt.clone(),
            },
        );
        let task_id = child.id.clone();
        let child_run_id = child.run_id.clone();
        let parent_owned = parent_run_id.to_string();
        let events = self.events.clone();
        let subagents = self.subagents.clone();
        let task_outputs = self.task_outputs.clone();
        let permissions = self.permissions.clone();
        let waiters = self.permission_waiters.clone();
        let engines = self.engines.clone();
        let task_id_bg = task_id.clone();

        task_outputs.lock().await.insert(
            task_id.clone(),
            TaskRecord {
                run_id: child_run_id.clone(),
                status: "running".into(),
                output: None,
            },
        );

        tokio::spawn(async move {
            let provider = RealProvider {
                provider_id: provider_id.clone(),
                key_id: Some(key_id.clone()),
            };
            let tools = PermissionGatedTools {
                gateway: {
                    let mut g = CapabilityGateway::new();
                    g.register_builtins();
                    Arc::new(g)
                },
                permissions,
                events: events.clone(),
                waiters,
                subagents: subagents.clone(),
                task_outputs: task_outputs.clone(),
                engines: engines.clone(),
                runtime: None,
                provider_id: provider_id.clone(),
                parent_run_id: child_run_id.clone(),
                conversation_id: format!("subagent-{}", child_run_id),
                model_id: model_id.clone(),
                permission_profile: permission_profile.clone(),
            };
            // Same production hook set as parent (M4) — not a reduced AllowAll-only registry.
            let hooks = build_production_hooks();
            let engine = Arc::new(AgentEngine::new(events.clone()).with_hooks(hooks));
            engines
                .lock()
                .await
                .insert(child_run_id.clone(), engine.clone());
            let config = EngineRunConfig {
                run_id: child_run_id.clone(),
                conversation_id: format!("subagent-{}", child_run_id),
                model: model_id,
                system_prompt: Some(
                    "You are a subagent with independent credentials. Complete the task."
                        .into(),
                ),
                user_content: prompt,
                max_steps: 20,
            };
            let result = engine.run(config, &provider, &tools).await;
            engines.lock().await.remove(&child_run_id);
            let (status, output) = match &result {
                Ok(s) => {
                    let text = events
                        .replay_after(&child_run_id, 0)
                        .into_iter()
                        .filter_map(|e| match e.payload {
                            RunEventKind::TextDelta { text } => Some(text),
                            _ => None,
                        })
                        .collect::<String>();
                    (s.as_str().to_string(), Some(text))
                }
                Err(e) => ("failed".into(), Some(e.to_string())),
            };
            if status == "completed" {
                let _ = subagents
                    .update_status(&task_id_bg, SubAgentStatus::Completed)
                    .await;
                events.append(
                    &parent_owned,
                    RunEventKind::SubagentCompleted {
                        sub_run_id: child_run_id.clone(),
                        result: output.clone().unwrap_or_default(),
                    },
                );
            } else {
                let _ = subagents
                    .update_status(
                        &task_id_bg,
                        SubAgentStatus::Failed(output.clone().unwrap_or_default()),
                    )
                    .await;
                events.append(
                    &parent_owned,
                    RunEventKind::SubagentFailed {
                        sub_run_id: child_run_id.clone(),
                        error: output.clone().unwrap_or_else(|| status.clone()),
                    },
                );
            }
            task_outputs.lock().await.insert(
                task_id_bg,
                TaskRecord {
                    run_id: child_run_id,
                    status,
                    output,
                },
            );
        });

        Ok(task_id)
    }

    pub async fn task_output(&self, task_id: &str) -> Option<TaskRecord> {
        self.task_outputs.lock().await.get(task_id).cloned()
    }

    pub async fn kill_task(&self, task_id: &str) -> bool {
        let child_run_id = self
            .task_outputs
            .lock()
            .await
            .get(task_id)
            .map(|r| r.run_id.clone());
        if let Some(rid) = child_run_id {
            // Tree cancel on the child run (and any nested grandchildren).
            self.cancel_run_tree(&rid).await;
            let _ = self
                .subagents
                .update_status(task_id, SubAgentStatus::Cancelled)
                .await;
            if let Some(rec) = self.task_outputs.lock().await.get_mut(task_id) {
                rec.status = "cancelled".into();
            }
            return true;
        }
        false
    }
}

impl Default for ProductionRuntime {
    fn default() -> Self {
        Self::new()
    }
}

/// Real HTTP provider adapter wrapper (never returns offline mock tool-call text).
pub struct RealProvider {
    pub provider_id: String,
    pub key_id: Option<String>,
}

#[async_trait::async_trait]
impl EngineProvider for RealProvider {
    async fn stream(
        &self,
        model: &str,
        messages: Vec<EngineMessage>,
        tools: &[ToolSchema],
        system_prompt: Option<&str>,
        cancel: Arc<AtomicBool>,
    ) -> Result<EngineProviderEventStream, EngineError> {
        let credential =
            resolve_credential_for_run(&self.provider_id, self.key_id.as_deref(), "provider-stream")
                .map_err(EngineError::Message)?;
        let adapter = resolve_adapter(
            credential.provider_type.as_deref().unwrap_or(&self.provider_id),
        );

        let provider_messages: Vec<_> = messages
            .into_iter()
            .map(engine_message_to_history)
            .map(history_message_to_provider)
            .collect();
        let provider_tools: Vec<ProviderTool> = tools
            .iter()
            .map(|t| ProviderTool {
                name: t.name.clone(),
                description: Some(t.description.clone()),
                input_schema: t.input_schema.clone(),
            })
            .collect();

        let request = ProviderRequest {
            model: model.to_string(),
            messages: provider_messages,
            system_prompt: system_prompt.map(str::to_string),
            tools: if provider_tools.is_empty() {
                None
            } else {
                Some(provider_tools)
            },
            max_tokens: Some(4096),
            temperature: None,
            stream: true,
            structured_output: None,
        };

        let stream = adapter
            .stream(request, credential)
            .await
            .map_err(|e| EngineError::Provider {
                message: e.message,
                code: e.code,
                retryable: e.retryable,
            })?;
        let mapped = futures_util::stream::unfold((stream, cancel), |(mut stream, cancel)| async move {
            loop {
                if cancel.load(Ordering::SeqCst) {
                    return None;
                }
                tokio::select! {
                    ev = stream.next() => {
                        return ev.map(|ev| {
                            let event = match ev {
                                ProviderEvent::TextDelta(t) => EngineProviderEvent::TextDelta(t),
                                ProviderEvent::ReasoningDelta(t) => EngineProviderEvent::ReasoningDelta(t),
                                ProviderEvent::ToolCallDelta {
                                    index,
                                    id,
                                    name,
                                    arguments_delta,
                                } => EngineProviderEvent::ToolCallDelta {
                                    index,
                                    id,
                                    name,
                                    arguments_delta,
                                },
                                ProviderEvent::Usage(u) => EngineProviderEvent::Usage {
                                    input_tokens: u.input_tokens,
                                    output_tokens: u.output_tokens,
                                    reasoning_tokens: u.reasoning_tokens,
                                },
                                ProviderEvent::Completed => EngineProviderEvent::Completed,
                                ProviderEvent::Error(e) => EngineProviderEvent::Error {
                                    message: e.message,
                                    code: e.code,
                                    retryable: e.retryable,
                                },
                            };
                            (event, (stream, cancel))
                        });
                    }
                    _ = tokio::time::sleep(Duration::from_millis(25)) => {}
                }
            }
        });
        Ok(Box::pin(mapped))
    }
}

/// Map engine history into provider history parts (preserves tool_calls / tool_call_id).
fn engine_message_to_history(m: EngineMessage) -> HistoryMessage {
    HistoryMessage {
        role: m.role,
        content: m.content,
        tool_call_id: m.tool_call_id,
        tool_name: m.tool_name,
        tool_calls: m.tool_calls.map(|calls| {
            calls
                .into_iter()
                .map(|c| HistoryToolCall {
                    id: c.id,
                    name: c.name,
                    arguments: c.arguments,
                })
                .collect()
        }),
    }
}

fn resolve_adapter(provider_id: &str) -> Box<dyn ProviderAdapter> {
    let lower = provider_id.to_ascii_lowercase();
    if lower.contains("anthropic") || lower.contains("claude") {
        Box::new(provider_adapters::providers::anthropic::AnthropicAdapter::new())
    } else if lower.contains("gemini") || lower.contains("google") {
        Box::new(provider_adapters::providers::gemini::GeminiAdapter::new())
    } else if lower.contains("deepseek") {
        Box::new(provider_adapters::providers::deepseek::DeepSeekAdapter::new())
    } else if lower.contains("ollama") {
        Box::new(provider_adapters::providers::ollama::OllamaAdapter::new())
    } else if lower.contains("compatible") || lower.contains("chat_completions") {
        Box::new(provider_adapters::providers::openai_compatible::OpenAiCompatibleAdapter::new())
    } else if lower.contains("responses") {
        // Force Responses API path via env for this adapter instance.
        std::env::set_var("NATIVES_OPENAI_API", "responses");
        Box::new(provider_adapters::providers::openai::OpenAiAdapter::new())
    } else {
        Box::new(provider_adapters::providers::openai::OpenAiAdapter::new())
    }
}

/// Injected by Tauri host: decrypt from natives.db via Credential Broker.
/// Signature: (provider_id, key_id, run_id) → Credential (memory only).
pub type CredentialBrokerFn = Arc<
    dyn Fn(&str, Option<&str>, &str) -> Result<Credential, String> + Send + Sync,
>;

static CREDENTIAL_BROKER: std::sync::Mutex<Option<CredentialBrokerFn>> =
    std::sync::Mutex::new(None);

/// Install the authenticated Credential Broker (from Tauri host).
pub fn install_credential_broker(broker: CredentialBrokerFn) {
    if let Ok(mut slot) = CREDENTIAL_BROKER.lock() {
        *slot = Some(broker);
    }
}

/// Clear broker (tests only).
#[cfg(test)]
pub fn clear_credential_broker_for_tests() {
    if let Ok(mut slot) = CREDENTIAL_BROKER.lock() {
        *slot = None;
    }
}

/// Resolve credentials: 1) installed Tauri broker (natives.db) 2) test env 3) fail.
/// Never invent mock completion text. Never log api_key.
pub fn resolve_credential(provider_id: &str, key_id: Option<&str>) -> Result<Credential, String> {
    // Legacy callers without a run must fail closed for lease binding in production
    // broker path; env-based test keys still work via resolve_credential_for_run
    // with an explicit synthetic id only from tests.
    resolve_credential_for_run(provider_id, key_id, "legacy-unbound")
}

pub fn resolve_credential_for_run(
    provider_id: &str,
    key_id: Option<&str>,
    run_id: &str,
) -> Result<Credential, String> {
    if run_id.trim().is_empty() {
        return Err("run_id required for credential lease binding".into());
    }
    // 1. Authenticated broker path (production): Tauri decrypts natives.db.
    //    Child agents must pass their own run_id — never inherit parent lease.
    let broker = CREDENTIAL_BROKER
        .lock()
        .ok()
        .and_then(|g| g.clone());
    if let Some(broker) = broker {
        match broker(provider_id, key_id, run_id) {
            Ok(cred) if !cred.api_key.trim().is_empty() => {
                // Attach key_id for audit; lease binding is enforced by run_id arg.
                return Ok(Credential {
                    api_key: cred.api_key,
                    base_url: cred.base_url,
                    key_id: cred.key_id.or_else(|| key_id.map(|s| s.to_string())),
                    provider_type: cred.provider_type,
                });
            }
            Ok(_) => {
                return Err(format!(
                    "Broker returned empty key for provider '{provider_id}'"
                ))
            }
            Err(e) => {
                // Fall through to env only when broker reports not-found and tests set env.
                let lower = e.to_ascii_lowercase();
                if !lower.contains("not found") && !lower.contains("no active key") {
                    return Err(redact_cred_err(&e));
                }
            }
        }
    }

    // 2. Explicit test/dev env keys (NATIVES_TEST_*) — never invent offline success text.
    // Anthropic also accepts common local env names used by CLI tooling (never logged).
    let lower = provider_id.to_ascii_lowercase();
    if lower.contains("anthropic") || lower.contains("claude") {
        let api_key = std::env::var("NATIVES_TEST_ANTHROPIC_KEY")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                std::env::var("ANTHROPIC_API_KEY")
                    .ok()
                    .filter(|s| !s.trim().is_empty())
            })
            .or_else(|| {
                std::env::var("ANTHROPIC_AUTH_TOKEN")
                    .ok()
                    .filter(|s| !s.trim().is_empty())
            });
        return match api_key {
            Some(api_key) => Ok(Credential {
                api_key,
                base_url: std::env::var("NATIVES_TEST_ANTHROPIC_BASE")
                    .ok()
                    .or_else(|| std::env::var("ANTHROPIC_BASE_URL").ok()),
                key_id: key_id.map(str::to_string),
                provider_type: Some("anthropic".into()),
            }),
            None => Err(format!(
                "No credential for provider '{provider_id}' (broker + NATIVES_TEST_ANTHROPIC_KEY/ANTHROPIC_API_KEY unavailable)"
            )),
        };
    }
    let (env_key, env_base) = if lower.contains("gemini") {
        ("NATIVES_TEST_GEMINI_KEY", None)
    } else if lower.contains("deepseek") {
        (
            "NATIVES_TEST_DEEPSEEK_KEY",
            Some("NATIVES_TEST_DEEPSEEK_BASE"),
        )
    } else if lower.contains("compatible") || lower.contains("sensenova") {
        (
            "NATIVES_TEST_OPENAI_KEY",
            Some("NATIVES_TEST_OPENAI_BASE"),
        )
    } else if lower.contains("ollama") {
        return Ok(Credential {
            api_key: "ollama".into(),
            base_url: std::env::var("NATIVES_TEST_OLLAMA_BASE").ok(),
            key_id: key_id.map(str::to_string),
            provider_type: Some("ollama".into()),
        });
    } else {
        ("NATIVES_TEST_OPENAI_KEY", Some("NATIVES_TEST_OPENAI_BASE"))
    };
    // OpenAI-compatible / SenseNova often expose keys via ANTHROPIC_* in local tooling.
    let api_key = std::env::var(env_key)
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            if lower.contains("compatible")
                || lower.contains("sensenova")
                || lower.contains("openai")
            {
                std::env::var("ANTHROPIC_AUTH_TOKEN")
                    .ok()
                    .filter(|s| !s.trim().is_empty())
                    .or_else(|| {
                        std::env::var("ANTHROPIC_API_KEY")
                            .ok()
                            .filter(|s| !s.trim().is_empty())
                    })
            } else {
                None
            }
        });
    match api_key {
        Some(api_key) => {
            let raw_base = env_base
                .and_then(|k| std::env::var(k).ok())
                .or_else(|| std::env::var("NATIVES_TEST_OPENAI_BASE").ok())
                .or_else(|| {
                    if lower.contains("compatible")
                        || lower.contains("sensenova")
                        || lower.contains("openai")
                    {
                        std::env::var("ANTHROPIC_BASE_URL").ok()
                    } else {
                        None
                    }
                });
            Ok(Credential {
                api_key,
                base_url: raw_base.map(normalize_openai_compatible_base),
                key_id: key_id.map(str::to_string),
                provider_type: Some(if lower.contains("deepseek") {
                    "deepseek"
                } else if lower.contains("gemini") {
                    "gemini"
                } else {
                    "openai_compatible"
                }.into()),
            })
        }
        None => Err(format!(
            "No credential for provider '{provider_id}' (broker + {env_key} both unavailable)"
        )),
    }
}

/// SenseNova and many gateways require `…/v1` for chat completions; accept host-only env.
pub(crate) fn normalize_openai_compatible_base(base: String) -> String {
    let t = base.trim().trim_end_matches('/').to_string();
    if t.is_empty() {
        return t;
    }
    // Already versioned or clearly a full API root.
    if t.ends_with("/v1") || t.contains("/v1/") || t.ends_with("/openai") {
        return t;
    }
    // token.sensenova.cn style host-only base → append /v1
    format!("{t}/v1")
}

#[cfg(test)]
mod base_url_normalize_tests {
    use super::normalize_openai_compatible_base;

    #[test]
    fn appends_v1_for_host_only() {
        assert_eq!(
            normalize_openai_compatible_base("https://token.sensenova.cn".into()),
            "https://token.sensenova.cn/v1"
        );
        assert_eq!(
            normalize_openai_compatible_base("https://token.sensenova.cn/".into()),
            "https://token.sensenova.cn/v1"
        );
    }

    #[test]
    fn keeps_existing_v1() {
        assert_eq!(
            normalize_openai_compatible_base("https://token.sensenova.cn/v1".into()),
            "https://token.sensenova.cn/v1"
        );
    }
}

#[cfg(test)]
mod permission_bind_tests {
    use super::*;
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn rejects_mismatched_run_id() {
        let rt = ProductionRuntime::new();
        let (tx, _rx) = oneshot::channel();
        rt.permission_waiters
            .lock()
            .await
            .insert("p1".into(), ("run-a".into(), tx));
        let err = rt
            .respond_permission("p1", true, Some("run-b"))
            .await
            .unwrap_err();
        assert!(err.contains("mismatch"), "{err}");
        // Still present for correct owner
        assert!(rt.permission_waiters.lock().await.contains_key("p1"));
        let ok = rt
            .respond_permission("p1", false, Some("run-a"))
            .await;
        assert!(ok.is_ok());
        assert!(!rt.permission_waiters.lock().await.contains_key("p1"));
    }
}

fn redact_cred_err(msg: &str) -> String {
    // Lightweight redaction without pulling regex into the daemon binary.
    let mut out = String::with_capacity(msg.len());
    let bytes = msg.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(b"sk-") || bytes[i..].starts_with(b"SK-") {
            out.push_str("[REDACTED_KEY]");
            i += 3;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'-')
            {
                i += 1;
            }
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

/// Tools with permission gate + real task orchestration.
pub struct PermissionGatedTools {
    pub gateway: Arc<CapabilityGateway>,
    pub permissions: Arc<PermissionManager>,
    pub events: EventSequencer,
    pub waiters: Arc<Mutex<HashMap<String, (String, oneshot::Sender<bool>)>>>,
    pub subagents: Arc<SubAgentManager>,
    pub task_outputs: Arc<Mutex<HashMap<String, TaskRecord>>>,
    /// Shared with ProductionRuntime so kill_task / cascade can cancel live engines.
    pub engines: Arc<Mutex<HashMap<String, Arc<AgentEngine>>>>,
    pub runtime: Option<Arc<ProductionRuntime>>,
    pub provider_id: String,
    pub parent_run_id: String,
    pub conversation_id: String,
    pub model_id: String,
    pub permission_profile: String,
}

#[async_trait::async_trait]
impl EngineToolRuntime for PermissionGatedTools {
    async fn list_tool_schemas(&self) -> Vec<ToolSchema> {
        self.gateway
            .list_tools()
            .into_iter()
            .map(|t| ToolSchema {
                name: t.name.to_string(),
                description: t.description.to_string(),
                input_schema: t.schema.clone(),
            })
            .collect()
    }

    async fn execute_tool(
        &self,
        name: &str,
        input: Value,
        cancel: &AtomicBool,
    ) -> ToolExecutionResult {
        if cancel.load(Ordering::SeqCst) {
            return ToolExecutionResult {
                output: serde_json::json!({"error": "cancelled"}),
                is_error: true,
                duration_ms: 0,
            };
        }

        let tool = self.gateway.get_tool(name);
        let is_mcp = name == "mcp_call" || name.starts_with("mcp__");
        if tool.is_none()
            && !matches!(name, "task" | "task_output" | "kill_task" | "mcp_call")
            && !name.starts_with("mcp__")
        {
            return ToolExecutionResult {
                output: serde_json::json!({"error": format!("unknown tool: {name}")}),
                is_error: true,
                duration_ms: 0,
            };
        }

        // Permission gate: PermissionClass + SideEffect (G5).
        // Orchestration tools always go through this gate (never early-return around it).
        let profile_str = match self.permission_profile.as_str() {
            "full_access" | "autonomous" | "full" => "autonomous",
            "readonly" => "readonly",
            _ => "ask",
        };
        let (perm_class, side_effect) = if let Some(tool) = tool {
            (tool.permission_class, tool.side_effect)
        } else if is_mcp {
            (
                capability_gateway::PermissionClass::ExternalWrite,
                SideEffect::Network,
            )
        } else {
            // Fail-closed defaults if somehow unregistered.
            match name {
                "task_output" => (
                    capability_gateway::PermissionClass::AlwaysAllowed,
                    SideEffect::ReadOnly,
                ),
                "task" | "kill_task" => (
                    capability_gateway::PermissionClass::ProjectWrite,
                    SideEffect::Process,
                ),
                _ => (
                    capability_gateway::PermissionClass::Elevation,
                    SideEffect::Destructive,
                ),
            }
        };
        let class_result =
            capability_gateway::policy::check_permission(perm_class, profile_str);
        let needs_ask = match class_result {
            capability_gateway::policy::PolicyResult::Allowed => false,
            capability_gateway::policy::PolicyResult::NeedsApproval(_)
            | capability_gateway::policy::PolicyResult::Denied(_) => true,
        };

        if profile_str == "readonly" && !matches!(side_effect, SideEffect::ReadOnly) {
            return ToolExecutionResult {
                output: serde_json::json!({"error": "readonly profile denies side effects", "denied": true}),
                is_error: true,
                duration_ms: 0,
            };
        }

        if needs_ask {
            if let Some(denied) = self.await_tool_permission(name, &input).await {
                return denied;
            }
        }

        // Orchestration tools after permission.
        if name == "task" {
            return self.execute_task(input).await;
        }
        if name == "task_output" {
            let id = input.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
            let rec = self.task_outputs.lock().await.get(id).cloned();
            return ToolExecutionResult {
                output: serde_json::to_value(rec)
                    .unwrap_or(serde_json::json!({"status":"unknown"})),
                is_error: false,
                duration_ms: 0,
            };
        }
        if name == "kill_task" {
            let id = input.get("task_id").and_then(|v| v.as_str()).unwrap_or("");
            let ok = self.kill_task_tree(id).await;
            return ToolExecutionResult {
                output: serde_json::json!({"cancelled": ok, "task_id": id}),
                is_error: !ok,
                duration_ms: 0,
            };
        }
        // MCP tools: always after permission gate (ExternalWrite / Network).
        if name == "mcp_call" || name.starts_with("mcp__") {
            return self.execute_mcp_call(name, input).await;
        }

        let Some(_tool) = tool else {
            return ToolExecutionResult {
                output: serde_json::json!({"error": format!("unknown tool: {name}")}),
                is_error: true,
                duration_ms: 0,
            };
        };

        // Inject project cwd for terminal when missing (sandbox).
        let mut input = input;
        if name == "run_terminal" {
            if input.get("cwd").and_then(|v| v.as_str()).is_none() {
                if let Some(root) = &self.gateway.project_root {
                    if let Some(obj) = input.as_object_mut() {
                        obj.insert("cwd".into(), Value::String(root.clone()));
                    }
                }
            }
        }

        let started = Instant::now();
        match self.gateway.execute(name, input).await {
            Ok(out) => ToolExecutionResult {
                output: out.result,
                is_error: false,
                duration_ms: out.duration_ms.max(started.elapsed().as_millis() as u64),
            },
            Err(err) => ToolExecutionResult {
                output: serde_json::json!({"error": err.message, "code": err.code}),
                is_error: true,
                duration_ms: started.elapsed().as_millis() as u64,
            },
        }
    }
}

impl PermissionGatedTools {
    async fn await_tool_permission(
        &self,
        name: &str,
        input: &Value,
    ) -> Option<ToolExecutionResult> {
        let tool_call_id = uuid::Uuid::new_v4().to_string();
        let permission_id = self
            .permissions
            .request_permission_for_profile(
                match self.permission_profile.as_str() {
                    "full_access" | "autonomous" | "full" => PermissionProfile::Autonomous,
                    _ => PermissionProfile::ConfirmEach,
                },
                &self.parent_run_id,
                &tool_call_id,
                name,
                format!("Approve {name}?"),
                input.clone(),
            )
            .await
            .unwrap_or_else(|_| uuid::Uuid::new_v4().to_string());

        if permission_id == "auto-approved" {
            return None;
        }
        // Install the waiter before publishing the event. Otherwise a fast UI
        // (or test responder) can observe PermissionRequested, respond, and
        // lose the race before the channel exists, leaving the engine blocked.
        let (tx, rx) = oneshot::channel();
        self.waiters.lock().await.insert(
            permission_id.clone(),
            (self.parent_run_id.clone(), tx),
        );
        self.events.append(
            &self.parent_run_id,
            RunEventKind::PermissionRequested {
                tool_call_id: tool_call_id.clone(),
                tool_name: name.to_string(),
                reason: format!("Approve tool `{name}`"),
                permission_id: permission_id.clone(),
                input: input.clone(),
            },
        );
        let approved = tokio::time::timeout(Duration::from_secs(120), rx)
            .await
            .ok()
            .and_then(|r| r.ok())
            .unwrap_or(false);
        self.events.append(
            &self.parent_run_id,
            RunEventKind::PermissionResponded {
                permission_id,
                approved,
                scope: "once".into(),
            },
        );
        if approved {
            None
        } else {
            Some(ToolExecutionResult {
                output: serde_json::json!({"error": "permission denied", "denied": true}),
                is_error: true,
                duration_ms: 0,
            })
        }
    }

    /// Same tree-cancel rules as ProductionRuntime::cancel_run_tree, using
    /// the shared engines/subagents/events maps held by this tool runtime.
    async fn cancel_run_tree_local(&self, run_id: &str) {
        if let Some(rt) = &self.runtime {
            rt.cancel_run_tree(run_id).await;
            return;
        }
        let descendants = self.subagents.list_descendants(run_id).await;
        let mut run_ids: Vec<String> = vec![run_id.to_string()];
        for d in &descendants {
            if !run_ids.contains(&d.run_id) {
                run_ids.push(d.run_id.clone());
            }
        }
        {
            let engines = self.engines.lock().await;
            for rid in &run_ids {
                if let Some(engine) = engines.get(rid) {
                    engine.request_cancel();
                }
            }
        }
        for d in &descendants {
            let _ = self
                .subagents
                .update_status(&d.id, SubAgentStatus::Cancelled)
                .await;
            if let Some(rec) = self.task_outputs.lock().await.get_mut(&d.id) {
                rec.status = "cancelled".into();
            }
        }
        let _ = self.subagents.cascade_cancel_metadata(run_id).await;
        for rid in &run_ids {
            self.events.append(
                rid,
                RunEventKind::Interrupted {
                    reason: "cancelled".into(),
                },
            );
        }
    }

    async fn kill_task_tree(&self, task_id: &str) -> bool {
        let child_run_id = self
            .task_outputs
            .lock()
            .await
            .get(task_id)
            .map(|r| r.run_id.clone());
        let Some(rid) = child_run_id else {
            return false;
        };
        self.cancel_run_tree_local(&rid).await;
        let _ = self
            .subagents
            .update_status(task_id, SubAgentStatus::Cancelled)
            .await;
        if let Some(rec) = self.task_outputs.lock().await.get_mut(task_id) {
            rec.status = "cancelled".into();
        }
        true
    }

    /// Route `mcp_call` / namespaced `mcp__server__tool` through daemon MCP runtime.
    async fn execute_mcp_call(&self, name: &str, input: Value) -> ToolExecutionResult {
        let started = Instant::now();
        let (server_id, tool_name, arguments) = if name == "mcp_call" {
            let server = input
                .get("server")
                .or_else(|| input.get("server_id"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let tool = input
                .get("tool")
                .or_else(|| input.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let args = input
                .get("arguments")
                .or_else(|| input.get("input"))
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            (server, tool, args)
        } else {
            // mcp__{server}__{tool}
            let rest = name.strip_prefix("mcp__").unwrap_or(name);
            let mut parts = rest.splitn(2, "__");
            let server = parts.next().unwrap_or("").to_string();
            let tool = parts.next().unwrap_or("").to_string();
            (server, tool, input)
        };
        if server_id.is_empty() || tool_name.is_empty() {
            return ToolExecutionResult {
                output: serde_json::json!({
                    "error": "mcp_call requires server and tool",
                    "code": "invalid_input",
                }),
                is_error: true,
                duration_ms: started.elapsed().as_millis() as u64,
            };
        }
        let call_id = format!("mcp-{server_id}-{tool_name}");
        let display_name = format!("mcp_call:{server_id}/{tool_name}");
        // Structured audit event (permission already passed).
        self.events.append(
            &self.parent_run_id,
            RunEventKind::ToolCallStarted {
                id: call_id.clone(),
                name: display_name.clone(),
            },
        );
        match crate::mcp_runtime::global_mcp().call_tool(&server_id, &tool_name, arguments) {
            Ok(result) => {
                let duration_ms = started.elapsed().as_millis() as u64;
                self.events.append(
                    &self.parent_run_id,
                    RunEventKind::ToolCallCompleted {
                        id: call_id,
                        name: display_name,
                        output: result.clone(),
                        is_error: false,
                        duration_ms,
                    },
                );
                ToolExecutionResult {
                    output: serde_json::json!({
                        "server": server_id,
                        "tool": tool_name,
                        "ok": true,
                        "result": result,
                    }),
                    is_error: false,
                    duration_ms,
                }
            }
            Err(e) => {
                let duration_ms = started.elapsed().as_millis() as u64;
                self.events.append(
                    &self.parent_run_id,
                    RunEventKind::ToolCallCompleted {
                        id: call_id,
                        name: display_name,
                        output: serde_json::json!({"error": e}),
                        is_error: true,
                        duration_ms,
                    },
                );
                ToolExecutionResult {
                    output: serde_json::json!({
                        "server": server_id,
                        "tool": tool_name,
                        "ok": false,
                        "error": e,
                        "code": "mcp_call_failed",
                    }),
                    is_error: true,
                    duration_ms,
                }
            }
        }
    }

    async fn execute_task(&self, input: Value) -> ToolExecutionResult {
        let prompt = input
            .get("prompt")
            .or_else(|| input.get("task"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let child_provider = input
            .get("provider_id")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.provider_id)
            .to_string();
        let child_model = input
            .get("model_id")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.model_id)
            .to_string();
        // Independent key identity: never inherit parent key. Require explicit
        // key_id or a broker-resolved key_id for the child provider.
        let child_key = if let Some(k) = input.get("key_id").and_then(|v| v.as_str()) {
            if k.is_empty() {
                return ToolExecutionResult {
                    output: serde_json::json!({
                        "error": "key_id required for subagent (must not inherit parent key)",
                    }),
                    is_error: true,
                    duration_ms: 0,
                };
            }
            k.to_string()
        } else {
            // Broker may return an active key for the provider with its key_id.
            match resolve_credential_for_run(&child_provider, None, &self.parent_run_id) {
                Ok(cred) => match cred.key_id {
                    Some(id) if !id.is_empty() => id,
                    _ => {
                        return ToolExecutionResult {
                            output: serde_json::json!({
                                "error": "subagent requires key_id (broker did not return key_id)",
                            }),
                            is_error: true,
                            duration_ms: 0,
                        };
                    }
                },
                Err(e) => {
                    return ToolExecutionResult {
                        output: serde_json::json!({
                            "error": format!("subagent key resolve failed: {e}"),
                            "hint": "pass key_id in task input or configure broker"
                        }),
                        is_error: true,
                        duration_ms: 0,
                    };
                }
            }
        };
        // Child always starts with ask permission unless explicitly full_access on tool input.
        let child_perm = input
            .get("permission_profile")
            .and_then(|v| v.as_str())
            .unwrap_or("ask")
            .to_string();

        let child = match self
            .subagents
            .spawn(
                &self.parent_run_id,
                prompt.clone(),
                1,
                child_provider.clone(),
                child_key.clone(),
                child_model.clone(),
                child_perm.clone(),
                vec!["read_file".into(), "list_dir".into(), "grep".into()],
                None,
                Some("none".into()),
                None,
            )
            .await
        {
            Ok(c) => c,
            Err(e) => {
                return ToolExecutionResult {
                    output: serde_json::json!({"error": e}),
                    is_error: true,
                    duration_ms: 0,
                };
            }
        };
        let _ = self
            .subagents
            .update_status(&child.id, SubAgentStatus::Running)
            .await;
        self.events.append(
            &self.parent_run_id,
            RunEventKind::SubagentCreated {
                sub_run_id: child.run_id.clone(),
                agent_profile_id: None,
                task: prompt.clone(),
            },
        );

        let task_id = child.id.clone();
        let child_run_id = child.run_id.clone();
        let child_run_id_ret = child_run_id.clone();
        let events = self.events.clone();
        let subagents = self.subagents.clone();
        let task_outputs = self.task_outputs.clone();
        let permissions = self.permissions.clone();
        let waiters = self.waiters.clone();
        let parent_run_id = self.parent_run_id.clone();
        let task_id_bg = task_id.clone();
        let child_provider_id = child.provider_id.clone();
        let child_key_id = child.key_id.clone();
        let child_model_id = child.model_id.clone();
        let child_perm_profile = child.permission_profile.clone();
        let child_provider_bg = child_provider.clone();
        let child_key_bg = child_key;
        let child_model_bg = child_model;
        let child_perm_bg = child_perm;
        let prompt_bg = prompt;
        let project_root = self.gateway.project_root.clone();
        let engines = self.engines.clone();
        let child_provider_for_tools = child_provider_id.clone();

        task_outputs.lock().await.insert(
            task_id.clone(),
            TaskRecord {
                run_id: child_run_id.clone(),
                status: "running".into(),
                output: None,
            },
        );

        // Capture fixture flag *before* spawn so parallel tests cannot flip env mid-flight.
        // Explicit task input `fixture: true` wins (CI dual-provider identity without env races).
        let use_fixture = input
            .get("fixture")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
            || std::env::var("NATIVES_DAEMON_FIXTURE")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false);

        tokio::spawn(async move {
            let tools = PermissionGatedTools {
                gateway: {
                    let mut g = CapabilityGateway::new();
                    if let Some(root) = project_root {
                        g.set_project_root(root);
                    }
                    g.register_builtins();
                    Arc::new(g)
                },
                permissions,
                events: events.clone(),
                waiters,
                subagents: subagents.clone(),
                task_outputs: task_outputs.clone(),
                engines: engines.clone(),
                runtime: None,
                // Inherit real child provider for nested task/credential resolve (M6).
                provider_id: child_provider_for_tools,
                parent_run_id: child_run_id.clone(),
                conversation_id: format!("sub-{}", child_run_id),
                model_id: child_model_bg.clone(),
                permission_profile: child_perm_bg,
            };
            // Full production hooks for subagents (M4).
            let hooks = build_production_hooks();
            let engine = Arc::new(AgentEngine::new(events.clone()).with_hooks(hooks));
            engines
                .lock()
                .await
                .insert(child_run_id.clone(), engine.clone());
            let config = EngineRunConfig {
                run_id: child_run_id.clone(),
                conversation_id: format!("sub-{}", child_run_id),
                model: child_model_bg,
                system_prompt: Some(
                    "Independent subagent. Do not assume parent permissions.".into(),
                ),
                user_content: prompt_bg,
                max_steps: 15,
            };
            // Offline/CI: fixture child still proves independent provider_id/key_id/model identity.
            let result = if use_fixture {
                let provider = FixtureProvider {
                    mode: FixtureMode::TextOnly,
                };
                engine.run(config, &provider, &tools).await
            } else {
                let provider = RealProvider {
                    provider_id: child_provider_bg,
                    key_id: Some(child_key_bg),
                };
                engine.run(config, &provider, &tools).await
            };
            engines.lock().await.remove(&child_run_id);
            let (status, output) = match result {
                Ok(s) => {
                    let text = events
                        .replay_after(&child_run_id, 0)
                        .into_iter()
                        .filter_map(|e| match e.payload {
                            RunEventKind::TextDelta { text } => Some(text),
                            _ => None,
                        })
                        .collect::<String>();
                    (s.as_str().to_string(), Some(text))
                }
                Err(e) => ("failed".into(), Some(e.to_string())),
            };
            if status == "completed" {
                let _ = subagents
                    .update_status(&task_id_bg, SubAgentStatus::Completed)
                    .await;
                events.append(
                    &parent_run_id,
                    RunEventKind::SubagentCompleted {
                        sub_run_id: child_run_id.clone(),
                        result: output.clone().unwrap_or_default(),
                    },
                );
            } else {
                let _ = subagents
                    .update_status(
                        &task_id_bg,
                        SubAgentStatus::Failed(output.clone().unwrap_or_default()),
                    )
                    .await;
                events.append(
                    &parent_run_id,
                    RunEventKind::SubagentFailed {
                        sub_run_id: child_run_id.clone(),
                        error: output.clone().unwrap_or_else(|| status.clone()),
                    },
                );
            }
            task_outputs.lock().await.insert(
                task_id_bg,
                TaskRecord {
                    run_id: child_run_id,
                    status,
                    output,
                },
            );
        });

        ToolExecutionResult {
            output: serde_json::json!({
                "task_id": task_id,
                "run_id": child_run_id_ret,
                "status": "running",
                "provider_id": child_provider_id,
                "key_id": child_key_id,
                "model_id": child_model_id,
                "permission_profile": child_perm_profile,
            }),
            is_error: false,
            duration_ms: 0,
        }
    }
}

/// Fixture provider for offline tests — emits tool_call then text, never fake "hello from adapter" as success content for tools.
pub struct FixtureProvider {
    pub mode: FixtureMode,
}

#[derive(Clone, Copy)]
pub enum FixtureMode {
    TextOnly,
    ToolThenText,
    RequestPermissionPath,
}

#[async_trait::async_trait]
impl EngineProvider for FixtureProvider {
    async fn stream(
        &self,
        _model: &str,
        messages: Vec<EngineMessage>,
        _tools: &[ToolSchema],
        _system_prompt: Option<&str>,
        _cancel: Arc<AtomicBool>,
    ) -> Result<EngineProviderEventStream, EngineError> {
        // If last message is a tool result, complete with text.
        if messages
            .last()
            .map(|m| m.role == "tool")
            .unwrap_or(false)
        {
            return Ok(Box::pin(futures_util::stream::iter(vec![
                EngineProviderEvent::TextDelta("tool path complete".into()),
                EngineProviderEvent::Completed,
            ])));
        }
        let events = match self.mode {
            FixtureMode::TextOnly => vec![
                EngineProviderEvent::TextDelta("fixture answer".into()),
                EngineProviderEvent::Completed,
            ],
            FixtureMode::ToolThenText => vec![
                EngineProviderEvent::ToolCallDelta {
                    index: 0,
                    id: Some("call_1".into()),
                    name: Some("read_file".into()),
                    arguments_delta: r#"{"path":"Cargo.toml"}"#.into(),
                },
                EngineProviderEvent::Completed,
            ],
            // Side-effecting tool so PermissionManager ConfirmEach emits permission_requested.
            FixtureMode::RequestPermissionPath => vec![
                EngineProviderEvent::ToolCallDelta {
                    index: 0,
                    id: Some("call_perm".into()),
                    name: Some("write_file".into()),
                    arguments_delta: r#"{"path":"/tmp/natives-perm-test.txt","content":"x"}"#.into(),
                },
                EngineProviderEvent::Completed,
            ],
        };
        Ok(Box::pin(futures_util::stream::iter(events)))
    }
}
