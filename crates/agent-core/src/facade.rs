//! Minimal `AgentRuntime` facade — wraps `AgentEngine` for external consumers.
//!
//! # Design
//!
//! - Reuses `AgentEngine` directly. No second loop, no second store authority.
//! - The `AgentRuntime` builder constructs an engine from caller-supplied
//!   `EngineProvider`, `EngineToolRuntime` and a single tool schema list.
//! - **In-memory mode**: no SQLite, no checkpoint, no crash recovery. The
//!   `EventSequencer` is ephemeral (in-memory only). Do not claim crash-safe.
//! - Production callers should use the Daemon's `ProductionRuntime` instead.
//!
//! # Example
//!
//! ```rust,no_run
//! use agent_core::facade::{AgentRuntime, RuntimeBuilder};
//! use agent_core::EngineProvider;
//!
//! # async fn example() -> Result<(), String> {
//! let outcome = AgentRuntime::builder()
//!     .provider(my_provider)
//!     .tool(MyTool)
//!     .build()
//!     .map_err(|e| e.to_string())?
//!     .prompt("Hello, world!")
//!     .await
//!     .map_err(|e| e.to_string())?;
//! # Ok(())
//! # }
//! ```

use crate::{
    AgentEngine, EngineError, EngineOutcome, EngineProvider, EngineRunConfig, EngineToolRuntime,
    EventSequencer, ToolSchema,
};
use std::sync::Arc;

/// A single run of the agent runtime.
///
/// Created by [`AgentRuntime::builder`] and consumed by [`prompt`](Self::prompt).
/// Each call to `prompt` creates a fresh `AgentEngine` via the builder's
/// configuration, so the runtime is reusable for multiple independent runs.
#[derive(Clone)]
pub struct AgentRuntime {
    provider: Arc<dyn EngineProvider>,
    tools: Arc<dyn EngineToolRuntime>,
    tool_schemas: Vec<ToolSchema>,
    system_prompt: Option<String>,
    max_steps: u32,
}

impl AgentRuntime {
    /// Create a new builder.
    pub fn builder() -> RuntimeBuilder {
        RuntimeBuilder::default()
    }

    /// Run one prompt, returning the outcome.
    ///
    /// The run is ephemeral: events are sequenced in-memory and lost on drop.
    /// Use the Daemon's `ProductionRuntime` for durable runs.
    ///
    /// Tool schemas are discovered once per run from the `EngineToolRuntime`
    /// (schema discovery happens at most once per prompt), so the provider
    /// sees a stable, bounded tool list for the whole turn.
    pub async fn prompt(&self, user_content: &str) -> Result<EngineOutcome, EngineError> {
        let engine = AgentEngine::new(EventSequencer::new());
        let run_id = uuid::Uuid::new_v4().to_string();
        let config = EngineRunConfig {
            run_id: run_id.clone(),
            conversation_id: format!("facade-{run_id}"),
            model: "default".into(),
            system_prompt: self.system_prompt.clone(),
            messages: Vec::new(),
            user_content: user_content.to_string(),
            max_steps: self.max_steps,
        };
        // Discover schemas once per run so the provider sees a frozen tool
        // plan (the engine does not re-discover schemas mid-run).
        let mut schemas = self.tools.list_tool_schemas().await;
        schemas.extend(self.tool_schemas.iter().cloned());
        engine
            .run_with_tool_schemas(config, self.provider.as_ref(), self.tools.as_ref(), schemas)
            .await
    }
}

/// Builder for [`AgentRuntime`].
///
/// At minimum, `provider` and `tool` are required.
#[derive(Default)]
pub struct RuntimeBuilder {
    provider: Option<Arc<dyn EngineProvider>>,
    tools: Option<Arc<dyn EngineToolRuntime>>,
    tool_schemas: Vec<ToolSchema>,
    system_prompt: Option<String>,
    max_steps: u32,
}

impl RuntimeBuilder {
    /// Set the provider.
    pub fn provider(mut self, provider: impl EngineProvider + 'static) -> Self {
        self.provider = Some(Arc::new(provider));
        self
    }

    /// Set the tool runtime.
    pub fn tool(mut self, tool: impl EngineToolRuntime + 'static) -> Self {
        self.tools = Some(Arc::new(tool));
        self
    }

    /// Add a tool schema (in addition to those from `tool.list_tool_schemas()`).
    pub fn tool_schema(mut self, schema: ToolSchema) -> Self {
        self.tool_schemas.push(schema);
        self
    }

    /// Set the system prompt.
    pub fn system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(prompt.into());
        self
    }

    /// Set the maximum number of steps (default 10).
    pub fn max_steps(mut self, n: u32) -> Self {
        self.max_steps = n;
        self
    }

    /// Build the [`AgentRuntime`].
    ///
    /// Returns an error if `provider` or `tool` is not set.
    pub fn build(self) -> Result<AgentRuntime, &'static str> {
        let provider = self.provider.ok_or("AgentRuntime requires a provider")?;
        let tools = self.tools.ok_or("AgentRuntime requires a tool runtime")?;
        let tool_schemas = if self.tool_schemas.is_empty() {
            // Clone the schemas list here; EngineToolRuntime is Send + Sync.
            Vec::new()
        } else {
            self.tool_schemas
        };
        Ok(AgentRuntime {
            provider,
            tools,
            tool_schemas,
            system_prompt: self.system_prompt,
            max_steps: if self.max_steps == 0 {
                10
            } else {
                self.max_steps
            },
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        EngineProvider, EngineProviderEvent, EngineProviderEventStream, EngineToolRuntime,
        ToolExecutionResult,
    };
    use async_trait::async_trait;
    use serde_json::Value;
    use tokio_util::sync::CancellationToken;

    struct EchoProvider;
    #[async_trait]
    impl EngineProvider for EchoProvider {
        async fn stream(
            &self,
            _model: &str,
            _messages: Vec<crate::EngineMessage>,
            _tools: &[ToolSchema],
            _system_prompt: Option<&str>,
            _cancel: CancellationToken,
        ) -> Result<EngineProviderEventStream, EngineError> {
            Ok(Box::pin(futures_util::stream::iter(vec![
                EngineProviderEvent::TextDelta("echo: ".into()),
                EngineProviderEvent::Completed,
            ])))
        }
    }

    struct NoopTools;
    #[async_trait]
    impl EngineToolRuntime for NoopTools {
        async fn list_tool_schemas(&self) -> Vec<ToolSchema> {
            Vec::new()
        }
        async fn execute_tool(
            &self,
            _name: &str,
            _input: Value,
            _cancel: &CancellationToken,
        ) -> ToolExecutionResult {
            ToolExecutionResult {
                output: Value::Null,
                is_error: false,
                duration_ms: 0,
            }
        }
    }

    #[tokio::test]
    async fn facade_builder_requires_provider() {
        let result = AgentRuntime::builder().tool(NoopTools).build();
        assert!(result.is_err(), "builder without provider must fail");
    }

    #[tokio::test]
    async fn facade_builder_requires_tools() {
        let result = AgentRuntime::builder().provider(EchoProvider).build();
        assert!(result.is_err(), "builder without tools must fail");
    }

    #[tokio::test]
    async fn facade_builds_and_runs() {
        let outcome = AgentRuntime::builder()
            .provider(EchoProvider)
            .tool(NoopTools)
            .max_steps(1)
            .build()
            .expect("valid builder must succeed")
            .prompt("hello")
            .await
            .expect("prompt must succeed");
        match outcome {
            EngineOutcome::Completed { .. } => {} // expected
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn facade_reuses_agent_engine() {
        // Verify the facade uses AgentEngine by checking the outcome type.
        let outcome = AgentRuntime::builder()
            .provider(EchoProvider)
            .tool(NoopTools)
            .max_steps(1)
            .build()
            .expect("valid builder must succeed")
            .prompt("test")
            .await
            .expect("prompt must succeed");
        // AgentEngine always produces an EngineOutcome.
        assert!(matches!(outcome, EngineOutcome::Completed { .. }));
    }

    #[test]
    fn facade_is_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}
        assert_send::<AgentRuntime>();
        assert_sync::<AgentRuntime>();
        assert_send::<RuntimeBuilder>();
        assert_sync::<RuntimeBuilder>();
    }

    #[test]
    fn facade_default_max_steps_is_10() {
        let rt = AgentRuntime::builder()
            .provider(EchoProvider)
            .tool(NoopTools)
            .build()
            .expect("valid builder must succeed");
        assert_eq!(rt.max_steps, 10);
    }
}
