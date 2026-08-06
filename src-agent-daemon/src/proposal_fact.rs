//! Durable creative proposal facts (T06).
//!
//! When a `creative_proposal` tool call completes, the Daemon converts the
//! typed tool payload into a durable [`CreativeProposalEnvelope`] here — the
//! single stable record of "an agent proposed X on run R, turn T, tool call C".
//! The Host pulls pending facts over UDS (`proposal.listPending`), validates
//! each one, and persists its own approval inbox. The fact survives both daemon
//! and Host restarts, so a proposal the user has not decided is never lost.
//!
//! Trust boundaries:
//! - `proposal_id` is generated here, never accepted from the agent.
//! - The payload is the agent's proposal; it carries NO trusted `approved` flag
//!   and no authoritative executable hash (the Host recomputes file identity at
//!   the user's explicit approve action).

use crate::storage::DataStore;
use assistant_protocol::v2::{CreativeProposalEnvelope, CreativeProposalPayload};
use chrono::Utc;

/// Fact lifecycle. `pending` is the only state the Host serves as actionable;
/// the rest are terminal decisions recorded so a decided fact is never re-served.
pub const STATUS_PENDING: &str = "pending";
pub const STATUS_APPROVED: &str = "approved";
pub const STATUS_REJECTED: &str = "rejected";
pub const STATUS_EXPIRED: &str = "expired";
pub const STATUS_FAILED: &str = "failed";

/// The tool name whose successful output carries a [`CreativeProposalPayload`].
pub const PROPOSAL_TOOL: &str = "creative_proposal";

/// Extract a typed proposal payload from a `creative_proposal` tool output.
///
/// The tool returns `{"ok": true, "proposal": <payload>, ...}`. Anything that
/// is not a well-formed payload yields `None` — the fact bridge must never
/// fabricate a fact from an unparsable or failed tool result.
pub fn proposal_from_tool_output(
    name: &str,
    output: &serde_json::Value,
) -> Option<CreativeProposalPayload> {
    if name != PROPOSAL_TOOL {
        return None;
    }
    if output.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
        return None;
    }
    let proposal = output.get("proposal")?;
    serde_json::from_value(proposal.clone())
        .map_err(|e| {
            eprintln!("[proposal_fact] tool returned ok=true but payload failed to parse: {e}");
        })
        .ok()
}

/// Persist a typed proposal fact. Returns the stable proposal id.
pub fn record_proposal_fact(
    store: &DataStore,
    run_id: &str,
    turn_id: Option<&str>,
    tool_call_id: &str,
    payload: &CreativeProposalPayload,
) -> Result<String, String> {
    let proposal_id = uuid::Uuid::new_v4().to_string();
    let envelope = CreativeProposalEnvelope {
        proposal_id: proposal_id.clone(),
        envelope_version: CreativeProposalEnvelope::ENVELOPE_VERSION,
        run_id: run_id.to_string(),
        turn_id: turn_id.map(str::to_string),
        tool_call_id: tool_call_id.to_string(),
        created_at: Utc::now(),
        payload: payload.clone(),
    };
    let payload_json = serde_json::to_string(&envelope)
        .map_err(|e| format!("serialize proposal envelope: {e}"))?;
    let conn = store.conn()?;
    conn.execute(
        "INSERT INTO creative_proposal_fact
            (proposal_id, envelope_version, run_id, turn_id, tool_call_id, payload_json, status)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            proposal_id,
            envelope.envelope_version as i64,
            run_id,
            turn_id,
            tool_call_id,
            payload_json,
            STATUS_PENDING
        ],
    )
    .map_err(|e| format!("insert proposal fact: {e}"))?;
    Ok(proposal_id)
}

/// List pending proposal facts (oldest first) for the Host's approval inbox.
pub fn list_pending_proposal_facts(
    store: &DataStore,
) -> Result<Vec<CreativeProposalEnvelope>, String> {
    let conn = store.conn()?;
    let mut stmt = conn
        .prepare(
            "SELECT payload_json, proposal_id FROM creative_proposal_fact
             WHERE status = ?1 ORDER BY created_at ASC",
        )
        .map_err(|e| format!("prepare list proposal facts: {e}"))?;
    let rows = stmt
        .query_map(rusqlite::params![STATUS_PENDING], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| format!("query proposal facts: {e}"))?;
    let mut out = Vec::new();
    for row in rows {
        let (payload_json, _id) = row.map_err(|e| format!("read proposal fact row: {e}"))?;
        let envelope: CreativeProposalEnvelope = serde_json::from_str(&payload_json)
            .map_err(|e| format!("deserialize proposal fact: {e}"))?;
        out.push(envelope);
    }
    Ok(out)
}

/// Transition a pending fact to a terminal status (CAS). A fact that is no
/// longer pending (already decided by the Host) is a no-op so a replayed Host
/// decision cannot corrupt a newer fact.
pub fn set_proposal_fact_status(
    store: &DataStore,
    proposal_id: &str,
    status: &str,
) -> Result<(), String> {
    let conn = store.conn()?;
    conn.execute(
        "UPDATE creative_proposal_fact
         SET status = ?1, updated_at = datetime('now')
         WHERE proposal_id = ?2 AND status = ?3",
        rusqlite::params![status, proposal_id, STATUS_PENDING],
    )
    .map_err(|e| format!("update proposal fact status: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::EnvRestore;
    use assistant_protocol::v2::CreativeProposedDriver;

    /// Reset the thread-local test DB override on drop so a later test on the
    /// same thread cannot reuse this test's temp database.
    struct ClearOverride;
    impl Drop for ClearOverride {
        fn drop(&mut self) {
            crate::storage::set_test_db_override(None, None);
        }
    }

    fn payload() -> CreativeProposalPayload {
        CreativeProposalPayload {
            schema_version: 1,
            kind: "create".into(),
            ownership: "managed".into(),
            title: "Dashboard".into(),
            project_root: "/proj".into(),
            driver: CreativeProposedDriver::StaticHttp,
            open_path: "/".into(),
            health_path: "/".into(),
            environment_keys: vec!["PORT".into()],
        }
    }

    fn fresh_store() -> (tempfile::TempDir, DataStore) {
        let dir = tempfile::tempdir().unwrap();
        let db = dir
            .path()
            .join(format!("natives-{}.db", uuid::Uuid::new_v4()));
        let art = dir.path().join("artifacts");
        crate::storage::set_test_db_override(Some(db.clone()), Some(art.clone()));
        let store = DataStore::new(&db, &art).expect("migrate");
        (dir, store)
    }

    fn with_store<F>(f: F)
    where
        F: FnOnce(DataStore),
    {
        let _guard = DataStore::env_test_lock();
        let _restore = EnvRestore::capture();
        let _clear = ClearOverride;
        let (_dir, store) = fresh_store();
        f(store);
    }

    #[test]
    fn fact_is_durable_and_survives_reopen() {
        with_store(|store| {
            let pid =
                record_proposal_fact(&store, "run-1", Some("turn-1"), "tc-1", &payload()).unwrap();
            assert!(!pid.is_empty());

            let facts = list_pending_proposal_facts(&store).unwrap();
            assert_eq!(facts.len(), 1);
            assert_eq!(facts[0].proposal_id, pid);
            assert_eq!(facts[0].run_id, "run-1");
            assert_eq!(facts[0].turn_id.as_deref(), Some("turn-1"));
            assert_eq!(facts[0].tool_call_id, "tc-1");
            assert_eq!(facts[0].payload.environment_keys, vec!["PORT".to_string()]);
        });
    }

    #[test]
    fn fact_survives_daemon_restart() {
        // T06 acceptance: a pending proposal must survive a daemon restart and
        // be re-served to the Host. Reopening the same DB file proves it.
        let _guard = DataStore::env_test_lock();
        let _restore = EnvRestore::capture();
        let _clear = ClearOverride;
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("restart.db");
        let art = dir.path().join("artifacts");
        crate::storage::set_test_db_override(Some(db.clone()), Some(art.clone()));

        let pid;
        {
            let store = DataStore::new(&db, &art).expect("migrate");
            pid = record_proposal_fact(&store, "run-1", None, "tc-1", &payload()).unwrap();
        }
        // "Restart": a brand new DataStore over the same file.
        {
            let store = DataStore::new(&db, &art).expect("reopen");
            let facts = list_pending_proposal_facts(&store).unwrap();
            assert_eq!(facts.len(), 1, "pending fact must survive daemon restart");
            assert_eq!(facts[0].proposal_id, pid);
        }
    }

    #[test]
    fn decided_fact_is_not_re_served() {
        with_store(|store| {
            let pid = record_proposal_fact(&store, "run-1", None, "tc-1", &payload()).unwrap();
            assert_eq!(list_pending_proposal_facts(&store).unwrap().len(), 1);

            set_proposal_fact_status(&store, &pid, STATUS_REJECTED).unwrap();
            assert_eq!(
                list_pending_proposal_facts(&store).unwrap().len(),
                0,
                "a rejected fact must not be re-served as pending"
            );
        });
    }

    #[test]
    fn tool_output_extraction_is_strict() {
        let good = serde_json::json!({
            "ok": true,
            "proposal": serde_json::to_value(payload()).unwrap(),
        });
        assert!(proposal_from_tool_output(PROPOSAL_TOOL, &good).is_some());

        // Wrong tool name.
        assert!(proposal_from_tool_output("write_file", &good).is_none());
        // Failed tool output.
        assert!(proposal_from_tool_output(
            PROPOSAL_TOOL,
            &serde_json::json!({"ok": false, "error": "nope"})
        )
        .is_none());
        // Malformed payload.
        assert!(proposal_from_tool_output(
            PROPOSAL_TOOL,
            &serde_json::json!({"ok": true, "proposal": {"schemaVersion": "not-a-number"}})
        )
        .is_none());
    }

    #[tokio::test]
    async fn rpc_serves_pending_facts_over_real_dispatch() {
        use assistant_protocol::v1::daemon::RpcRequest;
        use assistant_protocol::version::ProtocolVersion;
        use tokio::io::{AsyncBufReadExt, BufReader};

        let _guard = DataStore::env_test_lock();
        let _restore = EnvRestore::capture();
        let _clear = ClearOverride;
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("rpc.db");
        let art = dir.path().join("artifacts");
        crate::storage::set_test_db_override(Some(db.clone()), Some(art.clone()));
        std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db);
        std::env::set_var("NATIVES_DB_PATH", &db);
        std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
        std::env::set_var("NATIVES_DAEMON_FIXTURE", "1");

        // Warm the global RunManager against the temp store and record a fact.
        let _mgr =
            crate::run_manager::install_global_for_test(crate::run_manager::RunManager::new());
        let store = crate::run_manager::global_run_manager()
            .data_store_ref()
            .expect("env-driven store");
        let pid = record_proposal_fact(&store, "run-1", None, "tc-1", &payload()).unwrap();

        // Drive the real RPC dispatch (same path the Host uses over UDS).
        let (client, server) = tokio::net::UnixStream::pair().unwrap();
        let (_server_read, mut server_write) = server.into_split();
        let request = RpcRequest {
            protocol_version: assistant_protocol::v2::PROTOCOL_V2.to_string(),
            request_id: "probe-proposal".into(),
            client_id: "test".into(),
            session_token: "s".into(),
            method: "proposal.listPending".into(),
            params: serde_json::json!({}),
        };
        let protocol_version = ProtocolVersion::new(2, 0, 0);
        let started_at = std::time::Instant::now();
        let dispatch = crate::rpc::handle_rpc(
            &mut server_write,
            &request,
            &protocol_version,
            "0.0.0-test",
            &started_at,
        );
        tokio::time::timeout(std::time::Duration::from_secs(5), dispatch)
            .await
            .expect("dispatch must answer");
        drop(server_write);

        let mut line = String::new();
        let mut reader = BufReader::new(client);
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            reader.read_line(&mut line),
        )
        .await
        .expect("read rpc response");
        let value: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        let proposals = value["data"]["proposals"].as_array().unwrap();
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0]["proposalId"], pid);
        assert_eq!(proposals[0]["payload"]["environmentKeys"][0], "PORT");
    }
}
