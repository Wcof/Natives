//! Input drain helper (W9 split from engine_core.rs). Drains steering /
//! follow-up inputs from the injected receiver into the typed transcript.

use super::engine_core::AgentEngine;
use super::error::EngineError;

impl AgentEngine {
    pub(super) async fn drain_inputs(
        &self,
        kind: crate::PendingInputKind,
        mode: crate::DrainMode,
        point: crate::InputSafePoint,
        messages: &mut Vec<crate::AgentMessage>,
        turn_id: Option<&str>,
    ) -> Result<bool, EngineError> {
        let Some(receiver) = &self.input_receiver else {
            return Ok(false);
        };
        let pending = receiver
            .drain(kind, mode, point)
            .await
            .map_err(|error| EngineError::Message(format!("input drain failed: {error}")))?;
        if pending.is_empty() {
            return Ok(false);
        }
        for input in pending {
            receiver
                .ack(&input, turn_id)
                .await
                .map_err(|error| EngineError::Message(format!("input ack failed: {error}")))?;
            let label = match input.kind {
                crate::PendingInputKind::Steering => "steering",
                crate::PendingInputKind::FollowUp => "follow_up",
            };
            let content = if input.content.starts_with(&format!("[{label}]\n")) {
                input.content.clone()
            } else {
                format!("[{label}]\n{}", input.content)
            };
            messages.push(crate::AgentMessage::User(crate::UserMessage {
                message_id: crate::MessageId::from(format!("queue:{}", input.id)),
                content: vec![crate::ContentBlock::Text { text: content }],
            }));
        }
        Ok(true)
    }
}
