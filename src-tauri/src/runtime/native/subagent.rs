//! Minimal in-process subagent tools for the Native Agent Loop.

//!
//! Residual catalog/compat after Protocol v2 cutover (execution retired).
#![allow(dead_code)]
use super::capability::{
    AtomicCapability, CancellationToken, CapabilityContext, CapabilityMeta, CapabilityPermission,
    CapabilityRegistry, CapabilityRequest, CapabilitySideEffect, CapabilityVisibility,
};
use super::hook_pipeline::HookPipeline;
use super::rule_engine::RuleEngine;
use agent_core::{SubAgentConfig, SubAgentManager, SubAgentStatus};
use async_trait::async_trait;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{oneshot, Mutex, Notify};

#[derive(Clone)]
pub struct SubagentCoordinator {
    manager: Arc<SubAgentManager>,
    registry: Arc<Mutex<CapabilityRegistry>>,
    hooks: Arc<Mutex<HookPipeline>>,
    rules: Arc<Mutex<RuleEngine>>,
    cancels: Arc<Mutex<HashMap<String, oneshot::Sender<()>>>>,
    outputs: Arc<Mutex<HashMap<String, String>>>,
    notifications: Arc<Mutex<HashMap<String, Arc<Notify>>>>,
}

impl SubagentCoordinator {
    pub fn new(
        registry: Arc<Mutex<CapabilityRegistry>>,
        hooks: Arc<Mutex<HookPipeline>>,
        rules: Arc<Mutex<RuleEngine>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            manager: Arc::new(SubAgentManager::new(SubAgentConfig {
                max_concurrent: 3,
                max_concurrent_global: 3,
                max_concurrent_per_parent: 3,
                max_tasks_per_parent_total: 32,
                max_depth: 1,
                max_tokens_per_sub: 100_000,
                max_tokens_per_child: 100_000,
                max_tokens_per_tree: 500_000,
                max_tool_calls_per_child: 200,
                max_tool_calls_per_tree: 1_000,
                child_timeout_ms: 600_000,
                failure_policy: agent_core::subagents::FailurePolicy::Isolate,
            })),
            registry,
            hooks,
            rules,
            cancels: Arc::new(Mutex::new(HashMap::new())),
            outputs: Arc::new(Mutex::new(HashMap::new())),
            notifications: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    async fn spawn(
        &self,
        ctx: &CapabilityContext,
        prompt: String,
        definition_id: Option<&str>,
    ) -> crate::Result<String> {
        let allowlist = if let Some(definition_id) = definition_id {
            let conn =
                crate::db::get_main_conn().map_err(|e| crate::Error::Internal(e.to_string()))?;
            let (enabled, max_runs, tools): (bool, i64, String) = conn
                .query_row(
                    "SELECT enabled, max_runs, tools FROM subagents WHERE id = ?1",
                    rusqlite::params![definition_id],
                    |row| Ok((row.get::<_, i64>(0)? != 0, row.get(1)?, row.get(2)?)),
                )
                .map_err(|_| crate::Error::InvalidInput("Subagent definition not found".into()))?;
            if !enabled {
                return Err(crate::Error::InvalidInput("Subagent is disabled".into()));
            }
            let used = self.manager.get_children(&ctx.run_id).await.len() as i64;
            if used >= max_runs {
                return Err(crate::Error::InvalidInput(
                    "Subagent max_runs exceeded".into(),
                ));
            }
            let parsed = serde_json::from_str::<Vec<String>>(&tools).unwrap_or_else(|_| {
                tools
                    .split(',')
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(str::to_string)
                    .collect()
            });
            Some(parsed)
        } else {
            None
        };
        // Subagent identity is independent: never reuse parent key_id.
        // Provider/model may come from definition later; key_id is always a
        // dedicated subagent lease id (not the parent run's credential).
        let tool_allowlist = allowlist.clone().unwrap_or_default();
        let key_id = format!(
            "subagent:{}:{}",
            definition_id.unwrap_or("ephemeral"),
            uuid::Uuid::new_v4()
        );
        let child = self
            .manager
            .spawn(
                &ctx.run_id,
                prompt.clone(),
                1,
                ctx.provider_id.clone(),
                key_id,
                ctx.model.clone(),
                "ask".into(), // never inherit parent permission profile
                tool_allowlist,
                definition_id.map(str::to_string),
                Some("none".into()),
                None,
            )
            .await
            .map_err(crate::Error::InvalidInput)?;
        self.manager
            .update_status(&child.id, SubAgentStatus::Running)
            .await
            .map_err(crate::Error::Internal)?;

        let child_id = child.id.clone();
        let child_run_id = child.run_id.clone();
        let return_id = child_id.clone();
        let notify = Arc::new(Notify::new());
        self.notifications
            .lock()
            .await
            .insert(child_id.clone(), notify.clone());
        let (cancel_tx, cancel_rx) = oneshot::channel();
        self.cancels
            .lock()
            .await
            .insert(child_id.clone(), cancel_tx);

        if let Ok(conn) = crate::db::get_assistant_db_conn() {
            let now = chrono::Utc::now().to_rfc3339();
            let _ = conn.execute(
                "INSERT OR IGNORE INTO assistant_runs
                 (id, conversation_id, status, provider_id, model_id, parent_run_id,
                  subagent_definition_id, started_at)
                 VALUES (?1, ?2, 'running', ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    child_run_id,
                    ctx.session_id,
                    child.provider_id,
                    child.model_id,
                    ctx.run_id,
                    definition_id,
                    now
                ],
            );
        }

        let manager = self.manager.clone();
        let registry = self.registry.clone();
        let hooks = self.hooks.clone();
        let rules = self.rules.clone();
        let outputs = self.outputs.clone();
        let cancels = self.cancels.clone();
        let child_run_id = child_id.clone();
        let model = ctx.model.clone();
        let provider_id = ctx.provider_id.clone();
        let base_url = ctx.base_url.clone();
        let api_key = ctx.api_key.clone();
        tokio::spawn(async move {
            let mut registry = registry.lock().await.clone();
            if let Some(allowlist) = allowlist {
                for metadata in registry.list_metadata() {
                    if !allowlist.iter().any(|name| {
                        name == &metadata.name
                            || super::capability::legacy_to_claude_tool_name(name) == metadata.name
                    }) {
                        registry.set_enabled(&metadata.name, false);
                    }
                }
            }
            for name in ["Task", "TaskOutput", "KillTask"] {
                registry.set_enabled(name, false);
            }
            // G6: Old in-process AgentLoop subagent path is retired. Real child
            // runs go through Protocol v2 `task` / RunManager (independent Child Run).
            let _ = (
                model,
                provider_id,
                base_url,
                api_key,
                prompt,
                &registry,
                &hooks,
                &rules,
                &cancel_rx,
                &child_run_id,
            );
            let output =
                "Native AgentLoop subagent is retired. Use Protocol v2 task tool (Agent Daemon Child Run).".to_string();
            outputs.lock().await.insert(child_id.clone(), output);
            let status = SubAgentStatus::Failed(
                "Native AgentLoop subagent retired; use Protocol v2 task".into(),
            );
            let db_status = match &status {
                SubAgentStatus::Completed => "completed",
                SubAgentStatus::Cancelled => "interrupted",
                SubAgentStatus::Failed(_) => "failed",
                _ => "failed",
            };
            if let Ok(conn) = crate::db::get_assistant_db_conn() {
                let now = chrono::Utc::now().to_rfc3339();
                let output_text = outputs
                    .lock()
                    .await
                    .get(&child_id)
                    .cloned()
                    .unwrap_or_default();
                let _ = conn.execute(
                    "UPDATE assistant_runs SET status = ?1, finished_at = ?2 WHERE id = ?3",
                    rusqlite::params![db_status, now, child_id],
                );
                let _ = conn.execute(
                    "INSERT INTO assistant_run_events (run_id, sequence, timestamp, event_type, payload)
                     VALUES (?1, COALESCE((SELECT MAX(sequence)+1 FROM assistant_run_events WHERE run_id=?1),1), ?2, 'subagent_completed', ?3)",
                    rusqlite::params![child_id, now, json!({"output": output_text}).to_string()],
                );
            }
            let _ = manager.update_status(&child_id, status).await;
            cancels.lock().await.remove(&child_id);
            notify.notify_waiters();
        });

        Ok(return_id)
    }

    async fn output(&self, id: &str, timeout_ms: u64) -> crate::Result<serde_json::Value> {
        let notify = self.notifications.lock().await.get(id).cloned();
        let Some(notify) = notify else {
            return Err(crate::Error::InvalidInput("Subagent not found".into()));
        };
        if let Some(agent) = self.manager.get(id).await {
            if matches!(
                agent.status,
                SubAgentStatus::Queued | SubAgentStatus::Running
            ) {
                let _ = tokio::time::timeout(
                    std::time::Duration::from_millis(timeout_ms.min(30_000)),
                    notify.notified(),
                )
                .await;
            }
        }
        let agent = self
            .manager
            .get(id)
            .await
            .ok_or_else(|| crate::Error::InvalidInput("Subagent not found".into()))?;
        let output = self
            .outputs
            .lock()
            .await
            .get(id)
            .cloned()
            .unwrap_or_default();
        Ok(
            json!({"id": id, "status": format!("{:?}", agent.status).to_lowercase(), "output": output}),
        )
    }

    async fn cancel(&self, id: &str) -> crate::Result<serde_json::Value> {
        let sender = self.cancels.lock().await.remove(id);
        if let Some(sender) = sender {
            let _ = sender.send(());
        }
        let _ = self
            .manager
            .update_status(id, SubAgentStatus::Cancelled)
            .await;
        Ok(json!({"id": id, "status": "cancelled"}))
    }
}

pub struct TaskCapability {
    pub coordinator: Arc<SubagentCoordinator>,
}
pub struct TaskOutputCapability {
    pub coordinator: Arc<SubagentCoordinator>,
}
pub struct KillTaskCapability {
    pub coordinator: Arc<SubagentCoordinator>,
}

fn meta(name: &str, description: &str, schema: serde_json::Value) -> CapabilityMeta {
    CapabilityMeta::new(name, description, schema, "subagent")
        .with_permission(CapabilityPermission::Allow)
        .with_visibility(CapabilityVisibility::Public)
        .with_side_effects(vec![CapabilitySideEffect::None])
}

pub fn catalog_metadata() -> Vec<CapabilityMeta> {
    vec![
        meta(
            "Task",
            "Start a bounded child agent and return its id.",
            json!({"type":"object","properties":{"prompt":{"type":"string"},"subagent_id":{"type":"string"}},"required":["prompt"]}),
        ),
        meta(
            "TaskOutput",
            "Read or wait for a child agent result.",
            json!({"type":"object","properties":{"id":{"type":"string"},"timeout_ms":{"type":"integer"}},"required":["id"]}),
        ),
        meta(
            "KillTask",
            "Cancel a running child agent.",
            json!({"type":"object","properties":{"id":{"type":"string"}},"required":["id"]}),
        ),
    ]
}

#[async_trait]
impl AtomicCapability for TaskCapability {
    fn meta(&self) -> CapabilityMeta {
        meta(
            "Task",
            "Start a bounded child agent and return its id.",
            json!({"type":"object","properties":{"prompt":{"type":"string"},"subagent_id":{"type":"string"}},"required":["prompt"]}),
        )
    }
    async fn execute(
        &self,
        request: &CapabilityRequest,
        context: &CapabilityContext,
        cancellation: &CancellationToken,
    ) -> crate::Result<serde_json::Value> {
        let prompt = request
            .arguments
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .trim()
            .to_string();
        if prompt.is_empty() {
            return Err(crate::Error::InvalidInput(
                "Task prompt cannot be empty".into(),
            ));
        }
        let definition_id = request
            .arguments
            .get("subagent_id")
            .and_then(|v| v.as_str());
        let id = self
            .coordinator
            .spawn(context, prompt, definition_id)
            .await?;
        if cancellation.is_cancelled() {
            let _ = self.coordinator.cancel(&id).await;
        }
        Ok(json!({"id": id, "status": "running"}))
    }
}

#[async_trait]
impl AtomicCapability for TaskOutputCapability {
    fn meta(&self) -> CapabilityMeta {
        meta(
            "TaskOutput",
            "Read or wait for a child agent result.",
            json!({"type":"object","properties":{"id":{"type":"string"},"timeout_ms":{"type":"integer"}},"required":["id"]}),
        )
    }
    async fn execute(
        &self,
        request: &CapabilityRequest,
        _context: &CapabilityContext,
        _cancellation: &CancellationToken,
    ) -> crate::Result<serde_json::Value> {
        let id = request
            .arguments
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let timeout = request
            .arguments
            .get("timeout_ms")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        self.coordinator.output(id, timeout).await
    }
}

#[async_trait]
impl AtomicCapability for KillTaskCapability {
    fn meta(&self) -> CapabilityMeta {
        meta(
            "KillTask",
            "Cancel a running child agent.",
            json!({"type":"object","properties":{"id":{"type":"string"}},"required":["id"]}),
        )
    }
    async fn execute(
        &self,
        request: &CapabilityRequest,
        _context: &CapabilityContext,
        _cancellation: &CancellationToken,
    ) -> crate::Result<serde_json::Value> {
        let id = request
            .arguments
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        self.coordinator.cancel(id).await
    }
}
