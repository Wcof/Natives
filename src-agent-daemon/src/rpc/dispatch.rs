//! Method routing skeleton for the UDS RPC server.
//!
//! `handle_rpc` matches the request method and routes to one
//! `dispatch_<domain>` function per RPC domain; the domain handlers live in
//! `handlers/` (A2-03 split from the former single-file `rpc.rs`). This file
//! holds only the routing layer and the honest `_ =>` unsupported fallback.

use assistant_protocol::error::{DaemonError, ErrorCategory};
use assistant_protocol::v1::daemon::RpcRequest;
use assistant_protocol::v2::methods::names;
use assistant_protocol::version::ProtocolVersion;

use crate::rpc::framing::send_error;
use crate::rpc::handlers::artifact::dispatch_artifact;
use crate::rpc::handlers::capability::dispatch_capability;
use crate::rpc::handlers::conversation::dispatch_conversation;
use crate::rpc::handlers::daemon::dispatch_daemon;
use crate::rpc::handlers::discovery::dispatch_discovery;
use crate::rpc::handlers::harness::dispatch_harness;
use crate::rpc::handlers::mcp::dispatch_mcp;
use crate::rpc::handlers::permission::dispatch_permission;
use crate::rpc::handlers::provider::dispatch_provider;
use crate::rpc::handlers::run::dispatch_run;
use crate::rpc::handlers::task::dispatch_task;

/// Route a single RPC request to its domain handler.
pub async fn handle_rpc(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    request: &RpcRequest,
    protocol_version: &ProtocolVersion,
    daemon_version: &str,
    started_at: &std::time::Instant,
) {
    match request.method.as_str() {
        names::DAEMON_GET_STATUS => {
            dispatch_daemon(
                writer,
                request,
                protocol_version,
                daemon_version,
                started_at,
            )
            .await
        }
        names::DAEMON_PING => {
            dispatch_daemon(
                writer,
                request,
                protocol_version,
                daemon_version,
                started_at,
            )
            .await
        }
        names::ENGINE_RATE_LIMIT_GET => {
            dispatch_daemon(
                writer,
                request,
                protocol_version,
                daemon_version,
                started_at,
            )
            .await
        }
        names::ENGINE_RATE_LIMIT_UPDATE => {
            dispatch_daemon(
                writer,
                request,
                protocol_version,
                daemon_version,
                started_at,
            )
            .await
        }
        names::ENGINE_RATE_LIMIT_ACQUIRE => {
            dispatch_daemon(
                writer,
                request,
                protocol_version,
                daemon_version,
                started_at,
            )
            .await
        }
        names::ENGINE_RATE_LIMIT_COOLDOWN => {
            dispatch_daemon(
                writer,
                request,
                protocol_version,
                daemon_version,
                started_at,
            )
            .await
        }
        names::DAEMON_GET_CAPABILITIES => {
            dispatch_daemon(
                writer,
                request,
                protocol_version,
                daemon_version,
                started_at,
            )
            .await
        }
        names::CONVERSATION_CREATE
        | names::CONVERSATION_LIST
        | names::CONVERSATION_LIST_PAGE
        | names::CONVERSATION_GET
        | names::CONVERSATION_FORK
        | names::CONVERSATION_GET_MESSAGES
        | names::CONVERSATION_GET_MESSAGES_PAGE
        | names::CONVERSATION_SEARCH_MESSAGES
        | names::CONVERSATION_APPEND_MESSAGE
        | names::CONVERSATION_RENAME
        | names::CONVERSATION_UPDATE_MODEL
        | names::CONVERSATION_UPDATE_PERMISSION
        | names::CONVERSATION_ARCHIVE
        | names::CONVERSATION_DELETE => dispatch_conversation(writer, request).await,
        names::RUN_CREATE => dispatch_run(writer, request).await,
        names::RUN_START => dispatch_run(writer, request).await,
        names::RUN_CANCEL => dispatch_run(writer, request).await,
        names::PERMISSION_RESPOND => dispatch_permission(writer, request).await,
        names::RUN_RETRY => dispatch_run(writer, request).await,
        names::RUN_CONTINUE => dispatch_run(writer, request).await,
        names::RUN_RESUME => dispatch_run(writer, request).await,
        names::RUN_REPLAY | names::RUN_GET_EVENTS => dispatch_run(writer, request).await,
        names::RUN_WATCH => dispatch_run(writer, request).await,
        names::RUN_LIST => dispatch_run(writer, request).await,
        names::RUN_LIST_CHILDREN => dispatch_run(writer, request).await,
        names::RUN_GET_ACTIVITY => dispatch_run(writer, request).await,
        names::RUN_FINISH => dispatch_run(writer, request).await,
        names::PERMISSION_LIST_PENDING => dispatch_permission(writer, request).await,
        names::AGENT_LIST => dispatch_discovery(writer, request).await,
        names::CONVERSATION_UPDATE => dispatch_conversation(writer, request).await,
        names::PROMPT_QUEUE_LIST
        | names::PROMPT_QUEUE_ENQUEUE
        | names::PROMPT_QUEUE_UPDATE
        | names::PROMPT_QUEUE_REMOVE
        | names::PROMPT_QUEUE_REORDER
        | names::PROMPT_QUEUE_SEND_NOW
        | names::PROMPT_QUEUE_INTERJECT => dispatch_permission(writer, request).await,
        names::INTERACTION_LIST_PENDING | names::INTERACTION_RESPOND => {
            dispatch_permission(writer, request).await
        }
        names::SUBAGENT_LIST | names::SUBAGENT_TOUCH | names::SUBAGENT_SWITCH_ROUTE => {
            dispatch_permission(writer, request).await
        }
        names::TOOL_LIST => dispatch_capability(writer, request).await,
        names::PROVIDER_LIST => dispatch_provider(writer, request).await,
        names::PROVIDER_DISCOVER_MODELS => dispatch_provider(writer, request).await,
        names::PROVIDER_TEST => dispatch_provider(writer, request).await,
        names::CREATIVE_LOCAL_ANALYZE => dispatch_provider(writer, request).await,
        names::PROPOSAL_LIST_PENDING => dispatch_discovery(writer, request).await,
        names::MCP_LIST => dispatch_mcp(writer, request).await,
        names::MCP_START => dispatch_mcp(writer, request).await,
        names::MCP_STOP => dispatch_mcp(writer, request).await,
        names::MCP_CALL => dispatch_mcp(writer, request).await,
        names::MCP_LIVENESS => dispatch_mcp(writer, request).await,
        names::MCP_RECONNECT => dispatch_mcp(writer, request).await,
        names::MCP_AUTH_SET => dispatch_mcp(writer, request).await,
        names::MCP_AUTH_STATUS => dispatch_mcp(writer, request).await,
        names::MCP_AUTH_CLEAR => dispatch_mcp(writer, request).await,
        names::MCP_RESOURCES_LIST => dispatch_mcp(writer, request).await,
        names::MCP_RESOURCES_TEMPLATES_LIST => dispatch_mcp(writer, request).await,
        names::MCP_RESOURCES_READ => dispatch_mcp(writer, request).await,
        names::MCP_PROMPTS_LIST => dispatch_mcp(writer, request).await,
        names::MCP_PROMPTS_GET => dispatch_mcp(writer, request).await,
        names::MCP_ROOTS_LIST => dispatch_mcp(writer, request).await,
        names::MCP_NOTIFICATIONS_LIST => dispatch_mcp(writer, request).await,
        names::ARTIFACT_LIST => dispatch_artifact(writer, request).await,
        names::TASK_LIST => dispatch_task(writer, request).await,
        names::TASK_CANCEL => dispatch_task(writer, request).await,
        names::TASK_WAIT => dispatch_task(writer, request).await,
        names::ARTIFACT_OPEN => dispatch_artifact(writer, request).await,
        names::EXTENSION_LIST => dispatch_capability(writer, request).await,
        names::CONVERSATION_UPDATE_CAPABILITIES | names::CONVERSATION_GET_CAPABILITIES => {
            dispatch_conversation(writer, request).await
        }
        method
            if method.starts_with("capability.")
                && assistant_protocol::v2::is_implemented_method(method) =>
        {
            dispatch_capability(writer, request).await
        }
        names::SKILL_LIST => dispatch_discovery(writer, request).await,
        names::MEMORY_SEARCH => dispatch_discovery(writer, request).await,
        names::MEMORY_ADD => dispatch_discovery(writer, request).await,
        "workspace.restorePreview" | "workspace.restore" => dispatch_run(writer, request).await,
        method
            if method.starts_with(names::HARNESS_PREFIX)
                || matches!(
                    method,
                    names::PROJECT_IDENTITY_REGISTER | names::PROJECT_IDENTITY_LIST
                ) =>
        {
            dispatch_harness(writer, request).await
        }
        "conversation.getContextUsage" => dispatch_conversation(writer, request).await,

        _ => {
            let status = assistant_protocol::v2::method_status(&request.method);
            let code = match status {
                assistant_protocol::v2::MethodStatus::Unsupported => "unsupported",
                assistant_protocol::v2::MethodStatus::InvalidRequest => "invalid_request",
                assistant_protocol::v2::MethodStatus::Implemented => "internal_error",
            };
            let err = DaemonError::new(
                code,
                ErrorCategory::Unsupported,
                false,
                format!(
                    "method not implemented: {} (status={status:?}; see daemon.getCapabilities)",
                    request.method
                ),
            );
            send_error(writer, &err).await;
        }
    }
}
