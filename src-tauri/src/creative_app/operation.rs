//! Operation journal (batch 2, CR-201).
//!
//! Every lifecycle mutation (install / start / stop / restart / delete) records
//! a durable operation row — kind, phase, redacted input, error, timestamps —
//! so each external side effect is traceable to an operation and partial
//! failures have a recovery carrier. This is a minimal append/update journal,
//! deliberately NOT a workflow engine (see docs/audit
//! /creative-os-target-architecture.md §6).
//!
//! Phase machine (guarded transitions, never a silent 0-row Ok):
//!   pending → waiting → running → succeeded
//!                             → compensating → compensated
//!                                             → failed
//!                             → failed
//!   pending / waiting → cancelled   (before any side effect)
//!
//! `application_id` is nullable (install runs before the identity exists) and
//! the FK uses ON DELETE SET NULL so a delete operation survives the removal
//! of its own application row.

use crate::{Error, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

pub const KIND_START: &str = "start";
pub const KIND_STOP: &str = "stop";
pub const KIND_RESTART: &str = "restart";
pub const KIND_DELETE: &str = "delete";
pub const KIND_INSTALL: &str = "install";

pub const PHASE_PENDING: &str = "pending";
pub const PHASE_WAITING: &str = "waiting";
pub const PHASE_RUNNING: &str = "running";
pub const PHASE_COMPENSATING: &str = "compensating";
pub const PHASE_SUCCEEDED: &str = "succeeded";
pub const PHASE_FAILED: &str = "failed";
pub const PHASE_COMPENSATED: &str = "compensated";
pub const PHASE_CANCELLED: &str = "cancelled";

/// Terminal phases — a row in one of these will never transition again.
pub fn is_terminal(phase: &str) -> bool {
    matches!(
        phase,
        PHASE_SUCCEEDED | PHASE_FAILED | PHASE_COMPENSATED | PHASE_CANCELLED
    )
}

/// Non-terminal phases — an operation in one of these is "in flight".
pub fn is_active(phase: &str) -> bool {
    matches!(
        phase,
        PHASE_PENDING | PHASE_WAITING | PHASE_RUNNING | PHASE_COMPENSATING
    )
}

/// Journal retention: keep the newest `RETAIN_TERMINAL` terminal rows; active
/// rows are never pruned. Keeps the audit table bounded (#25).
pub const RETAIN_TERMINAL: i64 = 500;

fn now() -> String {
    crate::creative_app::runtime_store::now()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Operation {
    pub id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_instance_id: Option<String>,
    pub kind: String,
    pub phase: String,
    pub actor: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redacted_input: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    pub started_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    pub updated_at: String,
}

const COLUMNS: &str = "id, application_id, runtime_instance_id, kind, phase, actor, \
                       redacted_input_json, error_code, error_message, started_at, \
                       finished_at, updated_at";

fn row_to_operation(r: &rusqlite::Row<'_>) -> rusqlite::Result<Operation> {
    Ok(Operation {
        id: r.get(0)?,
        application_id: r.get(1)?,
        runtime_instance_id: r.get(2)?,
        kind: r.get(3)?,
        phase: r.get(4)?,
        actor: r.get(5)?,
        redacted_input: r.get(6)?,
        error_code: r.get(7)?,
        error_message: r.get(8)?,
        started_at: r.get(9)?,
        finished_at: r.get(10)?,
        updated_at: r.get(11)?,
    })
}

/// Create a journal row in `pending`. `application_id` may be None (install
/// runs before the identity exists; it is bound via [`set_application`]).
/// `redacted_input` must already be sanitized — never pass raw env/secrets.
pub fn create_operation(
    conn: &Connection,
    application_id: Option<&str>,
    kind: &str,
    actor: &str,
    redacted_input: Option<&str>,
) -> Result<i64> {
    let t = now();
    conn.execute(
        "INSERT INTO operations
            (application_id, kind, phase, actor, redacted_input_json, started_at, updated_at)
         VALUES (?1, ?2, 'pending', ?3, ?4, ?5, ?5)",
        params![application_id, kind, actor, redacted_input, t],
    )
    .map_err(Error::Database)?;
    Ok(conn.last_insert_rowid())
}

/// Bind the application identity once it exists (install path).
pub fn set_application(conn: &Connection, op_id: i64, application_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE operations SET application_id = ?2, updated_at = ?3 WHERE id = ?1",
        params![op_id, application_id, now()],
    )
    .map_err(Error::Database)?;
    Ok(())
}

pub fn get_operation(conn: &Connection, op_id: i64) -> Result<Option<Operation>> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM operations WHERE id = ?1"),
        params![op_id],
        row_to_operation,
    )
    .optional()
    .map_err(Error::Database)
}

pub fn get_operation_or(conn: &Connection, op_id: i64) -> Result<Operation> {
    get_operation(conn, op_id)?.ok_or_else(|| Error::NotFound(format!("operation {op_id}")))
}

/// The newest non-terminal operation for an application, if any.
pub fn active_operation_for_app(
    conn: &Connection,
    application_id: &str,
) -> Result<Option<Operation>> {
    conn.query_row(
        &format!(
            "SELECT {COLUMNS} FROM operations
             WHERE application_id = ?1 AND phase IN ('pending','waiting','running','compensating')
             ORDER BY id DESC LIMIT 1"
        ),
        params![application_id],
        row_to_operation,
    )
    .optional()
    .map_err(Error::Database)
}

/// Snapshot of all non-terminal operations (renderer projection, CR-203).
pub fn active_operations(conn: &Connection) -> Result<Vec<Operation>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COLUMNS} FROM operations
             WHERE phase IN ('pending','waiting','running','compensating')
             ORDER BY id DESC"
        ))
        .map_err(Error::Database)?;
    let rows = stmt
        .query_map([], row_to_operation)
        .map_err(Error::Database)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(Error::Database)?);
    }
    Ok(out)
}

/// Convert a guarded UPDATE's affected-row count into a typed error: 0 rows on
/// an existing row means the operation is in an unexpected phase (conflict),
/// never a silent success (#07 pattern from batch 1).
fn expect_phase(affected: usize, op_id: i64, from: &[&str], to: &str) -> Result<()> {
    if affected == 0 {
        return Err(Error::Conflict(format!(
            "operation {op_id} cannot transition to {to} from current phase (expected one of {from:?})"
        )));
    }
    Ok(())
}

/// Guarded phase transition: only succeeds when the operation is in one of
/// `from`. A 0-row update surfaces as a typed conflict.
pub fn transition(conn: &Connection, op_id: i64, from: &[&str], to: &str) -> Result<()> {
    if from.is_empty() {
        return Err(Error::Internal(
            "phase transition requires at least one from phase".into(),
        ));
    }
    let placeholders = (0..from.len())
        .map(|i| format!("?{}", i + 4))
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "UPDATE operations SET phase = ?1, updated_at = ?2
         WHERE id = ?3 AND phase IN ({placeholders})"
    );
    let mut args: Vec<String> = vec![to.to_string(), now(), op_id.to_string()];
    args.extend(from.iter().map(|f| f.to_string()));
    let n = conn
        .execute(&sql, rusqlite::params_from_iter(args))
        .map_err(Error::Database)?;
    expect_phase(n, op_id, from, to)
}

/// Settle a succeeded operation (running → succeeded), then prune.
pub fn finish_success(conn: &Connection, op_id: i64) -> Result<()> {
    let t = now();
    transition(
        conn,
        op_id,
        &[PHASE_PENDING, PHASE_WAITING, PHASE_RUNNING],
        PHASE_SUCCEEDED,
    )?;
    conn.execute(
        "UPDATE operations SET finished_at = ?2 WHERE id = ?1",
        params![op_id, t],
    )
    .map_err(Error::Database)?;
    prune(conn)
}

/// Settle a failed operation (running → failed) with error details, then prune.
pub fn finish_failure(
    conn: &Connection,
    op_id: i64,
    code: Option<&str>,
    message: &str,
) -> Result<()> {
    let t = now();
    transition(
        conn,
        op_id,
        &[
            PHASE_PENDING,
            PHASE_WAITING,
            PHASE_RUNNING,
            PHASE_COMPENSATING,
        ],
        PHASE_FAILED,
    )?;
    conn.execute(
        "UPDATE operations SET error_code = ?2, error_message = ?3, finished_at = ?4
         WHERE id = ?1",
        params![op_id, code, message, t],
    )
    .map_err(Error::Database)?;
    prune(conn)
}

/// Settle a cancelled operation (before or during side effects — e.g. a start
/// superseded by a concurrent stop), then prune.
pub fn finish_cancelled(conn: &Connection, op_id: i64, reason: Option<&str>) -> Result<()> {
    let t = now();
    transition(
        conn,
        op_id,
        &[PHASE_PENDING, PHASE_WAITING, PHASE_RUNNING],
        PHASE_CANCELLED,
    )?;
    conn.execute(
        "UPDATE operations SET error_message = ?2, finished_at = ?3 WHERE id = ?1",
        params![op_id, reason, t],
    )
    .map_err(Error::Database)?;
    prune(conn)
}

/// Cancel an operation that has not started any side effect (pending/waiting).
/// Returns the settled operation.
pub fn cancel(conn: &Connection, op_id: i64) -> Result<Operation> {
    let t = now();
    transition(
        conn,
        op_id,
        &[PHASE_PENDING, PHASE_WAITING],
        PHASE_CANCELLED,
    )?;
    conn.execute(
        "UPDATE operations SET finished_at = ?2 WHERE id = ?1",
        params![op_id, t],
    )
    .map_err(Error::Database)?;
    get_operation_or(conn, op_id)
}

/// Keep only the newest `RETAIN_TERMINAL` terminal rows. Called on settle so
/// the journal cannot grow without bound.
pub fn prune(conn: &Connection) -> Result<()> {
    conn.execute(
        "DELETE FROM operations
         WHERE phase IN ('succeeded','failed','compensated','cancelled')
           AND id NOT IN (SELECT id FROM operations ORDER BY id DESC LIMIT ?1)",
        params![RETAIN_TERMINAL],
    )
    .map_err(Error::Database)?;
    Ok(())
}

/// Settle every non-terminal operation to `failed` (host restarted). Called once
/// at Host startup: after a crash any in-flight operation is stale by
/// definition, and leaving it non-terminal would make the Renderer report a
/// busy app forever. This is the recovery carrier for #10.
pub fn settle_stale_on_startup(conn: &Connection) -> Result<u32> {
    let t = now();
    let n = conn
        .execute(
            "UPDATE operations
             SET phase = 'failed', error_code = 'host_restarted',
                 error_message = 'host restarted before the operation finished',
                 finished_at = ?1, updated_at = ?1
             WHERE phase IN ('pending','waiting','running','compensating')",
            params![t],
        )
        .map_err(Error::Database)?;
    Ok(n as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{apply_migrations, create_tables};

    fn mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        create_tables(&conn).unwrap();
        apply_migrations(&conn).unwrap();
        conn
    }

    fn count(conn: &Connection, sql: &str) -> i64 {
        conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }

    /// Seed an `applications` row so operations with an application_id satisfy
    /// the FK (foreign_keys is ON).
    fn with_app(conn: &Connection, app_id: &str) {
        conn.execute(
            "INSERT INTO applications (id, source, source_id, title, version, created_at, updated_at)
             VALUES (?1, 'local_project', ?2, 'T', '1', 't', 't')",
            params![app_id, app_id],
        )
        .unwrap();
    }

    #[test]
    fn create_then_phase_walk_to_success() {
        let conn = mem();
        with_app(&conn, "app-1");
        let op = create_operation(
            &conn,
            Some("app-1"),
            KIND_START,
            "user",
            Some("{\"appId\":\"a\"}"),
        )
        .unwrap();
        assert_eq!(
            get_operation(&conn, op).unwrap().unwrap().phase,
            PHASE_PENDING
        );

        transition(&conn, op, &[PHASE_PENDING], PHASE_WAITING).unwrap();
        transition(&conn, op, &[PHASE_WAITING], PHASE_RUNNING).unwrap();
        finish_success(&conn, op).unwrap();

        let op = get_operation_or(&conn, op).unwrap();
        assert_eq!(op.phase, PHASE_SUCCEEDED);
        assert!(op.finished_at.is_some());
        assert!(is_terminal(&op.phase));
        assert!(!is_active(&op.phase));
    }

    #[test]
    fn guarded_transition_rejects_wrong_phase() {
        let conn = mem();
        let op = create_operation(&conn, None, KIND_STOP, "user", None).unwrap();
        // running → waiting is not a legal move from pending without passing
        // through waiting first; the guarded UPDATE must conflict.
        let err = transition(&conn, op, &[PHASE_RUNNING], PHASE_WAITING).unwrap_err();
        assert!(matches!(err, Error::Conflict(_)));
        assert_eq!(
            get_operation(&conn, op).unwrap().unwrap().phase,
            PHASE_PENDING,
            "phase must be unchanged after a failed transition"
        );
    }

    #[test]
    fn terminal_phase_is_final() {
        let conn = mem();
        let op = create_operation(&conn, None, KIND_START, "user", None).unwrap();
        finish_failure(&conn, op, Some("start_failed"), "boom").unwrap();
        let err = finish_success(&conn, op).unwrap_err();
        assert!(
            matches!(err, Error::Conflict(_)),
            "a failed operation cannot be settled succeeded"
        );
        let op = get_operation_or(&conn, op).unwrap();
        assert_eq!(op.phase, PHASE_FAILED);
        assert_eq!(op.error_code.as_deref(), Some("start_failed"));
        assert_eq!(op.error_message.as_deref(), Some("boom"));
    }

    #[test]
    fn cancel_only_before_side_effects() {
        let conn = mem();
        let op = create_operation(&conn, None, KIND_DELETE, "user", None).unwrap();
        let cancelled = cancel(&conn, op).unwrap();
        assert_eq!(cancelled.phase, PHASE_CANCELLED);
        assert!(cancelled.finished_at.is_some());

        let op2 = create_operation(&conn, None, KIND_START, "user", None).unwrap();
        transition(&conn, op2, &[PHASE_PENDING], PHASE_RUNNING).unwrap();
        let err = cancel(&conn, op2).unwrap_err();
        assert!(
            matches!(err, Error::Conflict(_)),
            "running operations cannot be cancelled (resources may exist)"
        );
    }

    #[test]
    fn active_queries_ignore_terminal_rows() {
        let conn = mem();
        with_app(&conn, "app-1");
        create_operation(&conn, Some("app-1"), KIND_START, "user", None).unwrap();
        let done = create_operation(&conn, Some("app-1"), KIND_STOP, "user", None).unwrap();
        finish_success(&conn, done).unwrap();

        let active = active_operations(&conn).unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].kind, KIND_START);
        assert!(active_operation_for_app(&conn, "app-1").unwrap().is_some());

        // A different app has no active operation.
        assert!(active_operation_for_app(&conn, "app-2").unwrap().is_none());
    }

    #[test]
    fn prune_keeps_newest_terminal_rows_and_all_active() {
        let conn = mem();
        for i in 0..600 {
            let op = create_operation(&conn, None, KIND_START, "user", None).unwrap();
            finish_success(&conn, op).unwrap();
        }
        // All 600 are terminal now; retention keeps the newest 500.
        assert_eq!(
            count(&conn, "SELECT COUNT(*) FROM operations"),
            RETAIN_TERMINAL
        );

        // An active row is never pruned.
        create_operation(&conn, None, KIND_START, "user", None).unwrap();
        assert_eq!(
            count(
                &conn,
                "SELECT COUNT(*) FROM operations WHERE phase = 'pending'"
            ),
            1
        );
    }

    #[test]
    fn set_application_binds_install_operation() {
        let conn = mem();
        with_app(&conn, "app-9");
        let op = create_operation(&conn, None, KIND_INSTALL, "user", None).unwrap();
        assert!(get_operation(&conn, op)
            .unwrap()
            .unwrap()
            .application_id
            .is_none());
        set_application(&conn, op, "app-9").unwrap();
        assert_eq!(
            get_operation(&conn, op)
                .unwrap()
                .unwrap()
                .application_id
                .as_deref(),
            Some("app-9")
        );
    }

    #[test]
    fn settle_stale_on_startup_fails_in_flight_ops() {
        let conn = mem();
        with_app(&conn, "app-1");
        create_operation(&conn, Some("app-1"), KIND_START, "user", None).unwrap();
        let done = create_operation(&conn, Some("app-1"), KIND_STOP, "user", None).unwrap();
        finish_success(&conn, done).unwrap();

        let settled = settle_stale_on_startup(&conn).unwrap();
        assert_eq!(settled, 1, "only the in-flight operation is stale");

        let active = active_operations(&conn).unwrap();
        assert!(active.is_empty(), "no operation survives startup as active");
        let failed = get_operation(&conn, 1).unwrap().unwrap();
        assert_eq!(failed.phase, PHASE_FAILED);
        assert_eq!(failed.error_code.as_deref(), Some("host_restarted"));
    }
}
