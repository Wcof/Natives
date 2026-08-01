//! Run lineage and safe resume decisions. Storage remains daemon-owned.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineageFields {
    pub conversation_id: String,
    pub branch_id: String,
    pub run_id: String,
    pub parent_run_id: Option<String>,
    pub attempt: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SideEffectRecord {
    pub tool_call_id: String,
    pub tool_name: String,
    pub status: SideEffectStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SideEffectStatus {
    Completed,
    Failed,
    Cancelled,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResumeAction {
    ContinueFromCheckpoint,
    RetryProviderAttempt,
    RetryTurn,
    ForkConversation,
    BlockUnsafeSideEffects,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumePlan {
    pub source_run_id: String,
    pub new_run_id: String,
    pub action: ResumeAction,
    pub checkpoint_id: Option<String>,
    pub unresolved_side_effects: Vec<SideEffectRecord>,
}

impl ResumePlan {
    pub fn for_checkpoint(
        source_run_id: impl Into<String>,
        checkpoint_id: impl Into<String>,
    ) -> Self {
        Self {
            source_run_id: source_run_id.into(),
            new_run_id: uuid::Uuid::new_v4().to_string(),
            action: ResumeAction::ContinueFromCheckpoint,
            checkpoint_id: Some(checkpoint_id.into()),
            unresolved_side_effects: Vec::new(),
        }
    }

    pub fn is_safe(&self) -> bool {
        !self
            .unresolved_side_effects
            .iter()
            .any(|effect| matches!(effect.status, SideEffectStatus::Unknown))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_side_effect_blocks_resume() {
        let mut plan = ResumePlan::for_checkpoint("run-a", "cp-1");
        assert!(plan.is_safe());
        plan.unresolved_side_effects.push(SideEffectRecord {
            tool_call_id: "call-1".into(),
            tool_name: "shell".into(),
            status: SideEffectStatus::Unknown,
        });
        assert!(!plan.is_safe());
    }
}
