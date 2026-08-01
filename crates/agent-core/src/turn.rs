//! Explicit Turn identity and outcome model.

use crate::message::{AssistantMessage, StopReason, ToolResultMessage, TurnId};

#[derive(Debug, Clone)]
pub struct TurnOutcome {
    pub turn_id: TurnId,
    pub assistant_message: AssistantMessage,
    pub tool_results: Vec<ToolResultMessage>,
    pub stop_reason: StopReason,
    pub usage: Usage,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: Option<u64>,
}
