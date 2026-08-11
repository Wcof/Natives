//! Subagent RPC surface: `subagent.list` / `subagent.touch` /
//! `subagent.switchRoute` dispatch + the assignment-waiter wake hook.
//!
//! Split out of `subagent_store` to keep each file under 1000 lines (W10).
//! Session/route/directive/reservation items come from the parent via `super::*`.

use super::*;

/// RPC: subagent.list / subagent.switchRoute / subagent.touch
pub async fn request(method: &str, params: Value) -> Result<Value, String> {
    match method {
        "subagent.list" => {
            let parent = params
                .get("conversation_id")
                .or_else(|| params.get("parent_conversation_id"))
                .and_then(Value::as_str);
            let include_closed = params
                .get("include_closed")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let sessions = list_subagent_sessions(parent, include_closed)?;
            let policy = match parent {
                Some(p) => get_route_policy(p)?,
                None => None,
            };
            // NE-P0-08 / 19.3-⑤: field-level visibility — the parent-authored
            // pending directive text never leaves the protected store. The RPC
            // surface receives digest-only redaction (same persona identifiable
            // by digest, text unrecoverable).
            let sessions: Vec<Value> = sessions.iter().map(redact_session_for_export).collect();
            Ok(json!({
                "sessions": sessions,
                "route_policy": policy,
            }))
        }
        "subagent.touch" => {
            let subagent_id = params
                .get("subagent_id")
                .or_else(|| params.get("id"))
                .or_else(|| params.get("session_id"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let child_conversation_id = params
                .get("child_conversation_id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let conversation_id = params
                .get("conversation_id")
                .or_else(|| params.get("parent_conversation_id"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();

            // Parent-only heartbeat: conversation_id without subagent/child id.
            if subagent_id.is_empty() && child_conversation_id.is_empty() {
                if conversation_id.is_empty() {
                    return Err(
                        "conversation_id required for parent heartbeat, or pass subagent_id/child_conversation_id"
                            .into(),
                    );
                }
                touch_parent_heartbeat(&conversation_id)?;
                return Ok(json!({
                    "ok": true,
                    "parent_heartbeat": true,
                    "conversation_id": conversation_id,
                }));
            }

            if !subagent_id.is_empty() {
                touch_subagent_session(&subagent_id)?;
                return Ok(json!({ "ok": true, "id": subagent_id }));
            }
            touch_by_child_conversation(&child_conversation_id)?;
            Ok(json!({ "ok": true, "child_conversation_id": child_conversation_id }))
        }
        "subagent.switchRoute" => {
            let parent = params
                .get("conversation_id")
                .or_else(|| params.get("parent_conversation_id"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            if parent.is_empty() {
                return Err("conversation_id required for subagent.switchRoute".into());
            }
            let mode = params
                .get("mode")
                .and_then(Value::as_str)
                .unwrap_or("default");
            let bindings: Vec<RouteBinding> = params
                .get("bindings")
                .cloned()
                .and_then(|v| serde_json::from_value(v).ok())
                .or_else(|| {
                    params
                        .get("pool")
                        .cloned()
                        .and_then(|v| serde_json::from_value(v).ok())
                })
                .unwrap_or_default();
            if bindings.is_empty() {
                return Err("bindings required for subagent.switchRoute".into());
            }
            // Validate bindings before persisting.
            for b in &bindings {
                crate::production::validate_route_binding(b)?;
            }
            let policy = upsert_route_policy(&parent, mode, &bindings)?;

            let mut restarted_run_id: Option<String> = None;
            if let Some(sid) = params
                .get("session_id")
                .or_else(|| params.get("subagent_id"))
                .and_then(Value::as_str)
            {
                let next = if let Some(explicit) = params
                    .get("binding")
                    .cloned()
                    .and_then(|v| serde_json::from_value::<RouteBinding>(v).ok())
                {
                    crate::production::validate_route_binding(&explicit)?;
                    explicit
                } else {
                    pick_binding(&policy, &[])?
                };
                restarted_run_id =
                    crate::production::restart_subagent_with_binding(sid, &next).await?;
            }

            Ok(json!({
                "ok": true,
                "route_policy": policy,
                "restarted_run_id": restarted_run_id,
            }))
        }
        _ => Err(format!("unsupported subagent method: {method}")),
    }
}

/// Wake assignment waiters registered by ProductionRuntime (kind=subagent_assignment).
pub fn wake_assignment_waiter(interaction_id: &str, response: Value) -> bool {
    crate::production::wake_assignment_waiter(interaction_id, response)
}
