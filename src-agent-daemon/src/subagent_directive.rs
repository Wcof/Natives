//! Durable pending Child Directive (NE-P0-08 / 19.3-①): the protected
//! execution-plan persona persisted on `subagent_session.scope_snapshot_json`.
//!
//! Split from `subagent_store` (directive responsibility).

use super::*;

// ─────────────────────────────────────────────────────────────────────────────
// NE-P0-08 / 19.3-①: durable pending Child Directive.
//
// The dynamic `system_prompt` a parent writes on the `task` tool is prompt text
// only (never consulted for permissions or tool surface). It used to travel
// exclusively through the in-memory `run_agent_directives` registry on
// `ProductionRuntime`, so a daemon restart between spawn and child start lost
// the persona. The directive is now ALSO persisted into the *protected pending
// execution plan* — the reserved `subagent_session` row's `scope_snapshot_json`,
// which is written before the child run is created (persist-first, saga phase 1)
// and only released by a terminal child or restart recovery. restart/retry/
// continue read the same durable text + digest back, so the persona is
// byte-identical after a crash.
// ─────────────────────────────────────────────────────────────────────────────

/// One durable pending directive, embedded in `subagent_session.scope_snapshot_json`
/// under [`PENDING_DIRECTIVE_KEY`]. `digest` is the SHA-256 hex of `text`, so
/// retry/restart/continue can verify they restore the exact same persona.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingDirective {
    pub text: String,
    pub digest: String,
    pub persisted_at: String,
}

/// Reserved key inside `scope_snapshot_json` carrying the pending directive.
pub const PENDING_DIRECTIVE_KEY: &str = "pending_directive";

/// SHA-256 hex digest of a directive text. Same text ⇒ same digest (19.3-①).
pub fn directive_sha256_hex(text: &str) -> String {
    let mut hasher = <sha2::Sha256 as sha2::Digest>::new();
    sha2::Digest::update(&mut hasher, text.as_bytes());
    format!("{:x}", sha2::Digest::finalize(hasher))
}

/// Extract the pending directive from a raw `scope_snapshot_json` string.
pub fn parse_pending_directive(scope_snapshot_json: &str) -> Option<PendingDirective> {
    serde_json::from_str::<Value>(scope_snapshot_json)
        .ok()
        .and_then(|v| v.get(PENDING_DIRECTIVE_KEY).cloned())
        .and_then(|v| serde_json::from_value::<PendingDirective>(v).ok())
}

/// Embed a pending directive into a reservation scope snapshot (if any). Called
/// at spawn time so the durable copy exists before the child run is created.
pub fn with_pending_directive(
    snapshot: serde_json::Value,
    directive: Option<(&str, &str)>,
) -> serde_json::Value {
    let Some((text, digest)) = directive.filter(|(t, _)| !t.trim().is_empty()) else {
        return snapshot;
    };
    let mut obj = match snapshot {
        Value::Object(map) => map,
        other => match other {
            Value::Null => serde_json::Map::new(),
            _ => return other,
        },
    };
    obj.insert(
        PENDING_DIRECTIVE_KEY.to_string(),
        json!({
            "text": text,
            "digest": digest,
            "persisted_at": chrono::Utc::now().to_rfc3339(),
        }),
    );
    Value::Object(obj)
}

/// Read the durable pending directive for a session (the protected pending
/// execution plan). `None` for legacy sessions created before the directive
/// was persisted — callers fail closed rather than inventing a persona.
pub fn pending_directive_for_session(session_id: &str) -> Result<Option<PendingDirective>, String> {
    let session_id = session_id.trim();
    if session_id.is_empty() {
        return Ok(None);
    }
    let s = store()?;
    let conn = s.conn().map_err(|e| format!("conn lock: {e}"))?;
    let raw: Option<String> = conn
        .query_row(
            "SELECT scope_snapshot_json FROM subagent_session WHERE id = ?1",
            params![session_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| format!("pending directive read: {e}"))?;
    Ok(raw.and_then(|r| parse_pending_directive(&r)))
}

/// Read the durable pending directive for a child run (resolved through the
/// session's hidden conversation). Used by resume/continue seams that only hold
/// a run id.
pub fn pending_directive_for_run(run_id: &str) -> Result<Option<PendingDirective>, String> {
    let run_id = run_id.trim();
    if run_id.is_empty() {
        return Ok(None);
    }
    let s = store()?;
    let conn = s.conn().map_err(|e| format!("conn lock: {e}"))?;
    let raw: Option<String> = conn
        .query_row(
            "SELECT s.scope_snapshot_json
               FROM subagent_session s
              WHERE s.child_conversation_id = (
                    SELECT conversation_id FROM run WHERE id = ?1
              )
              ORDER BY s.created_at DESC LIMIT 1",
            params![run_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| format!("pending directive by run read: {e}"))?;
    Ok(raw.and_then(|r| parse_pending_directive(&r)))
}

/// 19.3-⑤ field-level visibility: a session view for RPC export/logs where the
/// parent-authored directive text is replaced by its digest marker. The digest
/// stays visible (so an operator can confirm the same persona was restored);
/// the persona text never leaves the protected store.
pub fn redact_session_for_export(session: &SubagentSession) -> Value {
    let mut v = serde_json::to_value(session).unwrap_or_else(|_| json!({}));
    let snapshot = session
        .scope_snapshot_json
        .parse::<Value>()
        .unwrap_or_else(|_| json!({}));
    let redacted_snapshot = match snapshot.get(PENDING_DIRECTIVE_KEY) {
        Some(pd) => {
            let digest = pd
                .get("digest")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let mut s = pd.clone();
            if let Some(obj) = s.as_object_mut() {
                obj.insert(
                    "text".into(),
                    json!(format!("[REDACTED system_prompt sha256:{digest}]")),
                );
            }
            s
        }
        None => Value::Null,
    };
    // The full scope snapshot (with the raw directive text) is a protected
    // execution-plan field: strip it from exports so `subagent.list` never
    // ships the persona text to the frontend.
    let mut view = snapshot.clone();
    if let Some(obj) = view.as_object_mut() {
        match &redacted_snapshot {
            Value::Null => {
                obj.remove(PENDING_DIRECTIVE_KEY);
            }
            other => {
                obj.insert(PENDING_DIRECTIVE_KEY.to_string(), other.clone());
            }
        }
    }
    if let Some(obj) = v.as_object_mut() {
        obj.insert("pendingDirective".into(), redacted_snapshot);
        // The wire field stays a JSON string (same shape as the stored column),
        // with the directive text stripped and replaced by a digest marker.
        obj.insert(
            "scope_snapshot_json".into(),
            Value::String(view.to_string()),
        );
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_temp_db<F: FnOnce()>(f: F) {
        let _guard = crate::storage::DataStore::env_test_lock();
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join(format!("subagent-{}.db", Uuid::new_v4()));
        let art = dir.path().join("artifacts");
        crate::storage::set_test_db_override(Some(db.clone()), Some(art.clone()));
        let _warm = crate::storage::DataStore::new(&db, &art).expect("subagent temp db migrate");
        // Ensure parent conversation exists for FK tests.
        crate::conversation_store::ensure_conversation_stub(
            "parent-1", "openai", "gpt-4o", None, None,
        )
        .unwrap();
        f();
        crate::storage::set_test_db_override(None, None);
    }

    fn temp_session(binding: &RouteBinding) -> (String, String) {
        create_hidden_child_session(
            "parent-1",
            Some("parent-run"),
            None,
            "worker",
            "do",
            binding,
            Some("ask"),
            None,
        )
        .unwrap()
    }

    fn snapshot(over: &serde_json::Value) -> SubagentReservation {
        SubagentReservation {
            session_id: String::new(),
            parent_run_id: "parent-run".into(),
            tree_root_run_id: "tree-root".into(),
            depth: 1,
            max_tokens: Some(1_000),
            max_cost_usd: None,
            failure_policy: "fail_fast".into(),
            max_retries: 2,
            scope_snapshot: over.clone(),
        }
    }

    // ── NE-P0-08 / 19.3-①: durable pending Child Directive ──

    #[test]
    fn pending_directive_persists_in_reservation_and_round_trips() {
        with_temp_db(|| {
            let binding = RouteBinding {
                provider_id: "openai".into(),
                key_id: "k1".into(),
                model_id: "gpt-4o".into(),
            };
            let (sid, _) = temp_session(&binding);
            let text = "You are a terse Rust reviewer.";
            let digest = directive_sha256_hex(text);
            let mut res = snapshot(&with_pending_directive(
                serde_json::json!({ "project_id": "p1" }),
                Some((text, &digest)),
            ));
            res.session_id = sid.clone();
            reserve_subagent_slot(&res).unwrap();

            let pd = pending_directive_for_session(&sid)
                .unwrap()
                .expect("durable pending directive");
            assert_eq!(pd.text, text, "restart restores the exact same text");
            assert_eq!(pd.digest, digest, "restart restores the exact same digest");
            assert!(!pd.persisted_at.is_empty());
        });
    }

    #[test]
    fn pending_directive_digest_stable_and_verifies() {
        with_temp_db(|| {
            let a = directive_sha256_hex("same persona");
            let b = directive_sha256_hex("same persona");
            let c = directive_sha256_hex("different persona");
            assert_eq!(a, b, "same text => same digest");
            assert_ne!(a, c, "different text => different digest");
            assert_eq!(a.len(), 64, "sha-256 hex is 64 chars");
        });
    }

    #[test]
    fn pending_directive_for_run_resolves_through_child_conversation() {
        with_temp_db(|| {
            let binding = RouteBinding {
                provider_id: "openai".into(),
                key_id: "k1".into(),
                model_id: "gpt-4o".into(),
            };
            let (sid, child) = temp_session(&binding);
            let text = "You are a terse Rust reviewer.";
            let digest = directive_sha256_hex(text);
            let mut res = snapshot(&with_pending_directive(
                serde_json::json!({}),
                Some((text, &digest)),
            ));
            res.session_id = sid.clone();
            reserve_subagent_slot(&res).unwrap();

            let s = store().unwrap();
            let conn = s.conn().unwrap();
            conn.execute(
                "INSERT INTO run (id, conversation_id, status, provider_id, model_id)
                 VALUES ('child-run-1', ?1, 'created', 'openai', 'gpt-4o')",
                params![child],
            )
            .unwrap();

            let pd = pending_directive_for_run("child-run-1")
                .unwrap()
                .expect("durable directive resolved by run id");
            assert_eq!(pd.text, text);
            assert_eq!(pd.digest, digest);
            // A run that is not a child of any subagent session has none.
            assert!(pending_directive_for_run("orphan-run").unwrap().is_none());
        });
    }

    #[test]
    fn redact_session_export_hides_directive_text_keeps_digest() {
        with_temp_db(|| {
            let binding = RouteBinding {
                provider_id: "openai".into(),
                key_id: "k1".into(),
                model_id: "gpt-4o".into(),
            };
            let (sid, _) = temp_session(&binding);
            let text = "secret persona text that must never be exported";
            let digest = directive_sha256_hex(text);
            let mut res = snapshot(&with_pending_directive(
                serde_json::json!({}),
                Some((text, &digest)),
            ));
            res.session_id = sid.clone();
            reserve_subagent_slot(&res).unwrap();

            let sess = get_subagent_session(&sid).unwrap().unwrap();
            let export = redact_session_for_export(&sess);
            let serialized = export.to_string();
            assert!(
                !serialized.contains("secret persona text"),
                "directive text must not leak through the RPC export"
            );
            assert!(
                serialized.contains(&digest),
                "digest stays visible for field-level verification"
            );
            // The protected execution-plan column never ships the raw text.
            let snapshot_json = export["scope_snapshot_json"]
                .as_str()
                .expect("redacted snapshot view");
            assert!(
                !snapshot_json.contains(text),
                "scope_snapshot_json must not carry the raw directive"
            );
        });
    }
}
