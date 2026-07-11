use assistant_protocol::v1::run::RunStatus;
use assistant_protocol::error::DaemonError;
use assistant_protocol::error::ErrorCategory;
use assistant_protocol::error::error_codes;

/// Error returned when an illegal state transition is attempted.
#[derive(Debug, thiserror::Error)]
pub enum TransitionError {
    #[error("Illegal transition from {from:?} to {to:?}")]
    IllegalTransition {
        from: RunStatus,
        to: RunStatus,
    },

    #[error("Transition from terminal state {from:?} is not allowed")]
    TerminalState {
        from: RunStatus,
    },

    #[error("{0}")]
    Other(String),
}

impl From<TransitionError> for DaemonError {
    fn from(err: TransitionError) -> Self {
        match err {
            TransitionError::IllegalTransition { from, to } => {
                DaemonError::new(
                    error_codes::RUN_INVALID_STATE,
                    ErrorCategory::Validation,
                    false,
                    format!("Illegal transition from {:?} to {:?}", from, to),
                )
                .with_user_key("error.run_invalid_state")
            }
            TransitionError::TerminalState { from } => {
                DaemonError::new(
                    error_codes::RUN_INVALID_STATE,
                    ErrorCategory::Validation,
                    false,
                    format!("Cannot transition from terminal state {:?}", from),
                )
                .with_user_key("error.run_invalid_state")
            }
            TransitionError::Other(msg) => {
                DaemonError::new(
                    error_codes::INTERNAL_ERROR,
                    ErrorCategory::Internal,
                    false,
                    msg,
                )
            }
        }
    }
}

/// Single authority for Run state transitions.
///
/// This is the only place where state transition rules are defined.
/// Neither the UI nor provider implementations should duplicate these rules.
///
/// # Returns
///
/// - `Ok(())` if the transition is legal.
/// - `Err(TransitionError::TerminalState)` if `current` is a terminal state.
/// - `Err(TransitionError::IllegalTransition)` if the pair is not a legal transition.
pub fn transition(current: RunStatus, target: RunStatus) -> Result<(), TransitionError> {
    // Terminal states cannot transition further.
    if current.is_terminal() {
        return Err(TransitionError::TerminalState { from: current });
    }

    // Use the built-in can_transition_to for validation.
    if current.can_transition_to(&target) {
        Ok(())
    } else {
        Err(TransitionError::IllegalTransition {
            from: current,
            to: target,
        })
    }
}

/// Returns all legal next states from the given current state.
pub fn legal_next_states(current: RunStatus) -> Vec<RunStatus> {
    use RunStatus::*;
    let all = vec![
        Queued,
        Preparing,
        Running,
        WaitingPermission,
        Cancelling,
        Completed,
        Failed,
        Interrupted,
    ];
    all.into_iter()
        .filter(|next| transition(current, *next).is_ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use RunStatus::*;

    /// Table-driven test: all legal transitions must succeed.
    #[test]
    fn test_legal_transitions() {
        let legal_cases = vec![
            (Queued, Preparing),
            (Queued, Cancelling),
            (Queued, Failed),
            (Preparing, Running),
            (Preparing, WaitingPermission),
            (Preparing, Cancelling),
            (Preparing, Failed),
            (Preparing, Interrupted),
            (Running, WaitingPermission),
            (Running, Cancelling),
            (Running, Completed),
            (Running, Failed),
            (Running, Interrupted),
            (WaitingPermission, Running),
            (WaitingPermission, Cancelling),
            (WaitingPermission, Completed),
            (WaitingPermission, Failed),
            (WaitingPermission, Interrupted),
            (Cancelling, Interrupted),
            (Cancelling, Failed),
        ];

        for (current, target) in &legal_cases {
            assert!(
                transition(*current, *target).is_ok(),
                "Expected OK: {:?} -> {:?}",
                current,
                target
            );
        }
    }

    /// Table-driven test: all illegal transitions must fail.
    #[test]
    fn test_illegal_transitions() {
        let illegal_cases = vec![
            (Queued, Running),
            (Queued, Completed),
            (Queued, Interrupted),
            (Queued, WaitingPermission),
            (Preparing, Completed),
            (Running, Queued),
            (Running, Preparing),
            (WaitingPermission, Queued),
            (WaitingPermission, Preparing),
            (Cancelling, Running),
            (Cancelling, Completed),
            (Cancelling, WaitingPermission),
            (Cancelling, Queued),
            (Cancelling, Preparing),
            (Completed, Queued),
            (Completed, Running),
            (Completed, Failed),
            (Completed, Interrupted),
            (Failed, Queued),
            (Failed, Running),
            (Failed, Completed),
            (Failed, Interrupted),
            (Interrupted, Queued),
            (Interrupted, Running),
            (Interrupted, Completed),
            (Interrupted, Failed),
            (Interrupted, WaitingPermission),
        ];

        for (current, target) in &illegal_cases {
            assert!(
                transition(*current, *target).is_err(),
                "Expected Err: {:?} -> {:?}",
                current,
                target
            );
        }
    }

    /// Terminal states must reject all transitions.
    #[test]
    fn test_terminal_state_rejects_transitions() {
        let terminal_states = vec![Completed, Failed, Interrupted];
        let all_targets = vec![
            Queued, Preparing, Running, WaitingPermission, Cancelling,
            Completed, Failed, Interrupted,
        ];

        for terminal in &terminal_states {
            for target in &all_targets {
                let err = transition(*terminal, *target).unwrap_err();
                assert!(
                    matches!(err, TransitionError::TerminalState { .. }),
                    "Expected TerminalState error for {:?} -> {:?}, got {:?}",
                    terminal,
                    target,
                    err
                );
            }
        }
    }

    /// Legal next states should be consistent.
    #[test]
    fn test_legal_next_states() {
        let next = legal_next_states(Queued);
        assert!(next.contains(&Preparing));
        assert!(next.contains(&Cancelling));
        assert!(next.contains(&Failed));
        assert_eq!(next.len(), 3);

        let next = legal_next_states(Completed);
        assert!(next.is_empty());
    }

    /// TransitionError should convert to DaemonError correctly.
    #[test]
    fn test_transition_error_to_daemon_error() {
        let err = TransitionError::IllegalTransition {
            from: RunStatus::Completed,
            to: RunStatus::Running,
        };
        let daemon_err: DaemonError = err.into();
        assert_eq!(daemon_err.code, error_codes::RUN_INVALID_STATE);
        assert_eq!(daemon_err.category, ErrorCategory::Validation);
        assert!(!daemon_err.retryable);
    }
}