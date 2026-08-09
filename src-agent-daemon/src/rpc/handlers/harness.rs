//! `harness.*` RPC dispatch.
//!
//! Split out of the former single-file `rpc.rs` (A2-03).

// {A2-03} dispatch_harness: routes the `harness.*` method family. Arms moved verbatim
// from handle_rpc's match in the former single-file rpc.rs; the trailing
// `_ =>` keeps any method that reaches this handler but has no arm here on the
// honest unsupported path (R-B1).
pub(crate) async fn dispatch_harness(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    request: &assistant_protocol::v1::daemon::RpcRequest,
) {
    use crate::rpc::{harness, send_error, send_success};
    use assistant_protocol::error::{DaemonError, ErrorCategory};
    use assistant_protocol::v2::methods::names;
    match request.method.as_str() {
        // Harness control plane. The Harness family and its project identity
        // support methods share one handler, and
        // `the_harness_prefix_and_the_advertised_harness_family_agree` in the
        // protocol crate pins the prefix to exactly the advertised set, so this
        // arm can never quietly serve something that was never advertised.
        method
            if method.starts_with(names::HARNESS_PREFIX)
                || matches!(
                    method,
                    names::PROJECT_IDENTITY_REGISTER | names::PROJECT_IDENTITY_LIST
                ) =>
        {
            match harness::request(method, request.params.clone()).await {
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
                    // The Harness error already carries a structured code and a
                    // category; re-deriving either from the message here would
                    // throw that away.
                    send_error(
                        writer,
                        &DaemonError::new(e.code, e.category, false, e.message),
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
            send_error(writer, &err).await;
        }
    }
}
