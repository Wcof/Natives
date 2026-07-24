//! RunStatusV2 transition authority — single source of truth for lifecycle rules.
//!
//! Protocol crates only carry wire enums/helpers. All production transition
//! validation must go through [`transition`] / [`legal_next_states`] here.
//! State commits in the daemon must go through [`RunLifecycleAuthority::commit_transition`].

use assistant_protocol::error::error_codes;
use assistant_protocol::error::DaemonError;
use assistant_protocol::error::ErrorCategory;
use assistant_protocol::v2::RunStatusV2;
use serde::{Deserialize, Serialize};

/// Error returned when an illegal state transition is attempted.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum TransitionError {
    #[error("Illegal transition from {from:?} to {to:?}")]
    IllegalTransition {
        from: RunStatusV2,
        to: RunStatusV2,
    },

    #[error("Transition from terminal state {from:?} is not allowed")]
    TerminalState { from: RunStatusV2 },

    #[error("{0}")]
    Other(String),
}

impl From<TransitionError> for DaemonError {
    fn from(err: TransitionError) -> Self {
        match err {
            TransitionError::IllegalTransition { from, to } => DaemonError::new(
                error_codes::RUN_INVALID_STATE,
                ErrorCategory::Validation,
                false,
                format!("Illegal transition from {:?} to {:?}", from, to),
            )
            .with_user_key("error.run_invalid_state"),
            TransitionError::TerminalState { from } => DaemonError::new(
                error_codes::RUN_INVALID_STATE,
                ErrorCategory::Validation,
                false,
                format!("Cannot transition from terminal state {:?}", from),
            )
            .with_user_key("error.run_invalid_state"),
            TransitionError::Other(msg) => {
                DaemonError::new(error_codes::INTERNAL_ERROR, ErrorCategory::Internal, false, msg)
            }
        }
    }
}

/// Sole authority for RunStatusV2 transition legality.
///
/// This is the only place where the production state graph is defined.
/// Protocol `RunStatusV2` keeps enum + wire helpers only.
pub fn transition(current: RunStatusV2, target: RunStatusV2) -> Result<(), TransitionError> {
    if current == target {
        // No-op self transitions are not state commits; callers should not use
        // transition() for identity checks.
        return Err(TransitionError::IllegalTransition {
            from: current,
            to: target,
        });
    }
    if current.is_terminal() {
        return Err(TransitionError::TerminalState { from: current });
    }
    if can_transition(current, target) {
        Ok(())
    } else {
        Err(TransitionError::IllegalTransition {
            from: current,
            to: target,
        })
    }
}

/// Exhaustive legal edges for [`RunStatusV2`].
///
/// Graph (matches protocol docs + remediation plan):
/// ```text
/// created → queued → preparing → running
///   → waiting_permission | waiting_subagent
///   → cancelling → completed | failed | cancelled | interrupted
/// ```
fn can_transition(current: RunStatusV2, next: RunStatusV2) -> bool {
    use RunStatusV2::*;
    matches!(
        (current, next),
        (Created, Queued)
            | (Created, Cancelling)
            | (Created, Failed)
            | (Queued, Preparing)
            | (Queued, Cancelling)
            | (Queued, Failed)
            | (Preparing, Running)
            | (Preparing, WaitingPermission)
            | (Preparing, Cancelling)
            | (Preparing, Failed)
            | (Preparing, Interrupted)
            | (Running, WaitingPermission)
            | (Running, WaitingSubagent)
            | (Running, Cancelling)
            | (Running, Completed)
            | (Running, Failed)
            | (Running, Interrupted)
            | (WaitingPermission, Running)
            | (WaitingPermission, Cancelling)
            | (WaitingPermission, Completed)
            | (WaitingPermission, Failed)
            | (WaitingPermission, Interrupted)
            | (WaitingSubagent, Running)
            | (WaitingSubagent, Cancelling)
            | (WaitingSubagent, Completed)
            | (WaitingSubagent, Failed)
            | (WaitingSubagent, Interrupted)
            | (Cancelling, Cancelled)
            | (Cancelling, Interrupted)
            | (Cancelling, Failed)
    )
}

/// All statuses (for exhaustive matrix tests and introspection).
pub fn all_statuses() -> [RunStatusV2; 11] {
    use RunStatusV2::*;
    [
        Created,
        Queued,
        Preparing,
        Running,
        WaitingPermission,
        WaitingSubagent,
        Cancelling,
        Completed,
        Failed,
        Cancelled,
        Interrupted,
    ]
}

/// Returns all legal next states from the given current state.
pub fn legal_next_states(current: RunStatusV2) -> Vec<RunStatusV2> {
    all_statuses()
        .into_iter()
        .filter(|next| transition(current, *next).is_ok())
        .collect()
}

// ---------------------------------------------------------------------------
// EngineOutcome — engines never write Run status; they only return outcomes.
// ---------------------------------------------------------------------------

/// Non-authoritative result of an engine / adapter execution attempt.
///
/// Engines, CLI adapters, and providers must return this instead of writing
/// Run status or emitting lifecycle terminal events. Only
/// [`RunLifecycleAuthority::commit_transition`] may change Run status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EngineOutcome {
    Completed { reason: String },
    Failed {
        code: String,
        error: String,
        retryable: bool,
    },
    Cancelled,
    Interrupted { reason: String },
}

impl EngineOutcome {
    pub fn completed(reason: impl Into<String>) -> Self {
        Self::Completed {
            reason: reason.into(),
        }
    }

    pub fn failed(code: impl Into<String>, error: impl Into<String>, retryable: bool) -> Self {
        Self::Failed {
            code: code.into(),
            error: error.into(),
            retryable,
        }
    }

    pub fn interrupted(reason: impl Into<String>) -> Self {
        Self::Interrupted {
            reason: reason.into(),
        }
    }

    /// Map outcome to the target terminal status (without validating the edge).
    pub fn target_status(&self) -> RunStatusV2 {
        match self {
            Self::Completed { .. } => RunStatusV2::Completed,
            Self::Failed { .. } => RunStatusV2::Failed,
            Self::Cancelled => RunStatusV2::Cancelled,
            Self::Interrupted { .. } => RunStatusV2::Interrupted,
        }
    }

    pub fn is_terminal(&self) -> bool {
        true
    }

    pub fn error_code(&self) -> Option<&str> {
        match self {
            Self::Failed { code, .. } => Some(code.as_str()),
            Self::Interrupted { reason } if reason == "daemon_restarted" => {
                Some("daemon_restarted")
            }
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Run transition / commit interface (shared with Agent B)
// ---------------------------------------------------------------------------

/// Optional metadata attached to a committed transition.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransitionMetadata {
    pub error_code: Option<String>,
    pub reason: Option<String>,
    pub step_count: Option<u32>,
    /// Lifecycle event kind hint (e.g. "preparing", "started", "completed").
    /// Concrete event construction remains in the daemon RunJournal.
    pub lifecycle_hint: Option<String>,
    /// Extra JSON-safe bag for journal extensions (must not carry secrets).
    #[serde(default)]
    pub extra: serde_json::Value,
}

impl TransitionMetadata {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn with_error_code(mut self, code: impl Into<String>) -> Self {
        self.error_code = Some(code.into());
        self
    }

    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    pub fn with_lifecycle_hint(mut self, hint: impl Into<String>) -> Self {
        self.lifecycle_hint = Some(hint.into());
        self
    }

    /// Build metadata from an [`EngineOutcome`].
    pub fn from_outcome(outcome: &EngineOutcome) -> Self {
        match outcome {
            EngineOutcome::Completed { reason } => Self::empty()
                .with_reason(reason.clone())
                .with_lifecycle_hint("completed"),
            EngineOutcome::Failed { code, error, .. } => Self {
                error_code: Some(code.clone()),
                reason: Some(error.clone()),
                step_count: None,
                lifecycle_hint: Some("failed".into()),
                extra: serde_json::json!({ "retryable": outcome_retryable(outcome) }),
            },
            EngineOutcome::Cancelled => Self::empty()
                .with_reason("cancelled")
                .with_lifecycle_hint("cancelled"),
            EngineOutcome::Interrupted { reason } => {
                let mut m = Self::empty()
                    .with_reason(reason.clone())
                    .with_lifecycle_hint("interrupted");
                if reason == "daemon_restarted" {
                    m.error_code = Some("daemon_restarted".into());
                }
                m
            }
        }
    }
}

fn outcome_retryable(outcome: &EngineOutcome) -> bool {
    matches!(
        outcome,
        EngineOutcome::Failed {
            retryable: true,
            ..
        }
    )
}

/// Successful commit view returned by the lifecycle authority.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommittedTransition {
    pub run_id: String,
    pub from: RunStatusV2,
    pub to: RunStatusV2,
    /// Revision after the commit (previous + 1).
    pub revision: u64,
    /// True when the call was a terminal-idempotent no-op (no new event).
    pub idempotent: bool,
}

/// Errors from [`RunLifecycleAuthority::commit_transition`].
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum CommitError {
    #[error("run not found: {run_id}")]
    NotFound { run_id: String },

    #[error(transparent)]
    Illegal(#[from] TransitionError),

    /// CAS miss: another writer advanced revision.
    #[error(
        "revision conflict for run {run_id}: expected {expected}, current {current} (status={status:?})"
    )]
    CasConflict {
        run_id: String,
        expected: u64,
        current: u64,
        status: RunStatusV2,
    },

    /// Terminal outcome arrived after an already-terminal run: return existing
    /// terminal without inserting another lifecycle event (caller should treat as OK).
    #[error("run {run_id} already terminal at {status:?} revision {revision}")]
    AlreadyTerminal {
        run_id: String,
        status: RunStatusV2,
        revision: u64,
    },

    #[error("storage: {0}")]
    Storage(String),

    #[error("{0}")]
    Other(String),
}

impl CommitError {
    pub fn is_cas_conflict(&self) -> bool {
        matches!(self, Self::CasConflict { .. })
    }

    pub fn is_already_terminal(&self) -> bool {
        matches!(self, Self::AlreadyTerminal { .. })
    }
}

/// Unique production interface for mutating Run status.
///
/// Implementations must:
/// 1. validate via [`transition`];
/// 2. CAS on `revision`;
/// 3. write run row + lifecycle event in one durable transaction;
/// 4. only then update memory cache and broadcast.
///
/// Startup hydration may construct historical rows directly, but active→Interrupted
/// recovery must still go through this interface.
pub trait RunLifecycleAuthority: Send + Sync {
    fn commit_transition(
        &self,
        run_id: &str,
        expected_revision: u64,
        target: RunStatusV2,
        metadata: TransitionMetadata,
    ) -> Result<CommittedTransition, CommitError>;
}

/// Helper: map an engine outcome into a commit request target + metadata.
pub fn outcome_commit_parts(outcome: &EngineOutcome) -> (RunStatusV2, TransitionMetadata) {
    (outcome.target_status(), TransitionMetadata::from_outcome(outcome))
}

#[cfg(test)]
mod tests {
    use super::*;
    use RunStatusV2::*;

    /// Exhaustive legal edges — must match can_transition matrix.
    fn legal_edges() -> Vec<(RunStatusV2, RunStatusV2)> {
        vec![
            (Created, Queued),
            (Created, Cancelling),
            (Created, Failed),
            (Queued, Preparing),
            (Queued, Cancelling),
            (Queued, Failed),
            (Preparing, Running),
            (Preparing, WaitingPermission),
            (Preparing, Cancelling),
            (Preparing, Failed),
            (Preparing, Interrupted),
            (Running, WaitingPermission),
            (Running, WaitingSubagent),
            (Running, Cancelling),
            (Running, Completed),
            (Running, Failed),
            (Running, Interrupted),
            (WaitingPermission, Running),
            (WaitingPermission, Cancelling),
            (WaitingPermission, Completed),
            (WaitingPermission, Failed),
            (WaitingPermission, Interrupted),
            (WaitingSubagent, Running),
            (WaitingSubagent, Cancelling),
            (WaitingSubagent, Completed),
            (WaitingSubagent, Failed),
            (WaitingSubagent, Interrupted),
            (Cancelling, Cancelled),
            (Cancelling, Interrupted),
            (Cancelling, Failed),
        ]
    }

    #[test]
    fn exhaustive_matrix_legal_edges_succeed() {
        for (from, to) in legal_edges() {
            assert!(
                transition(from, to).is_ok(),
                "expected OK: {:?} -> {:?}",
                from,
                to
            );
        }
    }

    #[test]
    fn exhaustive_matrix_all_other_edges_fail() {
        let legal: std::collections::HashSet<_> = legal_edges().into_iter().collect();
        for from in all_statuses() {
            for to in all_statuses() {
                if from == to {
                    assert!(
                        transition(from, to).is_err(),
                        "self-transition should fail: {:?}",
                        from
                    );
                    continue;
                }
                let ok = transition(from, to).is_ok();
                let expected = legal.contains(&(from, to));
                assert_eq!(
                    ok, expected,
                    "matrix mismatch for {:?} -> {:?} (ok={}, expected={})",
                    from, to, ok, expected
                );
            }
        }
    }

    #[test]
    fn terminal_states_reject_all() {
        for terminal in [Completed, Failed, Cancelled, Interrupted] {
            for target in all_statuses() {
                let err = transition(terminal, target).unwrap_err();
                if terminal == target {
                    assert!(
                        matches!(err, TransitionError::IllegalTransition { .. }),
                        "{terminal:?} -> {target:?}: {err:?}"
                    );
                } else {
                    assert!(
                        matches!(err, TransitionError::TerminalState { .. }),
                        "{terminal:?} -> {target:?}: {err:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn cancelling_reaches_cancelled_not_completed() {
        assert!(transition(Cancelling, Cancelled).is_ok());
        assert!(transition(Cancelling, Completed).is_err());
        assert!(transition(Running, Cancelled).is_err());
    }

    #[test]
    fn waiting_subagent_edges() {
        assert!(transition(Running, WaitingSubagent).is_ok());
        assert!(transition(WaitingSubagent, Running).is_ok());
        assert!(transition(Queued, WaitingSubagent).is_err());
    }

    #[test]
    fn legal_next_states_for_queued() {
        let next = legal_next_states(Queued);
        assert!(next.contains(&Preparing));
        assert!(next.contains(&Cancelling));
        assert!(next.contains(&Failed));
        assert_eq!(next.len(), 3);
    }

    #[test]
    fn legal_next_states_terminal_empty() {
        assert!(legal_next_states(Completed).is_empty());
        assert!(legal_next_states(Cancelled).is_empty());
    }

    #[test]
    fn engine_outcome_targets() {
        assert_eq!(
            EngineOutcome::completed("stop").target_status(),
            Completed
        );
        assert_eq!(EngineOutcome::Cancelled.target_status(), Cancelled);
        assert_eq!(
            EngineOutcome::failed("x", "y", false).target_status(),
            Failed
        );
        assert_eq!(
            EngineOutcome::interrupted("daemon_restarted").target_status(),
            Interrupted
        );
    }

    #[test]
    fn metadata_from_outcome() {
        let m = TransitionMetadata::from_outcome(&EngineOutcome::failed("e", "boom", true));
        assert_eq!(m.error_code.as_deref(), Some("e"));
        assert_eq!(m.reason.as_deref(), Some("boom"));
        assert_eq!(m.extra["retryable"], true);
    }

    #[test]
    fn transition_error_to_daemon_error() {
        let err = TransitionError::IllegalTransition {
            from: Completed,
            to: Running,
        };
        let daemon_err: DaemonError = err.into();
        assert_eq!(daemon_err.code, error_codes::RUN_INVALID_STATE);
        assert_eq!(daemon_err.category, ErrorCategory::Validation);
        assert!(!daemon_err.retryable);
    }

    /// Compile-time / unit stand-in proving the trait shape.
    struct RejectAllAuthority;

    impl RunLifecycleAuthority for RejectAllAuthority {
        fn commit_transition(
            &self,
            run_id: &str,
            _expected_revision: u64,
            _target: RunStatusV2,
            _metadata: TransitionMetadata,
        ) -> Result<CommittedTransition, CommitError> {
            Err(CommitError::NotFound {
                run_id: run_id.to_string(),
            })
        }
    }

    #[test]
    fn run_lifecycle_authority_trait_is_usable() {
        let auth = RejectAllAuthority;
        let err = auth
            .commit_transition("r1", 0, Preparing, TransitionMetadata::empty())
            .unwrap_err();
        assert!(matches!(err, CommitError::NotFound { .. }));
    }
}
