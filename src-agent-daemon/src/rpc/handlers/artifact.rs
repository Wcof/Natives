//! `artifact.*` RPC dispatch.
//!
//! Split out of the former single-file `rpc.rs` (A2-03).

// {A2-03} dispatch_artifact: routes the `artifact.*` method family. Arms moved verbatim
// from handle_rpc's match in the former single-file rpc.rs; the trailing
// `_ =>` keeps any method that reaches this handler but has no arm here on the
// honest unsupported path (R-B1).
pub(crate) async fn dispatch_artifact(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    request: &assistant_protocol::v2::V2Request,
) {
    use crate::rpc::{send_error, send_success};
    use assistant_protocol::error::{error_codes, DaemonError, ErrorCategory};
    use assistant_protocol::v2::methods::names;
    match request.method.as_str() {
        names::ARTIFACT_LIST => {
            let run_id = request.params.get("run_id").and_then(|v| v.as_str());
            let items = crate::artifact_store::global_artifacts().list(run_id);
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                serde_json::json!({ "artifacts": items }),
            )
            .await;
        }

        names::ARTIFACT_OPEN => {
            let id = request
                .params
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match crate::artifact_store::global_artifacts().open(id) {
                Ok((meta, bytes)) => {
                    // Never return huge binaries raw if over cap — return meta + base64 preview.
                    let preview = if bytes.len() <= 64 * 1024 {
                        Some(base64::Engine::encode(
                            &base64::engine::general_purpose::STANDARD,
                            &bytes,
                        ))
                    } else {
                        None
                    };
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        serde_json::json!({
                            "artifact": meta,
                            "content_base64": preview,
                            "truncated": preview.is_none(),
                        }),
                    )
                    .await;
                }
                Err(e) => {
                    send_error(
                        writer,
                        &request.request_id,
                        &DaemonError::new(
                            error_codes::NOT_FOUND,
                            ErrorCategory::NotFound,
                            false,
                            e,
                        ),
                    )
                    .await;
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
