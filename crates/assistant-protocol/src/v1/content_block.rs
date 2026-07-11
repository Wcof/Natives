use serde::{Deserialize, Serialize};

/// A structured content block inside a message.
///
/// Replaces the legacy approach of embedding reasoning in ` thinking` XML tags
/// and storing tool calls as JSON strings. Each variant carries its own typed payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// Plain text content.
    Text(TextContent),
    /// Model reasoning / thinking trace.
    Reasoning(ReasoningContent),
    /// Inline base64-encoded image.
    Image(ImageContent),
    /// Reference to an attached file.
    FileReference(FileReferenceContent),
    /// A tool call request from the model.
    ToolCall(ToolCallContent),
    /// The result of a tool execution.
    ToolResult(ToolResultContent),
    /// A citation / source reference.
    Citation(CitationContent),
    /// An error block.
    Error(ErrorContent),
    /// Legacy unstructured block (for migration compatibility).
    Legacy(LegacyContent),
}

/// Plain text content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextContent {
    pub text: String,
}

/// Model reasoning / thinking trace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningContent {
    pub text: String,
    pub signature: Option<String>,
}

/// Base64-encoded image.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageContent {
    pub mime_type: String,
    pub data: String, // base64-encoded
    pub alt_text: Option<String>,
}

/// Reference to an attached file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileReferenceContent {
    pub path: String,
    pub mime_type: String,
    pub size: u64,
    pub sha256: Option<String>,
}

/// A tool call request from the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallContent {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
    pub status: ToolCallStatus,
}

/// Status of a tool call.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Rejected,
}

/// The result of a tool execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResultContent {
    pub tool_call_id: String,
    pub output: serde_json::Value,
    pub is_error: bool,
    pub duration_ms: Option<u64>,
}

/// A citation / source reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CitationContent {
    pub uri: String,
    pub title: Option<String>,
    pub text: Option<String>,
    pub start_index: Option<u32>,
    pub end_index: Option<u32>,
}

/// An error block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorContent {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

/// Legacy unstructured block — for migration from old format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegacyContent {
    pub raw: String,
    pub original_type: String,
}

impl ContentBlock {
    /// Returns true if this block represents a tool call.
    pub fn is_tool_call(&self) -> bool {
        matches!(self, ContentBlock::ToolCall(_))
    }

    /// Returns true if this block represents a tool result.
    pub fn is_tool_result(&self) -> bool {
        matches!(self, ContentBlock::ToolResult(_))
    }

    /// Returns the text content if this is a Text block, or the reasoning text if Reasoning.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            ContentBlock::Text(t) => Some(&t.text),
            ContentBlock::Reasoning(r) => Some(&r.text),
            _ => None,
        }
    }
}