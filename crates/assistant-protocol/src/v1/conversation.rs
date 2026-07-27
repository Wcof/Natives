use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Conversation mode: chat (plain dialogue) or agent (tool-using loop)
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConversationMode {
    Chat,
    Agent,
}

/// A conversation (session) between the user and the AI assistant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub mode: ConversationMode,
    pub project_id: Option<String>,
    pub title: String,
    pub provider_id: String,
    pub model_id: String,
    pub permission_profile_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub archived_at: Option<DateTime<Utc>>,
}

/// Input for creating a new conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateConversation {
    pub mode: ConversationMode,
    pub project_id: Option<String>,
    pub title: Option<String>,
    pub provider_id: String,
    pub model_id: String,
    pub permission_profile_id: Option<String>,
}

/// Input for updating an existing conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateConversation {
    pub title: Option<String>,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub permission_profile_id: Option<String>,
    pub archived_at: Option<DateTime<Utc>>,
}

/// Query parameters for listing conversations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListConversationsQuery {
    pub project_id: Option<String>,
    pub mode: Option<ConversationMode>,
    pub search: Option<String>,
    pub include_archived: Option<bool>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}
