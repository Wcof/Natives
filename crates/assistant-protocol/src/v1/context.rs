use serde::{Deserialize, Serialize};

/// A preview of the context that will be sent to the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPreview {
    pub sections: Vec<ContextSection>,
    pub total_tokens: u64,
    pub max_tokens: u64,
}

/// A single section of the assembled context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSection {
    pub source: ContextSource,
    pub label: String,
    pub tokens: u64,
    pub included: bool,
}

/// Source of a context section.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContextSource {
    System,
    UserPreferences,
    ProjectRules,
    ConversationHistory,
    CurrentPlan,
    AttachedFiles,
    ToolResults,
    Summary,
    Custom(String),
}

/// Input for configuring context exclusion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextExclusion {
    pub source: ContextSource,
    pub exclude: bool,
}