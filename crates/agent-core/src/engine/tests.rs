use super::super::conversion::{
    agent_messages_to_values, engine_messages_to_values, provider_backoff_ms,
    tool_args_fingerprint, values_chars, values_to_engine_messages, MAX_PROVIDER_BACKOFF_MS,
};
use super::super::*;
use super::*;
use crate::EventPersistence;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

// Engine behaviour tests, split by domain. Each submodule starts with
// `use super::*;` so it inherits the shared imports and fixtures below.

#[path = "tests_run.rs"]
mod run;
#[path = "tests_tools.rs"]
mod tools;
#[path = "tests_hooks.rs"]
mod hooks;
#[path = "tests_provider.rs"]
mod provider;
#[path = "tests_compaction.rs"]
mod compaction;
#[path = "tests_conversion.rs"]
mod conversion;
#[path = "tests_api.rs"]
mod api;

struct FakeProvider {
    rounds: Mutex<Vec<Vec<EngineProviderEvent>>>,
}

#[async_trait::async_trait]
impl EngineProvider for FakeProvider {
    async fn stream(
        &self,
        _model: &str,
        _messages: Vec<EngineMessage>,
        _tools: &[ToolSchema],
        _system_prompt: Option<&str>,
        _cancel: CancellationToken,
    ) -> Result<EngineProviderEventStream, EngineError> {
        let mut rounds = self.rounds.lock().unwrap();
        let events = if rounds.is_empty() {
            vec![
                EngineProviderEvent::TextDelta("done".into()),
                EngineProviderEvent::Completed,
            ]
        } else {
            rounds.remove(0)
        };
        Ok(Box::pin(futures_util::stream::iter(events)))
    }
}

struct FakeTools;

#[async_trait::async_trait]
impl EngineToolRuntime for FakeTools {
    async fn list_tool_schemas(&self) -> Vec<ToolSchema> {
        vec![ToolSchema {
            name: "echo".into(),
            description: "echo".into(),
            input_schema: serde_json::json!({"type":"object"}),
        }]
    }
    async fn execute_tool(
        &self,
        name: &str,
        input: Value,
        _cancel: &CancellationToken,
    ) -> ToolExecutionResult {
        ToolExecutionResult {
            output: serde_json::json!({"tool": name, "input": input}),
            is_error: false,
            duration_ms: 1,
        }
    }
}
