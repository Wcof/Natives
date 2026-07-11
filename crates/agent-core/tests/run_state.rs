//! Integration tests for the agent-core run state machine.
//!
//! These tests verify the `transition` function with table-driven cases
//! covering all legal transitions, illegal transitions, and terminal state
//! rejection. They also verify that `DaemonError` conversion works correctly.

use agent_core::run_state::transition;
use agent_core::run_state::TransitionError;
use assistant_protocol::v1::run::RunStatus;
use assistant_protocol::error::DaemonError;
use assistant_protocol::error::ErrorCategory;
use assistant_protocol::error::error_codes;

// ---------------------------------------------------------------------------
// Legal transitions
// ---------------------------------------------------------------------------

#[test]
fn test_queued_to_preparing() {
    assert!(transition(RunStatus::Queued, RunStatus::Preparing).is_ok());
}

#[test]
fn test_queued_to_cancelling() {
    assert!(transition(RunStatus::Queued, RunStatus::Cancelling).is_ok());
}

#[test]
fn test_queued_to_failed() {
    assert!(transition(RunStatus::Queued, RunStatus::Failed).is_ok());
}

#[test]
fn test_preparing_to_running() {
    assert!(transition(RunStatus::Preparing, RunStatus::Running).is_ok());
}

#[test]
fn test_preparing_to_waiting_permission() {
    assert!(transition(RunStatus::Preparing, RunStatus::WaitingPermission).is_ok());
}

#[test]
fn test_preparing_to_cancelling() {
    assert!(transition(RunStatus::Preparing, RunStatus::Cancelling).is_ok());
}

#[test]
fn test_preparing_to_failed() {
    assert!(transition(RunStatus::Preparing, RunStatus::Failed).is_ok());
}

#[test]
fn test_preparing_to_interrupted() {
    assert!(transition(RunStatus::Preparing, RunStatus::Interrupted).is_ok());
}

#[test]
fn test_running_to_waiting_permission() {
    assert!(transition(RunStatus::Running, RunStatus::WaitingPermission).is_ok());
}

#[test]
fn test_running_to_cancelling() {
    assert!(transition(RunStatus::Running, RunStatus::Cancelling).is_ok());
}

#[test]
fn test_running_to_completed() {
    assert!(transition(RunStatus::Running, RunStatus::Completed).is_ok());
}

#[test]
fn test_running_to_failed() {
    assert!(transition(RunStatus::Running, RunStatus::Failed).is_ok());
}

#[test]
fn test_running_to_interrupted() {
    assert!(transition(RunStatus::Running, RunStatus::Interrupted).is_ok());
}

#[test]
fn test_waiting_permission_to_running() {
    assert!(transition(RunStatus::WaitingPermission, RunStatus::Running).is_ok());
}

#[test]
fn test_waiting_permission_to_cancelling() {
    assert!(transition(RunStatus::WaitingPermission, RunStatus::Cancelling).is_ok());
}

#[test]
fn test_waiting_permission_to_completed() {
    assert!(transition(RunStatus::WaitingPermission, RunStatus::Completed).is_ok());
}

#[test]
fn test_waiting_permission_to_failed() {
    assert!(transition(RunStatus::WaitingPermission, RunStatus::Failed).is_ok());
}

#[test]
fn test_waiting_permission_to_interrupted() {
    assert!(transition(RunStatus::WaitingPermission, RunStatus::Interrupted).is_ok());
}

#[test]
fn test_cancelling_to_interrupted() {
    assert!(transition(RunStatus::Cancelling, RunStatus::Interrupted).is_ok());
}

#[test]
fn test_cancelling_to_failed() {
    assert!(transition(RunStatus::Cancelling, RunStatus::Failed).is_ok());
}

// ---------------------------------------------------------------------------
// Illegal transitions
// ---------------------------------------------------------------------------

#[test]
fn test_queued_to_completed_illegal() {
    let err = transition(RunStatus::Queued, RunStatus::Completed).unwrap_err();
    assert!(matches!(err, TransitionError::IllegalTransition { .. }));
}

#[test]
fn test_completed_to_running_illegal() {
    let err = transition(RunStatus::Completed, RunStatus::Running).unwrap_err();
    assert!(matches!(err, TransitionError::TerminalState { .. }));
}

#[test]
fn test_failed_to_queued_illegal() {
    let err = transition(RunStatus::Failed, RunStatus::Queued).unwrap_err();
    assert!(matches!(err, TransitionError::TerminalState { .. }));
}

#[test]
fn test_interrupted_to_running_illegal() {
    let err = transition(RunStatus::Interrupted, RunStatus::Running).unwrap_err();
    assert!(matches!(err, TransitionError::TerminalState { .. }));
}

// ---------------------------------------------------------------------------
// DaemonError conversion
// ---------------------------------------------------------------------------

#[test]
fn test_illegal_transition_converts_to_daemon_error() {
    let err = transition(RunStatus::Queued, RunStatus::Completed).unwrap_err();
    let daemon_err: DaemonError = err.into();
    assert_eq!(daemon_err.code, error_codes::RUN_INVALID_STATE);
    assert_eq!(daemon_err.category, ErrorCategory::Validation);
}

#[test]
fn test_terminal_transition_converts_to_daemon_error() {
    let err = transition(RunStatus::Completed, RunStatus::Running).unwrap_err();
    let daemon_err: DaemonError = err.into();
    assert_eq!(daemon_err.code, error_codes::RUN_INVALID_STATE);
    assert_eq!(daemon_err.category, ErrorCategory::Validation);
}

// ---------------------------------------------------------------------------
// Bulk table-driven tests for completeness
// ---------------------------------------------------------------------------

#[test]
fn test_all_legal_transitions_bulk() {
    let legal: Vec<(RunStatus, RunStatus)> = vec![
        (RunStatus::Queued, RunStatus::Preparing),
        (RunStatus::Queued, RunStatus::Cancelling),
        (RunStatus::Queued, RunStatus::Failed),
        (RunStatus::Preparing, RunStatus::Running),
        (RunStatus::Preparing, RunStatus::WaitingPermission),
        (RunStatus::Preparing, RunStatus::Cancelling),
        (RunStatus::Preparing, RunStatus::Failed),
        (RunStatus::Preparing, RunStatus::Interrupted),
        (RunStatus::Running, RunStatus::WaitingPermission),
        (RunStatus::Running, RunStatus::Cancelling),
        (RunStatus::Running, RunStatus::Completed),
        (RunStatus::Running, RunStatus::Failed),
        (RunStatus::Running, RunStatus::Interrupted),
        (RunStatus::WaitingPermission, RunStatus::Running),
        (RunStatus::WaitingPermission, RunStatus::Cancelling),
        (RunStatus::WaitingPermission, RunStatus::Completed),
        (RunStatus::WaitingPermission, RunStatus::Failed),
        (RunStatus::WaitingPermission, RunStatus::Interrupted),
        (RunStatus::Cancelling, RunStatus::Interrupted),
        (RunStatus::Cancelling, RunStatus::Failed),
    ];

    for (current, target) in &legal {
        assert!(
            transition(*current, *target).is_ok(),
            "Expected OK: {:?} -> {:?}",
            current,
            target
        );
    }
}

#[test]
fn test_all_illegal_transitions_bulk() {
    let illegal: Vec<(RunStatus, RunStatus)> = vec![
        (RunStatus::Queued, RunStatus::Running),
        (RunStatus::Queued, RunStatus::Completed),
        (RunStatus::Queued, RunStatus::Interrupted),
        (RunStatus::Queued, RunStatus::WaitingPermission),
        (RunStatus::Preparing, RunStatus::Completed),
        (RunStatus::Running, RunStatus::Queued),
        (RunStatus::Running, RunStatus::Preparing),
        (RunStatus::WaitingPermission, RunStatus::Queued),
        (RunStatus::WaitingPermission, RunStatus::Preparing),
        (RunStatus::Cancelling, RunStatus::Running),
        (RunStatus::Cancelling, RunStatus::Completed),
        (RunStatus::Cancelling, RunStatus::WaitingPermission),
        (RunStatus::Cancelling, RunStatus::Queued),
        (RunStatus::Cancelling, RunStatus::Preparing),
        (RunStatus::Completed, RunStatus::Queued),
        (RunStatus::Completed, RunStatus::Running),
        (RunStatus::Completed, RunStatus::Failed),
        (RunStatus::Completed, RunStatus::Interrupted),
        (RunStatus::Failed, RunStatus::Queued),
        (RunStatus::Failed, RunStatus::Running),
        (RunStatus::Failed, RunStatus::Completed),
        (RunStatus::Failed, RunStatus::Interrupted),
        (RunStatus::Interrupted, RunStatus::Queued),
        (RunStatus::Interrupted, RunStatus::Running),
        (RunStatus::Interrupted, RunStatus::Completed),
        (RunStatus::Interrupted, RunStatus::Failed),
        (RunStatus::Interrupted, RunStatus::WaitingPermission),
    ];

    for (current, target) in &illegal {
        assert!(
            transition(*current, *target).is_err(),
            "Expected Err: {:?} -> {:?}",
            current,
            target
        );
    }
}