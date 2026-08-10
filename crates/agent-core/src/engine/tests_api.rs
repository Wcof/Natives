use super::*;

// ---- Characterization tests for public API --------------------------------
// These verify that the engine.rs module split preserved the public API types,
// traits, and functions. They are compile-time checks that fail if a public
// item was removed or renamed during the refactor.

#[test]
fn engine_public_api_types_are_accessible() {
    // EngineMessage and related types
    let _msg = EngineMessage::text("user", "hello");
    let _img = EngineImage::new("data:image/png;base64,test");
    let _call = EngineToolCall {
        id: "call-1".into(),
        name: "read_file".into(),
        arguments: r#"{}"#.into(),
    };
    let _event = EngineProviderEvent::TextDelta("delta".into());

    // EngineRunConfig
    let _config = EngineRunConfig {
        run_id: "test".into(),
        conversation_id: "conv".into(),
        model: "gpt-4o".into(),
        system_prompt: None,
        messages: vec![EngineMessage::text("user", "hello")],
        user_content: "hello".into(),
        max_steps: 3,
    };

    // EngineProviderContext
    let _ctx = EngineProviderContext {
        run_id: "test".into(),
        attempt: 0,
    };

    // AgentEngine
    let _engine = AgentEngine::new(EventSequencer::memory_only());

    // EngineError
    let _err = EngineError::Message("test".into());
    let _code = _err.code();
    let _retryable = _err.retryable();
    let _rate_limited = _err.is_rate_limited();
    let _overflow = _err.is_context_overflow();
}

#[test]
fn engine_public_traits_are_accessible() {
    // EngineToolRuntime trait
    struct TestTools;
    #[async_trait::async_trait]
    impl EngineToolRuntime for TestTools {
        async fn list_tool_schemas(&self) -> Vec<ToolSchema> {
            vec![ToolSchema {
                name: "echo".into(),
                description: "echo".into(),
                input_schema: serde_json::json!({"type":"object"}),
            }]
        }
        async fn execute_tool(
            &self,
            _name: &str,
            _input: serde_json::Value,
            _cancel: &CancellationToken,
        ) -> ToolExecutionResult {
            ToolExecutionResult {
                output: serde_json::json!({}),
                is_error: false,
                duration_ms: 0,
            }
        }
    }

    // EngineProvider trait
    struct TestProvider;
    #[async_trait::async_trait]
    impl EngineProvider for TestProvider {
        async fn stream(
            &self,
            _model: &str,
            _messages: Vec<EngineMessage>,
            _tools: &[ToolSchema],
            _system_prompt: Option<&str>,
            _cancel: CancellationToken,
        ) -> Result<EngineProviderEventStream, EngineError> {
            Ok(Box::pin(futures_util::stream::iter(vec![
                EngineProviderEvent::TextDelta("ok".into()),
                EngineProviderEvent::Completed,
            ])))
        }
    }

    // Construct the trait impls so dead-code does not fire (their purpose is
    // to prove the public traits are implementable).
    let _tools = TestTools;
    let _provider = TestProvider;

    // ToolProgressSink trait
    struct TestSink;
    #[async_trait::async_trait]
    impl ToolProgressSink for TestSink {
        async fn publish(&self, _update: ToolProgressUpdate) {}
    }

    let _sink = NoopToolProgressSink;
    let _sink_ref: &dyn ToolProgressSink = &_sink;
    let _sink_arc: Arc<dyn ToolProgressSink> = Arc::new(TestSink);
}

#[test]
fn engine_conversion_functions_are_accessible() {
    // engine_messages_to_agent_messages
    let engine_msgs = vec![EngineMessage::text("user", "hello")];
    let agent_msgs = engine_messages_to_agent_messages(&engine_msgs);
    assert_eq!(agent_msgs.len(), 1);

    // agent_messages_to_engine_messages
    let back = agent_messages_to_engine_messages(&agent_msgs);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].content, "hello");

    // agent_messages_from_json / try_agent_messages_from_json
    // The typed snapshot format requires specific fields.
    let json = serde_json::json!([{
        "role": "user",
        "message_id": "m-1",
        "content": [{"type": "text", "text": "hello"}]
    }]);
    let from_json = agent_messages_from_json(&json);
    assert_eq!(from_json.len(), 1);
    let try_from = try_agent_messages_from_json(&json);
    assert!(
        try_from.is_ok(),
        "try_agent_messages_from_json failed: {:?}",
        try_from.err()
    );

    // ToolCapability and ToolExecutionMode
    let _cap = ToolCapability {
        name: "read_file".into(),
        schema: serde_json::json!({"type":"object"}),
        execution_mode: ToolExecutionMode::Sequential,
        side_effect: ToolSideEffect::Write,
        conflict_key: None,
    };
}

#[test]
fn engine_agentengine_builder_chain_compiles() {
    let engine = AgentEngine::new(EventSequencer::memory_only())
        .with_model_compaction(false)
        .with_cancel_token(CancellationToken::new())
        .with_context_budget(50_000, 4_000)
        .with_provider_context_window(Some(128_000));

    let _token = engine.cancel_token();
    let _cancelled = engine.is_cancelled();
    // Note: request_cancel would cancel the token, so we don't call it here.
}

#[test]
fn engine_public_types_have_expected_sizes() {
    // Smoke-check that the types compile and have reasonable sizes.
    use std::mem::size_of;

    // Core message types
    assert!(size_of::<EngineMessage>() > 0);
    assert!(size_of::<EngineImage>() > 0);
    assert!(size_of::<EngineToolCall>() > 0);

    // Engine configuration
    assert!(size_of::<EngineRunConfig>() > 0);

    // Provider types
    assert!(size_of::<EngineProviderContext>() > 0);
    assert!(size_of::<EngineError>() > 0);

    // Tool types
    assert!(size_of::<ToolSchema>() > 0);
    assert!(size_of::<ToolCapability>() > 0);
    assert!(size_of::<ToolExecutionResult>() > 0);
    assert!(size_of::<ToolProgressUpdate>() > 0);
}
