//! Critical event append + failed-turn closure helpers (W9 split from
//! engine_core.rs). These are the only places the engine writes durable
//! terminal/transition events; the run loop calls back into them.

use super::engine_core::AgentEngine;
use super::error::EngineError;
use assistant_protocol::v2::RunEventKind;
use serde_json::json;

impl AgentEngine {
    pub(super) fn append_critical(
        &self,
        run_id: &str,
        event: RunEventKind,
    ) -> Result<(), EngineError> {
        self.events
            .append_checked(run_id, event)
            .map(|_| ())
            .map_err(EngineError::Message)
    }

    pub(super) fn close_failed_turn(
        &self,
        run_id: &str,
        turn_id: &crate::TurnId,
        message_id: &crate::MessageId,
        stop_reason: &str,
        text: &str,
        reasoning: &str,
    ) -> Result<(), EngineError> {
        let mut content = Vec::new();
        if !reasoning.is_empty() {
            content.push(crate::ContentBlock::Thinking {
                text: reasoning.to_string(),
                signature: None,
            });
        }
        if !text.is_empty() {
            content.push(crate::ContentBlock::Text {
                text: text.to_string(),
            });
        }
        self.append_critical(
            run_id,
            RunEventKind::MessageCompleted {
                turn_id: turn_id.to_string(),
                message_id: message_id.to_string(),
                role: "assistant".into(),
                content: Some(json!({
                    "message_id": message_id.to_string(),
                    "role": "assistant",
                    "content": content,
                })),
            },
        )?;
        self.append_critical(
            run_id,
            RunEventKind::TurnCompleted {
                turn_id: turn_id.to_string(),
                stop_reason: stop_reason.to_string(),
                input_tokens: 0,
                output_tokens: 0,
            },
        )
    }
}
