use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceMeta {
    pub id: String,
    pub name: String,
    #[serde(rename = "sortOrder")]
    pub sort_order: i64,
    #[serde(rename = "isPinned")]
    pub is_pinned: bool,
    #[serde(rename = "isOpen")]
    pub is_open: bool,
    pub revision: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenTab {
    #[serde(rename = "workspaceId")]
    pub workspace_id: String,
    #[serde(rename = "sortOrder")]
    pub sort_order: i64,
    #[serde(rename = "isPinned")]
    pub is_pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSnapshot {
    #[serde(rename = "activeWorkspaceId")]
    pub active_workspace_id: Option<String>,
    #[serde(rename = "openedTabs")]
    pub opened_tabs: Vec<OpenTab>,
    pub workspaces: Vec<WorkspaceMeta>,
    pub revision: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WidgetRecord {
    pub id: String,
    #[serde(default, rename = "workspaceId")]
    pub workspace_id: String,
    pub key: String,
    pub order: i64,
    pub enabled: bool,
    #[serde(rename = "configJson")]
    pub config_json: Value,
    #[serde(rename = "displayJson")]
    pub display_json: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceSnapshot {
    pub workspace: WorkspaceMeta,
    pub name: String,
    #[serde(rename = "backgroundJson")]
    pub background_json: Value,
    #[serde(rename = "templateSource")]
    pub template_source: Option<String>,
    pub widgets: Vec<WidgetRecord>,
    pub revision: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateSummary {
    pub id: String,
    pub origin: String,
    pub name: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplatePayload {
    #[serde(rename = "backgroundJson", default)]
    pub background_json: Value,
    #[serde(default)]
    pub widgets: Vec<WidgetRecord>,
}

#[derive(Debug)]
pub enum WorkspaceError {
    NotFound,
    RevisionConflict { expected: i64, actual: i64 },
    InvalidInput(String),
    Io(std::io::Error),
    Sql(rusqlite::Error),
}

#[allow(dead_code)]
impl WorkspaceError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound => "WORKSPACE_NOT_FOUND",
            Self::RevisionConflict { .. } => "WORKSPACE_REVISION_CONFLICT",
            Self::InvalidInput(_) => "WORKSPACE_INVALID_INPUT",
            Self::Io(_) => "WORKSPACE_IO",
            Self::Sql(_) => "WORKSPACE_INTERNAL",
        }
    }

    pub fn actual_revision(&self) -> Option<i64> {
        match self {
            Self::RevisionConflict { actual, .. } => Some(*actual),
            _ => None,
        }
    }

    pub fn to_error_json(&self) -> Value {
        let mut output = json!({
            "code": self.code(),
            "message": self.to_string(),
        });
        if let Some(revision) = self.actual_revision() {
            output["actualRevision"] = json!(revision);
        }
        output
    }
}

impl std::fmt::Display for WorkspaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "workspace not found"),
            Self::RevisionConflict { expected, actual } => {
                write!(f, "revision conflict: expected {expected}, actual {actual}")
            }
            Self::InvalidInput(message) => write!(f, "invalid input: {message}"),
            Self::Io(error) => write!(f, "io error: {error}"),
            Self::Sql(error) => write!(f, "sql error: {error}"),
        }
    }
}

impl std::error::Error for WorkspaceError {}

impl From<rusqlite::Error> for WorkspaceError {
    fn from(error: rusqlite::Error) -> Self {
        match error {
            rusqlite::Error::QueryReturnedNoRows => Self::NotFound,
            other => Self::Sql(other),
        }
    }
}

pub type StoreResult<T> = Result<T, WorkspaceError>;
