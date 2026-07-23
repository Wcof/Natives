//! Integration tests for the agent-core RunStatusV2 state machine.
//!
//! Covers legal/illegal edges, terminal rejection, DaemonError conversion,
//! EngineOutcome, and the RunLifecycleAuthority trait surface.

use agent_core::run_state::{
    all_statuses, legal_next_states, outcome_commit_parts, transition, CommitError, EngineOutcome,
    RunLifecycleAuthority, TransitionError, TransitionMetadata,
};
use assistant_protocol::error::error_codes;
use assistant_protocol::error::DaemonError;
use assistant_protocol::error::ErrorCategory;
use assistant_protocol::v2::RunStatusV2;

use RunStatusV2::*;

#[test]
fn exhaustive_matrix() {
    let legal = [
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
    ];
    let legal_set: std::collections::HashSet<_> = legal.into_iter().collect();

    for (from, to) in legal {
        assert!(
            transition(from, to).is_ok(),
            "expected OK: {from:?} -> {to:?}"
        );
    }

    for from in all_statuses() {
        for to in all_statuses() {
            let ok = transition(from, to).is_ok();
            let expected = legal_set.contains(&(from, to));
            assert_eq!(ok, expected, "matrix mismatch {from:?} -> {to:?}");
        }
    }
}

#[test]
fn terminal_rejects() {
    for terminal in [Completed, Failed, Cancelled, Interrupted] {
        for target in all_statuses() {
            assert!(
                transition(terminal, target).is_err(),
                "{terminal:?} -> {target:?}"
            );
        }
        assert!(legal_next_states(terminal).is_empty());
    }
}

#[test]
fn cancelled_path_only_from_cancelling() {
    assert!(transition(Cancelling, Cancelled).is_ok());
    assert!(transition(Running, Cancelled).is_err());
    assert!(transition(Cancelling, Completed).is_err());
}

#[test]
fn daemon_error_conversion() {
    let err = transition(Queued, Completed).unwrap_err();
    assert!(matches!(err, TransitionError::IllegalTransition { .. }));
    let daemon_err: DaemonError = err.into();
    assert_eq!(daemon_err.code, error_codes::RUN_INVALID_STATE);
    assert_eq!(daemon_err.category, ErrorCategory::Validation);

    let err = transition(Completed, Running).unwrap_err();
    assert!(matches!(err, TransitionError::TerminalState { .. }));
    let daemon_err: DaemonError = err.into();
    assert_eq!(daemon_err.code, error_codes::RUN_INVALID_STATE);
}

#[test]
fn engine_outcome_and_metadata() {
    let o = EngineOutcome::completed("stop");
    let (target, meta) = outcome_commit_parts(&o);
    assert_eq!(target, Completed);
    assert_eq!(meta.reason.as_deref(), Some("stop"));

    let o = EngineOutcome::Cancelled;
    assert_eq!(o.target_status(), Cancelled);

    let o = EngineOutcome::failed("max_steps", "too many", false);
    let meta = TransitionMetadata::from_outcome(&o);
    assert_eq!(meta.error_code.as_deref(), Some("max_steps"));
    assert_eq!(meta.extra["retryable"], false);

    let o = EngineOutcome::interrupted("daemon_restarted");
    assert_eq!(o.error_code(), Some("daemon_restarted"));
}

struct MemAuthority;

impl RunLifecycleAuthority for MemAuthority {
    fn commit_transition(
        &self,
        run_id: &str,
        expected_revision: u64,
        target: RunStatusV2,
        _metadata: TransitionMetadata,
    ) -> Result<agent_core::run_state::CommittedTransition, CommitError> {
        if run_id == "missing" {
            return Err(CommitError::NotFound {
                run_id: run_id.into(),
            });
        }
        // Simulate CAS: only revision 0 accepted for this stub.
        if expected_revision != 0 {
            return Err(CommitError::CasConflict {
                run_id: run_id.into(),
                expected: expected_revision,
                current: 0,
                status: Queued,
            });
        }
        Ok(agent_core::run_state::CommittedTransition {
            run_id: run_id.into(),
            from: Queued,
            to: target,
            revision: 1,
            idempotent: false,
        })
    }
}

#[test]
fn commit_interface_shape() {
    let auth = MemAuthority;
    let committed = auth
        .commit_transition("r1", 0, Preparing, TransitionMetadata::empty())
        .unwrap();
    assert_eq!(committed.revision, 1);
    assert_eq!(committed.to, Preparing);

    let err = auth
        .commit_transition("r1", 5, Preparing, TransitionMetadata::empty())
        .unwrap_err();
    assert!(err.is_cas_conflict());
}
