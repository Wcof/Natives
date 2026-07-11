use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// An artifact — a file change, patch, report, screenshot, or structured result
/// produced during a run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub id: String,
    pub run_id: String,
    pub conversation_id: String,
    /// The tool that created this artifact.
    pub source_tool: String,
    /// Local filesystem path.
    pub path: String,
    /// Content-addressable hash (SHA-256).
    pub sha256: String,
    /// Size in bytes.
    pub size: u64,
    /// MIME type.
    pub mime_type: String,
    /// Display label.
    pub label: Option<String>,
    /// Artifact kind.
    pub kind: ArtifactKind,
    pub created_at: DateTime<Utc>,
}

/// The kind of artifact.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    /// A file that was created or modified.
    File,
    /// A unified diff / patch.
    Patch,
    /// A report (markdown, text, etc.).
    Report,
    /// A screenshot or image.
    Screenshot,
    /// An exported file (JSON, CSV, etc.).
    Export,
    /// A structured result (JSON, etc.).
    StructuredResult,
    /// An error artifact.
    Error,
}

/// Input for creating an artifact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateArtifactInput {
    pub run_id: String,
    pub conversation_id: String,
    pub source_tool: String,
    pub path: String,
    pub sha256: String,
    pub size: u64,
    pub mime_type: String,
    pub label: Option<String>,
    pub kind: ArtifactKind,
    /// Base64-encoded content for inline creation.
    pub content: String,
}