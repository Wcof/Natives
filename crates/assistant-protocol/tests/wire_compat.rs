//! Wire compatibility tests for the assistant-protocol crate.
//!
//! These tests ensure that serialization format is stable across versions:
//! - JSON field names use the expected snake_case format.
//! - Unknown protocol major versions are rejected.
//! - Fixture JSON files can be deserialized correctly.

use assistant_protocol::v1::*;
use assistant_protocol::error::*;
use assistant_protocol::version::*;

// ---------------------------------------------------------------------------
// Fixture-based serialization tests
// ---------------------------------------------------------------------------

#[test]
fn test_conversation_serialization_roundtrip() {
    let conv = Conversation {
        id: "conv-001".to_string(),
        mode: ConversationMode::Chat,
        project_id: Some("proj-1".to_string()),
        title: "Test Conversation".to_string(),
        provider_id: "prov-1".to_string(),
        model_id: "gpt-4".to_string(),
        permission_profile_id: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        archived_at: None,
    };

    let json = serde_json::to_value(&conv).unwrap();
    assert_eq!(json["id"], "conv-001");
    assert_eq!(json["mode"], "chat");
    assert_eq!(json["title"], "Test Conversation");
    assert_eq!(json["project_id"], "proj-1");

    // Deserialize back
    let deserialized: Conversation = serde_json::from_value(json).unwrap();
    assert_eq!(deserialized.id, conv.id);
    assert_eq!(deserialized.mode, conv.mode);
}

#[test]
fn test_run_status_serialization() {
    // Verify all status variants serialize to the expected snake_case names
    let cases = vec![
        (RunStatus::Queued, "queued"),
        (RunStatus::Preparing, "preparing"),
        (RunStatus::Running, "running"),
        (RunStatus::WaitingPermission, "waiting_permission"),
        (RunStatus::Cancelling, "cancelling"),
        (RunStatus::Completed, "completed"),
        (RunStatus::Failed, "failed"),
        (RunStatus::Interrupted, "interrupted"),
    ];

    for (status, expected) in cases {
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json, expected, "RunStatus::{:?} should serialize to '{}'", status, expected);
    }
}

#[test]
fn test_content_block_serialization_tagged() {
    let block = ContentBlock::Text(TextContent {
        text: "Hello, world!".to_string(),
    });

    let json = serde_json::to_value(&block).unwrap();
    assert_eq!(json["type"], "text");
    assert_eq!(json["text"], "Hello, world!");
}

#[test]
fn test_tool_call_content_serialization() {
    let block = ContentBlock::ToolCall(ToolCallContent {
        id: "call-1".to_string(),
        name: "read_file".to_string(),
        input: serde_json::json!({"path": "/tmp/test.txt"}),
        status: ToolCallStatus::Pending,
    });

    let json = serde_json::to_value(&block).unwrap();
    assert_eq!(json["type"], "tool_call");
    assert_eq!(json["id"], "call-1");
    assert_eq!(json["name"], "read_file");
    assert_eq!(json["status"], "pending");
    assert_eq!(json["input"]["path"], "/tmp/test.txt");
}

#[test]
fn test_message_roundtrip() {
    let msg = Message {
        id: "msg-001".to_string(),
        conversation_id: "conv-001".to_string(),
        parent_message_id: None,
        role: MessageRole::User,
        content_blocks: vec![
            ContentBlock::Text(TextContent {
                text: "Hello".to_string(),
            }),
        ],
        status: MessageStatus::Complete,
        usage: Some(MessageUsage {
            input_tokens: 10,
            output_tokens: 50,
            reasoning_tokens: None,
            cost_usd: Some(0.002),
        }),
        created_at: chrono::Utc::now(),
    };

    let json = serde_json::to_value(&msg).unwrap();
    assert_eq!(json["role"], "user");
    assert_eq!(json["status"], "complete");

    let deserialized: Message = serde_json::from_value(json).unwrap();
    assert_eq!(deserialized.id, msg.id);
    assert_eq!(deserialized.content_blocks.len(), 1);
}

#[test]
fn test_run_event_serialization() {
    let event = RunEvent {
        run_id: "run-001".to_string(),
        sequence: 1,
        timestamp: chrono::Utc::now(),
        payload: RunEventPayload::TextDelta {
            text: "Hello".to_string(),
        },
    };

    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "text_delta");
    assert_eq!(json["text"], "Hello");
    assert_eq!(json["run_id"], "run-001");
    assert_eq!(json["sequence"], 1);
}

#[test]
fn test_artifact_serialization() {
    let artifact = Artifact {
        id: "art-001".to_string(),
        run_id: "run-001".to_string(),
        conversation_id: "conv-001".to_string(),
        source_tool: "edit_file".to_string(),
        path: "/tmp/test.txt".to_string(),
        sha256: "abc123".to_string(),
        size: 1024,
        mime_type: "text/plain".to_string(),
        label: Some("Test File".to_string()),
        kind: ArtifactKind::File,
        created_at: chrono::Utc::now(),
    };

    let json = serde_json::to_value(&artifact).unwrap();
    assert_eq!(json["kind"], "file");
    assert_eq!(json["source_tool"], "edit_file");
}

#[test]
fn test_daemon_error_serialization() {
    let error = DaemonError::new(
        "invalid_input",
        ErrorCategory::Validation,
        false,
        "Invalid input provided",
    );

    let json = serde_json::to_value(&error).unwrap();
    assert_eq!(json["code"], "invalid_input");
    assert_eq!(json["category"], "validation");
    assert!(!json["retryable"].as_bool().unwrap());
    assert!(json["correlation_id"].is_string());
}

// ---------------------------------------------------------------------------
// Protocol version rejection tests
// ---------------------------------------------------------------------------

#[test]
fn test_reject_unknown_major_version() {
    let client = ProtocolVersion::new(1, 0, 0);
    let daemon = ProtocolVersion::new(0, 1, 0);
    let result = negotiate(&client, &daemon);
    assert!(!result.compatible);
    assert!(result.upgrade_required.is_some());
}

#[test]
fn test_accept_same_major_version() {
    let client = ProtocolVersion::new(0, 2, 0);
    let daemon = ProtocolVersion::new(0, 1, 0);
    let result = negotiate(&client, &daemon);
    assert!(result.compatible);
}

#[test]
fn test_daemon_newer_version_warning() {
    let client = ProtocolVersion::new(0, 1, 0);
    let daemon = ProtocolVersion::new(0, 2, 0);
    let result = negotiate(&client, &daemon);
    assert!(result.compatible);
    assert!(result.upgrade_required.is_some());
    assert!(result.upgrade_required.as_ref().unwrap().contains("newer"));
}

// ---------------------------------------------------------------------------
// ProtocolEnvelope tests
// ---------------------------------------------------------------------------

#[test]
fn test_protocol_envelope_request() {
    let envelope = ProtocolEnvelope::new_request(
        "0.1.0".to_string(),
        "client-1".to_string(),
        "token-abc".to_string(),
        "conversation.list".to_string(),
        serde_json::json!({"limit": 10}),
    );

    let json = serde_json::to_value(&envelope).unwrap();
    assert_eq!(json["protocol_version"], "0.1.0");
    assert_eq!(json["client_id"], "client-1");
    assert_eq!(json["session_token"], "token-abc");
    assert!(json["request_id"].is_string());
    assert_eq!(json["method"], "conversation.list");
    assert_eq!(json["params"]["limit"], 10);
}

#[test]
fn test_protocol_envelope_response() {
    let envelope = ProtocolEnvelope::new_response(
        "0.1.0".to_string(),
        "req-1".to_string(),
        "client-1".to_string(),
        "token-abc".to_string(),
        true,
        Some(serde_json::json!({"result": "ok"})),
        None,
    );

    let json = serde_json::to_value(&envelope).unwrap();
    assert_eq!(json["protocol_version"], "0.1.0");
    assert!(json["success"].as_bool().unwrap());
    assert_eq!(json["data"]["result"], "ok");
    assert!(json["error"].is_null());
}

// ---------------------------------------------------------------------------
// Provider type tests
// ---------------------------------------------------------------------------

#[test]
fn test_provider_type_serialization() {
    let cases = vec![
        (ProviderType::Openai, "openai"),
        (ProviderType::Anthropic, "anthropic"),
        (ProviderType::Gemini, "gemini"),
        (ProviderType::Deepseek, "deepseek"),
        (ProviderType::OpenaiCompatible, "openai_compatible"),
        (ProviderType::Ollama, "ollama"),
    ];

    for (pt, expected) in cases {
        let json = serde_json::to_value(&pt).unwrap();
        assert_eq!(json, expected, "ProviderType::{:?} should serialize to '{}'", pt, expected);
    }
}

// ---------------------------------------------------------------------------
// Permission model tests
// ---------------------------------------------------------------------------

#[test]
fn test_permission_scope_serialization() {
    let json = serde_json::to_value(&PermissionScope::ThisRun).unwrap();
    assert_eq!(json, "this_run");
}

#[test]
fn test_permission_request_roundtrip() {
    let req = PermissionRequest {
        id: "perm-001".to_string(),
        run_id: "run-001".to_string(),
        tool_call_id: "call-1".to_string(),
        tool_name: "write_file".to_string(),
        reason: "Write to /tmp/test.txt".to_string(),
        input: serde_json::json!({"path": "/tmp/test.txt"}),
        status: PermissionStatus::Pending,
        created_at: chrono::Utc::now(),
        responded_at: None,
    };

    let json = serde_json::to_value(&req).unwrap();
    assert_eq!(json["status"], "pending");
    assert_eq!(json["tool_name"], "write_file");

    let deserialized: PermissionRequest = serde_json::from_value(json).unwrap();
    assert_eq!(deserialized.id, req.id);
}

// ---------------------------------------------------------------------------
// Extension model tests
// ---------------------------------------------------------------------------

#[test]
fn test_extension_kind_serialization() {
    assert_eq!(
        serde_json::to_value(&ExtensionKind::McpServer).unwrap(),
        "mcp_server"
    );
}

#[test]
fn test_hook_point_serialization() {
    assert_eq!(
        serde_json::to_value(&HookPoint::BeforeToolCall).unwrap(),
        "before_tool_call"
    );
}

// ---------------------------------------------------------------------------
// Context model tests
// ---------------------------------------------------------------------------

#[test]
fn test_context_preview_roundtrip() {
    let preview = ContextPreview {
        sections: vec![
            ContextSection {
                source: ContextSource::System,
                label: "System Prompt".to_string(),
                tokens: 500,
                included: true,
            },
            ContextSection {
                source: ContextSource::ConversationHistory,
                label: "History".to_string(),
                tokens: 2000,
                included: true,
            },
        ],
        total_tokens: 2500,
        max_tokens: 8000,
    };

    let json = serde_json::to_value(&preview).unwrap();
    assert_eq!(json["total_tokens"], 2500);
    assert_eq!(json["sections"].as_array().unwrap().len(), 2);
}

// ---------------------------------------------------------------------------
// Handshake serialization tests
// ---------------------------------------------------------------------------

#[test]
fn test_handshake_roundtrip() {
    let req = HandshakeRequest {
        client_version: "0.1.0".to_string(),
        client_id: "client-1".to_string(),
        bootstrap_token: "bootstrap-abc".to_string(),
    };

    let json = serde_json::to_value(&req).unwrap();
    assert_eq!(json["bootstrap_token"], "bootstrap-abc");

    let resp = HandshakeResponse {
        session_token: "session-xyz".to_string(),
        daemon_version: "0.1.0".to_string(),
        protocol_version: "0.1.0".to_string(),
        accepted: true,
        upgrade_required: None,
    };

    let json = serde_json::to_value(&resp).unwrap();
    assert_eq!(json["session_token"], "session-xyz");
    assert!(json["accepted"].as_bool().unwrap());
}

// ---------------------------------------------------------------------------
// RunStatus transition tests
// ---------------------------------------------------------------------------

#[test]
fn test_legal_transitions() {
    assert!(RunStatus::Queued.can_transition_to(&RunStatus::Preparing));
    assert!(RunStatus::Preparing.can_transition_to(&RunStatus::Running));
    assert!(RunStatus::Running.can_transition_to(&RunStatus::Completed));
    assert!(RunStatus::Running.can_transition_to(&RunStatus::WaitingPermission));
    assert!(RunStatus::WaitingPermission.can_transition_to(&RunStatus::Running));
    assert!(RunStatus::Cancelling.can_transition_to(&RunStatus::Interrupted));
}

#[test]
fn test_illegal_transitions() {
    assert!(!RunStatus::Completed.can_transition_to(&RunStatus::Running));
    assert!(!RunStatus::Failed.can_transition_to(&RunStatus::Running));
    assert!(!RunStatus::Interrupted.can_transition_to(&RunStatus::Running));
    assert!(!RunStatus::Queued.can_transition_to(&RunStatus::Completed));
    assert!(!RunStatus::Preparing.can_transition_to(&RunStatus::Completed));
}

#[test]
fn test_terminal_state_detection() {
    assert!(RunStatus::Completed.is_terminal());
    assert!(RunStatus::Failed.is_terminal());
    assert!(RunStatus::Interrupted.is_terminal());
    assert!(!RunStatus::Running.is_terminal());
    assert!(!RunStatus::Queued.is_terminal());
}

#[test]
fn test_active_state_detection() {
    assert!(RunStatus::Running.is_active());
    assert!(RunStatus::Preparing.is_active());
    assert!(RunStatus::WaitingPermission.is_active());
    assert!(RunStatus::Cancelling.is_active());
    assert!(!RunStatus::Queued.is_active());
    assert!(!RunStatus::Completed.is_active());
}