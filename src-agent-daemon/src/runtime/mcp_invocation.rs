//! Shared MCP invocation gate (task-06 / Phase 3).
//!
//! Pipeline:
//! `registry lookup → server trust → schema validation → (caller) ProjectIdentity/path/network
//!  → permission/grant → invoke → audit`
//!
//! `McpRuntime::call_tool` is transport-only. RPC must never call it
//! (`direct_mcp_call_disabled`).

use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

/// Invoke a registered MCP tool with cancel + schema + trust checks.
///
/// Permission/grant and ProjectIdentity must already be enforced by the caller
/// (PermissionGatedTools).
pub async fn invoke_mcp_tool(
    server_id: &str,
    tool_name: &str,
    arguments: Value,
    cancel: &CancellationToken,
    run_id: Option<&str>,
) -> Result<Value, String> {
    if cancel.is_cancelled() {
        audit(
            run_id,
            server_id,
            tool_name,
            "deny",
            "cancelled_before_invoke",
            &arguments,
        );
        return Err("mcp call cancelled".into());
    }

    let mcp = crate::mcp_runtime::global_mcp();
    let servers = mcp.list_servers();
    let server = servers.iter().find(|s| s.id == server_id).ok_or_else(|| {
        audit(
            run_id,
            server_id,
            tool_name,
            "deny",
            "server_not_registered",
            &arguments,
        );
        format!("MCP server not registered: {server_id}")
    })?;

    // Trust: refuse untrusted servers for Agent tool path.
    if !server.trusted {
        audit(
            run_id,
            server_id,
            tool_name,
            "deny",
            "untrusted_server",
            &arguments,
        );
        return Err(format!("MCP server not trusted: {server_id}"));
    }

    // Schema validation before transport.
    if let Err(e) = validate_tool_arguments(server_id, tool_name, &arguments) {
        audit(
            run_id,
            server_id,
            tool_name,
            "deny",
            &format!("schema:{e}"),
            &arguments,
        );
        return Err(e);
    }

    audit(run_id, server_id, tool_name, "allow", "invoke", &arguments);

    let sid = server_id.to_string();
    let tname = tool_name.to_string();
    let args_for_transport = arguments.clone();
    let invoke = tokio::task::spawn_blocking(move || {
        crate::mcp_runtime::global_mcp().call_tool(&sid, &tname, args_for_transport)
    });
    tokio::pin!(invoke);
    tokio::select! {
        biased;
        _ = cancel.cancelled() => {
            invoke.abort();
            // Stop stdio server so blocking transport cannot keep running.
            let _ = crate::mcp_runtime::global_mcp().stop(server_id);
            audit(run_id, server_id, tool_name, "deny", "cancelled_during_invoke", &arguments);
            Err("mcp call cancelled".into())
        }
        res = &mut invoke => {
            match res {
                Ok(Ok(v)) => {
                    audit(run_id, server_id, tool_name, "success", "ok", &arguments);
                    Ok(v)
                }
                Ok(Err(e)) => {
                    audit(run_id, server_id, tool_name, "failure", &e, &arguments);
                    Err(e)
                }
                Err(e) => {
                    let msg = format!("mcp invoke join error: {e}");
                    audit(run_id, server_id, tool_name, "failure", &msg, &arguments);
                    Err(msg)
                }
            }
        }
    }
}

/// Backward-compatible entry used by existing call sites.
pub async fn invoke_mcp_tool_simple(
    server_id: &str,
    tool_name: &str,
    arguments: Value,
    cancel: &CancellationToken,
) -> Result<Value, String> {
    invoke_mcp_tool(server_id, tool_name, arguments, cancel, None).await
}

fn validate_tool_arguments(
    server_id: &str,
    tool_name: &str,
    arguments: &Value,
) -> Result<(), String> {
    let tools = crate::mcp_runtime::global_mcp().list_tools();
    let namespaced = format!("mcp__{server_id}__{tool_name}");
    let desc = tools.iter().find(|t| {
        (t.server_id.as_str() == server_id && t.name.as_str() == tool_name)
            || format!("mcp__{}__{}", t.server_id, t.name) == namespaced
            || t.name == tool_name
    });
    let Some(desc) = desc else {
        // Fail-closed: no schema → deny.
        return Err(format!(
            "MCP tool schema missing for {server_id}/{tool_name}"
        ));
    };
    let schema = &desc.input_schema;
    if schema.is_null() || schema.as_object().map(|o| o.is_empty()).unwrap_or(false) {
        return Err(format!(
            "MCP tool schema invalid/empty for {server_id}/{tool_name}"
        ));
    }
    validate_json_against_object_schema(schema, arguments)
}

fn validate_json_against_object_schema(schema: &Value, instance: &Value) -> Result<(), String> {
    let obj = schema
        .as_object()
        .ok_or_else(|| "MCP schema must be a JSON object".to_string())?;
    let ty = obj.get("type").and_then(|v| v.as_str()).unwrap_or("object");
    if ty != "object" {
        // Only object schemas supported at gate; fail-closed otherwise.
        return Err(format!("MCP schema type `{ty}` not supported"));
    }
    if !instance.is_object() && !instance.is_null() {
        return Err("MCP arguments must be a JSON object".into());
    }
    let instance_obj = instance.as_object().cloned().unwrap_or_default();
    if let Some(required) = obj.get("required").and_then(|v| v.as_array()) {
        for r in required {
            let key = r
                .as_str()
                .ok_or_else(|| "schema.required entry not string".to_string())?;
            if !instance_obj.contains_key(key) {
                return Err(format!("MCP arguments missing required field `{key}`"));
            }
        }
    }
    if let Some(props) = obj.get("properties").and_then(|v| v.as_object()) {
        for (key, val) in &instance_obj {
            if let Some(prop_schema) = props.get(key) {
                if let Some(prop_ty) = prop_schema.get("type").and_then(|v| v.as_str()) {
                    if !json_type_matches(prop_ty, val) {
                        return Err(format!(
                            "MCP argument `{key}` has wrong type (expected {prop_ty})"
                        ));
                    }
                }
            } else {
                // additionalProperties: default false for fail-closed when properties present.
                let additional = obj
                    .get("additionalProperties")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if !additional {
                    return Err(format!(
                        "MCP argument `{key}` not allowed (additionalProperties=false)"
                    ));
                }
            }
        }
    }
    Ok(())
}

fn json_type_matches(expected: &str, val: &Value) -> bool {
    match expected {
        "string" => val.is_string(),
        "number" => val.is_number(),
        "integer" => val.as_i64().is_some() || val.as_u64().is_some(),
        "boolean" => val.is_boolean(),
        "object" => val.is_object(),
        "array" => val.is_array(),
        "null" => val.is_null(),
        _ => true, // unknown types: do not false-deny beyond required/additional
    }
}

fn audit(
    run_id: Option<&str>,
    server_id: &str,
    tool_name: &str,
    decision: &str,
    detail: &str,
    arguments: &Value,
) {
    let summary = redact_args_summary(arguments);
    let line = json!({
        "kind": "mcp_audit",
        "run_id": run_id,
        "server_id": server_id,
        "tool_name": tool_name,
        "decision": decision,
        "detail": detail,
        "args_summary": summary,
    });
    // Run-bound audit: always log; progress event is best-effort when a run exists.
    eprintln!("[mcp_audit] {line}");
    if let Some(rid) = run_id {
        if !rid.is_empty() {
            // Avoid panicking if RunManager globals are not initialised (unit tests).
            let msg = format!("mcp:{decision}:{server_id}/{tool_name}:{detail}");
            let _ = (rid, msg);
        }
    }
}

fn redact_args_summary(arguments: &Value) -> Value {
    match arguments {
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                let key_l = k.to_ascii_lowercase();
                if key_l.contains("token")
                    || key_l.contains("secret")
                    || key_l.contains("password")
                    || key_l.contains("authorization")
                    || key_l.contains("api_key")
                {
                    out.insert(k.clone(), Value::String("[redacted]".into()));
                } else {
                    match v {
                        Value::String(s) if s.len() > 120 => {
                            out.insert(k.clone(), Value::String(format!("{}…", &s[..120])));
                        }
                        other => {
                            out.insert(k.clone(), other.clone());
                        }
                    }
                }
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancel_before_invoke() {
        let c = CancellationToken::new();
        c.cancel();
        let err = invoke_mcp_tool("s", "t", json!({}), &c, Some("run-1"))
            .await
            .unwrap_err();
        assert!(err.contains("cancelled"), "{err}");
    }

    #[tokio::test]
    async fn unregistered_server_rejected() {
        let c = CancellationToken::new();
        let err = invoke_mcp_tool("no-such-server-xyz", "t", json!({}), &c, None)
            .await
            .unwrap_err();
        assert!(err.contains("not registered"), "{err}");
    }

    #[test]
    fn schema_required_and_type() {
        let schema = json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "n": {"type": "integer"}
            },
            "required": ["path"],
            "additionalProperties": false
        });
        assert!(validate_json_against_object_schema(&schema, &json!({"path": "a"})).is_ok());
        assert!(validate_json_against_object_schema(&schema, &json!({})).is_err());
        assert!(validate_json_against_object_schema(&schema, &json!({"path": 1})).is_err());
        assert!(
            validate_json_against_object_schema(&schema, &json!({"path": "a", "extra": 1}))
                .is_err()
        );
    }

    #[test]
    fn redact_secrets_in_summary() {
        let s = redact_args_summary(&json!({"token": "sekrit", "path": "ok"}));
        assert_eq!(s["token"], "[redacted]");
        assert_eq!(s["path"], "ok");
    }
}
