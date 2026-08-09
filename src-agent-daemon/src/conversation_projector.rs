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

use agent_core::{AssistantMessage, ContentBlock, MessageId, ToolResultBlock, ToolResultMessage};
use assistant_protocol::v2::{RunEventKind, RunEventV2};
use rusqlite::params;
use serde_json::Value;
use sha2::{Digest, Sha256};

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

/// Split events into turn groups: a new TurnStarted closes the previous group,
/// TurnCompleted closes its own group, and any leftover events form a final
/// (possibly partial) group.
fn group_turns(events: &[RunEventV2]) -> Vec<Vec<RunEventV2>> {
    let mut groups: Vec<Vec<RunEventV2>> = Vec::new();
    let mut current: Vec<RunEventV2> = Vec::new();
    for event in events {
        if matches!(&event.payload, RunEventKind::TurnStarted { .. }) && !current.is_empty() {
            groups.push(std::mem::take(&mut current));
        }
        current.push(event.clone());
        if matches!(&event.payload, RunEventKind::TurnCompleted { .. }) {
            groups.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        groups.push(current);
    }
    groups
}

/// Project one turn group in a single transaction. Returns:
///
/// - `Ok(None)` when the group is skipped (partial typed turn, or nothing
///   projectable);
/// - `Ok(Some(status))` when the group was handled with that status;
/// - `Err(ProjectionError)` when a DB/FK/commit error aborted the group — the
///   watermark is untouched so the retry re-attempts it.
fn project_committed_turn(
    conversation_id: &str,
    run_id: &str,
    events: &[RunEventV2],
) -> Result<Option<ProjectionStatus>, ProjectionError> {
    let has_turn_start = events
        .iter()
        .any(|e| matches!(&e.payload, RunEventKind::TurnStarted { .. }));
    let has_turn_completed = events
        .iter()
        .any(|e| matches!(&e.payload, RunEventKind::TurnCompleted { .. }));
    if has_turn_start && !has_turn_completed {
        // A crashed/partial typed turn is never materialized as a complete
        // assistant message.
        return Ok(None);
    }
    let turn_id = events.iter().find_map(|event| match &event.payload {
        RunEventKind::TurnStarted { turn_id } => Some(turn_id.clone()),
        _ => None,
    });
    // Retired: a turn group without TurnStarted is a pre-typed delta-only
    // batch (the engine always emits typed turns now). It is not materialized.
    let Some(typed_turn_id) = turn_id else {
        return Ok(None);
    };
    let assistant_message_id = events.iter().find_map(|event| match &event.payload {
        RunEventKind::MessageStarted {
            message_id, role, ..
        } if role == "assistant" => Some(message_id.clone()),
        _ => None,
    });
    let stop_reason = events.iter().find_map(|event| match &event.payload {
        RunEventKind::TurnCompleted { stop_reason, .. } => Some(stop_reason.clone()),
        _ => None,
    });
    let mut text = String::new();
    let mut thinking = String::new();
    let mut tool_calls = Vec::new();
    let mut tool_results = Vec::new();
    let mut committed_content: Option<Vec<ContentBlock>> = None;
    for event in events {
        match &event.payload {
            RunEventKind::TextDelta { text: delta } => text.push_str(delta),
            RunEventKind::ReasoningDelta { text: delta } => thinking.push_str(delta),
            RunEventKind::ToolCallRequested { id, name, input } => {
                tool_calls.push(agent_core::ToolCall {
                    tool_call_id: id.clone().into(),
                    name: name.clone(),
                    arguments_json: input.to_string(),
                });
            }
            RunEventKind::ToolCallCompleted {
                id,
                name,
                output,
                is_error,
                duration_ms,
                result_message_id,
            } => tool_results.push((
                id.clone(),
                name.clone(),
                output.clone(),
                *is_error,
                *duration_ms,
                result_message_id.clone(),
            )),
            RunEventKind::MessageCompleted {
                content: Some(content),
                ..
            } => {
                // A corrupt committed payload is quarantined explicitly, never
                // swallowed. If the quarantine row itself cannot be written,
                // that is a retryable failure — the next recovery re-attempts.
                let blocks = match content
                    .get("content")
                    .ok_or_else(|| content.to_string())
                    .and_then(|blocks| {
                        serde_json::from_value::<Vec<ContentBlock>>(blocks.clone())
                            .map_err(|e| e.to_string())
                    }) {
                    Ok(blocks) => blocks,
                    Err(detail) => {
                        quarantine(
                            run_id,
                            Some(&typed_turn_id),
                            assistant_message_id.as_deref(),
                            "corrupt_message_completed",
                            &detail,
                        )
                        .map_err(|e| {
                            ProjectionError::retryable(format!(
                                "corrupt turn could not be quarantined: {e}"
                            ))
                        })?;
                        return Ok(Some(ProjectionStatus::Quarantined));
                    }
                };
                committed_content = Some(blocks);
            }
            RunEventKind::MessageCompleted { content: None, .. } => {}
            _ => {}
        }
    }
    let content = committed_content.unwrap_or_else(|| {
        let mut content = Vec::new();
        if !thinking.trim().is_empty() {
            content.push(ContentBlock::Thinking {
                text: thinking,
                signature: None,
            });
        }
        if !text.trim().is_empty() {
            content.push(ContentBlock::Text { text });
        }
        content.extend(
            tool_calls
                .into_iter()
                .map(agent_core::ContentBlock::ToolCall),
        );
        content
    });
    if content.is_empty() && tool_results.is_empty() {
        return Ok(None);
    }
    let assistant_id = assistant_message_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let last_sequence = events
        .iter()
        .map(|event| event.effective_run_sequence())
        .max()
        .unwrap_or(0);
    let turn_sequence = events
        .iter()
        .find_map(|event| match &event.payload {
            RunEventKind::TurnStarted { .. } => Some(event.effective_run_sequence()),
            _ => None,
        })
        .unwrap_or(0);

    let store = conversation_store::store()?;
    let conn = store.conn()?;
    let outcome: Result<TurnWrite, ProjectionFailure> = (|| {
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| sqlite_failure("project turn begin", e))?;

        // Turn record — idempotent; the affected count tells us whether this
        // turn is new (count the watermark once) or an already-projected
        // re-run.
        let turn_affected = tx
            .execute(
                "INSERT OR IGNORE INTO turn (id, run_id, sequence, status, stop_reason, created_at, completed_at)
                 VALUES (?1, ?2, ?3, 'committed', ?4, datetime('now'), ?5)",
                params![
                    typed_turn_id,
                    run_id,
                    turn_sequence as i64,
                    stop_reason,
                    chrono::Utc::now().to_rfc3339()
                ],
            )
            .map_err(|e| sqlite_failure("project turn insert", e))?;

        // Assistant message + blocks: content blocks + run_reference.
        let assistant = AssistantMessage {
            message_id: MessageId::from(assistant_id.clone()),
            content: content.clone(),
            stop_reason: stop_reason.as_deref().map(parse_stop_reason),
        };
        let mut assistant_blocks = content_blocks_to_json(&assistant.content);
        assistant_blocks.push(serde_json::json!({ "type": "run_reference", "run_id": run_id }));
        upsert_message_blocks(
            &tx,
            conversation_id,
            run_id,
            &typed_turn_id,
            &assistant_id,
            "assistant",
            &assistant_blocks,
            stop_reason.as_deref(),
        )?;

        // Tool result messages (one per ToolCallCompleted, by stable result id).
        let mut projected_ids: Vec<String> = vec![assistant_id.clone()];
        for (id, name, output, is_error, _duration_ms, result_message_id) in &tool_results {
            let result_id = result_message_id
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            let result = ToolResultMessage {
                message_id: MessageId::from(result_id.clone()),
                tool_call_id: id.clone().into(),
                tool_name: name.clone(),
                content: result_blocks_for(output),
                is_error: *is_error,
                code: error_code_for(output),
            };
            let blocks = vec![tool_result_block(&result)];
            upsert_message_blocks(
                &tx,
                conversation_id,
                run_id,
                &typed_turn_id,
                &result_id,
                "assistant",
                &blocks,
                None,
            )?;
            projected_ids.push(result_id);
        }

        // Watermark — the event prefix + a digest of the projected message ids.
        // New turns advance the counter; re-projections only ever advance the
        // sequence, so recovery stays idempotent. (The `compat_hits` column is
        // legacy observability and stays 0 — the compat path is retired.)
        let mut hasher = Sha256::new();
        for id in &projected_ids {
            hasher.update(id.as_bytes());
            hasher.update(b"\0");
        }
        let digest = hex::encode(hasher.finalize());
        if turn_affected > 0 {
            tx.execute(
                "INSERT INTO projection_watermark
                    (projector, run_id, event_sequence, turn_count, digest, created_at, updated_at)
                 VALUES ('conversation', ?1, ?2, 1, ?3, datetime('now'), datetime('now'))
                 ON CONFLICT(projector, run_id) DO UPDATE SET
                    event_sequence = excluded.event_sequence,
                    turn_count = turn_count + 1,
                    digest = excluded.digest,
                    updated_at = datetime('now')",
                params![run_id, last_sequence as i64, digest],
            )
            .map_err(|e| sqlite_failure("project watermark upsert", e))?;
        } else {
            tx.execute(
                "INSERT INTO projection_watermark
                    (projector, run_id, event_sequence, turn_count, digest, created_at, updated_at)
                 VALUES ('conversation', ?1, ?2, 1, ?3, datetime('now'), datetime('now'))
                 ON CONFLICT(projector, run_id) DO UPDATE SET
                    event_sequence = MAX(event_sequence, excluded.event_sequence),
                    updated_at = datetime('now')",
                params![run_id, last_sequence as i64, digest],
            )
            .map_err(|e| sqlite_failure("project watermark idempotent", e))?;
        }
        tx.commit()
            .map_err(|e| sqlite_failure("project turn commit", e))?;
        if turn_affected > 0 {
            Ok(TurnWrite::Projected)
        } else {
            Ok(TurnWrite::AlreadyProjected)
        }
    })();
    match outcome {
        Ok(TurnWrite::Projected) => Ok(Some(ProjectionStatus::Projected)),
        Ok(TurnWrite::AlreadyProjected) => Ok(Some(ProjectionStatus::AlreadyProjected)),
        Err(ProjectionFailure::Conflict {
            run_id,
            turn_id,
            message_id,
        }) => {
            // The transaction rolled back when the closure dropped `tx`.
            // Quarantine on a fresh connection so the isolation survives the
            // rollback — never a silent overwrite. A quarantine write failure
            // is surfaced as retryable; the conflict was not silently dropped.
            let detail =
                format!("stored content for message {message_id} disagrees with the events");
            quarantine(
                &run_id,
                Some(&turn_id),
                Some(&message_id),
                "content_conflict",
                &detail,
            )
            .map_err(|e| {
                ProjectionError::retryable(format!(
                    "content conflict could not be quarantined: {e}"
                ))
            })?;
            Ok(Some(ProjectionStatus::Quarantined))
        }
        Err(ProjectionFailure::Retryable(message)) => Err(ProjectionError::retryable(message)),
        Err(ProjectionFailure::Fatal(message)) => Err(ProjectionError::fatal(message)),
    }
}

/// Result of a successful per-turn write.
enum TurnWrite {
    /// The turn row was newly inserted (watermark counted once).
    Projected,
    /// The turn row already existed (idempotent re-projection).
    AlreadyProjected,
}

/// Insert a message row + blocks when absent; when present, verify the stored
/// blocks match the projected blocks (idempotent no-op). A content mismatch is
/// surfaced as `ProjectionFailure::Conflict`; the caller quarantines it AFTER
/// the transaction rolls back, never silently overwriting.
#[allow(clippy::too_many_arguments)] // pre-existing: parameter list is fixed
fn upsert_message_blocks(
    tx: &rusqlite::Transaction,
    conversation_id: &str,
    run_id: &str,
    turn_id: &str,
    message_id: &str,
    role: &str,
    blocks: &[Value],
    stop_reason: Option<&str>,
) -> Result<(), ProjectionFailure> {
    let existing: Vec<(i64, String)> = tx
        .prepare(
            "SELECT sort_order, block_json FROM message_block
             WHERE message_id = ?1 ORDER BY sort_order",
        )
        .map_err(|e| sqlite_failure("project existing blocks read", e))?
        .query_map(params![message_id], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|e| sqlite_failure("project existing blocks map", e))?
        .collect::<Result<_, _>>()
        .map_err(|e| sqlite_failure("project existing blocks collect", e))?;
    if !existing.is_empty() {
        let projected: Vec<(i64, String)> = blocks
            .iter()
            .enumerate()
            .map(|(index, block)| (index as i64, block.to_string()))
            .collect();
        if existing != projected {
            return Err(ProjectionFailure::Conflict {
                run_id: run_id.to_string(),
                turn_id: turn_id.to_string(),
                message_id: message_id.to_string(),
            });
        }
        // Already projected with identical content — idempotent no-op.
        return Ok(());
    }
    let now = chrono::Utc::now().to_rfc3339();
    tx.execute(
        "INSERT INTO message
            (id, conversation_id, role, status, turn_id, run_id, legacy_marker, truncated, stop_reason, created_at)
         VALUES (?1, ?2, ?3, 'complete', ?4, ?5, NULL, 0, ?6, ?7)",
        params![
            message_id,
            conversation_id,
            role,
            turn_id,
            run_id,
            stop_reason,
            now
        ],
    )
    .map_err(|e| sqlite_failure("project message insert", e))?;
    for (index, block) in blocks.iter().enumerate() {
        let block_type = block.get("type").and_then(Value::as_str).unwrap_or("text");
        tx.execute(
            "INSERT INTO message_block (message_id, sort_order, block_type, block_json, artifact_id, truncated)
             VALUES (?1, ?2, ?3, ?4, NULL, 0)",
            params![message_id, index as i64, block_type, block.to_string()],
        )
        .map_err(|e| sqlite_failure("project block insert", e))?;
    }
    Ok(())
}

/// Record an explicitly isolated event/turn. This is the "no silent skip"
/// contract: the quarantine row is durable and queryable.
fn quarantine(
    run_id: &str,
    turn_id: Option<&str>,
    message_id: Option<&str>,
    reason: &str,
    detail: &str,
) -> Result<(), String> {
    conversation_store::store()?
        .conn()?
        .execute(
            "INSERT INTO projection_quarantine
                (projector, run_id, turn_id, message_id, reason, detail)
             VALUES ('conversation', ?1, ?2, ?3, ?4, ?5)",
            params![run_id, turn_id, message_id, reason, detail],
        )
        .map_err(|e| format!("projection quarantine: {e}"))?;
    Ok(())
}

fn content_blocks_to_json(blocks: &[ContentBlock]) -> Vec<Value> {
    blocks
        .iter()
        .map(|block| match block {
            ContentBlock::Text { text } => serde_json::json!({ "type": "text", "text": text }),
            ContentBlock::Thinking { text, signature } => {
                serde_json::json!({ "type": "thinking", "text": text, "signature": signature })
            }
            ContentBlock::Image { source } => {
                serde_json::json!({ "type": "image", "source": source })
            }
            ContentBlock::ToolCall(call) => serde_json::json!({
                "type": "tool_call",
                "tool_call_id": call.tool_call_id,
                "name": call.name,
                "arguments": call.arguments_json,
            }),
        })
        .collect()
}

fn result_blocks_for(output: &Value) -> Vec<ToolResultBlock> {
    if let Some(artifact_id) = output.get("artifact_id").and_then(Value::as_str) {
        return vec![ToolResultBlock::Artifact {
            artifact_id: artifact_id.to_string(),
            preview: output
                .get("preview")
                .and_then(Value::as_str)
                .map(str::to_string),
        }];
    }
    vec![ToolResultBlock::Json {
        value: output.clone(),
    }]
}

fn error_code_for(output: &Value) -> Option<String> {
    output
        .get("error_code")
        .or_else(|| output.get("code"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn tool_result_block(result: &ToolResultMessage) -> Value {
    let content: Vec<Value> = result
        .content
        .iter()
        .map(|block| match block {
            ToolResultBlock::Text { text } => serde_json::json!({ "type": "text", "text": text }),
            ToolResultBlock::Json { value } => {
                serde_json::json!({ "type": "json", "value": value })
            }
            ToolResultBlock::Artifact {
                artifact_id,
                preview,
            } => serde_json::json!({
                "type": "artifact",
                "artifact_id": artifact_id,
                "preview": preview,
            }),
        })
        .collect();
    serde_json::json!({
        "type": "tool_result",
        "tool_call_id": result.tool_call_id,
        "name": result.tool_name,
        "is_error": result.is_error,
        "error_code": result.code,
        "content": content,
    })
}

fn parse_stop_reason(value: &str) -> agent_core::StopReason {
    match value {
        "stop" => agent_core::StopReason::Stop,
        "tool_use" => agent_core::StopReason::ToolUse,
        "length" => agent_core::StopReason::Length,
        "cancelled" => agent_core::StopReason::Cancelled,
        "error" => agent_core::StopReason::Error,
        other => agent_core::StopReason::Provider(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversation_store;

    /// Shared test store (migrations run once per test on a fresh temp DB);
    /// each test uses unique run/conv ids. Returns the tempdir so it stays
    /// alive for the whole test.
    ///
    /// Note: like the rest of the daemon test suite, the NATIVES_* env
    /// mutations here are intentionally NOT restored — several production
    /// fixtures (e.g. `subagent_persona_tests`) rely on `NATIVES_ASSISTANT_DB_PATH`
    /// being present and recreate the path on demand. Restoring it here breaks
    /// those tests (pre-existing coupling, T01 hermeticity debt).
    fn setup() -> ((String, String), tempfile::TempDir) {
        let _guard = crate::storage::DataStore::env_test_lock();
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("projector.db");
        std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db);
        std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
        crate::storage::set_test_db_override(Some(db.clone()), Some(dir.path().join("artifacts")));
        let _store = crate::storage::DataStore::new(&db, &dir.path().join("artifacts")).unwrap();
        let conv = format!("proj-conv-{}", uuid::Uuid::new_v4());
        let run = format!("proj-run-{}", uuid::Uuid::new_v4());
        {
            let store = conversation_store::store().unwrap();
            let conn = store.conn().unwrap();
            conn.execute(
                "INSERT OR IGNORE INTO conversation (id, mode, title, provider_id, model_id)
                 VALUES (?1, 'chat', 'Projector Test', 'prov-1', 'model-1')",
                params![conv],
            )
            .unwrap();
            conn.execute(
                "INSERT OR IGNORE INTO run (id, conversation_id, status, provider_id, model_id)
                 VALUES (?1, ?2, 'completed', 'prov-1', 'model-1')",
                params![run, conv],
            )
            .unwrap();
        }
        ((conv, run), dir)
    }

    fn event(run_id: &str, sequence: u64, payload: RunEventKind) -> RunEventV2 {
        RunEventV2 {
            event_id: format!("evt-{run_id}-{sequence}"),
            global_sequence: 0,
            run_sequence: sequence,
            run_id: run_id.into(),
            sequence,
            timestamp: chrono::Utc::now(),
            payload,
        }
    }

    fn typed_turn_events(run_id: &str, turn: &str) -> Vec<RunEventV2> {
        vec![
            event(
                run_id,
                1,
                RunEventKind::TurnStarted {
                    turn_id: turn.into(),
                },
            ),
            event(
                run_id,
                2,
                RunEventKind::MessageStarted {
                    turn_id: turn.into(),
                    message_id: format!("msg-{turn}"),
                    role: "assistant".into(),
                },
            ),
            event(
                run_id,
                3,
                RunEventKind::TextDelta {
                    text: "hello".into(),
                },
            ),
            event(
                run_id,
                4,
                RunEventKind::ToolCallRequested {
                    id: format!("call-{turn}-1"),
                    name: "read_file".into(),
                    input: serde_json::json!({"path": "/tmp/a.txt"}),
                },
            ),
            event(
                run_id,
                5,
                RunEventKind::ToolCallCompleted {
                    id: format!("call-{turn}-1"),
                    name: "read_file".into(),
                    output: serde_json::json!({"content": "file body"}),
                    is_error: false,
                    duration_ms: 2,
                    result_message_id: Some(format!("rm-{turn}-1")),
                },
            ),
            event(
                run_id,
                6,
                RunEventKind::MessageCompleted {
                    turn_id: turn.into(),
                    message_id: format!("msg-{turn}"),
                    role: "assistant".into(),
                    // The engine stores the typed content as an externally-tagged
                    // `Vec<ContentBlock>`; mirror that exact serialization.
                    content: Some(serde_json::json!({
                        "message_id": format!("msg-{turn}"),
                        "role": "assistant",
                        "content": serde_json::to_value(vec![
                            agent_core::ContentBlock::Text { text: "hello".into() },
                            agent_core::ContentBlock::ToolCall(agent_core::ToolCall {
                                tool_call_id: format!("call-{turn}-1").into(),
                                name: "read_file".into(),
                                arguments_json: "{\"path\":\"/tmp/a.txt\"}".into(),
                            }),
                        ]).unwrap(),
                    })),
                },
            ),
            event(
                run_id,
                7,
                RunEventKind::TurnCompleted {
                    turn_id: turn.into(),
                    stop_reason: "tool_use".into(),
                    input_tokens: 0,
                    output_tokens: 0,
                },
            ),
        ]
    }

    /// TASK-005 acceptance #1/#2: the projector writes the full typed turn
    /// (assistant content + complete tool pair) so a reloaded AgentMessage
    /// transcript matches the event-derived content and the tool result follows
    /// its tool call in block order.
    #[test]
    fn projector_preserves_full_tool_pair_and_block_order() {
        let ((conv, run), _dir) = setup();
        let events = typed_turn_events(&run, "t1");
        let projected = project_run_from_events(&conv, &run, &events)
            .unwrap()
            .projected;
        assert_eq!(projected, 1, "one committed turn projects");
        let messages = conversation_store::load_agent_messages(&conv).unwrap();
        assert_eq!(messages.len(), 2, "assistant + tool result");
        let assistant = messages
            .iter()
            .find_map(|m| match m {
                agent_core::AgentMessage::Assistant(a) => Some(a),
                _ => None,
            })
            .unwrap();
        let tool_call = assistant
            .content
            .iter()
            .find_map(|b| match b {
                ContentBlock::ToolCall(call) => Some(call),
                _ => None,
            })
            .unwrap();
        assert_eq!(tool_call.tool_call_id.to_string(), format!("call-t1-1"));
        let result = messages
            .iter()
            .find_map(|m| match m {
                agent_core::AgentMessage::ToolResult(r) => Some(r),
                _ => None,
            })
            .unwrap();
        assert_eq!(result.tool_call_id.to_string(), format!("call-t1-1"));
        assert_eq!(result.tool_name, "read_file");
        assert!(!result.is_error);
    }

    /// TASK-005: re-projecting the same events is a zero-side-effect no-op.
    #[test]
    fn projector_is_idempotent() {
        let ((conv, run), _dir) = setup();
        let events = typed_turn_events(&run, "t1");
        project_run_from_events(&conv, &run, &events).unwrap();
        let before = conversation_store::load_agent_messages(&conv).unwrap();
        project_run_from_events(&conv, &run, &events).unwrap();
        let after = conversation_store::load_agent_messages(&conv).unwrap();
        assert_eq!(before.len(), after.len());
        assert_eq!(
            before, after,
            "re-projection must not change the transcript"
        );
        let store = conversation_store::store().unwrap();
        let conn = store.conn().unwrap();
        let turn_count: i64 = conn
            .query_row(
                "SELECT turn_count FROM projection_watermark
                 WHERE projector='conversation' AND run_id=?1",
                params![run],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(turn_count, 1, "a re-projected turn must not double-count");
    }

    /// TASK-005: a crashed/partial typed turn (no TurnCompleted) is never
    /// materialized as a complete message.
    #[test]
    fn partial_turn_is_not_projected() {
        let ((conv, run), _dir) = setup();
        let events = typed_turn_events(&run, "t1");
        let mut partial = events;
        partial.retain(|e| !matches!(e.payload, RunEventKind::TurnCompleted { .. }));
        let projected = project_run_from_events(&conv, &run, &partial)
            .unwrap()
            .projected;
        assert_eq!(projected, 0);
        assert!(conversation_store::load_agent_messages(&conv)
            .unwrap()
            .is_empty());
    }

    /// TASK-005: a corrupt MessageCompleted payload is quarantined explicitly,
    /// never silently skipped, and the turn is not projected.
    #[test]
    fn corrupt_event_is_quarantined_not_skipped() {
        let ((conv, run), _dir) = setup();
        let mut events = typed_turn_events(&run, "t1");
        if let RunEventV2 {
            payload: RunEventKind::MessageCompleted { content, .. },
            ..
        } = &mut events[5]
        {
            *content = Some(
                serde_json::json!({ "message_id": "msg-t1", "role": "assistant", "content": "not-an-array" }),
            );
        }
        let report = project_run_from_events(&conv, &run, &events).unwrap();
        assert_eq!(report.projected, 0, "corrupt turn must not project");
        assert_eq!(report.quarantined, 1, "corrupt turn must be quarantined");
        let store = conversation_store::store().unwrap();
        let conn = store.conn().unwrap();
        let quarantined: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM projection_quarantine WHERE run_id=?1 AND reason='corrupt_message_completed'",
                params![run],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            quarantined, 1,
            "corrupt event must be explicitly quarantined"
        );
    }

    /// TASK-005: projecting a turn whose stored content disagrees with the
    /// events quarantines the conflict instead of silently overwriting.
    #[test]
    fn content_conflict_is_quarantined() {
        let ((conv, run), _dir) = setup();
        let events = typed_turn_events(&run, "t1");
        project_run_from_events(&conv, &run, &events).unwrap();
        // Tamper with the stored assistant block, then re-project the same
        // events: the projector must detect the disagreement.
        let store = conversation_store::store().unwrap();
        let conn = store.conn().unwrap();
        conn.execute(
            "UPDATE message_block SET block_json = '{\"type\":\"text\",\"text\":\"tampered\"}'
             WHERE message_id = 'msg-t1' AND block_type='text'",
            [],
        )
        .unwrap();
        drop(conn);
        let result = project_run_from_events(&conv, &run, &events);
        assert!(
            result.is_ok(),
            "conflict isolates the turn, it does not fail the run"
        );
        let store = conversation_store::store().unwrap();
        let conn = store.conn().unwrap();
        let quarantined: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM projection_quarantine
                 WHERE run_id=?1 AND reason='content_conflict'",
                params![run],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            quarantined, 1,
            "content conflict must be quarantined explicitly"
        );
        // The stored content is untouched — no silent overwrite.
        let stored: String = conn
            .query_row(
                "SELECT block_json FROM message_block
                 WHERE message_id = 'msg-t1' AND block_type='text'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(stored.contains("tampered"));
    }

    /// MIG-004: projecting a run without an FK row (the retired legacy/symbolic
    /// path that used to route to the compat reader) now fails closed with an
    /// explicit error instead of materializing delta-only turns.
    #[test]
    fn missing_run_is_rejected_not_routed_to_compat() {
        let ((conv, _), _dir) = setup();
        let run = "run-1".to_string();
        // Pre-typed delta-only events (no typed turn, no run row) are what a
        // legacy/symbolic run carried.
        let events = vec![event(
            &run,
            1,
            RunEventKind::TextDelta {
                text: "hello".into(),
            },
        )];
        let error = project_run_from_events(&conv, &run, &events)
            .expect_err("a run without an FK row must be rejected, not routed to compat");
        assert!(
            error.message.contains("requires an existing run row"),
            "error must name the retired run-row requirement: {error:?}"
        );
        assert!(
            !error.retryable,
            "a missing run row is a data anomaly, not a retryable failure"
        );
        let messages = conversation_store::load_agent_messages(&conv).unwrap();
        assert!(
            messages.is_empty(),
            "rejected projection must not materialize messages"
        );
    }

    /// TASK-005 (B03): startup recovery backfills committed turns whose events
    /// are in the log but whose projection never ran (crash before projection),
    /// and is idempotent.
    #[test]
    fn recover_projections_backfills_unprojected_committed_turns() {
        let ((conv, run), _dir) = setup();
        let events = typed_turn_events(&run, "t1");
        // Simulate a crash before projection: events durable in the log, no
        // watermark, no projected messages.
        {
            let store = conversation_store::store().unwrap();
            let conn = store.conn().unwrap();
            for event in &events {
                conn.execute(
                    "INSERT INTO run_event (run_id, sequence, event_type, payload, timestamp, event_id)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        run,
                        event.effective_run_sequence() as i64,
                        event.payload.type_name(),
                        serde_json::to_string(event).unwrap(),
                        event.timestamp.to_rfc3339(),
                        event.event_id,
                    ],
                )
                .unwrap();
            }
        }
        assert!(
            conversation_store::load_agent_messages(&conv)
                .unwrap()
                .is_empty(),
            "nothing projected before recovery"
        );
        let recovered = recover_projections().unwrap();
        assert_eq!(
            recovered.total_projected, 1,
            "the committed turn is backfilled at startup"
        );
        let messages = conversation_store::load_agent_messages(&conv).unwrap();
        assert_eq!(messages.len(), 2, "assistant + tool result after recovery");
        // Re-running recovery is a no-op.
        assert_eq!(
            recover_projections().unwrap().total_projected,
            0,
            "recovery is idempotent"
        );
    }

    // ---- T03: fail loud on DB/FK/commit errors, quarantine only corruption --

    /// A plain DB error (constraint/fk failure injected via a trigger) must
    /// fail the run and NOT be treated as a quarantine; the watermark stays
    /// untouched and the next recovery succeeds after the fault clears.
    #[test]
    fn recovery_fails_on_db_failure_and_retries_after_fault_clears() {
        let ((conv, run), _dir) = setup();
        let events = typed_turn_events(&run, "t1");
        // Seed the events into the log so recovery sees a target run.
        {
            let store = conversation_store::store().unwrap();
            let conn = store.conn().unwrap();
            for event in &events {
                conn.execute(
                    "INSERT INTO run_event (run_id, sequence, event_type, payload, timestamp, event_id)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        run,
                        event.effective_run_sequence() as i64,
                        event.payload.type_name(),
                        serde_json::to_string(event).unwrap(),
                        event.timestamp.to_rfc3339(),
                        event.event_id,
                    ],
                )
                .unwrap();
            }
        }
        // Inject a mid-transaction FK-style failure: every message_block insert
        // aborts, which rolls the whole per-turn transaction back.
        {
            let store = conversation_store::store().unwrap();
            let conn = store.conn().unwrap();
            conn.execute_batch(
                "CREATE TRIGGER proj_test_fk_fail BEFORE INSERT ON message_block
                 BEGIN
                     SELECT RAISE(ABORT, 'FOREIGN KEY constraint failed');
                 END;",
            )
            .unwrap();
        }
        let failure = recover_projections().unwrap_err();
        assert!(
            failure.contains("projection recovery failed"),
            "recovery must fail loudly on a DB error, got: {failure}"
        );
        assert!(
            failure.contains("FOREIGN KEY constraint failed") || failure.contains("constraint"),
            "the injected FK failure must be surfaced, got: {failure}"
        );
        // Atomic rollback: nothing may become provider history.
        assert!(
            conversation_store::load_agent_messages(&conv)
                .unwrap()
                .is_empty(),
            "a failed per-turn transaction must leave no partial rows"
        );
        // The retry watermark was NOT advanced.
        let store = conversation_store::store().unwrap();
        let conn = store.conn().unwrap();
        let watermark: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM projection_watermark WHERE projector='conversation' AND run_id=?1",
                params![run],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(watermark, 0, "retry watermark must be preserved");
        // Fault clears -> recovery succeeds and materializes the full turn.
        conn.execute_batch("DROP TRIGGER proj_test_fk_fail;")
            .unwrap();
        drop(conn);
        let report = recover_projections().unwrap();
        assert_eq!(
            report.total_projected, 1,
            "the next recovery continues after the fault clears"
        );
        assert_eq!(
            conversation_store::load_agent_messages(&conv)
                .unwrap()
                .len(),
            2,
            "assistant + tool result after the successful retry"
        );
    }

    /// A busy database aborts recovery with a retryable failure; after the
    /// lock is released the same recovery succeeds.
    #[test]
    fn recovery_fails_on_busy_db_and_retries_after_lock_release() {
        let _guard = crate::storage::DataStore::env_test_lock();
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("projector-busy.db");
        // Short busy timeout so the held write lock surfaces SQLITE_BUSY
        // immediately instead of parking the test for 30s.
        std::env::set_var("NATIVES_TEST_BUSY_TIMEOUT_MS", "0");
        std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db);
        std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
        crate::storage::set_test_db_override(Some(db.clone()), Some(dir.path().join("artifacts")));
        let _store = crate::storage::DataStore::new(&db, &dir.path().join("artifacts")).unwrap();
        let conv = format!("proj-busy-conv-{}", uuid::Uuid::new_v4());
        let run = format!("proj-busy-run-{}", uuid::Uuid::new_v4());
        {
            let store = conversation_store::store().unwrap();
            let conn = store.conn().unwrap();
            conn.execute(
                "INSERT OR IGNORE INTO conversation (id, mode, title, provider_id, model_id)
                 VALUES (?1, 'chat', 'Busy Test', 'prov-1', 'model-1')",
                params![conv],
            )
            .unwrap();
            conn.execute(
                "INSERT OR IGNORE INTO run (id, conversation_id, status, provider_id, model_id)
                 VALUES (?1, ?2, 'completed', 'prov-1', 'model-1')",
                params![run, conv],
            )
            .unwrap();
            for event in typed_turn_events(&run, "t1") {
                conn.execute(
                    "INSERT INTO run_event (run_id, sequence, event_type, payload, timestamp, event_id)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        run,
                        event.effective_run_sequence() as i64,
                        event.payload.type_name(),
                        serde_json::to_string(&event).unwrap(),
                        event.timestamp.to_rfc3339(),
                        event.event_id,
                    ],
                )
                .unwrap();
            }
        }
        // Hold the SQLite write lock from a second connection. The short busy
        // timeout (still set) makes the projector's next write surface
        // SQLITE_BUSY immediately instead of parking for the default 30s.
        let lock_conn = rusqlite::Connection::open(&db).unwrap();
        lock_conn.execute_batch("BEGIN IMMEDIATE;").unwrap();
        let failure = recover_projections().unwrap_err();
        assert!(
            failure.contains("projection recovery failed") || failure.contains("busy"),
            "recovery must fail on a busy database, got: {failure}"
        );
        assert!(
            conversation_store::load_agent_messages(&conv)
                .unwrap()
                .is_empty(),
            "busy failure must not leave partial rows"
        );
        // Release the lock; the retry watermark was preserved, so the same
        // recovery re-attempts and succeeds.
        drop(lock_conn);
        let report = recover_projections().unwrap();
        assert_eq!(
            report.total_projected, 1,
            "retry after lock release succeeds"
        );
        // Remove the test-only knob so it never leaks into a later test.
        std::env::remove_var("NATIVES_TEST_BUSY_TIMEOUT_MS");
    }

    /// The classifier must map busy/locked/disk-full to retryable and
    /// constraint (FK) to fatal — the load-bearing distinction for the retry
    /// watermark.
    #[test]
    fn db_error_classifier_maps_busy_full_to_retryable_and_fk_to_fatal() {
        let busy = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ffi::ErrorCode::DatabaseBusy,
                extended_code: 5,
            },
            None,
        );
        let locked = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ffi::ErrorCode::DatabaseLocked,
                extended_code: 6,
            },
            None,
        );
        let full = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ffi::ErrorCode::DiskFull,
                extended_code: 13,
            },
            None,
        );
        let fk = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ffi::ErrorCode::ConstraintViolation,
                extended_code: 787, // SQLITE_CONSTRAINT_FOREIGNKEY
            },
            None,
        );
        for transient in [&busy, &locked, &full] {
            assert!(
                classify_db_error(transient),
                "{transient:?} must be retryable"
            );
        }
        assert!(
            !classify_db_error(&fk),
            "an FK violation is permanent, never retryable"
        );
    }

    /// Kill/restart atomicity (T03): when the per-turn transaction fails
    /// mid-write, no partial rows become provider history, and a later restart
    /// re-projects cleanly.
    #[test]
    fn failed_turn_never_leaves_partial_provider_history() {
        fn seed(run: &str, events: &[RunEventV2]) {
            let store = conversation_store::store().unwrap();
            let conn = store.conn().unwrap();
            for event in events {
                conn.execute(
                    "INSERT INTO run_event (run_id, sequence, event_type, payload, timestamp, event_id)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        run,
                        event.effective_run_sequence() as i64,
                        event.payload.type_name(),
                        serde_json::to_string(event).unwrap(),
                        event.timestamp.to_rfc3339(),
                        event.event_id,
                    ],
                )
                .unwrap();
            }
        }
        let ((conv, run), _dir) = setup();
        let events_t1 = typed_turn_events(&run, "t1");
        // Second turn carries the next sequence range so the recovery query
        // (event_sequence > watermark) still sees it as unprojected.
        let events_t2: Vec<RunEventV2> = typed_turn_events(&run, "t2")
            .into_iter()
            .enumerate()
            .map(|(i, mut e)| {
                e.sequence += 7;
                e.run_sequence += 7;
                e.global_sequence += 7;
                e.event_id = format!("evt-{run}-{}", 8 + i as u64);
                e
            })
            .collect();
        // First projection succeeds fully and is durable in the log.
        seed(&run, &events_t1);
        assert_eq!(
            project_run_from_events(&conv, &run, &events_t1)
                .unwrap()
                .projected,
            1
        );
        let baseline = conversation_store::load_agent_messages(&conv).unwrap();
        // Add the second turn, then inject a fault at its message insert.
        seed(&run, &events_t2);
        {
            let store = conversation_store::store().unwrap();
            let conn = store.conn().unwrap();
            conn.execute_batch(
                "CREATE TRIGGER proj_test_fk_fail2 BEFORE INSERT ON message_block
                 BEGIN
                     SELECT RAISE(ABORT, 'FOREIGN KEY constraint failed');
                 END;",
            )
            .unwrap();
        }
        let mut events2 = events_t1.clone();
        events2.extend(events_t2);
        let failure = project_run_from_events(&conv, &run, &events2).unwrap_err();
        assert!(
            !failure.retryable || failure.message.contains("constraint"),
            "an FK-style abort is a fatal failure, got: {failure}"
        );
        let after_failure = conversation_store::load_agent_messages(&conv).unwrap();
        assert_eq!(
            baseline, after_failure,
            "the failed second turn must not alter the transcript"
        );
        // Restart (fault cleared): recovery completes the second turn.
        {
            let store = conversation_store::store().unwrap();
            let conn = store.conn().unwrap();
            conn.execute_batch("DROP TRIGGER proj_test_fk_fail2;")
                .unwrap();
        }
        assert_eq!(
            recover_projections().unwrap().total_projected,
            1,
            "restart re-projects the failed turn"
        );
        assert_eq!(
            conversation_store::load_agent_messages(&conv)
                .unwrap()
                .len(),
            baseline.len() + 2,
            "second turn materialized after restart"
        );
    }
}

/// A8 — watermark-driven incremental projection tests (standalone module so it
/// does not depend on the private `mod tests` helpers).
#[cfg(test)]
mod incremental_projection_tests {
    use super::*;
    use crate::conversation_store;

    fn setup() -> ((String, String), tempfile::TempDir) {
        let _guard = crate::storage::DataStore::env_test_lock();
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("proj-incremental.db");
        std::env::set_var("NATIVES_ASSISTANT_DB_PATH", &db);
        std::env::set_var("NATIVES_RUNTIME_DIR", dir.path());
        crate::storage::set_test_db_override(Some(db.clone()), Some(dir.path().join("artifacts")));
        let _store = crate::storage::DataStore::new(&db, &dir.path().join("artifacts")).unwrap();
        let conv = format!("proj-incr-conv-{}", uuid::Uuid::new_v4());
        let run = format!("proj-incr-run-{}", uuid::Uuid::new_v4());
        {
            let store = conversation_store::store().unwrap();
            let conn = store.conn().unwrap();
            conn.execute(
                "INSERT OR IGNORE INTO conversation (id, mode, title, provider_id, model_id)
                 VALUES (?1, 'chat', 'Projector Incremental', 'prov-1', 'model-1')",
                params![conv],
            )
            .unwrap();
            conn.execute(
                "INSERT OR IGNORE INTO run (id, conversation_id, status, provider_id, model_id)
                 VALUES (?1, ?2, 'completed', 'prov-1', 'model-1')",
                params![run, conv],
            )
            .unwrap();
        }
        ((conv, run), dir)
    }

    fn event(run_id: &str, sequence: u64, payload: RunEventKind) -> RunEventV2 {
        RunEventV2 {
            event_id: format!("evt-{run_id}-{sequence}"),
            global_sequence: 0,
            run_sequence: sequence,
            run_id: run_id.into(),
            sequence,
            timestamp: chrono::Utc::now(),
            payload,
        }
    }

    fn typed_turn_events(run_id: &str, turn: &str, base: u64) -> Vec<RunEventV2> {
        let mut events = Vec::new();
        let mut push = |kind: RunEventKind, offset: u64| {
            let seq = base + offset;
            events.push(event(run_id, seq, kind));
        };
        push(
            RunEventKind::TurnStarted {
                turn_id: turn.into(),
            },
            0,
        );
        push(
            RunEventKind::MessageStarted {
                turn_id: turn.into(),
                message_id: format!("msg-{turn}"),
                role: "assistant".into(),
            },
            1,
        );
        push(
            RunEventKind::TextDelta {
                text: "hello".into(),
            },
            2,
        );
        push(
            RunEventKind::ToolCallRequested {
                id: format!("call-{turn}-1"),
                name: "read_file".into(),
                input: serde_json::json!({"path": "/tmp/a.txt"}),
            },
            3,
        );
        push(
            RunEventKind::ToolCallCompleted {
                id: format!("call-{turn}-1"),
                name: "read_file".into(),
                output: serde_json::json!({"content": "file body"}),
                is_error: false,
                duration_ms: 2,
                result_message_id: Some(format!("rm-{turn}-1")),
            },
            4,
        );
        push(
            RunEventKind::MessageCompleted {
                turn_id: turn.into(),
                message_id: format!("msg-{turn}"),
                role: "assistant".into(),
                content: Some(serde_json::json!({
                    "message_id": format!("msg-{turn}"),
                    "role": "assistant",
                    "content": serde_json::to_value(vec![
                        agent_core::ContentBlock::Text { text: "hello".into() },
                    ]).unwrap(),
                })),
            },
            5,
        );
        push(
            RunEventKind::TurnCompleted {
                turn_id: turn.into(),
                stop_reason: "stop".into(),
                input_tokens: 10,
                output_tokens: 5,
            },
            6,
        );
        events
    }

    fn persist_events(run_id: &str, events: &[RunEventV2]) {
        let store = conversation_store::store().unwrap();
        let conn = store.conn().unwrap();
        for event in events {
            conn.execute(
                "INSERT INTO run_event (run_id, sequence, event_type, payload, timestamp, event_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    run_id.to_string(),
                    event.effective_run_sequence() as i64,
                    event.payload.type_name().to_string(),
                    serde_json::to_string(event).unwrap(),
                    event.timestamp.to_rfc3339(),
                    event.event_id.clone(),
                ],
            )
            .unwrap();
        }
    }

    /// A8: incremental projection replays only events AFTER the watermark and
    /// skips the already-projected prefix; second pass is a no-op.
    #[test]
    fn projector_incremental_uses_watermark_prefix() {
        let ((conv, run), _dir) = setup();
        let t1 = typed_turn_events(&run, "t1", 0);
        let t2 = typed_turn_events(&run, "t2", 7);
        persist_events(&run, &t1);
        persist_events(&run, &t2);

        // Project turn 1 only (watermark now at sequence 7).
        let first = project_run_from_events(&conv, &run, &t1).unwrap();
        assert_eq!(first.projected, 1);

        // Incremental pass picks up ONLY turn 2.
        let incremental = project_run_incremental(&conv, &run).unwrap();
        assert_eq!(incremental.projected, 1, "only the new turn projects");
        assert_eq!(incremental.already_projected, 0);
        let messages = conversation_store::load_agent_messages(&conv).unwrap();
        assert_eq!(messages.len(), 4, "both turns' assistant + tool results");
        // MessageCompleted.content remains the committed-content authority.
        let assistant_texts: Vec<String> = messages
            .iter()
            .filter_map(|m| match m {
                agent_core::AgentMessage::Assistant(a) => Some(
                    a.content
                        .iter()
                        .filter_map(|b| match b {
                            ContentBlock::Text { text } => Some(text.clone()),
                            _ => None,
                        })
                        .collect::<String>(),
                ),
                _ => None,
            })
            .collect();
        assert_eq!(
            assistant_texts,
            vec!["hello".to_string(), "hello".to_string()]
        );
        // Second incremental pass is a no-op.
        let again = project_run_incremental(&conv, &run).unwrap();
        assert_eq!(again.projected, 0);
        assert_eq!(
            conversation_store::load_agent_messages(&conv)
                .unwrap()
                .len(),
            4
        );
    }

    /// §5 exact-name regression: projector uses watermark prefix (A8).
    #[test]
    fn projector_uses_watermark_prefix() {
        projector_incremental_uses_watermark_prefix();
    }
}
