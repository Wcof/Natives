use super::content_block::ContentBlock;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Message role.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    System,
    User,
    Assistant,
}

/// Message status.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MessageStatus {
    Sending,
    Streaming,
    Complete,
    Failed,
    Interrupted,
}

/// A message within a conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub conversation_id: String,
    pub parent_message_id: Option<String>,
    pub role: MessageRole,
    pub content_blocks: Vec<ContentBlock>,
    pub status: MessageStatus,
    pub usage: Option<MessageUsage>,
    pub created_at: DateTime<Utc>,
}

/// Token usage attached to a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
}

/// Input for creating a new message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMessageInput {
    pub conversation_id: String,
    pub parent_message_id: Option<String>,
    pub role: MessageRole,
    pub content_blocks: Vec<ContentBlock>,
}
