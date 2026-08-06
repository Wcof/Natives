//! Host-side creative proposal inbox (T06).
//!
//! The Host pulls durable proposal facts from the Agent Daemon over UDS
//! (`proposal.listPending`), validates each payload through the Host gate
//! (`validate_protocol_proposal`), and persists a pending inbox row keyed by
//! the Daemon's stable proposal id. The Renderer lists pending rows through
//! `creative_app_proposal_list`; approve/reject operate on the persisted row by
//! id (never on a Renderer-supplied proposal body), and every decision is a
//! pending→terminal CAS so repeated clicks are idempotent.
//!
//! Trust boundaries:
//! - The Renderer never supplies executable paths, hashes, or approval claims.
//! - The agent's payload cannot carry a trusted `approved` flag (protocol
//!   enforces this); the Host recomputes file identity at approve time.
//! - Invalid payloads are recorded as `failed` rows so they are never re-shown
//!   as pending, and a decided row is never re-served.

use crate::creative_app::process_driver::{resolve_binary_identity, resolve_python_interpreter};
use crate::creative_app::proposal::{validate_protocol_proposal, AgentProposal, ProposedDriver};
use crate::db::DbPool;
use crate::Error;
use assistant_protocol::v2::CreativeProposalEnvelope;
use rusqlite::{params, OptionalExtension};
use serde::Serialize;

pub const STATUS_PENDING: &str = "pending";
pub const STATUS_APPROVED: &str = "approved";
pub const STATUS_REJECTED: &str = "rejected";
pub const STATUS_EXPIRED: &str = "expired";
pub const STATUS_FAILED: &str = "failed";

/// A stored inbox row with its parsed envelope.
pub struct StoredProposal {
    pub envelope: CreativeProposalEnvelope,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    pub failure: Option<String>,
}

/// The pending-proposal shape exposed to the Renderer. The validated
/// [`AgentProposal`] is flattened in so the approval card can render the full
/// intent (executable/interpreter, argv, cwd, env keys, port, ownership).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposalInboxEntry {
    pub proposal_id: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    pub run_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    pub tool_call_id: String,
    #[serde(flatten)]
    pub proposal: AgentProposal,
}

fn conn<'a>(
    pool: &'a DbPool,
) -> crate::Result<r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>> {
    pool.get().map_err(|e| Error::Internal(format!("db: {e}")))
}

/// Pull pending proposal facts from the daemon and merge them into the Host
/// inbox. Idempotent: a proposal id already present (pending or decided) is
/// never duplicated. Reconnect-recoverable: after a Host or daemon restart,
/// pending facts are re-pulled and re-merged; `list_pending` re-serves them.
pub async fn sync_pending_from_daemon(pool: &DbPool) -> crate::Result<()> {
    let data = crate::daemon_authority::request("proposal.listPending", serde_json::json!({}))
        .await
        .map_err(|e| Error::Internal(format!("proposal.listPending: {e}")))?;
    let facts: Vec<CreativeProposalEnvelope> = data
        .get("proposals")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    let c = conn(pool)?;
    merge_daemon_facts(&c, &facts)
}

/// Merge daemon facts into the inbox. Each payload is re-validated through the
/// Host gate; invalid payloads are recorded as `failed` so they can never be
/// shown as pending.
pub fn merge_daemon_facts(
    c: &rusqlite::Connection,
    facts: &[CreativeProposalEnvelope],
) -> crate::Result<()> {
    for envelope in facts {
        let known: Option<String> = c
            .query_row(
                "SELECT status FROM creative_proposal_inbox WHERE proposal_id = ?1",
                params![envelope.proposal_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(Error::Database)?;
        if known.is_some() {
            continue;
        }
        let (status, failure) = match validate_protocol_proposal(&envelope.payload) {
            Ok(_) => (STATUS_PENDING, None),
            Err(e) => (STATUS_FAILED, Some(e.to_string())),
        };
        let envelope_json = serde_json::to_string(envelope)
            .map_err(|e| Error::Internal(format!("serialize proposal envelope: {e}")))?;
        c.execute(
            "INSERT INTO creative_proposal_inbox (proposal_id, envelope_json, status, failure)
             VALUES (?1, ?2, ?3, ?4)",
            params![envelope.proposal_id, envelope_json, status, failure],
        )
        .map_err(Error::Database)?;
    }
    Ok(())
}

/// List pending inbox rows (oldest first), each flattened with its validated
/// [`AgentProposal`] for the approval card.
pub fn list_pending(c: &rusqlite::Connection) -> crate::Result<Vec<ProposalInboxEntry>> {
    let mut stmt = c
        .prepare(
            "SELECT proposal_id, envelope_json, status, created_at, updated_at
             FROM creative_proposal_inbox WHERE status = ?1 ORDER BY created_at ASC",
        )
        .map_err(Error::Database)?;
    let rows = stmt
        .query_map(params![STATUS_PENDING], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(Error::Database)?;
    let mut out = Vec::new();
    for row in rows {
        let (proposal_id, envelope_json, status, created_at, updated_at) =
            row.map_err(Error::Database)?;
        let envelope: CreativeProposalEnvelope = serde_json::from_str(&envelope_json)
            .map_err(|e| Error::Internal(format!("deserialize proposal envelope: {e}")))?;
        let validated = validate_protocol_proposal(&envelope.payload)?;
        out.push(ProposalInboxEntry {
            proposal_id,
            status,
            created_at,
            updated_at,
            run_id: envelope.run_id,
            turn_id: envelope.turn_id,
            tool_call_id: envelope.tool_call_id,
            proposal: validated.proposal,
        });
    }
    Ok(out)
}

/// Look up a stored proposal by id.
pub fn get_stored(
    c: &rusqlite::Connection,
    proposal_id: &str,
) -> crate::Result<Option<StoredProposal>> {
    let row = c
        .query_row(
            "SELECT envelope_json, status, created_at, updated_at, failure
             FROM creative_proposal_inbox WHERE proposal_id = ?1",
            params![proposal_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            },
        )
        .optional()
        .map_err(Error::Database)?;
    let Some((envelope_json, status, created_at, updated_at, failure)) = row else {
        return Ok(None);
    };
    let envelope: CreativeProposalEnvelope = serde_json::from_str(&envelope_json)
        .map_err(|e| Error::Internal(format!("deserialize proposal envelope: {e}")))?;
    Ok(Some(StoredProposal {
        envelope,
        status,
        created_at,
        updated_at,
        failure,
    }))
}

/// CAS a pending row to a terminal status. Returns `true` when this call made
/// the transition; `false` (no-op) when the row was already decided — the
/// idempotency guard for repeated approve/reject clicks.
pub fn cas_status(
    c: &rusqlite::Connection,
    proposal_id: &str,
    from: &str,
    to: &str,
) -> crate::Result<bool> {
    let changed = c
        .execute(
            "UPDATE creative_proposal_inbox
             SET status = ?1, updated_at = datetime('now')
             WHERE proposal_id = ?2 AND status = ?3",
            params![to, proposal_id, from],
        )
        .map_err(Error::Database)?;
    Ok(changed == 1)
}

/// Record (or refresh) the executable approval record for an approved
/// proposal. The identity is the Host-recomputed SHA-256 of the canonical
/// path — never an agent-supplied value. A binary whose content changed since
/// its last approval is refused (re-approval required).
pub fn record_executable_approval(
    c: &rusqlite::Connection,
    canonical_path: &str,
    file_identity: &str,
    scope: &str,
    approver: &str,
    proposal_id: &str,
) -> crate::Result<()> {
    let existing: Option<String> = c
        .query_row(
            "SELECT file_identity FROM creative_executable_approval WHERE canonical_path = ?1",
            params![canonical_path],
            |row| row.get(0),
        )
        .optional()
        .map_err(Error::Database)?;
    match existing {
        Some(prev) if prev != file_identity => {
            return Err(Error::InvalidInput(
                "executable changed since its last approval; re-approval required".into(),
            ));
        }
        Some(_) => {
            // Re-approving the same file (same identity) — refresh approver/time.
            c.execute(
                "UPDATE creative_executable_approval
                 SET scope = ?1, approver = ?2, approved_at = datetime('now'), proposal_id = ?3
                 WHERE canonical_path = ?4",
                params![scope, approver, proposal_id, canonical_path],
            )
            .map_err(Error::Database)?;
        }
        None => {
            let id = uuid::Uuid::new_v4().to_string();
            c.execute(
                "INSERT INTO creative_executable_approval
                    (id, canonical_path, file_identity, scope, approver, approved_at, proposal_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'), ?6)",
                params![
                    id,
                    canonical_path,
                    file_identity,
                    scope,
                    approver,
                    proposal_id
                ],
            )
            .map_err(Error::Database)?;
        }
    }
    Ok(())
}

/// Host-side security verification at approve time. For binary drivers, the
/// executable is canonicalized and re-hashed; for python drivers, the
/// interpreter must be a real Python executable (never a shell). Returns the
/// canonical path + identity the approval record will pin, plus a copy of the
/// proposal whose driver profile carries the verified canonical path and the
/// Host-computed identity (never an agent-supplied value).
pub fn resolve_proposal_identity(
    proposal: &AgentProposal,
) -> crate::Result<(String, String, AgentProposal)> {
    match &proposal.driver {
        ProposedDriver::Binary(b) => {
            let (canonical, hash) = resolve_binary_identity(&b.executable_path)?;
            let mut b2 = b.clone();
            b2.executable_path = canonical.clone();
            b2.executable_hash = hash.clone();
            b2.approved = true;
            let mut verified = proposal.clone();
            verified.driver = ProposedDriver::Binary(b2);
            Ok((canonical, hash, verified))
        }
        ProposedDriver::Python(p) => {
            let canonical = resolve_python_interpreter(&p.interpreter)?;
            let hash =
                crate::creative_app::process_driver::sha256_hex(std::path::Path::new(&canonical))?;
            let mut p2 = p.clone();
            p2.interpreter = canonical.clone();
            let mut verified = proposal.clone();
            verified.driver = ProposedDriver::Python(p2);
            Ok((canonical, hash, verified))
        }
        // Static/compose have no host executable to pin.
        _ => Ok((String::new(), String::new(), proposal.clone())),
    }
}

/// Identity-only view of [`resolve_proposal_identity`].
pub fn verify_driver_identity(proposal: &AgentProposal) -> crate::Result<(String, String)> {
    let (canonical, hash, _) = resolve_proposal_identity(proposal)?;
    Ok((canonical, hash))
}

/// True when an approved proposal must ALSO be started (kind=start), not just
/// registered (kind=create). T09: create registers only; start runs
/// start→health→endpoint after registration.
pub fn proposal_should_start(proposal: &AgentProposal) -> bool {
    proposal.kind == crate::creative_app::proposal::ProposalKind::Start
}

/// Resolve the source id a kind=start proposal should launch.
///
/// Returns `Some(source_id)` when the project root is already registered as a
/// local creative app (reuse — do not duplicate), `None` when the app must be
/// registered first. A kind=create proposal always returns `None`.
pub fn start_target_for_proposal(
    c: &rusqlite::Connection,
    proposal: &AgentProposal,
) -> crate::Result<Option<String>> {
    if !proposal_should_start(proposal) {
        return Ok(None);
    }
    let root = crate::creative_app::local::canonical_project_root(&proposal.project_root)?;
    let root_s = root.to_string_lossy().to_string();
    Ok(crate::creative_app::local::get_app_by_root(c, &root_s)?.map(|r| r.id))
}

/// Tagged approve result: `approved` with the registered app, or
/// `already_decided` (idempotent no-op for a repeat click / concurrent click).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ProposalApproveResult {
    Approved {
        proposal_id: String,
        app: crate::creative_app::model::CreativeAppSummary,
    },
    AlreadyDecided {
        proposal_id: String,
        current_status: String,
    },
}

/// Tagged reject result: `rejected`, or `already_decided` (idempotent no-op).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ProposalRejectResult {
    Rejected {
        proposal_id: String,
    },
    AlreadyDecided {
        proposal_id: String,
        current_status: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use assistant_protocol::v2::{CreativeProposalPayload, CreativeProposedDriver};

    fn mem_conn() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::create_tables(&conn).unwrap();
        crate::db::apply_migrations(&conn).unwrap();
        conn
    }

    fn python_payload() -> CreativeProposalPayload {
        CreativeProposalPayload {
            schema_version: 1,
            kind: "create".into(),
            ownership: "managed".into(),
            title: "Dashboard".into(),
            project_root: "/proj".into(),
            driver: CreativeProposedDriver::Python {
                schema_version: 1,
                interpreter: "/proj/.venv/bin/python".into(),
                entry: "app.py".into(),
                args: vec![],
                cwd_relative: ".".into(),
                environment_keys: vec!["PORT".into()],
                open_path: "/".into(),
                health_path: "/".into(),
                startup_timeout_ms: 60_000,
            },
            open_path: "/".into(),
            health_path: "/".into(),
            environment_keys: vec!["PORT".into()],
        }
    }

    fn envelope(id: &str, payload: CreativeProposalPayload) -> CreativeProposalEnvelope {
        CreativeProposalEnvelope {
            proposal_id: id.into(),
            envelope_version: CreativeProposalEnvelope::ENVELOPE_VERSION,
            run_id: "run-1".into(),
            turn_id: Some("turn-1".into()),
            tool_call_id: "tc-1".into(),
            created_at: chrono::Utc::now(),
            payload,
        }
    }

    #[test]
    fn merge_and_list_pending_round_trips() {
        let conn = mem_conn();
        merge_daemon_facts(&conn, &[envelope("p-1", python_payload())]).unwrap();
        let pending = list_pending(&conn).unwrap();
        assert_eq!(pending.len(), 1);
        let entry = &pending[0];
        assert_eq!(entry.proposal_id, "p-1");
        assert_eq!(entry.status, STATUS_PENDING);
        assert_eq!(entry.run_id, "run-1");
        assert_eq!(entry.tool_call_id, "tc-1");
        assert_eq!(entry.proposal.title, "Dashboard");
        assert_eq!(entry.proposal.environment_keys, vec!["PORT".to_string()]);
    }

    #[test]
    fn pending_survives_restart_and_merge_is_idempotent() {
        // T06 acceptance: a pending proposal must survive a Host restart and be
        // re-served. Re-merging the same facts must not duplicate or resurrect.
        let conn = mem_conn();
        merge_daemon_facts(&conn, &[envelope("p-1", python_payload())]).unwrap();
        assert_eq!(list_pending(&conn).unwrap().len(), 1);

        // "Restart": the Host re-pulls the same pending facts from the daemon.
        merge_daemon_facts(&conn, &[envelope("p-1", python_payload())]).unwrap();
        assert_eq!(
            list_pending(&conn).unwrap().len(),
            1,
            "re-merge must not duplicate a known proposal"
        );

        // After a decision, the fact must stay decided even if re-merged.
        assert!(cas_status(&conn, "p-1", STATUS_PENDING, STATUS_REJECTED).unwrap());
        assert_eq!(list_pending(&conn).unwrap().len(), 0);
        merge_daemon_facts(&conn, &[envelope("p-1", python_payload())]).unwrap();
        assert_eq!(
            list_pending(&conn).unwrap().len(),
            0,
            "a rejected proposal must not reappear after restart"
        );
    }

    #[test]
    fn invalid_payload_is_recorded_failed_not_pending() {
        // T06: a `/bin/sh` pseudo-python proposal is blocked before it is ever
        // shown to the user — the Host gate rejects it at merge time.
        let conn = mem_conn();
        let mut payload = python_payload();
        payload.driver = CreativeProposedDriver::Python {
            schema_version: 1,
            interpreter: "/bin/sh".into(),
            entry: "app.py".into(),
            args: vec![],
            cwd_relative: ".".into(),
            environment_keys: vec![],
            open_path: "/".into(),
            health_path: "/".into(),
            startup_timeout_ms: 60_000,
        };
        merge_daemon_facts(&conn, &[envelope("p-bad", payload)]).unwrap();
        assert_eq!(
            list_pending(&conn).unwrap().len(),
            0,
            "a shell pseudo-python must never surface as pending"
        );
        let stored = get_stored(&conn, "p-bad").unwrap().unwrap();
        assert_eq!(stored.status, STATUS_FAILED);
    }

    #[test]
    fn cas_status_is_idempotent_for_repeat_clicks() {
        let conn = mem_conn();
        merge_daemon_facts(&conn, &[envelope("p-1", python_payload())]).unwrap();

        // First approve wins.
        assert!(cas_status(&conn, "p-1", STATUS_PENDING, STATUS_APPROVED).unwrap());
        // Repeat approve is a no-op (returns false, does not corrupt).
        assert!(!cas_status(&conn, "p-1", STATUS_PENDING, STATUS_APPROVED).unwrap());
        assert_eq!(
            get_stored(&conn, "p-1").unwrap().unwrap().status,
            STATUS_APPROVED
        );
    }

    #[test]
    fn verify_driver_identity_blocks_shell_python_and_missing_binary() {
        // Shell pseudo-python blocked.
        let mut proposal = validate_protocol_proposal(&envelope("p-1", python_payload()).payload)
            .unwrap()
            .proposal;
        proposal.driver = ProposedDriver::Python(crate::creative_app::model::PythonLaunchProfile {
            schema_version: 1,
            interpreter: "/bin/sh".into(),
            entry: "app.py".into(),
            args: vec![],
            cwd_relative: ".".into(),
            environment_keys: vec![],
            port: crate::creative_app::model::LaunchPort {
                mode: crate::creative_app::model::LaunchPortMode::Auto,
                value: None,
            },
            open_path: "/".into(),
            health_path: "/".into(),
            startup_timeout_ms: 60_000,
            is_venv: false,
        });
        assert!(verify_driver_identity(&proposal).is_err());

        // Missing binary blocked.
        proposal.driver = ProposedDriver::Binary(crate::creative_app::model::BinaryLaunchProfile {
            schema_version: 1,
            executable_path: "/nonexistent/definitely-not-here".into(),
            executable_hash: String::new(),
            approved: false,
            args: vec![],
            cwd_relative: ".".into(),
            environment_keys: vec![],
            port: crate::creative_app::model::LaunchPort {
                mode: crate::creative_app::model::LaunchPortMode::Auto,
                value: None,
            },
            open_path: "/".into(),
            health_path: "/".into(),
            startup_timeout_ms: 60_000,
        });
        assert!(verify_driver_identity(&proposal).is_err());
    }

    #[test]
    fn executable_approval_record_rejects_content_change() {
        let conn = mem_conn();
        record_executable_approval(
            &conn,
            "/bin/true",
            &"a".repeat(64),
            "proposal:p-1",
            "user",
            "p-1",
        )
        .unwrap();
        // Same identity → re-approve is fine.
        record_executable_approval(
            &conn,
            "/bin/true",
            &"a".repeat(64),
            "proposal:p-2",
            "user",
            "p-2",
        )
        .unwrap();
        // Changed identity → refused (re-approval required).
        let err = record_executable_approval(
            &conn,
            "/bin/true",
            &"b".repeat(64),
            "proposal:p-3",
            "user",
            "p-3",
        )
        .unwrap_err();
        assert!(err.to_string().contains("changed since"));
    }

    /// T09: a kind=start proposal on an already-registered path must target the
    /// existing app (reuse), while a kind=create proposal has no target (it
    /// registers). This drives "kind=start 对已有/新 app 走 start→health→endpoint".
    #[test]
    fn start_target_resolves_existing_app_and_create_has_none() {
        use crate::creative_app::model::{
            LaunchMode, LaunchPlan, LaunchPlanSource, LaunchProgram, LocalCreativeAppRecord,
            LocalLaunchRuntime, LocalProjectKind, OwnershipMode,
        };
        use crate::creative_app::proposal::{AgentProposal, ProposalKind, ProposedDriver};

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("index.html"), "<html></html>").unwrap();
        let root_s = root.to_string_lossy().to_string();
        // The record stores the CANONICAL root (same normalization the
        // registration path uses), matching `start_target_for_proposal`.
        let canonical_root = crate::creative_app::local::canonical_project_root(&root_s)
            .unwrap()
            .to_string_lossy()
            .to_string();

        let conn = mem_conn();
        let now = chrono::Utc::now().to_rfc3339();
        let plan = LaunchPlan {
            schema_version: 1,
            source: LaunchPlanSource::Rule,
            project_kind: LocalProjectKind::Html,
            runtime: LocalLaunchRuntime::StaticHttp,
            program: LaunchProgram::Internal,
            cwd_relative: ".".into(),
            script: None,
            entry_file: Some("index.html".into()),
            script_runner: None,
            args: vec![],
            environment_keys: vec![],
            port: crate::creative_app::model::LaunchPort {
                mode: crate::creative_app::model::LaunchPortMode::Auto,
                value: None,
            },
            open_path: "/".into(),
            health_path: "/".into(),
            startup_timeout_ms: 60_000,
            auto_open: false,
            confidence: None,
            reason: "test".into(),
            compose: None,
            trade_approval: None,
            process_profile: None,
        };
        let rec = LocalCreativeAppRecord {
            id: "loc-exists".into(),
            title: "Existing".into(),
            description: None,
            icon: None,
            canonical_project_root: canonical_root.clone(),
            device_id: "d".into(),
            device_name: "n".into(),
            project_kind: LocalProjectKind::Html,
            launch_mode: LaunchMode::Smart,
            launch_plan_json: plan.to_json().unwrap(),
            plan_fingerprint: "fp".into(),
            state: crate::creative_app::model::CreativeAppState::InstalledStopped,
            status_detail_json: None,
            open_url: None,
            current_port: None,
            process_identity_json: None,
            volume_identity: String::new(),
            auto_open: false,
            startup_timeout_ms: 60_000,
            last_started_at: None,
            last_exit_reason: None,
            last_error: None,
            created_at: now.clone(),
            updated_at: now,
        };
        crate::creative_app::local::insert_app(&conn, &rec).unwrap();

        let start = AgentProposal {
            schema_version: 1,
            kind: ProposalKind::Start,
            ownership: OwnershipMode::Managed,
            title: "Existing".into(),
            project_root: root_s.clone(),
            driver: ProposedDriver::StaticHttp,
            open_path: "/".into(),
            health_path: "/".into(),
            environment_keys: vec![],
        };
        let target = start_target_for_proposal(&conn, &start).unwrap();
        assert_eq!(
            target.as_deref(),
            Some("loc-exists"),
            "kind=start on a registered path must reuse the existing app"
        );

        let create = AgentProposal {
            kind: ProposalKind::Create,
            ..start
        };
        assert!(
            start_target_for_proposal(&conn, &create).unwrap().is_none(),
            "kind=create has no start target — it registers only"
        );
    }
}
