//! Permission / prompt-queue interaction helpers.
//!
//! Split out of the former single-file `rpc.rs` (A2-03).

// {A2-03} filter_permission_interactions (moved verbatim from rpc.rs)
/// Narrow `interaction.listPending` rows down to permission requests and flatten the
/// stored payload into the shape the permission UI reads.
///
/// Returns a bare JSON array (not `{interactions: […]}`) — that is what the gateway
/// adapter expects from `permission.listPending`.
pub(crate) fn filter_permission_interactions(value: serde_json::Value) -> serde_json::Value {
    let rows = value
        .get("interactions")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let out: Vec<serde_json::Value> = rows
        .into_iter()
        .filter(|row| {
            row.get("kind")
                .and_then(|v| v.as_str())
                .map(|k| k.contains("permission"))
                .unwrap_or(false)
        })
        .map(|row| {
            let payload = row
                .get("payload")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            let field = |name: &str| {
                payload
                    .get(name)
                    .cloned()
                    .unwrap_or(serde_json::Value::Null)
            };
            serde_json::json!({
                "id": row.get("id").cloned().unwrap_or(serde_json::Value::Null),
                "run_id": row.get("run_id").cloned().unwrap_or(serde_json::Value::Null),
                "conversation_id": row
                    .get("conversation_id")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
                "kind": row.get("kind").cloned().unwrap_or(serde_json::Value::Null),
                "created_at": row.get("created_at").cloned().unwrap_or(serde_json::Value::Null),
                "tool_call_id": field("tool_call_id"),
                "tool_name": field("tool_name"),
                "reason": field("reason"),
                "input": field("input"),
            })
        })
        .collect();
    serde_json::Value::Array(out)
}

// {A2-03} dispatch_permission: routes the `permission.*` method family. Arms moved verbatim
// from handle_rpc's match in the former single-file rpc.rs; the trailing
// `_ =>` keeps any method that reaches this handler but has no arm here on the
// honest unsupported path (R-B1).
pub(crate) async fn dispatch_permission(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    request: &assistant_protocol::v2::V2Request,
) {
    use crate::rpc::{run_manager, send_error, send_success};
    use assistant_protocol::error::{error_codes, DaemonError, ErrorCategory};
    use assistant_protocol::v2::methods::names;
    match request.method.as_str() {
        names::PERMISSION_RESPOND => {
            let request_id = request
                .params
                .get("request_id")
                .or_else(|| request.params.get("id"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let approved = request
                .params
                .get("approved")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let run_id = request.params.get("run_id").and_then(|v| v.as_str());
            let scope = request.params.get("scope").and_then(|v| v.as_str());
            if request_id.is_empty() {
                send_error(
                    writer,
                    &request.request_id,
                    &DaemonError::new(
                        error_codes::INVALID_INPUT,
                        ErrorCategory::Validation,
                        false,
                        "request_id required for permission.respond",
                    ),
                )
                .await;
            } else {
                match run_manager()
                    .respond_permission_for_run(request_id, approved, run_id, scope)
                    .await
                {
                    Ok(()) => {
                        send_success(
                            writer,
                            &request.request_id,
                            &request.client_id,
                            &request.session_token,
                            serde_json::json!({
                                "ok": true,
                                "approved": approved,
                                "request_id": request_id,
                                "run_id": run_id,
                                "scope": scope.unwrap_or("once"),
                            }),
                        )
                        .await
                    }
                    Err(e) => {
                        send_error(
                            writer,
                            &request.request_id,
                            &DaemonError::new(
                                "permission_failed",
                                ErrorCategory::PermissionDenied,
                                false,
                                e,
                            ),
                        )
                        .await
                    }
                }
            }
        }

        names::PERMISSION_LIST_PENDING => {
            match crate::interaction_store::list_pending(request.params.clone()) {
                Ok(value) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        filter_permission_interactions(value),
                    )
                    .await
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
                    .await
                }
            }
        }

        names::PROMPT_QUEUE_LIST
        | names::PROMPT_QUEUE_ENQUEUE
        | names::PROMPT_QUEUE_UPDATE
        | names::PROMPT_QUEUE_REMOVE
        | names::PROMPT_QUEUE_REORDER
        | names::PROMPT_QUEUE_SEND_NOW
        | names::PROMPT_QUEUE_INTERJECT => {
            match crate::prompt_queue_store::request(&request.method, request.params.clone()).await
            {
                Ok(value) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        value,
                    )
                    .await
                }
                Err(e) => {
                    let code = if e.contains("not found") {
                        error_codes::NOT_FOUND
                    } else {
                        error_codes::INVALID_INPUT
                    };
                    let category = if e.contains("not found") {
                        ErrorCategory::NotFound
                    } else {
                        ErrorCategory::Validation
                    };
                    send_error(
                        writer,
                        &request.request_id,
                        &DaemonError::new(code, category, false, e),
                    )
                    .await
                }
            }
        }

        names::INTERACTION_LIST_PENDING | names::INTERACTION_RESPOND => {
            match crate::interaction_store::request(&request.method, request.params.clone()).await {
                Ok(value) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        value,
                    )
                    .await
                }
                Err(e) => {
                    let code = if e.contains("not found") {
                        error_codes::NOT_FOUND
                    } else {
                        error_codes::INVALID_INPUT
                    };
                    let category = if e.contains("not found") {
                        ErrorCategory::NotFound
                    } else {
                        ErrorCategory::Validation
                    };
                    send_error(
                        writer,
                        &request.request_id,
                        &DaemonError::new(code, category, false, e),
                    )
                    .await
                }
            }
        }

        names::SUBAGENT_LIST | names::SUBAGENT_TOUCH | names::SUBAGENT_SWITCH_ROUTE => {
            match crate::subagent_store::request(&request.method, request.params.clone()).await {
                Ok(value) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        value,
                    )
                    .await
                }
                Err(e) => {
                    let code = if e.contains("not found") {
                        error_codes::NOT_FOUND
                    } else {
                        error_codes::INVALID_INPUT
                    };
                    let category = if e.contains("not found") {
                        ErrorCategory::NotFound
                    } else {
                        ErrorCategory::Validation
                    };
                    send_error(
                        writer,
                        &request.request_id,
                        &DaemonError::new(code, category, false, e),
                    )
                    .await
                }
            }
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
