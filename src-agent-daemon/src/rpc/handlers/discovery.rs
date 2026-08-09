//! Agent profile discovery helper.
//!
//! Split out of the former single-file `rpc.rs` (A2-03).

// {A2-03} discover_agent_profiles (moved verbatim from rpc.rs)
/// `agent.list` — discover declarative Agent Profiles on disk.
///
/// Discovery itself lives in `agent_core::list_agent_profiles` so that the ids reported
/// here are exactly the ids `agent_core::load_agent_profile` can resolve. This function
/// only projects to wire JSON and re-attaches `sourcePath`, which `AgentProfile` skips
/// during serialization.
pub(crate) fn discover_agent_profiles(
    project_root: Option<&std::path::Path>,
) -> Vec<serde_json::Value> {
    agent_core::list_agent_profiles(project_root)
        .into_iter()
        .map(|profile| {
            let source_path = profile
                .source_path
                .as_ref()
                .map(|p| p.to_string_lossy().to_string());
            let mut value = serde_json::to_value(&profile).unwrap_or_default();
            if let (Some(obj), Some(path)) = (value.as_object_mut(), source_path) {
                obj.insert("sourcePath".into(), serde_json::Value::String(path));
            }
            value
        })
        .collect()
}

// {A2-03} dispatch_discovery: routes the `discovery.*` method family. Arms moved verbatim
// from handle_rpc's match in the former single-file rpc.rs; the trailing
// `_ =>` keeps any method that reaches this handler but has no arm here on the
// honest unsupported path (R-B1).
pub(crate) async fn dispatch_discovery(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    request: &assistant_protocol::v1::daemon::RpcRequest,
) {
    use crate::rpc::{send_error, send_success};
    use assistant_protocol::error::{error_codes, DaemonError, ErrorCategory};
    use assistant_protocol::v2::methods::names;
    match request.method.as_str() {
        names::AGENT_LIST => {
            let project = request
                .params
                .get("project_path")
                .or_else(|| request.params.get("projectPath"))
                .and_then(|v| v.as_str())
                .map(std::path::PathBuf::from);
            let agents = discover_agent_profiles(project.as_deref());
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                serde_json::json!({ "agents": agents }),
            )
            .await;
        }

        names::PROPOSAL_LIST_PENDING => {
            // T06: serve pending proposal facts to the Host. The Host pulls
            // these over UDS, validates each, and persists its approval inbox.
            // A fact is only served while its status is `pending`, so a decided
            // fact is never re-served after a Host restart.
            let store = crate::run_manager::global_run_manager()
                .data_store_ref()
                .ok_or_else(|| "no data store available for proposal facts".to_string());
            match store.and_then(|s| crate::proposal_fact::list_pending_proposal_facts(&s)) {
                Ok(facts) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        serde_json::json!({ "proposals": facts }),
                    )
                    .await;
                }
                Err(e) => {
                    send_error(
                        writer,
                        &DaemonError::new(
                            error_codes::INTERNAL_ERROR,
                            ErrorCategory::Internal,
                            true,
                            format!("proposal.listPending: {e}"),
                        ),
                    )
                    .await;
                }
            }
        }

        names::SKILL_LIST => {
            let project = request
                .params
                .get("project_path")
                .and_then(|v| v.as_str())
                .map(std::path::PathBuf::from);
            if let Some(p) = project.as_deref() {
                crate::skill_store::global_skills().discover_for_project(Some(p));
            }
            let items = crate::skill_store::global_skills().list();
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                serde_json::json!({ "skills": items }),
            )
            .await;
        }

        names::MEMORY_SEARCH => {
            let query = request
                .params
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let limit = request
                .params
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(10) as usize;
            let hits = crate::memory_store::global_memory().search(query, limit);
            send_success(
                writer,
                &request.request_id,
                &request.client_id,
                &request.session_token,
                serde_json::json!({ "hits": hits }),
            )
            .await;
        }

        names::MEMORY_ADD => {
            let text = request
                .params
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let scope = request
                .params
                .get("scope")
                .and_then(|v| v.as_str())
                .unwrap_or("workspace");
            let project_path = request
                .params
                .get("project_path")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let tags = request
                .params
                .get("tags")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            match crate::memory_store::global_memory().add(scope, project_path, text, tags) {
                Ok(entry) => {
                    send_success(
                        writer,
                        &request.request_id,
                        &request.client_id,
                        &request.session_token,
                        serde_json::to_value(entry).unwrap_or_default(),
                    )
                    .await;
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
            send_error(writer, &err).await;
        }
    }
}
