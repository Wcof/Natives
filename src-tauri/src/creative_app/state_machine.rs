//! State machine transitions for external creative apps.

use super::model::CreativeAppState;
use crate::{Error, Result};

/// Validate and return the next state. Rejects illegal transitions.
pub fn transition(from: CreativeAppState, to: CreativeAppState) -> Result<CreativeAppState> {
    if is_allowed(from, to) {
        Ok(to)
    } else {
        Err(Error::InvalidInput(format!(
            "illegal creative-app state transition: {} → {}",
            from.as_str(),
            to.as_str()
        )))
    }
}

fn is_allowed(from: CreativeAppState, to: CreativeAppState) -> bool {
    use CreativeAppState::*;
    if from == to {
        return true;
    }
    match (from, to) {
        // Install pipeline
        (Installing, Starting)
        | (Installing, InstallFailed)
        | (Installing, RuntimeUnavailable)
        | (Installing, Deleting) => true,

        (InstallFailed, Installing)
        | (InstallFailed, Deleting)
        | (InstallFailed, InstalledStopped) => true,

        // Start pipeline
        (InstalledStopped, Starting)
        | (InstalledStopped, Deleting)
        | (InstalledStopped, RuntimeUnavailable) => true,

        (Starting, Running)
        | (Starting, StartFailed)
        | (Starting, RuntimeUnavailable)
        | (Starting, Deleting) => true,

        (StartFailed, Starting)
        | (StartFailed, Stopping)
        | (StartFailed, InstalledStopped)
        | (StartFailed, Deleting)
        | (StartFailed, RuntimeUnavailable) => true,

        (Running, Stopping)
        | (Running, StartFailed) // health later failed
        | (Running, RuntimeUnavailable)
        | (Running, Deleting) => true,

        (Stopping, InstalledStopped)
        | (Stopping, RuntimeUnavailable)
        | (Stopping, Deleting)
        | (Stopping, StartFailed)
        | (Stopping, CleanupFailed)
        | (Stopping, Orphaned) => true,

        // Stop could not verify release / live orphan — the user must retry stop or
        // restart; resources are never assumed gone.
        (CleanupFailed, Stopping)
        | (CleanupFailed, InstalledStopped)
        | (CleanupFailed, Starting)
        | (CleanupFailed, Deleting) => true,
        (Orphaned, Stopping)
        | (Orphaned, InstalledStopped)
        | (Orphaned, Starting)
        | (Orphaned, Deleting) => true,

        (RuntimeUnavailable, Starting)
        | (RuntimeUnavailable, InstalledStopped)
        | (RuntimeUnavailable, Running)
        | (RuntimeUnavailable, Deleting)
        | (RuntimeUnavailable, InstallFailed)
        | (RuntimeUnavailable, StartFailed) => true,

        (Deleting, DeleteFailed) => true,
        // success deletes the row — no terminal "deleted" state in DB
        (DeleteFailed, Deleting) => true,

        // Recovery / reconcile transitions not covered above
        (
            Available | Disabled | InstallFailed | DeleteFailed,
            RuntimeUnavailable,
        ) => true,

        // Reconcile may jump to observed truth from transient
        (Installing, InstalledStopped) | (Installing, Running) => true,
        (Starting, InstalledStopped) => true,
        (Stopping, Running) => true, // stop failed, still running
        (Deleting, InstalledStopped) | (Deleting, Running) | (Deleting, InstallFailed) => true,

        _ => false,
    }
}

/// Map leftover transient state to a stable failure/stopped state when Docker
/// cannot confirm the resource still exists.
pub fn converge_orphan(from: CreativeAppState) -> CreativeAppState {
    use CreativeAppState::*;
    match from {
        Installing => InstallFailed,
        Starting => StartFailed,
        Stopping => InstalledStopped,
        Deleting => DeleteFailed,
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use CreativeAppState::*;

    #[test]
    fn install_happy_path() {
        assert!(transition(Installing, Starting).is_ok());
        assert!(transition(Starting, Running).is_ok());
        assert!(transition(Running, Stopping).is_ok());
        assert!(transition(Stopping, InstalledStopped).is_ok());
    }

    #[test]
    fn rejects_running_to_installing() {
        assert!(transition(Running, Installing).is_err());
    }

    #[test]
    fn converge_transients() {
        assert_eq!(converge_orphan(Installing), InstallFailed);
        assert_eq!(converge_orphan(Starting), StartFailed);
        assert_eq!(converge_orphan(Stopping), InstalledStopped);
        assert_eq!(converge_orphan(Deleting), DeleteFailed);
        assert_eq!(converge_orphan(Running), Running);
    }

    #[test]
    fn cleanup_failed_and_orphaned_are_stable_and_retryable() {
        // Stop-failure states are not transient and can be retried from.
        assert!(!CleanupFailed.is_transient());
        assert!(!Orphaned.is_transient());
        assert!(transition(Stopping, CleanupFailed).is_ok());
        assert!(transition(Stopping, Orphaned).is_ok());
        assert!(transition(CleanupFailed, Stopping).is_ok());
        assert!(transition(Orphaned, Stopping).is_ok());
        assert!(transition(Orphaned, Starting).is_ok());
        // Orphan is stable under converge — it is not a transient to auto-clear.
        assert_eq!(converge_orphan(Orphaned), Orphaned);
        assert_eq!(converge_orphan(CleanupFailed), CleanupFailed);
    }
}
