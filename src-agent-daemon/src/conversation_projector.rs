//! Idempotent, transaction-scoped projection of committed typed turns from the
//! replayable event log into the conversation tables (TASK-005 / B03).
//!
//! - The event log is the single replayable source of truth; only committed
//!   (TurnCompleted) turns are materialized as complete assistant messages.
//! - Each committed turn writes its turn row, assistant message + blocks, tool
//!   result messages + blocks, and projection watermark in ONE transaction, so
//!   a crash mid-projection leaves either nothing or everything for that turn.
//! - Re-running the projector over the same events is idempotent. A content
//!   conflict or a corrupt event is quarantined explicitly in
//!   `projection_quarantine` — never silently skipped, never silently
//!   overwritten — and the remaining turns still project.
//! - FK decisions are made by actual row existence, not by run-/turn- name
//!   prefixes. The projector requires an existing run row; a run without one
//!   fails closed with an explicit error (legacy/symbolic-run projection is
//!   retired — the pre-typed engine that produced such data is gone).

use assistant_protocol::v2::{RunEventKind, RunEventV2};
use rusqlite::params;

use self::conversation_projector_commit::{group_turns, project_committed_turn, quarantine};
use crate::conversation_store;

/// A projection write that failed inside the per-turn transaction. The
/// `Conflict` variant must be quarantined AFTER the transaction rolls back —
/// a quarantine row written inside the transaction would roll back with it.
enum ProjectionFailure {
    /// A stored message disagrees with the content derived from the events.
    Conflict {
        run_id: String,
        turn_id: String,
        message_id: String,
    },
    /// A transient DB error (busy/locked/full); the watermark was not advanced.
    Retryable(String),
    /// A non-transient error (FK/constraint/other); the watermark was not
    /// advanced. Recovery must surface it, not silently skip the run.
    Fatal(String),
}

/// How one committed turn group ended after a projection attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProjectionStatus {
    /// The turn was newly materialized and the watermark advanced.
    Projected,
    /// The turn was already projected; nothing changed (idempotent no-op).
    AlreadyProjected,
    /// A corrupt/conflicting turn was explicitly quarantined and is durable.
    Quarantined,
    /// A transient DB error (busy/locked/disk-full) aborted the turn; the
    /// watermark was NOT advanced so a later retry re-attempts it.
    RetryableFailure,
    /// A non-transient error (FK/constraint/other) aborted the turn; the
    /// watermark was NOT advanced.
    FatalFailure,
}

/// A run-level projection failure. `retryable` separates transient DB errors
/// from permanent ones so startup recovery can keep a retry watermark: a
/// retryable failure is re-attempted on the next start, a fatal one is surfaced
/// to the operator instead of being silently swallowed.
#[derive(Debug, Clone)]
pub struct ProjectionError {
    pub message: String,
    pub retryable: bool,
}

impl ProjectionError {
    fn retryable(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            retryable: true,
        }
    }

    fn fatal(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            retryable: false,
        }
    }
}

impl std::fmt::Display for ProjectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ProjectionError {}

impl From<ProjectionError> for String {
    fn from(value: ProjectionError) -> Self {
        value.message
    }
}

/// String errors from store/open paths are conservatively retryable: re-running
/// recovery is always safe (projection is idempotent), so failing a run over a
/// one-shot open error only delays the retry, never corrupts data.
impl From<String> for ProjectionError {
    fn from(value: String) -> Self {
        ProjectionError::retryable(value)
    }
}

impl From<ProjectionFailure> for ProjectionError {
    fn from(value: ProjectionFailure) -> Self {
        match value {
            ProjectionFailure::Conflict {
                run_id,
                turn_id,
                message_id,
            } => ProjectionError::fatal(format!(
                "content conflict for message {message_id} (run {run_id}, turn {turn_id}) was not quarantined"
            )),
            ProjectionFailure::Retryable(message) => ProjectionError::retryable(message),
            ProjectionFailure::Fatal(message) => ProjectionError::fatal(message),
        }
    }
}

/// Classify a rusqlite error as retryable (busy/locked/disk-full) or not.
/// FK/constraint violations are permanent — no amount of retrying changes the
/// rows — so they surface as fatal failures, never as quarantines.
fn classify_db_error(error: &rusqlite::Error) -> bool {
    matches!(
        error.sqlite_error_code(),
        Some(
            rusqlite::ffi::ErrorCode::DatabaseBusy
                | rusqlite::ffi::ErrorCode::DatabaseLocked
                | rusqlite::ffi::ErrorCode::DiskFull
                | rusqlite::ffi::ErrorCode::OperationInterrupted
                | rusqlite::ffi::ErrorCode::OutOfMemory
                | rusqlite::ffi::ErrorCode::SystemIoFailure
        )
    )
}

fn sqlite_failure(context: &str, error: rusqlite::Error) -> ProjectionFailure {
    if classify_db_error(&error) {
        ProjectionFailure::Retryable(format!("{context}: {error}"))
    } else {
        ProjectionFailure::Fatal(format!("{context}: {error}"))
    }
}

/// Per-run projection report.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunProjection {
    pub projected: usize,
    pub already_projected: usize,
    pub quarantined: usize,
}

/// Startup-recovery report: every target run ended projected or quarantined.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RecoveryReport {
    /// Runs that were actually processed (projected or quarantined).
    pub runs: usize,
    pub total_projected: usize,
    pub total_quarantined: usize,
}

/// Project all committed turns in `events` into the conversation tables for a
/// run.
///
/// Production entry point (TASK-005): the daemon calls this after a run's
/// engine completes and again during startup recovery. Both calls are
/// idempotent; corrupt events are isolated in `projection_quarantine` and the
/// other turns still project.
///
/// Error contract (T03): a DB/FK/commit error aborts the whole run with a
/// [`ProjectionError`] and leaves the projection watermark exactly where it
/// was — the successfully projected turns stay durable, the failed turn is
/// retried on the next call. No error is swallowed: only genuinely corrupt or
/// conflicting turns are quarantined, and never a plain DB failure.
pub fn project_run_from_events(
    conversation_id: &str,
    run_id: &str,
    events: &[RunEventV2],
) -> Result<RunProjection, ProjectionError> {
    let run_exists: i64 = conversation_store::store()?
        .conn()?
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM run WHERE id = ?1)",
            params![run_id],
            |row| row.get(0),
        )
        .map_err(|e| sqlite_failure("project run existence", e))?;
    if run_exists == 0 {
        // Retired: the pre-typed engine wrote events without a run FK row and
        // they were routed to the compat reader in `conversation_store`. That
        // path is gone (MIG-004); a missing run row is now an explicit failure.
        return Err(ProjectionError::fatal(format!(
            "projection requires an existing run row (legacy/symbolic-run projection is retired): {run_id}"
        )));
    }
    let mut report = RunProjection::default();
    for group in group_turns(events) {
        match project_committed_turn(conversation_id, run_id, &group)? {
            Some(ProjectionStatus::Projected) => report.projected += 1,
            Some(ProjectionStatus::AlreadyProjected) => report.already_projected += 1,
            Some(ProjectionStatus::Quarantined) => report.quarantined += 1,
            Some(ProjectionStatus::RetryableFailure) | Some(ProjectionStatus::FatalFailure) => {
                unreachable!("projection failures are returned as Err, never as a status")
            }
            None => {}
        }
    }
    Ok(report)
}

/// Startup recovery (B03): backfill projections for every run that has
/// committed turns not yet covered by its projection watermark.
///
/// Fails (returns `Err`) whenever any target run ends in a DB/FK/commit error,
/// so a half-projected run is never silently accepted as recovered. Runs whose
/// events/turns were explicitly quarantined count as handled — recovery only
/// succeeds when every target run is projected or quarantined.
pub fn recover_projections() -> Result<RecoveryReport, String> {
    let store = conversation_store::store()?;
    let run_ids: Vec<String> = {
        let conn = store.conn()?;
        runs_needing_recovery(&conn)?
    };
    let mut report = RecoveryReport::default();
    for run_id in run_ids {
        let conversation_id = {
            let conn = store.conn()?;
            conn.query_row(
                "SELECT conversation_id FROM run WHERE id = ?1",
                params![run_id],
                |row| row.get::<_, String>(0),
            )
            .map_err(|e| {
                format!("projection recovery: conversation lookup for run {run_id}: {e}")
            })?
        };
        if conversation_id.trim().is_empty() {
            // No FK row to attach the projection to; nothing projectable.
            continue;
        }
        let conn = store.conn()?;
        match load_events_for_run(&conn, &run_id)? {
            LoadEvents::Events(events) if events.is_empty() => {}
            LoadEvents::Events(events) => {
                let projection = project_run_from_events(&conversation_id, &run_id, &events)
                    .map_err(|e| format!("projection recovery failed for run {run_id}: {e}"))?;
                report.runs += 1;
                report.total_projected += projection.projected;
                report.total_quarantined += projection.quarantined;
            }
            LoadEvents::QuarantinedRun => {
                report.runs += 1;
                report.total_quarantined += 1;
            }
        }
    }
    Ok(report)
}

/// Runs with a committed turn not yet covered by the projection watermark.
fn runs_needing_recovery(conn: &rusqlite::Connection) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT re.run_id
             FROM run_event re
             WHERE re.event_type = 'turn_completed'
               AND re.sequence > COALESCE(
                   (SELECT w.event_sequence FROM projection_watermark w
                    WHERE w.projector = 'conversation' AND w.run_id = re.run_id), -1)
             GROUP BY re.run_id",
        )
        .map_err(|e| e.to_string())?;
    let ids = stmt
        .query_map([], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<String>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(ids)
}

/// Result of decoding a run's stored events.
enum LoadEvents {
    Events(Vec<RunEventV2>),
    /// A corrupt stored payload explicitly quarantined the run; recovery counts
    /// it as handled (quarantine is durable and queryable) and moves on.
    QuarantinedRun,
}

/// A8 — watermark-driven incremental projection.
///
/// Replays only the events AFTER the run's last projection watermark — never
/// the full stream from sequence 0 — so already projected prefixes are not
/// re-scanned on every run-end/startup pass. Per-turn idempotency, quarantine,
/// and partial-turn safety are unchanged: `project_committed_turn` still
/// materializes only TurnCompleted turns and leaves a crashed/partial turn
/// un-materialized; `MessageCompleted.content` remains the committed-content
/// authority.
pub fn project_run_incremental(
    conversation_id: &str,
    run_id: &str,
) -> Result<RunProjection, ProjectionError> {
    let store = conversation_store::store()
        .map_err(|e| ProjectionError::retryable(format!("projection store: {e}")))?;
    let conn = store
        .conn()
        .map_err(|e| ProjectionError::retryable(format!("projection conn: {e}")))?;
    let run_exists: i64 = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM run WHERE id = ?1)",
            params![run_id],
            |row| row.get(0),
        )
        .map_err(|e| sqlite_failure("project incremental run existence", e))?;
    if run_exists == 0 {
        // Retired: the pre-typed engine wrote events without a run FK row and
        // they were routed to the compat reader in `conversation_store`. That
        // path is gone (MIG-004); a missing run row is now an explicit failure.
        return Err(ProjectionError::fatal(format!(
            "projection requires an existing run row (legacy/symbolic-run projection is retired): {run_id}"
        )));
    }
    // Read the durable projection watermark (0 when never projected).
    let after: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(event_sequence), 0) FROM projection_watermark
             WHERE projector = 'conversation' AND run_id = ?1",
            params![run_id],
            |row| row.get(0),
        )
        .map_err(|e| sqlite_failure("project incremental watermark read", e))?;
    let events = match load_events_after(&conn, run_id, after.max(0) as u64)
        .map_err(ProjectionError::retryable)?
    {
        LoadEvents::Events(events) => events,
        LoadEvents::QuarantinedRun => {
            return Ok(RunProjection {
                quarantined: 1,
                ..RunProjection::default()
            })
        }
    };
    if events.is_empty() {
        return Ok(RunProjection::default());
    }
    let mut report = RunProjection::default();
    for group in group_turns(&events) {
        match project_committed_turn(conversation_id, run_id, &group)? {
            Some(ProjectionStatus::Projected) => report.projected += 1,
            Some(ProjectionStatus::AlreadyProjected) => report.already_projected += 1,
            Some(ProjectionStatus::Quarantined) => report.quarantined += 1,
            Some(ProjectionStatus::RetryableFailure) | Some(ProjectionStatus::FatalFailure) => {
                unreachable!("projection failures are returned as Err, never as a status")
            }
            None => {}
        }
    }
    Ok(report)
}

/// Explicit projector for usage aggregates (W2): the EventLog only appends and
/// replays durable events; all read-model projection lives here. Called by the
/// EventLog append path when a `UsageUpdated` event is persisted.
pub fn project_usage_rollup(conn: &rusqlite::Connection, event: &RunEventV2) -> Result<(), String> {
    let RunEventKind::UsageUpdated {
        input_tokens,
        output_tokens,
        cache_creation_tokens,
        cache_read_tokens,
        ..
    } = &event.payload
    else {
        return Ok(());
    };
    let input = *input_tokens as i64;
    let output = *output_tokens as i64;
    // A provider that does not report cache usage contributes 0 to the
    // rollup rather than poisoning it — the per-event `None` is still
    // preserved verbatim in the serialized payload.
    let cache_creation = cache_creation_tokens.unwrap_or(0) as i64;
    let cache_read = cache_read_tokens.unwrap_or(0) as i64;
    let _ = conn.execute(
        "UPDATE run
         SET total_input_tokens = COALESCE(total_input_tokens, 0) + ?1,
             total_output_tokens = COALESCE(total_output_tokens, 0) + ?2
         WHERE id = ?3",
        params![input, output, event.run_id],
    );
    // Best-effort dual-write to message token columns when a trigger
    // message exists (keeps conversation-level history useful).
    let _ = conn.execute(
        "UPDATE message
         SET input_tokens = COALESCE(input_tokens, 0) + ?1,
             output_tokens = COALESCE(output_tokens, 0) + ?2
         WHERE id = (
            SELECT trigger_message_id FROM run WHERE id = ?3 AND trigger_message_id IS NOT NULL
         )",
        params![input, output, event.run_id],
    );

    // Aggregate into usage_stats when the table exists.
    // date uses UTC YYYY-MM-DD; dashboard localizes via range filters.
    let model = conn
        .query_row(
            "SELECT model_id FROM run WHERE id = ?1",
            params![event.run_id],
            |row| row.get::<_, String>(0),
        )
        .unwrap_or_else(|_| "unknown".into());
    let date = event.timestamp.format("%Y-%m-%d").to_string();
    // Ensure table exists (older DBs may not have been migrated by Host).
    let _ = conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS usage_stats (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            date TEXT NOT NULL,
            source TEXT NOT NULL,
            source_path TEXT,
            model TEXT NOT NULL,
            input_tokens INTEGER NOT NULL DEFAULT 0,
            output_tokens INTEGER NOT NULL DEFAULT 0,
            cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
            cache_read_tokens INTEGER NOT NULL DEFAULT 0,
            request_count INTEGER NOT NULL DEFAULT 0,
            cost_usd REAL NOT NULL DEFAULT 0.0,
            UNIQUE(date, source, model)
        );",
    );
    let _ = conn.execute(
        "INSERT INTO usage_stats
            (date, source, source_path, model, input_tokens, output_tokens,
             cache_creation_tokens, cache_read_tokens, request_count, cost_usd)
         VALUES (?1, 'natives', 'daemon:run_event', ?2, ?3, ?4, ?5, ?6, 1, 0.0)
         ON CONFLICT(date, source, model) DO UPDATE SET
            input_tokens = input_tokens + excluded.input_tokens,
            output_tokens = output_tokens + excluded.output_tokens,
            cache_creation_tokens =
                cache_creation_tokens + excluded.cache_creation_tokens,
            cache_read_tokens = cache_read_tokens + excluded.cache_read_tokens,
            request_count = request_count + 1",
        params![date, model, input, output, cache_creation, cache_read],
    );
    Ok(())
}

/// Decode only the stored events with `sequence > after_sequence` for a run
/// (the watermark prefix is skipped — A8 incremental replay).
fn load_events_after(
    conn: &rusqlite::Connection,
    run_id: &str,
    after_sequence: u64,
) -> Result<LoadEvents, String> {
    let payloads: Vec<String> = conn
        .prepare(
            "SELECT payload FROM run_event WHERE run_id = ?1 AND sequence > ?2 ORDER BY sequence",
        )
        .map_err(|e| e.to_string())?
        .query_map(params![run_id, after_sequence as i64], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    let mut events = Vec::with_capacity(payloads.len());
    for payload in payloads {
        match serde_json::from_str::<RunEventV2>(&payload) {
            Ok(event) => events.push(event),
            Err(error) => {
                quarantine(
                    run_id,
                    None,
                    None,
                    "corrupt_stored_event",
                    &error.to_string(),
                )
                .map_err(|e| {
                    format!("corrupt stored event for run {run_id} could not be quarantined: {e}")
                })?;
                return Ok(LoadEvents::QuarantinedRun);
            }
        }
    }
    Ok(LoadEvents::Events(events))
}

/// Decode every stored event for a run. A corrupt payload quarantines the run
/// explicitly (no silent skip) and is reported as [`LoadEvents::QuarantinedRun`]
/// so recovery never mistakes it for a swallowed failure.
fn load_events_for_run(conn: &rusqlite::Connection, run_id: &str) -> Result<LoadEvents, String> {
    let payloads: Vec<String> = conn
        .prepare("SELECT payload FROM run_event WHERE run_id = ?1 ORDER BY sequence")
        .map_err(|e| e.to_string())?
        .query_map(params![run_id], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    let mut events = Vec::with_capacity(payloads.len());
    for payload in payloads {
        match serde_json::from_str::<RunEventV2>(&payload) {
            Ok(event) => events.push(event),
            Err(error) => {
                quarantine(
                    run_id,
                    None,
                    None,
                    "corrupt_stored_event",
                    &error.to_string(),
                )
                .map_err(|e| {
                    format!("corrupt stored event for run {run_id} could not be quarantined: {e}")
                })?;
                return Ok(LoadEvents::QuarantinedRun);
            }
        }
    }
    Ok(LoadEvents::Events(events))
}

mod conversation_projector_blocks;
mod conversation_projector_commit;

#[cfg(test)]
#[path = "conversation_projector_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "conversation_projector_incremental_tests.rs"]
mod incremental_projection_tests;
