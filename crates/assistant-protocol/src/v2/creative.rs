//! Local-creative AI analysis (P0): the Host never calls a Provider directly.
//!
//! The Host performs the sanitized project scan and payload construction, then
//! sends a [`CreativeLocalAnalyzeRequest`] over UDS to the Agent Daemon, which
//! owns Provider credentials and calls the model. The returned text is parsed
//! and validated back on the Host.

use serde::{Deserialize, Serialize};

/// What the analysis should produce.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CreativeAnalyzeKind {
    /// Propose a LaunchPlan from a sanitized project scan.
    Launch,
    /// Diagnose a start failure from a redacted log tail.
    Diagnose,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreativeLocalAnalyzeRequest {
    pub kind: CreativeAnalyzeKind,
    pub provider_id: String,
    pub model: String,
    /// System prompt telling the model what JSON to return. Never contains
    /// credentials or absolute paths.
    pub system_prompt: String,
    /// Sanitized payload (virtual `/project` root, redacted logs). Never contains
    /// secrets, credentials, or absolute paths.
    pub payload: serde_json::Value,
    #[serde(default = "default_analyze_timeout_ms")]
    pub timeout_ms: u64,
}

fn default_analyze_timeout_ms() -> u64 {
    45_000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreativeLocalAnalyzeResponse {
    /// Raw model text; the Host parses and validates it.
    pub text: String,
}

/// An agent's versioned proposal to create or start a creative app (batch 10
/// CR-1001). The Host validates this before it can become an
/// Application → Profile → Runtime → Window. Never carries secret values —
/// only env KEY names and validated driver payloads.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CreativeProposalPayload {
    pub schema_version: u32,
    /// "create" | "start" — what the agent intends to do.
    pub kind: String,
    /// Proposed ownership: "managed" | "attached" | "remote".
    pub ownership: String,
    pub title: String,
    /// Absolute project root the app would run from (Host-validated).
    pub project_root: String,
    /// Proposed driver payload, validated per kind.
    pub driver: CreativeProposedDriver,
    pub open_path: String,
    pub health_path: String,
    /// Env KEY names only — values are never carried.
    pub environment_keys: Vec<String>,
}

/// Driver payload variants a proposal can carry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CreativeProposedDriver {
    Python {
        schema_version: u32,
        interpreter: String,
        entry: String,
        args: Vec<String>,
        cwd_relative: String,
        environment_keys: Vec<String>,
        open_path: String,
        health_path: String,
        startup_timeout_ms: u32,
    },
    Binary {
        schema_version: u32,
        executable_path: String,
        executable_hash: String,
        approved: bool,
        args: Vec<String>,
        cwd_relative: String,
        environment_keys: Vec<String>,
        open_path: String,
        health_path: String,
        startup_timeout_ms: u32,
    },
    StaticHttp,
    Compose {
        command: Option<Vec<String>>,
        privileged: bool,
    },
}
