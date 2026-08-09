//! Status transitions and terminal arbitration (RunLifecycleAuthority impl).

use super::manager::RunManager;
use agent_core::CommitError;
use agent_core::EngineOutcome;
use agent_core::RunLifecycleAuthority;
use agent_core::TransitionMetadata;
use assistant_protocol::v2::{RunEventKind, RunEventV2, RunStatusV2, RunV2};

impl RunManager {
    /// Idempotent fail-close: mark run failed, persist, and emit a terminal `failed` event.
    /// No-op when the run is already terminal (completed/failed/cancelled/interrupted).
    pub fn fail_run_if_active(&self, run_id: &str, error: impl Into<String>, code: &str) {
        let error = assistant_protocol::v2::redact_secrets(&error.into());
        let code = if code.trim().is_empty() {
            "START_FAILED"
        } else {
            code
        };
        let meta = TransitionMetadata::empty()
            .with_error_code(code)
            .with_reason(error)
            .with_lifecycle_hint("failed");
        let _ = self.commit_status(run_id, RunStatusV2::Failed, meta);
    }
    /// Map a status target to a lifecycle event payload.
    fn lifecycle_event_for(target: RunStatusV2, metadata: &TransitionMetadata) -> RunEventKind {
        let reason = metadata
            .reason
            .clone()
            .filter(|s| !s.is_empty())
            .or_else(|| metadata.lifecycle_hint.clone())
            .unwrap_or_default();
        match target {
            RunStatusV2::Queued => RunEventKind::Queued,
            RunStatusV2::Preparing => RunEventKind::Preparing,
            RunStatusV2::Running => RunEventKind::Started,
            RunStatusV2::WaitingPermission => RunEventKind::Progress {
                message: if reason.is_empty() {
                    "waiting_permission".into()
                } else {
                    reason
                },
                percentage: None,
            },
            RunStatusV2::WaitingSubagent => RunEventKind::Progress {
                message: if reason.is_empty() {
                    "waiting_subagent".into()
                } else {
                    reason
                },
                percentage: None,
            },
            RunStatusV2::Cancelling => RunEventKind::Progress {
                message: if reason.is_empty() {
                    "cancelling".into()
                } else {
                    reason
                },
                percentage: None,
            },
            RunStatusV2::Completed => RunEventKind::Completed {
                reason: if reason.is_empty() {
                    "stop".into()
                } else {
                    reason
                },
            },
            RunStatusV2::Failed => RunEventKind::Failed {
                error: if reason.is_empty() {
                    "failed".into()
                } else {
                    reason
                },
                code: metadata
                    .error_code
                    .clone()
                    .unwrap_or_else(|| "failed".into()),
            },
            RunStatusV2::Cancelled => RunEventKind::Cancelled {
                reason: if reason.is_empty() {
                    "cancelled".into()
                } else {
                    reason
                },
            },
            RunStatusV2::Interrupted => RunEventKind::Interrupted {
                reason: if reason.is_empty() {
                    "interrupted".into()
                } else {
                    reason
                },
            },
            RunStatusV2::Created => RunEventKind::Queued,
        }
    }
    fn apply_transition_metadata(
        run: &mut RunV2,
        target: RunStatusV2,
        metadata: &TransitionMetadata,
    ) {
        if let Some(code) = &metadata.error_code {
            run.error_code = Some(code.clone());
        }
        if let Some(steps) = metadata.step_count {
            run.step_count = steps;
        }
        if matches!(target, RunStatusV2::Preparing | RunStatusV2::Running)
            && run.started_at.is_none()
        {
            run.started_at = Some(chrono::Utc::now());
        }
        if target.is_terminal() {
            run.finished_at = Some(chrono::Utc::now());
        }
    }
    /// Persist-first CAS commit of run status + lifecycle event, then memory/broadcast.
    pub fn commit_transition(
        &self,
        run_id: &str,
        expected_revision: u64,
        target: RunStatusV2,
        metadata: TransitionMetadata,
    ) -> Result<agent_core::CommittedTransition, CommitError> {
        <Self as RunLifecycleAuthority>::commit_transition(
            self,
            run_id,
            expected_revision,
            target,
            metadata,
        )
    }
    /// Commit using the current in-memory revision. Terminal races are idempotent.
    pub fn commit_status(
        &self,
        run_id: &str,
        target: RunStatusV2,
        metadata: TransitionMetadata,
    ) -> Result<RunV2, String> {
        let revision = {
            let runs = self.runs.lock().map_err(|e| e.to_string())?;
            let run = runs
                .get(run_id)
                .ok_or_else(|| format!("run not found: {run_id}"))?;
            if run.status == target || run.status.is_terminal() {
                return Ok(run.clone());
            }
            run.revision
        };
        match self.commit_transition(run_id, revision, target, metadata) {
            Ok(_) => {
                // STREAM-CONTRACT-V2 Terminal: clear the run's live ring/bus
                // state once a durable terminal status is committed. This is
                // idempotent — remove_run on a missing run is a no-op.
                if target.is_terminal() {
                    self.runtime.live.remove_run(run_id);
                }
                self.get_run(run_id)
                    .ok_or_else(|| "run disappeared after commit".into())
            }
            Err(CommitError::AlreadyTerminal { .. }) => {
                if target.is_terminal() {
                    self.runtime.live.remove_run(run_id);
                }
                self.get_run(run_id).ok_or_else(|| "run not found".into())
            }
            Err(CommitError::CasConflict { status, .. }) if status.is_terminal() => {
                self.runtime.live.remove_run(run_id);
                self.get_run(run_id).ok_or_else(|| "run not found".into())
            }
            Err(e) => Err(e.to_string()),
        }
    }
    /// Commit terminal status from an EngineOutcome (idempotent).
    pub fn commit_outcome(&self, run_id: &str, outcome: &EngineOutcome) -> Result<RunV2, String> {
        let status = self
            .get_run(run_id)
            .map(|r| r.status)
            .unwrap_or(RunStatusV2::Running);
        if status.is_terminal() {
            return self.get_run(run_id).ok_or_else(|| "run not found".into());
        }
        let (mut target, mut meta) = agent_core::outcome_commit_parts(outcome);
        // Pre-task-03: cooperative cancel mid-run -> Interrupted; cancel() path uses Cancelled.
        if matches!(outcome, EngineOutcome::Cancelled) {
            if status == RunStatusV2::Cancelling {
                target = RunStatusV2::Cancelled;
                meta = TransitionMetadata::empty()
                    .with_reason("cancelled")
                    .with_lifecycle_hint("cancelled");
            } else {
                target = RunStatusV2::Interrupted;
                meta = TransitionMetadata::empty()
                    .with_reason("cancelled")
                    .with_lifecycle_hint("interrupted");
            }
        }
        // Bridge non-adjacent active states so engines that skip explicit Running
        // commits still land on a legal edge (Preparing/Queued → Running → terminal).
        self.ensure_running_before_terminal(run_id, target)?;
        self.commit_status(run_id, target, meta)
    }
    /// If the run is still Queued/Preparing and the target requires Running as
    /// predecessor, commit Running first.
    fn ensure_running_before_terminal(
        &self,
        run_id: &str,
        target: RunStatusV2,
    ) -> Result<(), String> {
        let status = self
            .get_run(run_id)
            .map(|r| r.status)
            .unwrap_or(RunStatusV2::Running);
        if status.is_terminal() || status == RunStatusV2::Running {
            return Ok(());
        }
        // Targets that are legal from Running (and not from Preparing).
        let needs_running = matches!(
            target,
            RunStatusV2::Completed
                | RunStatusV2::Failed
                | RunStatusV2::Interrupted
                | RunStatusV2::WaitingPermission
                | RunStatusV2::WaitingSubagent
                | RunStatusV2::Cancelling
        );
        if !needs_running {
            return Ok(());
        }
        if status == RunStatusV2::Queued {
            self.commit_status(
                run_id,
                RunStatusV2::Preparing,
                TransitionMetadata::empty().with_lifecycle_hint("preparing"),
            )?;
        }
        let status = self
            .get_run(run_id)
            .map(|r| r.status)
            .unwrap_or(RunStatusV2::Preparing);
        if status == RunStatusV2::Preparing {
            self.commit_status(
                run_id,
                RunStatusV2::Running,
                TransitionMetadata::empty().with_lifecycle_hint("started"),
            )?;
        }
        Ok(())
    }
}
impl RunLifecycleAuthority for RunManager {
    fn commit_transition(
        &self,
        run_id: &str,
        expected_revision: u64,
        target: RunStatusV2,
        metadata: TransitionMetadata,
    ) -> Result<agent_core::CommittedTransition, CommitError> {
        // 1. Read current from memory (authority cache); fall back to constructing error.
        let (from, current_revision) = {
            let runs = self
                .runs
                .lock()
                .map_err(|e| CommitError::Other(e.to_string()))?;
            let run = runs.get(run_id).ok_or_else(|| CommitError::NotFound {
                run_id: run_id.to_string(),
            })?;
            (run.status, run.revision)
        };

        // Terminal idempotency: late outcomes against terminal do not insert events.
        if from.is_terminal() {
            return Err(CommitError::AlreadyTerminal {
                run_id: run_id.to_string(),
                status: from,
                revision: current_revision,
            });
        }

        if current_revision != expected_revision {
            return Err(CommitError::CasConflict {
                run_id: run_id.to_string(),
                expected: expected_revision,
                current: current_revision,
                status: from,
            });
        }

        // 2. Validate edge via agent-core sole authority.
        agent_core::transition(from, target).map_err(CommitError::from)?;

        let new_revision = expected_revision.saturating_add(1);
        let lifecycle = Self::lifecycle_event_for(target, &metadata);

        // 3. Durable transaction when store present; else memory CAS critical section.
        let committed_event = if let Some(store) = &self.data_store {
            let conn = store.conn().map_err(CommitError::Storage)?;
            let tx = conn
                .unchecked_transaction()
                .map_err(|e| CommitError::Storage(e.to_string()))?;

            // CAS update run row
            let now = chrono::Utc::now().to_rfc3339();
            let finished_at = if target.is_terminal() {
                Some(now.clone())
            } else {
                None
            };
            let started_at = if matches!(target, RunStatusV2::Preparing | RunStatusV2::Running) {
                Some(now.clone())
            } else {
                None
            };
            let changed = tx
                .execute(
                    "UPDATE run SET
                        status = ?1,
                        revision = ?2,
                        error_code = COALESCE(?3, error_code),
                        finished_at = COALESCE(?4, finished_at),
                        started_at = COALESCE(started_at, ?5),
                        step_count = COALESCE(?6, step_count)
                     WHERE id = ?7 AND revision = ?8",
                    rusqlite::params![
                        target.as_str(),
                        new_revision as i64,
                        metadata.error_code,
                        finished_at,
                        started_at,
                        metadata.step_count.map(|s| s as i64),
                        run_id,
                        expected_revision as i64,
                    ],
                )
                .map_err(|e| CommitError::Storage(e.to_string()))?;
            if changed == 0 {
                // Re-read status for accurate error
                let (status, rev): (String, i64) = tx
                    .query_row(
                        "SELECT status, COALESCE(revision, 0) FROM run WHERE id = ?1",
                        rusqlite::params![run_id],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .map_err(|e| CommitError::Storage(e.to_string()))?;
                let status = run_status_from_db(&status);
                if status.is_terminal() {
                    return Err(CommitError::AlreadyTerminal {
                        run_id: run_id.to_string(),
                        status,
                        revision: rev as u64,
                    });
                }
                return Err(CommitError::CasConflict {
                    run_id: run_id.to_string(),
                    expected: expected_revision,
                    current: rev as u64,
                    status,
                });
            }

            // Allocate next run_sequence and insert lifecycle event in same tx.
            let next_seq: i64 = tx
                .query_row(
                    "SELECT COALESCE(MAX(sequence), 0) + 1 FROM run_event WHERE run_id = ?1",
                    rusqlite::params![run_id],
                    |row| row.get(0),
                )
                .map_err(|e| CommitError::Storage(e.to_string()))?;
            let mut event = RunEventV2::new(run_id, next_seq as u64, lifecycle.clone());
            let payload =
                serde_json::to_string(&event).map_err(|e| CommitError::Storage(e.to_string()))?;
            tx.execute(
                "INSERT INTO run_event (run_id, sequence, event_type, payload, timestamp, event_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    run_id,
                    next_seq,
                    event.payload.type_name(),
                    payload,
                    event.timestamp.to_rfc3339(),
                    event.event_id,
                ],
            )
            .map_err(|e| CommitError::Storage(e.to_string()))?;
            let global = tx.last_insert_rowid() as u64;
            event.global_sequence = global;
            tx.commit()
                .map_err(|e| CommitError::Storage(e.to_string()))?;
            event
        } else {
            // Memory-only: CAS inside the runs mutex (same critical section as status write).
            RunEventV2::new(
                run_id,
                self.runtime.events.last_sequence(run_id).saturating_add(1),
                lifecycle,
            )
        };

        // 4. Update memory cache only after durable success (or memory CAS).
        {
            let mut runs = self
                .runs
                .lock()
                .map_err(|e| CommitError::Other(e.to_string()))?;
            let run = runs.get_mut(run_id).ok_or_else(|| CommitError::NotFound {
                run_id: run_id.to_string(),
            })?;
            // Re-check CAS in memory for the memory-only path.
            if run.revision != expected_revision {
                if run.status.is_terminal() {
                    return Err(CommitError::AlreadyTerminal {
                        run_id: run_id.to_string(),
                        status: run.status,
                        revision: run.revision,
                    });
                }
                return Err(CommitError::CasConflict {
                    run_id: run_id.to_string(),
                    expected: expected_revision,
                    current: run.revision,
                    status: run.status,
                });
            }
            run.status = target;
            run.revision = new_revision;
            Self::apply_transition_metadata(run, target, &metadata);
        }

        // 5. Broadcast committed lifecycle event (no re-persist).
        self.runtime.events.inject_committed(committed_event);
        let _ = self.persist_runs_snapshot();

        Ok(agent_core::CommittedTransition {
            run_id: run_id.to_string(),
            from,
            to: target,
            revision: new_revision,
            idempotent: false,
        })
    }
}

pub(crate) fn run_status_from_db(status: &str) -> RunStatusV2 {
    match status {
        "created" => RunStatusV2::Created,
        "queued" => RunStatusV2::Queued,
        "preparing" => RunStatusV2::Preparing,
        "running" => RunStatusV2::Running,
        "waiting_permission" => RunStatusV2::WaitingPermission,
        "waiting_subagent" => RunStatusV2::WaitingSubagent,
        "cancelling" => RunStatusV2::Cancelling,
        "completed" => RunStatusV2::Completed,
        "failed" => RunStatusV2::Failed,
        "cancelled" => RunStatusV2::Cancelled,
        "interrupted" => RunStatusV2::Interrupted,
        _ => RunStatusV2::Interrupted,
    }
}
pub(crate) fn parse_db_time(value: Option<String>) -> Option<chrono::DateTime<chrono::Utc>> {
    value
        .and_then(|raw| chrono::DateTime::parse_from_rfc3339(&raw).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc))
}
