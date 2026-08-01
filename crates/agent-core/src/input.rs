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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingInput {
    pub id: String,
    pub kind: PendingInputKind,
    pub content: String,
}

#[async_trait]
pub trait EngineInputReceiver: Send + Sync {
    async fn drain(
        &self,
        kind: PendingInputKind,
        mode: DrainMode,
        point: InputSafePoint,
    ) -> Vec<PendingInput>;
    async fn ack(&self, input_id: &str);
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
    ) -> Vec<PendingInput> {
        Vec::new()
    }
    async fn ack(&self, _input_id: &str) {}
}
