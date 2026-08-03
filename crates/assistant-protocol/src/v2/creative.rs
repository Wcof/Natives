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
