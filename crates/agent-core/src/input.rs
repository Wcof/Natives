//! Core-owned input queues. The daemon may persist/lease entries, but only the
//! engine consumes them at safe points.

use async_trait::async_trait;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingInputKind {
    Steering,
    FollowUp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrainMode {
    One,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputSafePoint {
    BeforeProvider,
    AfterToolBatch,
    BeforeRunEnd,
}

/// Host-owned persistence hook for safe-point state transitions.
/// Implementations must not consume durable interjections unless the actor
/// mutation is persisted successfully.
#[async_trait]
pub trait EngineSafePointReceiver: Send + Sync {
    async fn on_safe_point(&self, point: InputSafePoint) -> Result<Option<String>, String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingInput {
    pub id: String,
    pub kind: PendingInputKind,
    pub content: String,
    /// Durable queue lease token. In-memory receivers may leave this empty;
    /// the daemon-backed receiver uses it to prevent cross-lease acks.
    pub lease_token: Option<String>,
}

#[async_trait]
pub trait EngineInputReceiver: Send + Sync {
    async fn drain(
        &self,
        kind: PendingInputKind,
        mode: DrainMode,
        point: InputSafePoint,
    ) -> Result<Vec<PendingInput>, String>;
    /// Persist the consumed input before Core mutates the next Provider
    /// transcript. Failure must stop the run rather than silently losing a
    /// steering/follow-up message.
    async fn ack(&self, input: &PendingInput, turn_id: Option<&str>) -> Result<(), String>;
}

#[derive(Debug, Default)]
pub struct NoopInputReceiver;

#[async_trait]
impl EngineInputReceiver for NoopInputReceiver {
    async fn drain(
        &self,
        _kind: PendingInputKind,
        _mode: DrainMode,
        _point: InputSafePoint,
    ) -> Result<Vec<PendingInput>, String> {
        Ok(Vec::new())
    }
    async fn ack(&self, _input: &PendingInput, _turn_id: Option<&str>) -> Result<(), String> {
        Ok(())
    }
}
