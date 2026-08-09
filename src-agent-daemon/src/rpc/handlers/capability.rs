//! `capability.*` RPC dispatch.
//!
//! Split out of the former single-file `rpc.rs` (A2-03).

// {A2-03} dispatch_capability: routes the `capability.*` method family. Arms moved verbatim
// from handle_rpc's match in the former single-file rpc.rs; the trailing
// `_ =>` keeps any method that reaches this handler but has no arm here on the
// honest unsupported path (R-B1).
pub(crate) async fn dispatch_capability(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    request: &assistant_protocol::v1::daemon::RpcRequest,
) {
    use crate::rpc::{send_error, send_success};
    use assistant_protocol::error::{error_codes, DaemonError, ErrorCategory};
    use assistant_protocol::v2::methods::names;
    match request.method.as_str() {
        names::TOOL_LIST => {
            let mut gateway = capability_gateway::CapabilityGateway::new();
            let _ = gateway.register_builtins();
            let tools = gateway
                .list_tools()
                .into_iter()
                .map(|t| {
                    serde_json::json!({
                        "id": t.name,
                        "description": t.description,
                        "input_schema": t.schema,
                    })
                })
                .collect::<Vec<_>>();
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                serde_json::json!({ "tools": tools }),
            )
            .await;
        }

        names::EXTENSION_LIST => {
            let items = crate::extension_store::global_extensions()
                .list()
                .into_iter()
                .map(|item| {
                    let mut value = serde_json::to_value(item).unwrap_or_default();
                    if let Some(object) = value.as_object_mut() {
                        object.insert(
                            "execution_status".into(),
                            serde_json::Value::String("discovered_not_executable".into()),
                        );
                    }
                    value
                })
                .collect::<Vec<_>>();
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                serde_json::json!({
                    "extensions": items,
                    "execution_status": "discovered_not_executable"
                }),
            )
            .await;
        }

        // Capability library configuration surface (ADR-0016). One routing arm;
        // per-method dispatch lives in capability::request to keep rpc.rs flat.
        // The is_implemented_method guard keeps catalogued-but-unimplemented
        // methods (e.g. hub before it ships) on the honest unsupported path.
        method
            if method.starts_with("capability.")
                && assistant_protocol::v2::is_implemented_method(method) =>
        {
            match crate::capability::request(method, request.params.clone()).await {
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
