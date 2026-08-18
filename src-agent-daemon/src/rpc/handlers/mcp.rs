//! `mcp.*` RPC helper functions (server id / error mapping).
//!
//! Split out of the former single-file `rpc.rs` (A2-03).

use assistant_protocol::error::{error_codes, DaemonError, ErrorCategory};
use assistant_protocol::v2::V2Request;

// {A2-03} mcp_error_to_daemon (moved verbatim from rpc.rs)
/// Map an [`McpError`] onto the daemon error envelope.
///
/// The distinction that matters is `Unsupported`: "this server does not do
/// resources" and "you sent bad arguments" are different facts, and collapsing
/// them into `invalid_input` would make a GUI show a broken-input message for a
/// server that is working exactly as advertised. `Denied` stays separate for the
/// same reason — a blocked `file://` read is a policy outcome, not a bug.
pub(crate) fn mcp_error_to_daemon(err: crate::mcp_runtime::McpError) -> DaemonError {
    use crate::mcp_runtime::McpError;
    match err {
        McpError::Invalid(m) => DaemonError::new(
            error_codes::INVALID_INPUT,
            ErrorCategory::Validation,
            false,
            m,
        ),
        McpError::NotFound(m) => {
            DaemonError::new(error_codes::NOT_FOUND, ErrorCategory::NotFound, false, m)
        }
        // Not `unsupported`: that code is reserved for methods this daemon does
        // not implement, and this method *is* implemented. The unsupported thing
        // is the remote server's capability set.
        McpError::Unsupported(m) => DaemonError::new(
            error_codes::INVALID_INPUT,
            ErrorCategory::Validation,
            false,
            m,
        ),
        McpError::Denied(m) => DaemonError::new(
            error_codes::PERMISSION_DENIED,
            ErrorCategory::PermissionDenied,
            false,
            m,
        ),
        McpError::Transport(m) => {
            DaemonError::new(error_codes::NETWORK_ERROR, ErrorCategory::Network, true, m)
        }
    }
}

// {A2-03} mcp_server_id (moved verbatim from rpc.rs)
/// Read the MCP server id from either accepted param spelling.
pub(crate) fn mcp_server_id(request: &V2Request) -> &str {
    request
        .params
        .get("server_id")
        .or_else(|| request.params.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
}

// {A2-03} dispatch_mcp: routes the `mcp.*` method family. Arms moved verbatim
// from handle_rpc's match in the former single-file rpc.rs; the trailing
// `_ =>` keeps any method that reaches this handler but has no arm here on the
// honest unsupported path (R-B1).
pub(crate) async fn dispatch_mcp(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    request: &V2Request,
) {
    use crate::rpc::{send_error, send_success};
    use assistant_protocol::error::{error_codes, DaemonError, ErrorCategory};
    use assistant_protocol::v2::methods::names;
    match request.method.as_str() {
        names::MCP_LIST => {
            let mcp = crate::mcp_runtime::global_mcp();
            let servers = mcp.list_servers();
            let tools = mcp.list_tools();
            // `capabilities` is per-server and may be null. Null means "no
            // completed handshake, we do not know" — never "supports nothing".
            // The GUI must render unknown differently from unsupported, which is
            // only possible because this field is nullable rather than defaulted.
            let capabilities: Vec<serde_json::Value> = servers
                .iter()
                .map(|s| {
                    serde_json::json!({
                        "server_id": s.id,
                        "capabilities": mcp.server_capabilities(&s.id),
                    })
                })
                .collect();
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                serde_json::json!({
                    "servers": servers,
                    "tools": tools,
                    "namespaced": mcp.namespaced_tools(),
                    "capabilities": capabilities,
                }),
            )
            .await;
        }

        names::MCP_START => {
            let id = request
                .params
                .get("id")
                .or_else(|| request.params.get("server_id"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            // task-06: refuse inline trusted server registration over RPC.
            // Only start already-registered servers from trusted config sources.
            if request.params.get("server").is_some() {
                send_error(
                    writer,
                    &request.request_id,
                    &DaemonError::new(
                        error_codes::INVALID_INPUT,
                        ErrorCategory::Validation,
                        false,
                        "inline MCP server registration disabled; register via trusted config only",
                    ),
                )
                .await;
                return;
            }
            // Transport-aware start (stdio session or HTTP/SSE probe).
            match crate::mcp_runtime::global_mcp().start(id) {
                Ok(v) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        v,
                    )
                    .await;
                }
                Err(e) => {
                    send_error(
                        writer,
                        &request.request_id,
                        &DaemonError::new(
                            error_codes::INVALID_INPUT,
                            ErrorCategory::Validation,
                            false,
                            e,
                        ),
                    )
                    .await;
                }
            }
        }

        names::MCP_STOP => {
            let id = request
                .params
                .get("id")
                .or_else(|| request.params.get("server_id"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match crate::mcp_runtime::global_mcp().stop(id) {
                Ok(()) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        serde_json::json!({ "ok": true, "id": id }),
                    )
                    .await;
                }
                Err(e) => {
                    send_error(
                        writer,
                        &request.request_id,
                        &DaemonError::new(
                            error_codes::INTERNAL_ERROR,
                            ErrorCategory::Internal,
                            true,
                            e,
                        ),
                    )
                    .await;
                }
            }
        }

        names::MCP_CALL => {
            // task-06 phase 1: close direct RPC MCP transport bypass.
            // Never call global_mcp().call_tool from RPC. Agent path uses
            // PermissionGatedTools -> shared invocation only.
            let _ = (
                request.params.get("server_id"),
                request.params.get("tool"),
                request.params.get("arguments"),
            );
            send_error(
                writer,
                &request.request_id,
                &DaemonError::new(
                    error_codes::INVALID_INPUT,
                    ErrorCategory::Validation,
                    false,
                    "direct_mcp_call_disabled",
                ),
            )
            .await;
        }

        names::MCP_LIVENESS => {
            let id = request
                .params
                .get("id")
                .or_else(|| request.params.get("server_id"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match crate::mcp_runtime::global_mcp().liveness(id) {
                Ok(v) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        v,
                    )
                    .await;
                }
                Err(e) => {
                    send_error(
                        writer,
                        &request.request_id,
                        &DaemonError::new(
                            error_codes::INVALID_INPUT,
                            ErrorCategory::Validation,
                            false,
                            e,
                        ),
                    )
                    .await;
                }
            }
        }

        names::MCP_RECONNECT => {
            let id = request
                .params
                .get("id")
                .or_else(|| request.params.get("server_id"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match crate::mcp_runtime::global_mcp().reconnect(id) {
                Ok(v) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        v,
                    )
                    .await;
                }
                Err(e) => {
                    send_error(
                        writer,
                        &request.request_id,
                        &DaemonError::new(
                            error_codes::INVALID_INPUT,
                            ErrorCategory::Validation,
                            true,
                            e,
                        ),
                    )
                    .await;
                }
            }
        }

        names::MCP_AUTH_SET => {
            let id = request
                .params
                .get("server_id")
                .or_else(|| request.params.get("id"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let token = request
                .params
                .get("token")
                .or_else(|| request.params.get("access_token"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let token_type = request
                .params
                .get("token_type")
                .and_then(|v| v.as_str())
                .unwrap_or("bearer");
            let expires_at = request.params.get("expires_at").and_then(|v| v.as_u64());
            // Never echo token back.
            match crate::mcp_runtime::global_mcp().set_auth_token(
                id,
                token.to_string(),
                token_type,
                expires_at,
            ) {
                Ok(lease) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        serde_json::to_value(lease)
                            .unwrap_or(serde_json::json!({"has_token": true})),
                    )
                    .await;
                }
                Err(e) => {
                    send_error(
                        writer,
                        &request.request_id,
                        &DaemonError::new(
                            error_codes::INVALID_INPUT,
                            ErrorCategory::Validation,
                            false,
                            e,
                        ),
                    )
                    .await;
                }
            }
        }

        names::MCP_AUTH_STATUS => {
            let id = request
                .params
                .get("server_id")
                .or_else(|| request.params.get("id"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match crate::mcp_runtime::global_mcp().auth_status(id) {
                Ok(lease) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        serde_json::to_value(lease)
                            .unwrap_or(serde_json::json!({"has_token": false})),
                    )
                    .await;
                }
                Err(e) => {
                    send_error(
                        writer,
                        &request.request_id,
                        &DaemonError::new(
                            error_codes::INVALID_INPUT,
                            ErrorCategory::Validation,
                            false,
                            e,
                        ),
                    )
                    .await;
                }
            }
        }

        names::MCP_AUTH_CLEAR => {
            let id = request
                .params
                .get("server_id")
                .or_else(|| request.params.get("id"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match crate::mcp_runtime::global_mcp().clear_auth_token(id) {
                Ok(()) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        serde_json::json!({ "ok": true, "server_id": id }),
                    )
                    .await;
                }
                Err(e) => {
                    send_error(
                        writer,
                        &request.request_id,
                        &DaemonError::new(
                            error_codes::INVALID_INPUT,
                            ErrorCategory::Validation,
                            false,
                            e,
                        ),
                    )
                    .await;
                }
            }
        }

        names::MCP_RESOURCES_LIST => {
            let cursor = request.params.get("cursor").and_then(|v| v.as_str());
            match crate::mcp_runtime::global_mcp().list_resources(mcp_server_id(request), cursor) {
                Ok(v) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        v,
                    )
                    .await;
                }
                Err(e) => send_error(writer, &request.request_id, &mcp_error_to_daemon(e)).await,
            }
        }

        names::MCP_RESOURCES_TEMPLATES_LIST => {
            match crate::mcp_runtime::global_mcp().list_resource_templates(mcp_server_id(request)) {
                Ok(v) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        v,
                    )
                    .await;
                }
                Err(e) => send_error(writer, &request.request_id, &mcp_error_to_daemon(e)).await,
            }
        }

        names::MCP_RESOURCES_READ => {
            // Human-initiated read only. There is deliberately no model-facing
            // tool for this: `tools/call` stays closed over RPC because it has
            // side effects, and a resource read is gated instead by the server's
            // own published URI set plus the scheme policy in `mcp_runtime`.
            let uri = request
                .params
                .get("uri")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match crate::mcp_runtime::global_mcp().read_resource(mcp_server_id(request), uri) {
                Ok(v) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        v,
                    )
                    .await;
                }
                Err(e) => send_error(writer, &request.request_id, &mcp_error_to_daemon(e)).await,
            }
        }

        names::MCP_PROMPTS_LIST => {
            let cursor = request.params.get("cursor").and_then(|v| v.as_str());
            match crate::mcp_runtime::global_mcp().list_prompts(mcp_server_id(request), cursor) {
                Ok(v) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        v,
                    )
                    .await;
                }
                Err(e) => send_error(writer, &request.request_id, &mcp_error_to_daemon(e)).await,
            }
        }

        names::MCP_PROMPTS_GET => {
            let name = request
                .params
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let arguments = request
                .params
                .get("arguments")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            match crate::mcp_runtime::global_mcp().get_prompt(
                mcp_server_id(request),
                name,
                arguments,
            ) {
                Ok(v) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        v,
                    )
                    .await;
                }
                Err(e) => send_error(writer, &request.request_id, &mcp_error_to_daemon(e)).await,
            }
        }

        names::MCP_ROOTS_LIST => {
            // What *we* would hand a server that asks. Empty is a real answer
            // ("no roots granted"), not a placeholder, so `source` states where
            // the set came from instead of leaving the GUI to guess.
            let roots = crate::mcp_runtime::global_mcp().client_roots();
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                serde_json::json!({
                    "roots": roots,
                    "source": if std::env::var("NATIVES_MCP_ROOTS").is_ok() {
                        "env:NATIVES_MCP_ROOTS"
                    } else {
                        "explicit"
                    },
                }),
            )
            .await;
        }

        names::MCP_NOTIFICATIONS_LIST => {
            let server_id = request
                .params
                .get("server_id")
                .or_else(|| request.params.get("id"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty());
            let items = crate::mcp_runtime::global_mcp().notifications(server_id);
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                serde_json::json!({
                    "server_id": server_id,
                    "notifications": items,
                }),
            )
            .await;
        }

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
            send_error(writer, &request.request_id, &err).await;
        }
    }
}
