//! `task.*` RPC dispatch.
//!
//! Split out of the former single-file `rpc.rs` (A2-03).

// {A2-03} dispatch_task: routes the `task.*` method family. Arms moved verbatim
// from handle_rpc's match in the former single-file rpc.rs; the trailing
// `_ =>` keeps any method that reaches this handler but has no arm here on the
// honest unsupported path (R-B1).
pub(crate) async fn dispatch_task(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    request: &assistant_protocol::v2::V2Request,
) {
    use crate::rpc::{run_manager, send_error, send_success};
    use assistant_protocol::error::{error_codes, DaemonError, ErrorCategory};
    use assistant_protocol::v2::methods::names;
    match request.method.as_str() {
        names::TASK_LIST => {
            let filter_run_id = request.params.get("run_id").and_then(|v| v.as_str());
            let filter_conversation_id = request
                .params
                .get("conversation_id")
                .and_then(|v| v.as_str());

            // Get tasks from database (persistent)
            let mut tasks = match crate::task_store::list_all_tasks() {
                Ok(db_tasks) => db_tasks,
                Err(e) => {
                    eprintln!("Failed to list tasks from DB: {e}");
                    Vec::new()
                }
            };

            // Also get in-memory tasks (for running tasks not yet persisted)
            for (task_id, rec) in run_manager().runtime.list_tasks().await {
                // Skip if already in DB results
                if tasks.iter().any(|t| t["id"].as_str() == Some(&task_id)) {
                    continue;
                }
                if let Some(want) = filter_run_id {
                    if rec.run_id != want {
                        continue;
                    }
                }
                let conversation_id = run_manager()
                    .get_run(&rec.run_id)
                    .map(|r| r.conversation_id);
                if let Some(want) = filter_conversation_id {
                    match conversation_id.as_deref() {
                        Some(cid) if cid == want => {}
                        _ => continue,
                    }
                }
                tasks.push(serde_json::json!({
                    "id": task_id,
                    "run_id": rec.run_id,
                    "conversation_id": conversation_id,
                    "status": rec.status,
                    "output": rec.output,
                    "kind": "subagent",
                }));
            }

            // Apply filters to DB results
            if filter_run_id.is_some() || filter_conversation_id.is_some() {
                tasks.retain(|t| {
                    if let Some(want) = filter_run_id {
                        if t["run_id"].as_str() != Some(want) {
                            return false;
                        }
                    }
                    if let Some(want) = filter_conversation_id {
                        if t["conversation_id"].as_str() != Some(want) {
                            return false;
                        }
                    }
                    true
                });
            }

            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                serde_json::json!({ "tasks": tasks }),
            )
            .await;
        }

        names::TASK_CANCEL => {
            let task_id = request
                .params
                .get("task_id")
                .or_else(|| request.params.get("id"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if task_id.is_empty() {
                send_error(
                    writer,
                    &request.request_id,
                    &DaemonError::new(
                        error_codes::INVALID_INPUT,
                        ErrorCategory::Validation,
                        false,
                        "task_id required for task.cancel",
                    ),
                )
                .await;
            } else {
                let cancelled = run_manager().runtime.kill_task(task_id).await;
                if cancelled {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        serde_json::json!({
                            "ok": true,
                            "cancelled": true,
                            "task_id": task_id,
                        }),
                    )
                    .await;
                } else {
                    send_error(
                        writer,
                        &request.request_id,
                        &DaemonError::new(
                            error_codes::NOT_FOUND,
                            ErrorCategory::NotFound,
                            false,
                            format!("unknown task_id: {task_id}"),
                        ),
                    )
                    .await;
                }
            }
        }

        names::TASK_WAIT => {
            let task_id = request
                .params
                .get("task_id")
                .or_else(|| request.params.get("id"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if task_id.is_empty() {
                send_error(
                    writer,
                    &request.request_id,
                    &DaemonError::new(
                        error_codes::INVALID_INPUT,
                        ErrorCategory::Validation,
                        false,
                        "task_id required for task.wait",
                    ),
                )
                .await;
            } else {
                let timeout_ms = request
                    .params
                    .get("timeout_ms")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(60_000);
                match run_manager().runtime.wait_task(&task_id, timeout_ms).await {
                    Ok(rec) => {
                        send_success(
                            writer,
                            &request.request_id,
                            &request.client_id,
                            &request.session_token,
                            serde_json::json!({
                                "id": task_id,
                                "run_id": rec.run_id,
                                "status": rec.status,
                                "output": rec.output,
                                "kind": "subagent",
                            }),
                        )
                        .await;
                    }
                    Err(e) if e == "timeout" => {
                        send_error(
                            writer,
                            &request.request_id,
                            &DaemonError::new(
                                error_codes::TIMEOUT,
                                ErrorCategory::Timeout,
                                true,
                                format!("task.wait timed out after {timeout_ms}ms: {task_id}"),
                            ),
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
