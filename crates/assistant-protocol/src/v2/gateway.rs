//! Unified model access (`model.gateway.stream`, ADR-0019 P2).
//!
//! A generic single-turn model completion stream over the daemon UDS — the
//! native RPC equivalent of the loopback ingress, distinct from the full Agent
//! Run state machine. The daemon executes it through the same `RoutedProvider`
//! engine path (single routing executor); these are protocol-level
//! request/event shapes only, so `agent-core` never leaks into the wire
//! contract (R-B4).

use serde::{Deserialize, Serialize};

/// Wire version advertised in the ACK (`data.streamVersion`), mirroring
/// `run.watch`'s `stream_ack`.
pub const GATEWAY_STREAM_VERSION: u64 = 1;

/// ACK data payload of a successful `model.gateway.stream` RPC.
pub fn gateway_stream_ack() -> serde_json::Value {
    serde_json::json!({
        "stream": "model.gateway.stream",
        "streamVersion": GATEWAY_STREAM_VERSION,
    })
}

/// Request payload for `model.gateway.stream`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelGatewayStreamRequest {
    /// Model id to execute.
    pub model: String,
    /// Provider id seeding the route plan (the `load_plan` primary target).
    pub provider_id: String,
    /// Conversation history.
    #[serde(default)]
    pub messages: Vec<ModelGatewayMessage>,
    /// Optional system prompt.
    #[serde(default)]
    pub system: Option<String>,
    /// Optional concrete API-key id. When absent the router resolves the
    /// credential, including the OAuth account pool (ADR-0019 P4).
    #[serde(default)]
    pub key_id: Option<String>,
    /// Optional tool schemas in the daemon's native `ToolSchema` shape.
    #[serde(default)]
    pub tools: Vec<ModelGatewayToolSchema>,
    /// Sampling controls forwarded to the adapter.
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
}

/// One conversation message. Mirrors `agent_core::EngineMessage`'s text fast
/// path plus tool calls and images, without depending on it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelGatewayMessage {
    pub role: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub tool_call_id: Option<String>,
    #[serde(default)]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<ModelGatewayToolCall>>,
    #[serde(default)]
    pub images: Vec<ModelGatewayImage>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelGatewayToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelGatewayImage {
    pub url: String,
    #[serde(default)]
    pub media_type: Option<String>,
    #[serde(default)]
    pub detail: Option<String>,
}

/// A tool schema in the daemon's native shape (`name` / `description` /
/// `input_schema`). Separate from [`ModelGatewayToolCall`] so a request carries
/// the tool *definition*, not a historical assistant tool call.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelGatewayToolSchema {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub input_schema: serde_json::Value,
}

/// One streamed event on `model.gateway.stream` (newline-delimited JSON after
/// the RPC ACK). Mirrors `agent_core::EngineProviderEvent` without depending on
/// it; the daemon is the only translator.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ModelGatewayStreamEvent {
    TextDelta {
        text: String,
    },
    ReasoningDelta {
        text: String,
    },
    ToolCallDelta {
        index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments_delta: String,
    },
    Usage {
        input_tokens: u64,
        output_tokens: u64,
        reasoning_tokens: Option<u64>,
        cache_creation_tokens: Option<u64>,
        cache_read_tokens: Option<u64>,
    },
    Completed {
        reason: String,
    },
    Error {
        message: String,
        code: String,
        retryable: bool,
        category: String,
        retry_after_ms: Option<u64>,
    },
}
