//! `conversation.*` RPC handlers and helpers.
//!
//! Split out of the former single-file `rpc.rs` (A2-03).

use crate::rpc::required_param;

// {A2-03} handle_conversation_update (moved verbatim from rpc.rs)
/// `conversation.update` — generic partial update.
///
/// Composed from the existing single-field conversation_store commands so there is
/// exactly one SQL writer per field. Unknown/absent fields are simply not applied;
/// an update naming no known field is a validation error rather than a silent no-op.
pub(crate) async fn handle_conversation_update(
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let id = required_param(&params, &["id", "conversation_id", "conversationId"])?.to_string();
    let get = |names: &[&str]| -> Option<String> {
        names
            .iter()
            .find_map(|n| params.get(*n).and_then(|v| v.as_str()))
            .map(|s| s.to_string())
    };
    let mut applied: Vec<&str> = Vec::new();

    if let Some(title) = get(&["title", "name"]) {
        crate::conversation_store::request(
            assistant_protocol::v2::methods::names::CONVERSATION_RENAME,
            serde_json::json!({ "id": id, "title": title }),
        )
        .await?;
        applied.push("title");
    }

    let provider_id = get(&["provider_id", "providerId"]);
    let model_id = get(&["model_id", "modelId"]);
    match (&provider_id, &model_id) {
        (Some(provider_id), Some(model_id)) => {
            crate::conversation_store::request(
                assistant_protocol::v2::methods::names::CONVERSATION_UPDATE_MODEL,
                serde_json::json!({
                    "id": id,
                    "provider_id": provider_id,
                    "model_id": model_id,
                }),
            )
            .await?;
            applied.push("provider_id");
            applied.push("model_id");
        }
        // Partial model routing would leave the conversation pointing at a model the
        // provider does not serve — refuse instead of half-applying.
        (Some(_), None) | (None, Some(_)) => {
            return Err("provider_id and model_id must be updated together".into())
        }
        (None, None) => {}
    }

    if let Some(profile) = get(&[
        "permission_profile_id",
        "permissionProfileId",
        "permission_profile",
    ]) {
        crate::conversation_store::request(
            assistant_protocol::v2::methods::names::CONVERSATION_UPDATE_PERMISSION,
            serde_json::json!({ "id": id, "permission_profile_id": profile }),
        )
        .await?;
        applied.push("permission_profile_id");
    }

    if applied.is_empty() {
        return Err(
            "conversation.update requires at least one of: title, provider_id+model_id, \
             permission_profile_id"
                .into(),
        );
    }
    // Return the fresh row so callers do not have to re-read.
    crate::conversation_store::request(
        assistant_protocol::v2::methods::names::CONVERSATION_GET,
        serde_json::json!({ "id": id }),
    )
    .await
    .map(|conversation| serde_json::json!({ "id": id, "updated": applied, "conversation": conversation }))
}

// {A2-03} handle_context_usage_rpc (moved verbatim from rpc.rs)
pub(crate) fn handle_context_usage_rpc(
    params: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    use crate::checkpoint::estimate_context_usage;
    let conversation_id = params
        .get("conversation_id")
        .or_else(|| params.get("id"))
        .and_then(|v| v.as_str())
        .ok_or("conversation_id is required")?;
    // Load messages from daemon store and estimate.
    let history = crate::conversation_store::engine_history(conversation_id).unwrap_or_default();
    let mut conv_chars = 0usize;
    let mut tool_chars = 0usize;
    for m in &history {
        let n = m.content.len();
        if m.role == "tool" {
            tool_chars += n;
        } else {
            conv_chars += n;
        }
    }
    let max_tokens = params
        .get("max_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(128_000);
    let mut usage = estimate_context_usage(0, conv_chars, tool_chars, max_tokens);
    if let Some(obj) = usage.as_object_mut() {
        obj.insert(
            "conversationId".into(),
            serde_json::Value::String(conversation_id.to_string()),
        );
        obj.insert(
            "conversation_id".into(),
            serde_json::Value::String(conversation_id.to_string()),
        );
        if let Some(used) = obj.get("used_tokens").cloned() {
            obj.insert("usedTokens".into(), used);
        }
        if let Some(max) = obj.get("max_tokens").cloned() {
            obj.insert("maxTokens".into(), max);
        }
    }
    Ok(usage)
}

// {A2-03} dispatch_conversation: routes the `conversation.*` method family. Arms moved verbatim
// from handle_rpc's match in the former single-file rpc.rs; the trailing
// `_ =>` keeps any method that reaches this handler but has no arm here on the
// honest unsupported path (R-B1).
pub(crate) async fn dispatch_conversation(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    request: &assistant_protocol::v1::daemon::RpcRequest,
) {
    use crate::rpc::{send_error, send_success};
    use assistant_protocol::error::{error_codes, DaemonError, ErrorCategory};
    use assistant_protocol::v2::methods::names;
    match request.method.as_str() {
        names::CONVERSATION_CREATE
        | names::CONVERSATION_LIST
        // Paged variants are advertised in IMPLEMENTED_METHODS and handled by
        // conversation_store::request — they must be routed here or they fall through
        // to the fail-closed arm and surface as `internal_error`. The frontend calls
        // both with a silent `.catch()` fallback to an unpaged fetch, so the failure
        // showed up as a performance regression rather than a visible error.
        | names::CONVERSATION_LIST_PAGE
        | names::CONVERSATION_GET
        | names::CONVERSATION_FORK
        | names::CONVERSATION_GET_MESSAGES
        | names::CONVERSATION_GET_MESSAGES_PAGE
        | names::CONVERSATION_APPEND_MESSAGE
        | names::CONVERSATION_RENAME
        | names::CONVERSATION_UPDATE_MODEL
        | names::CONVERSATION_UPDATE_PERMISSION
        | names::CONVERSATION_ARCHIVE
        | names::CONVERSATION_DELETE => {
            match crate::conversation_store::request(&request.method, request.params.clone()).await
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
                    send_error(
                        writer,
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

        names::CONVERSATION_UPDATE => {
            match handle_conversation_update(request.params.clone()).await {
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
                    send_error(
                        writer,
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

        // Conversation-level capability selection (ADR-0016).
        names::CONVERSATION_UPDATE_CAPABILITIES | names::CONVERSATION_GET_CAPABILITIES => {
            match crate::capability_resolution::handle_conversation_rpc(
                &request.method,
                &request.params,
            ) {
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
                    send_error(writer, &DaemonError::new(code, category, false, e)).await
                }
            }
        }

        "conversation.getContextUsage" => match handle_context_usage_rpc(&request.params) {
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
                send_error(
                    writer,
                    &DaemonError::new(
                        error_codes::INVALID_INPUT,
                        ErrorCategory::Validation,
                        false,
                        e,
                    ),
                )
                .await
            }
        },

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
