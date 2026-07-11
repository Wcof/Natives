use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

/// A permission request awaiting user response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRequest {
    pub id: String,
    pub run_id: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub reason: String,
    pub input: serde_json::Value,
    pub status: PermissionStatus,
    pub created_at: DateTime<Utc>,
    pub responded_at: Option<DateTime<Utc>>,
}

/// Status of a permission request.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PermissionStatus {
    Pending,
    Approved,
    Rejected,
    Expired,
}

/// Response to a permission request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionResponse {
    pub request_id: String,
    pub approved: bool,
    /// Scope of the permission grant.
    pub scope: PermissionScope,
}

/// How long a permission grant lasts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PermissionScope {
    /// Allow this one time only.
    Once,
    /// Allow for the current run.
    ThisRun,
    /// Remember for this project.
    Project,
    /// Always allow.
    Forever,
}

impl fmt::Display for PermissionScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PermissionScope::Once => write!(f, "once"),
            PermissionScope::ThisRun => write!(f, "this_run"),
            PermissionScope::Project => write!(f, "project"),
            PermissionScope::Forever => write!(f, "forever"),
        }
    }
}