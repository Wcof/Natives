//! Safe-point application + provider backoff (W9 split from engine_core.rs).

use super::conversion::provider_backoff_ms;
use super::engine_core::AgentEngine;
use super::error::EngineError;

impl AgentEngine {
    /// Wait out a provider backoff before the next generation attempt.
    ///
    /// Returns `false` when the run was cancelled mid-wait; the caller must
    /// unwind instead of retrying. A 60s `Retry-After` that ignored cancel
    /// would make Stop feel broken, so the wait races the run's cancel token.
    pub(super) async fn sleep_provider_backoff(
        &self,
        attempt: u32,
        retry_after_ms: Option<u64>,
    ) -> bool {
        let delay = provider_backoff_ms(attempt, retry_after_ms);
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_millis(delay)) => {
                !self.cancel.is_cancelled()
            }
            _ = self.cancel.cancelled() => false,
        }
    }

    /// Apply coordinator action at a safe point: inject interjection into messages.
    pub(super) async fn apply_safe_point(
        &self,
        conversation_id: &str,
        point: crate::session_coordinator::SafePoint,
        messages: &mut Vec<crate::AgentMessage>,
    ) -> Result<(), EngineError> {
        // Honest mapping (NE-P0-07): each SafePoint variant maps to the durable
        // InputSafePoint slot the engine is actually at. The `InputSafePoint`
        // enum has only three positions (BeforeProvider / AfterToolBatch /
        // BeforeRunEnd) and the daemon's `DurableSafePointReceiver` exposes no
        // finer tool-phase slot, so `BeforeTool` and `AfterTool` both drain at
        // `AfterToolBatch`. This is a durable-receiver constraint, not a topology
        // lie: their *engine* call sites are distinct and real — `BeforeTool`
        // fires before the tool batch executes (`engine_tools.rs`), `AfterTool`
        // after the batch and its transcript are committed (ditto), and
        // `ProviderBatchBoundary` between the batch and the next provider turn.
        //
        // `AfterPermissionResolved` is NEVER dispatched through this method: it
        // lives on the daemon permission path (`tools/permission.rs`,
        // `on_safe_point_checked`), not in the engine loop. The arm is retained
        // only for exhaustiveness — if it ever fires here that is a bug, and the
        // map should be revisited rather than silently routing it.
        let input_point = match point {
            crate::session_coordinator::SafePoint::BeforeTool => {
                crate::InputSafePoint::AfterToolBatch
            }
            crate::session_coordinator::SafePoint::AfterTool => {
                crate::InputSafePoint::AfterToolBatch
            }
            crate::session_coordinator::SafePoint::AfterPermissionResolved => {
                crate::InputSafePoint::AfterToolBatch
            }
            crate::session_coordinator::SafePoint::ProviderBatchBoundary => {
                crate::InputSafePoint::BeforeProvider
            }
        };
        let content = if let Some(receiver) = &self.safe_point_receiver {
            receiver.on_safe_point(input_point).await.map_err(|error| {
                EngineError::Message(format!("safe-point persistence failed: {error}"))
            })?
        } else if let Some(harness) = &self.session_harness {
            match harness.on_safe_point(conversation_id, point) {
                crate::session_coordinator::CoordinatorAction::InjectInterjection { content } => {
                    Some(content)
                }
                _ => None,
            }
        } else {
            None
        };
        if let Some(content) = content {
            messages.push(crate::AgentMessage::User(crate::UserMessage {
                message_id: crate::MessageId::new(),
                content: vec![crate::ContentBlock::Text {
                    text: format!("[interjection]\n{content}"),
                }],
            }));
        }
        Ok(())
    }
}
