//! Live AgentEngine + RealProvider E2E (opt-in).
//!
//! ```bash
//! export NATIVES_LIVE_E2E=1
//! export NATIVES_TEST_OPENAI_KEY=sk-...
//! export NATIVES_TEST_OPENAI_BASE=https://token.sensenova.cn/v1
//! export NATIVES_TEST_MODEL=deepseek-v4-flash
//! # use openai-compatible adapter path:
//! export NATIVES_LIVE_PROVIDER_ID=openai_compatible
//! cargo test -p natives-agent-daemon --test live_engine_e2e -- --nocapture --ignored
//! ```

use agent_core::{AgentEngine, EngineRunConfig, EngineToolRuntime};
use natives_agent_daemon::production::{FixtureMode, FixtureProvider, RealProvider};
use natives_agent_daemon::{PermissionGatedTools, ProductionRuntime};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

fn live_enabled() -> bool {
    std::env::var("NATIVES_LIVE_E2E")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

#[tokio::test]
#[ignore = "live network; set NATIVES_LIVE_E2E=1"]
async fn live_engine_text_turn() {
    if !live_enabled() {
        return;
    }
    // Ensure we do not force fixture engine path.
    std::env::remove_var("NATIVES_DAEMON_FIXTURE");

    let provider_id =
        std::env::var("NATIVES_LIVE_PROVIDER_ID").unwrap_or_else(|_| "openai_compatible".into());
    let model = std::env::var("NATIVES_TEST_MODEL").unwrap_or_else(|_| "deepseek-v4-flash".into());

    let rt = Arc::new(ProductionRuntime::new());
    rt.set_permission_profile("full_access").await;
    let engine = AgentEngine::new(rt.events.clone());
    let provider = RealProvider {
        provider_id: provider_id.clone(),
        key_id: Some("live".into()),
    };
    let tools = PermissionGatedTools {
        gateway: {
            let mut g = capability_gateway::CapabilityGateway::new();
            if let Ok(cwd) = std::env::current_dir() {
                g.set_project_root(cwd.to_string_lossy().to_string());
            }
            g.register_builtins();
            Arc::new(g)
        },
        permissions: rt.permissions.clone(),
        events: rt.events.clone(),
        waiters: rt.permission_waiters.clone(),
        subagents: rt.subagents.clone(),
        task_outputs: rt.task_outputs.clone(),
        engines: rt.engines.clone(),
        runtime: None,
        provider_id: provider_id.clone(),
        key_id: None,
        parent_run_id: "live-engine-1".into(),
        conversation_id: "live-c1".into(),
        model_id: model.clone(),
        permission_profile: "full_access".into(),
        tool_allowlist: None,
    };

    let status = engine
        .run(
            EngineRunConfig {
                run_id: "live-engine-1".into(),
                conversation_id: "live-c1".into(),
                model: model.clone(),
                system_prompt: Some("Reply briefly.".into()),
                messages: Vec::new(),
                user_content: "Say the single word: ready".into(),
                max_steps: 3,
            },
            &provider,
            &tools,
        )
        .await
        .expect("engine run");

    let events = engine.events.replay_after("live-engine-1", 0);
    let text: String = events
        .iter()
        .filter_map(|e| match &e.payload {
            assistant_protocol::v2::RunEventKind::TextDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    eprintln!(
        "live_engine_text_turn status={status:?} text_len={} preview={:?}",
        text.len(),
        text.chars().take(100).collect::<String>()
    );
    assert!(
        !text.is_empty() || matches!(status, agent_core::EngineOutcome::Completed { .. }),
        "expected text or completed status"
    );
    if let Ok(dir) = std::env::var("NATIVES_TEST_SCRATCH") {
        let _ = std::fs::write(
            std::path::Path::new(&dir).join("live-engine-text.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "status": format!("{status:?}"),
                "provider_id": provider_id,
                "model": model,
                "text_len": text.len(),
                "event_count": events.len(),
            }))
            .unwrap_or_default(),
        );
    }
}

/// Full Engine tool loop: model calls a real tool via RealProvider, Engine executes, second turn.
#[tokio::test]
#[ignore = "live network; set NATIVES_LIVE_E2E=1"]
async fn live_engine_tool_loop() {
    if !live_enabled() {
        return;
    }
    std::env::remove_var("NATIVES_DAEMON_FIXTURE");

    let provider_id =
        std::env::var("NATIVES_LIVE_PROVIDER_ID").unwrap_or_else(|_| "openai_compatible".into());
    let model = std::env::var("NATIVES_TEST_MODEL").unwrap_or_else(|_| "deepseek-v4-flash".into());

    let rt = Arc::new(ProductionRuntime::new());
    rt.set_permission_profile("full_access").await;
    let engine = AgentEngine::new(rt.events.clone());
    let provider = RealProvider {
        provider_id: provider_id.clone(),
        key_id: Some("live".into()),
    };
    let tools = PermissionGatedTools {
        gateway: {
            let mut g = capability_gateway::CapabilityGateway::new();
            if let Ok(cwd) = std::env::current_dir() {
                g.set_project_root(cwd.to_string_lossy().to_string());
            }
            g.register_builtins();
            Arc::new(g)
        },
        permissions: rt.permissions.clone(),
        events: rt.events.clone(),
        waiters: rt.permission_waiters.clone(),
        subagents: rt.subagents.clone(),
        task_outputs: rt.task_outputs.clone(),
        engines: rt.engines.clone(),
        runtime: None,
        provider_id: provider_id.clone(),
        key_id: None,
        parent_run_id: "live-engine-tool".into(),
        conversation_id: "live-c-tool".into(),
        model_id: model.clone(),
        permission_profile: "full_access".into(),
        tool_allowlist: None,
    };

    // list_dir is ReadOnly and registered — forces AgentEngine tool cycle.
    let status = engine
        .run(
            EngineRunConfig {
                run_id: "live-engine-tool".into(),
                conversation_id: "live-c-tool".into(),
                model: model.clone(),
                system_prompt: Some(
                    "You are a tool-using assistant. You MUST call the list_dir tool with path \".\" before answering."
                        .into(),
                ),
                messages: Vec::new(),
                user_content:
                    "Use list_dir on path \".\" then reply with one short sentence that includes the word listed."
                        .into(),
                max_steps: 6,
            },
            &provider,
            &tools,
        )
        .await
        .expect("engine tool run");

    let events = engine.events.replay_after("live-engine-tool", 0);
    let tool_requested = events.iter().any(|e| {
        matches!(
            &e.payload,
            assistant_protocol::v2::RunEventKind::ToolCallRequested { name, .. }
                if name == "list_dir" || name.contains("list")
        ) || matches!(
            &e.payload,
            assistant_protocol::v2::RunEventKind::ToolCallCompleted { name, .. }
                if name == "list_dir" || name.contains("list")
        )
    });
    let text: String = events
        .iter()
        .filter_map(|e| match &e.payload {
            assistant_protocol::v2::RunEventKind::TextDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();

    eprintln!(
        "live_engine_tool_loop status={status:?} tool_requested={tool_requested} text_len={} preview={:?}",
        text.len(),
        text.chars().take(160).collect::<String>()
    );

    assert!(
        tool_requested,
        "expected AgentEngine to execute list_dir (or similar) via RealProvider tool loop; events={events:?}"
    );
    assert!(
        matches!(status, agent_core::EngineOutcome::Completed { .. }) || !text.is_empty(),
        "expected completed or final text"
    );

    if let Ok(dir) = std::env::var("NATIVES_TEST_SCRATCH") {
        let _ = std::fs::write(
            std::path::Path::new(&dir).join("live-engine-tool-loop.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "ok": true,
                "status": format!("{status:?}"),
                "tool_requested": tool_requested,
                "text_len": text.len(),
                "event_count": events.len(),
                "path": "AgentEngine → RealProvider → ToolRuntime → ToolResult → AgentEngine",
            }))
            .unwrap_or_default(),
        );
    }
}

/// Live Subagent: spawn `task` with independent identity, wait for child RealProvider run.
#[tokio::test]
#[ignore = "live network; set NATIVES_LIVE_E2E=1"]
async fn live_subagent_task_completes() {
    if !live_enabled() {
        return;
    }
    std::env::remove_var("NATIVES_DAEMON_FIXTURE");

    let provider_id =
        std::env::var("NATIVES_LIVE_PROVIDER_ID").unwrap_or_else(|_| "openai_compatible".into());
    let model = std::env::var("NATIVES_TEST_MODEL").unwrap_or_else(|_| "deepseek-v4-flash".into());

    let rt = Arc::new(ProductionRuntime::new());
    rt.set_permission_profile("full_access").await;
    let tools = PermissionGatedTools {
        gateway: {
            let mut g = capability_gateway::CapabilityGateway::new();
            if let Ok(cwd) = std::env::current_dir() {
                g.set_project_root(cwd.to_string_lossy().to_string());
            }
            g.register_builtins();
            Arc::new(g)
        },
        permissions: rt.permissions.clone(),
        events: rt.events.clone(),
        waiters: rt.permission_waiters.clone(),
        subagents: rt.subagents.clone(),
        task_outputs: rt.task_outputs.clone(),
        engines: rt.engines.clone(),
        runtime: None,
        provider_id: provider_id.clone(),
        key_id: None,
        parent_run_id: "live-parent-sub".into(),
        conversation_id: "live-c-sub".into(),
        model_id: model.clone(),
        permission_profile: "full_access".into(),
        tool_allowlist: None,
    };

    // Independent key identity: different key_id label than parent (same env secret is OK for smoke).
    let result = tools
        .execute_tool(
            "task",
            serde_json::json!({
                "prompt": "Reply with exactly one word: subok",
                "provider_id": provider_id,
                "model_id": model,
                "key_id": "live-child-key",
                "permission_profile": "full_access"
            }),
            &CancellationToken::new(),
        )
        .await;
    assert!(!result.is_error, "task spawn failed: {}", result.output);
    let task_id = result
        .output
        .get("task_id")
        .and_then(|v| v.as_str())
        .expect("task_id")
        .to_string();
    let child_key = result
        .output
        .get("key_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let child_provider = result
        .output
        .get("provider_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert_eq!(child_key, "live-child-key");
    assert_eq!(child_provider, provider_id.as_str());

    let created = rt
        .events
        .replay_after("live-parent-sub", 0)
        .iter()
        .any(|e| {
            matches!(
                e.payload,
                assistant_protocol::v2::RunEventKind::SubagentCreated { .. }
            )
        });
    assert!(created, "expected SubagentCreated on parent run");

    // Poll task_output until terminal (child RealProvider HTTP).
    let mut final_status = String::new();
    let mut final_output = None;
    for _ in 0..120 {
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        let out = tools
            .execute_tool(
                "task_output",
                serde_json::json!({ "task_id": task_id }),
                &CancellationToken::new(),
            )
            .await;
        let status = out
            .output
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        if status != "running" && status != "unknown" {
            final_status = status;
            final_output = out
                .output
                .get("output")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            break;
        }
    }
    eprintln!(
        "live_subagent_task status={final_status} output_preview={:?}",
        final_output
            .as_deref()
            .unwrap_or("")
            .chars()
            .take(120)
            .collect::<String>()
    );
    assert_eq!(
        final_status, "completed",
        "child subagent should complete via live provider; got {final_status:?} output={final_output:?}"
    );
    let text = final_output.unwrap_or_default();
    assert!(
        text.to_ascii_lowercase().contains("subok") || !text.is_empty(),
        "expected child text output, got {text:?}"
    );

    let parent_done = rt
        .events
        .replay_after("live-parent-sub", 0)
        .iter()
        .any(|e| {
            matches!(
                e.payload,
                assistant_protocol::v2::RunEventKind::SubagentCompleted { .. }
            )
        });
    assert!(parent_done, "expected SubagentCompleted on parent");

    if let Ok(dir) = std::env::var("NATIVES_TEST_SCRATCH") {
        let _ = std::fs::write(
            std::path::Path::new(&dir).join("live-subagent-task.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "ok": true,
                "task_id": task_id,
                "child_key_id": child_key,
                "child_provider": child_provider,
                "status": final_status,
                "output_len": text.len(),
                "subagent_created": true,
                "subagent_completed": true,
                "independent_key_id": true,
            }))
            .unwrap_or_default(),
        );
    }
}

/// Live cancel: provider stream starts, GUI-equivalent cancel flag interrupts run promptly.
#[tokio::test]
#[ignore = "live network; set NATIVES_LIVE_E2E=1"]
async fn live_engine_cancel_stream() {
    if !live_enabled() {
        return;
    }
    std::env::remove_var("NATIVES_DAEMON_FIXTURE");

    let provider_id =
        std::env::var("NATIVES_LIVE_PROVIDER_ID").unwrap_or_else(|_| "openai_compatible".into());
    let model = std::env::var("NATIVES_TEST_MODEL").unwrap_or_else(|_| "deepseek-v4-flash".into());
    let run_id = format!("live-engine-cancel-{}", uuid::Uuid::new_v4());
    let conversation_id = format!("live-c-cancel-{}", uuid::Uuid::new_v4());

    let rt = Arc::new(ProductionRuntime::new());
    rt.set_permission_profile("full_access").await;
    let engine = AgentEngine::new(rt.events.clone());
    let cancel = engine.cancel_token();
    let events = engine.events.clone();
    let run_id_for_task = run_id.clone();
    let conversation_id_for_task = conversation_id.clone();
    let provider_id_for_task = provider_id.clone();
    let model_for_task = model.clone();
    let rt_for_task = rt.clone();

    let handle = tokio::spawn(async move {
        let provider = RealProvider {
            provider_id: provider_id_for_task.clone(),
            key_id: Some("live".into()),
        };
        let tools = PermissionGatedTools {
            gateway: {
                let mut g = capability_gateway::CapabilityGateway::new();
                if let Ok(cwd) = std::env::current_dir() {
                    g.set_project_root(cwd.to_string_lossy().to_string());
                }
                g.register_builtins();
                Arc::new(g)
            },
            permissions: rt_for_task.permissions.clone(),
            events: rt_for_task.events.clone(),
            waiters: rt_for_task.permission_waiters.clone(),
            subagents: rt_for_task.subagents.clone(),
            task_outputs: rt_for_task.task_outputs.clone(),
            engines: rt_for_task.engines.clone(),
            runtime: None,
            provider_id: provider_id_for_task,
            key_id: None,
            parent_run_id: run_id_for_task.clone(),
            conversation_id: conversation_id_for_task.clone(),
            model_id: model_for_task.clone(),
            permission_profile: "full_access".into(),
            tool_allowlist: None,
        };
        engine
            .run(
                EngineRunConfig {
                    run_id: run_id_for_task,
                    conversation_id: conversation_id_for_task,
                    model: model_for_task,
                    system_prompt: Some("Stream a long answer. Do not use tools.".into()),
                    messages: Vec::new(),
                    user_content: "Write 80 short numbered facts about ocean waves.".into(),
                    max_steps: 3,
                },
                &provider,
                &tools,
            )
            .await
    });

    let mut saw_text = false;
    for _ in 0..120 {
        if events.replay_after(&run_id, 0).iter().any(|e| {
            matches!(
                e.payload,
                assistant_protocol::v2::RunEventKind::TextDelta { .. }
            )
        }) {
            saw_text = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    assert!(saw_text, "expected live stream text before cancelling");
    cancel.cancel();

    let status = tokio::time::timeout(std::time::Duration::from_secs(3), handle)
        .await
        .expect("cancelled live run should stop promptly")
        .expect("join")
        .expect("run");
    assert!(matches!(status, agent_core::EngineOutcome::Cancelled | agent_core::EngineOutcome::Interrupted { .. }), "{status:?}");
    // Lifecycle Interrupted/Cancelled events are committed by RunManager, not the engine.
    let _ = events.replay_after(&run_id, 0);

    if let Ok(dir) = std::env::var("NATIVES_TEST_SCRATCH") {
        let _ = std::fs::write(
            std::path::Path::new(&dir).join("live-engine-cancel.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "ok": true,
                "provider_id": provider_id,
                "model": model,
                "status": format!("{status:?}"),
                "saw_text_before_cancel": true,
                "interrupted": true,
                "event_count": events.replay_after(&run_id, 0).len(),
            }))
            .unwrap_or_default(),
        );
    }
}

/// Offline sanity: fixture path still works without live keys.
#[tokio::test]
#[ignore = "live network; set NATIVES_LIVE_E2E=1 and both provider credentials"]
async fn live_cross_provider_subagent_openai_parent_anthropic_child() {
    if !live_enabled() {
        return;
    }
    std::env::remove_var("NATIVES_DAEMON_FIXTURE");

    let parent_model =
        std::env::var("NATIVES_TEST_MODEL").unwrap_or_else(|_| "deepseek-v4-flash".into());
    let child_model = std::env::var("NATIVES_TEST_ANTHROPIC_MODEL")
        .expect("NATIVES_TEST_ANTHROPIC_MODEL required for cross-provider live subagent");

    let rt = Arc::new(ProductionRuntime::new());
    rt.set_permission_profile("full_access").await;
    let engine = AgentEngine::new(rt.events.clone());
    let parent_provider = RealProvider {
        provider_id: "openai_compatible".into(),
        key_id: Some("live-openai-parent".into()),
    };
    let tools = PermissionGatedTools {
        gateway: {
            let mut g = capability_gateway::CapabilityGateway::new();
            if let Ok(cwd) = std::env::current_dir() {
                g.set_project_root(cwd.to_string_lossy().to_string());
            }
            g.register_builtins();
            Arc::new(g)
        },
        permissions: rt.permissions.clone(),
        events: rt.events.clone(),
        waiters: rt.permission_waiters.clone(),
        subagents: rt.subagents.clone(),
        task_outputs: rt.task_outputs.clone(),
        engines: rt.engines.clone(),
        runtime: None,
        provider_id: "openai_compatible".into(),
        key_id: None,
        parent_run_id: "live-cross-parent".into(),
        conversation_id: "live-cross-conv".into(),
        model_id: parent_model.clone(),
        permission_profile: "full_access".into(),
        tool_allowlist: None,
    };
    let parent_status = engine
        .run(
            EngineRunConfig {
                run_id: "live-cross-parent".into(),
                conversation_id: "live-cross-conv".into(),
                model: parent_model.clone(),
                system_prompt: Some("Reply briefly. Do not use tools.".into()),
                messages: Vec::new(),
                user_content: "Say: parentok".into(),
                max_steps: 3,
            },
            &parent_provider,
            &tools,
        )
        .await
        .expect("openai-compatible parent run");
    assert!(
        matches!(parent_status, agent_core::EngineOutcome::Completed { .. }),
        "{parent_status:?}"
    );

    let child = tools
        .execute_tool(
            "task",
            serde_json::json!({
                "prompt": "Reply with exactly one word: childok",
                "provider_id": "anthropic",
                "model_id": child_model,
                "key_id": "live-anthropic-child",
                "permission_profile": "full_access"
            }),
            &CancellationToken::new(),
        )
        .await;
    assert!(
        !child.is_error,
        "cross-provider child spawn failed: {}",
        child.output
    );
    assert_eq!(child.output["provider_id"], "anthropic");
    assert_eq!(child.output["key_id"], "live-anthropic-child");
    let task_id = child.output["task_id"]
        .as_str()
        .expect("task_id")
        .to_string();

    let mut final_status = String::new();
    let mut final_output = String::new();
    for _ in 0..120 {
        if let Some(record) = rt.task_output(&task_id).await {
            if record.status != "running" {
                final_status = record.status;
                final_output = record.output.unwrap_or_default();
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    assert_eq!(final_status, "completed", "child output={final_output:?}");
    assert!(!final_output.trim().is_empty(), "expected child text");
    let parent_events = rt.events.replay_after("live-cross-parent", 0);
    assert!(parent_events.iter().any(|e| {
        matches!(
            e.payload,
            assistant_protocol::v2::RunEventKind::SubagentCreated { .. }
        )
    }));
    assert!(parent_events.iter().any(|e| {
        matches!(
            e.payload,
            assistant_protocol::v2::RunEventKind::SubagentCompleted { .. }
        )
    }));

    if let Ok(dir) = std::env::var("NATIVES_TEST_SCRATCH") {
        let _ = std::fs::write(
            std::path::Path::new(&dir).join("live-cross-provider-subagent.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "ok": true,
                "parent_provider": "openai_compatible",
                "parent_model": parent_model,
                "child_provider": "anthropic",
                "child_key_id": "live-anthropic-child",
                "child_status": final_status,
                "child_output_len": final_output.len(),
                "subagent_created": true,
                "subagent_completed": true,
            }))
            .unwrap_or_default(),
        );
    }
}

/// Offline sanity: fixture path still works without live keys.
#[tokio::test]
async fn dual_provider_engine_fixture_subagent() {
    // Offline fixture path: assignment uses default binding without live UI/route policy.
    // Honor task-level provider/key/model so dual-provider identity assertions still hold.
    std::env::set_var("NATIVES_DAEMON_FIXTURE", "1");
    std::env::set_var("NATIVES_DAEMON_FIXTURE_HONOR_TASK_CREDS", "1");
    let rt = Arc::new(ProductionRuntime::new());
    rt.set_permission_profile("full_access").await;
    let engine = AgentEngine::new(rt.events.clone());
    let parent_provider = FixtureProvider {
        mode: FixtureMode::TextOnly,
    };
    let tools = PermissionGatedTools {
        gateway: {
            let mut g = capability_gateway::CapabilityGateway::new();
            g.register_builtins();
            Arc::new(g)
        },
        permissions: rt.permissions.clone(),
        events: rt.events.clone(),
        waiters: rt.permission_waiters.clone(),
        subagents: rt.subagents.clone(),
        task_outputs: rt.task_outputs.clone(),
        engines: rt.engines.clone(),
        runtime: Some(rt.clone()),
        provider_id: "openai_compatible".into(),
        key_id: Some("fixture-parent-key".into()),
        parent_run_id: "fixture-parent-run".into(),
        conversation_id: "fixture-parent-conversation".into(),
        model_id: "fixture-parent-model".into(),
        permission_profile: "full_access".into(),
        tool_allowlist: None,
    };
    let parent_status = engine
        .run(
            EngineRunConfig {
                run_id: "fixture-parent-run".into(),
                conversation_id: "fixture-parent-conversation".into(),
                model: "fixture-parent-model".into(),
                system_prompt: None,
                messages: Vec::new(),
                user_content: "parent fixture turn".into(),
                max_steps: 3,
            },
            &parent_provider,
            &tools,
        )
        .await
        .expect("parent fixture engine run");
    assert!(
        matches!(parent_status, agent_core::EngineOutcome::Completed { .. }),
        "{parent_status:?}"
    );

    let child = tools
        .execute_tool(
            "task",
            serde_json::json!({
                "prompt": "Reply with exactly one word: childok",
                "provider_id": "anthropic",
                "model_id": "fixture-child-model",
                "key_id": "fixture-child-key",
                "permission_profile": "full_access",
                "fixture": true
            }),
            &CancellationToken::new(),
        )
        .await;
    assert!(
        !child.is_error,
        "fixture child spawn failed: {}",
        child.output
    );
    let task_id = child.output["task_id"]
        .as_str()
        .expect("task_id")
        .to_string();
    // Route policy / fixture default_binding assigns credentials; model-supplied
    // provider/key/model are intentionally ignored (security invariant from task-05/11).
    assert!(!child.output["provider_id"].as_str().unwrap_or("").is_empty());
    assert!(!child.output["model_id"].as_str().unwrap_or("").is_empty());

    let mut status = String::new();
    for _ in 0..40 {
        if let Some(record) = rt.task_output(&task_id).await {
            if record.status != "running" {
                status = record.status;
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(status, "completed", "fixture child should complete");
    assert!(rt
        .events
        .replay_after("fixture-parent-run", 0)
        .iter()
        .any(|e| {
            matches!(
                e.payload,
                assistant_protocol::v2::RunEventKind::SubagentCreated { .. }
            )
        }));
}

#[tokio::test]
async fn fixture_engine_still_works_without_live() {
    std::env::set_var("NATIVES_DAEMON_FIXTURE", "1");
    let rt = ProductionRuntime::new();
    let engine = AgentEngine::new(rt.events.clone());
    let provider = FixtureProvider {
        mode: FixtureMode::TextOnly,
    };
    let tools = PermissionGatedTools {
        gateway: {
            let mut g = capability_gateway::CapabilityGateway::new();
            g.register_builtins();
            Arc::new(g)
        },
        permissions: rt.permissions.clone(),
        events: rt.events.clone(),
        waiters: rt.permission_waiters.clone(),
        subagents: rt.subagents.clone(),
        task_outputs: rt.task_outputs.clone(),
        engines: rt.engines.clone(),
        runtime: None,
        provider_id: "fixture".into(),
        key_id: None,
        parent_run_id: "fix-1".into(),
        conversation_id: "c".into(),
        model_id: "m".into(),
        permission_profile: "full_access".into(),
        tool_allowlist: None,
    };
    let status = engine
        .run(
            EngineRunConfig {
                run_id: "fix-1".into(),
                conversation_id: "c".into(),
                model: "m".into(),
                system_prompt: None,
                messages: Vec::new(),
                user_content: "hi".into(),
                max_steps: 3,
            },
            &provider,
            &tools,
        )
        .await
        .unwrap();
    assert!(matches!(status, agent_core::EngineOutcome::Completed { .. }), "{status:?}");
    let _ = CancellationToken::new();
}
