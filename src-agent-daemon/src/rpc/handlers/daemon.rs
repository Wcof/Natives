//! `daemon.*` RPC dispatch.
//!
//! Split out of the former single-file `rpc.rs` (A2-03).

// {A2-03} dispatch_daemon: routes the `daemon.*` method family. Arms moved verbatim
// from handle_rpc's match in the former single-file rpc.rs; the trailing
// `_ =>` keeps any method that reaches this handler but has no arm here on the
// honest unsupported path (R-B1).
pub(crate) async fn dispatch_daemon(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    request: &assistant_protocol::v1::daemon::RpcRequest,
    protocol_version: &assistant_protocol::version::ProtocolVersion,
    daemon_version: &str,
    started_at: &std::time::Instant,
) {
    use crate::rpc::{run_manager, send_error, send_rpc_failure, send_success};
    use assistant_protocol::error::{DaemonError, ErrorCategory};
    use assistant_protocol::v2::daemon::{DaemonStatusV2, ReadinessItem};
    use assistant_protocol::v2::methods::names;
    match request.method.as_str() {
        names::DAEMON_GET_STATUS => {
            let active_runs = run_manager()
                .list_runs(None)
                .iter()
                .filter(|r| r.status.is_active())
                .count() as u32;
            // W3 P0-03: status uses the frozen v2 DaemonStatusV2 contract —
            // decomposed readiness (health / storage / broker), NO db path,
            // NO home-absolute path leak. The Host readiness probe resolves
            // exactly these fields.
            let storage_ready = match crate::storage::open_daemon_store() {
                Ok(_) => ReadinessItem::Ready,
                Err(e) => ReadinessItem::Unavailable(e),
            };
            let broker_ready = match crate::natives_db_broker::default_broker_socket_path() {
                Ok(p) if p.exists() => ReadinessItem::Ready,
                _ => ReadinessItem::Unavailable("broker socket not present".into()),
            };
            let health = if storage_ready.is_ready() {
                ReadinessItem::Ready
            } else {
                ReadinessItem::Degraded("storage unavailable".into())
            };
            let status = DaemonStatusV2 {
                instance_id: format!("{}-{}", std::process::id(), started_at.elapsed().as_millis()),
                protocol_version: protocol_version.to_string(),
                health,
                active_runs,
                storage_ready,
                credential_broker_ready: broker_ready,
                degraded: vec![],
            };
            let value = serde_json::to_value(&status).unwrap_or_default();
            // W1: never expose a home/absolute natives.db path in status —
            // the daemon holds only broker leases and must not leak the path.
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                value,
            )
            .await;
        }

        names::DAEMON_PING => {
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                serde_json::json!({"pong": true, "timestamp": chrono::Utc::now().to_rfc3339()}),
            )
            .await;
        }

        names::ENGINE_RATE_LIMIT_GET => {
            let snapshot = if let Some(gov) = crate::global_governor() {
                gov.snapshot().await
            } else {
                crate::governor::EngineRateLimitSnapshot {
                    settings: crate::governor::EngineRateLimitSettings::default(),
                    effective_interval_ms: 0,
                    queued_requests: 0,
                    cooling_routes: 0,
                }
            };
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                serde_json::to_value(&snapshot).unwrap_or_default(),
            )
            .await;
        }

        names::ENGINE_RATE_LIMIT_UPDATE => {
            let req: crate::governor::EngineRateLimitSettings =
                match serde_json::from_value(request.params.clone()) {
                    Ok(req) => req,
                    Err(e) => {
                        send_rpc_failure(
                            writer,
                            request,
                            "invalid_params",
                            format!("Invalid parameters: {e}"),
                        )
                        .await;
                        return;
                    }
                };
            if let Err(message) = req.validate() {
                send_rpc_failure(writer, request, "invalid_params", message).await;
                return;
            }
            let encoded = match serde_json::to_string(&req) {
                Ok(encoded) => encoded,
                Err(e) => {
                    send_rpc_failure(writer, request, "serialization_failed", e.to_string()).await;
                    return;
                }
            };
            if let Err(message) =
                crate::natives_db_broker::write_setting(crate::governor::SETTINGS_KEY, &encoded)
            {
                send_rpc_failure(writer, request, "persistence_failed", message).await;
                return;
            }
            if let Some(gov) = crate::global_governor() {
                gov.update_settings(req.clone()).await;
            }
            let snapshot = if let Some(gov) = crate::global_governor() {
                gov.snapshot().await
            } else {
                crate::governor::EngineRateLimitSnapshot {
                    settings: req,
                    effective_interval_ms: 0,
                    queued_requests: 0,
                    cooling_routes: 0,
                }
            };
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                serde_json::to_value(snapshot).unwrap_or_default(),
            )
            .await;
        }

        names::ENGINE_RATE_LIMIT_ACQUIRE => {
            let provider_id = request
                .params
                .get("provider_id")
                .and_then(serde_json::Value::as_str);
            let key_id = request
                .params
                .get("key_id")
                .and_then(serde_json::Value::as_str);
            let (Some(provider_id), Some(key_id)) = (provider_id, key_id) else {
                send_rpc_failure(
                    writer,
                    request,
                    "invalid_params",
                    "provider_id and key_id are required".into(),
                )
                .await;
                return;
            };
            if let Some(governor) = crate::global_governor() {
                if let Err(message) = governor
                    .acquire(
                        provider_id,
                        key_id,
                        tokio_util::sync::CancellationToken::new(),
                    )
                    .await
                {
                    send_rpc_failure(writer, request, "rate_limit_cancelled", message).await;
                    return;
                }
            }
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                serde_json::json!({"acquired": true}),
            )
            .await;
        }

        names::ENGINE_RATE_LIMIT_COOLDOWN => {
            let provider_id = request
                .params
                .get("provider_id")
                .and_then(serde_json::Value::as_str);
            let key_id = request
                .params
                .get("key_id")
                .and_then(serde_json::Value::as_str);
            let retry_after_ms = request
                .params
                .get("retry_after_ms")
                .and_then(serde_json::Value::as_u64);
            let (Some(provider_id), Some(key_id)) = (provider_id, key_id) else {
                send_rpc_failure(
                    writer,
                    request,
                    "invalid_params",
                    "provider_id and key_id are required".into(),
                )
                .await;
                return;
            };
            if let Some(governor) = crate::global_governor() {
                governor
                    .record_rate_limit(provider_id, key_id, retry_after_ms)
                    .await;
            }
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                serde_json::json!({"recorded": true}),
            )
            .await;
        }

        names::DAEMON_GET_CAPABILITIES => {
            let caps = crate::run_manager::RunManager::capabilities();
            let mut value = serde_json::to_value(&caps).unwrap_or_default();
            // event_stream_v1: persistent run event stream (`run.watch`) with
            // after_sequence reconnect; negotiated by the host. The legacy
            // `run.subscribe` long-poll surface is retired (MIG-004).
            if let Some(obj) = value.as_object_mut() {
                obj.insert("event_stream_v1".into(), serde_json::json!(true));
                obj.insert(
                    "runtime_capabilities".into(),
                    crate::capability_resolution::runtime_capability_matrix(),
                );
            }
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                value,
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
            send_error(writer, &err).await;
        }
    }
}
