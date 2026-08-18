//! Native Hook handlers for the Production Hook seam (extracted from
//! `production_hooks.rs`, task-01 structure).
//!
//! `NativeMcpHook` / `NativePromptHook` / `NativeAgentHook` implement the v3
//! MCP-tool, prompt, and agent Hook adapters; `UnsupportedNativeHook` fails
//! closed for adapters this Daemon build cannot run. `substitute_hook_input`
//! and `prompt_decision` are the shared input-substitution and decision
//! helpers those handlers rely on.

use super::*;

pub(crate) struct UnsupportedNativeHook;

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

pub(crate) struct NativeMcpHook {
    pub(crate) server_id: String,
    pub(crate) tool_name: String,
    pub(crate) input_template: Option<String>,
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

pub(crate) fn substitute_hook_input(value: &mut Value, input: &Value) {
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

pub(crate) struct NativePromptHook {
    pub(crate) template: String,
    pub(crate) model_override: Option<String>,
    pub(crate) timeout_ms: u64,
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

pub(crate) fn prompt_decision(text: &str) -> HookOutcome {
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

pub(crate) struct NativeAgentHook {
    pub(crate) prompt: String,
    pub(crate) model_override: Option<String>,
    pub(crate) max_steps: u32,
    pub(crate) readonly_tools: Vec<String>,
    pub(crate) timeout_ms: u64,
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
        let policy = match crate::subagent_store::get_route_policy(&parent.conversation_id) {
            Ok(Some(policy)) => policy,
            Ok(None) => {
                return HookOutcome::Failed {
                    reason: "Agent Hook requires an independent route policy binding".into(),
                }
            }
            Err(reason) => return HookOutcome::Failed { reason },
        };
        let binding = match crate::subagent_store::pick_independent_binding(
            &policy,
            &key_id,
            self.model_override.as_deref(),
        ) {
            Ok(binding) => binding,
            Err(reason) => return HookOutcome::Failed { reason },
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
        let created = match crate::child_run_orchestrator::create_child_run(
            crate::child_run_orchestrator::ChildRunSpec {
                conversation_id,
                provider_id: binding.provider_id,
                model_id: binding.model_id,
                key_id: Some(binding.key_id),
                agent_profile_id: None,
                permission_profile: Some("readonly".into()),
                content: Some(task),
                max_steps: Some(self.max_steps.clamp(1, 32)),
                parent_run_id: Some(parent.id.clone()),
                project_path: parent.project_path.clone(),
                runtime_id: Some("native".into()),
            },
        )
        .await
        {
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
        crate::child_run_orchestrator::apply_child_surface(
            &created.id,
            self.readonly_tools.clone(),
            Some(self.prompt.clone()),
        )
        .await;
        crate::global_run_manager().runtime.events.append(
            &parent.id,
            RunEventKind::SubagentCreated {
                sub_run_id: created.id.clone(),
                agent_profile_id: None,
                task: "Harness Agent Hook".into(),
            },
        );
        if let Err(reason) = crate::child_run_orchestrator::start_child_run(
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
        )
        .await
        {
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
