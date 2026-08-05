//! Minimal agent using the public `agent-core::facade::AgentRuntime`.
//!
//! This example is deliberately external: it depends only on the facade,
//! never on the Daemon, storage, or protocol internals. It demonstrates
//! that a consumer can run one prompt without understanding the SQLite
//! Daemon, checkpoint, or RunManager machinery.
//!
//! The runtime is **in-memory and not crash-safe** — it uses an ephemeral
//! EventSequencer and no durable store. For durable runs, use the Daemon.

use agent_core::facade::AgentRuntime;
use agent_core::{
    EngineError, EngineMessage, EngineProvider, EngineProviderEvent, EngineProviderEventStream,
    EngineToolRuntime, ToolExecutionResult, ToolSchema,
};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// A provider that always replies with a fixed text answer.
struct FixedProvider;

#[async_trait]
impl EngineProvider for FixedProvider {
    async fn stream(
        &self,
        _model: &str,
        messages: Vec<EngineMessage>,
        _tools: &[ToolSchema],
        _system_prompt: Option<&str>,
        _cancel: CancellationToken,
    ) -> Result<EngineProviderEventStream, EngineError> {
        let last_user = messages
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .map(|m| m.content.clone())
            .unwrap_or_default();
        Ok(Box::pin(futures_util::stream::iter(vec![
            EngineProviderEvent::TextDelta(format!("echo: {last_user}")),
            EngineProviderEvent::Completed,
        ])))
    }
}

/// A tool runtime exposing a single `ping` tool.
struct PingTools;

#[async_trait]
impl EngineToolRuntime for PingTools {
    async fn list_tool_schemas(&self) -> Vec<ToolSchema> {
        vec![ToolSchema {
            name: "ping".into(),
            description: "Reply with pong".into(),
            input_schema: serde_json::json!({"type": "object"}),
        }]
    }

    async fn execute_tool(
        &self,
        _name: &str,
        _input: Value,
        _cancel: &CancellationToken,
    ) -> ToolExecutionResult {
        ToolExecutionResult {
            output: serde_json::json!({"pong": true}),
            is_error: false,
            duration_ms: 1,
        }
    }
}

/// Counting tool runtime — tracks how many schemas were discovered.
struct CountingTools(Arc<AtomicUsize>);

#[async_trait]
impl EngineToolRuntime for CountingTools {
    async fn list_tool_schemas(&self) -> Vec<ToolSchema> {
        self.0.fetch_add(1, Ordering::SeqCst);
        vec![ToolSchema {
            name: "count".into(),
            description: "Count tool schema discoveries".into(),
            input_schema: serde_json::json!({"type": "object"}),
        }]
    }

    async fn execute_tool(
        &self,
        _name: &str,
        _input: Value,
        _cancel: &CancellationToken,
    ) -> ToolExecutionResult {
        ToolExecutionResult {
            output: serde_json::json!({"ok": true}),
            is_error: false,
            duration_ms: 0,
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), String> {
    // Two tools: the ping tool runtime plus one frozen schema for the echo tool.
    let runtime = AgentRuntime::builder()
        .provider(FixedProvider)
        .tool(PingTools)
        .system_prompt("You are a minimal agent.")
        .max_steps(2)
        .build()
        .map_err(|e| e.to_string())?;

    let outcome = runtime.prompt("hello").await.map_err(|e| e.to_string())?;
    println!("first run: {outcome:?}");

    // Run again to show the runtime is reusable.
    let outcome = runtime.prompt("second prompt").await.map_err(|e| e.to_string())?;
    println!("second run: {outcome:?}");

    // Tool schema discovery must happen exactly once per run.
    let count = Arc::new(AtomicUsize::new(0));
    let counting = CountingTools(count.clone());
    let rt2 = AgentRuntime::builder()
        .provider(FixedProvider)
        .tool(counting)
        .build()
        .map_err(|e| e.to_string())?;
    let _ = rt2.prompt("count").await.map_err(|e| e.to_string())?;
    // list_tool_schemas is called at most once per run.
    println!("tool schema discovery count: {}", count.load(Ordering::SeqCst));

    println!("minimal agent completed successfully");
    Ok(())
}