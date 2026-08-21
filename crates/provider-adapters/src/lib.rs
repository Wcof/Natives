//! # Provider Adapters
//!
//! Unified interface for LLM provider implementations.
//! Each adapter implements the `ProviderAdapter` trait and declares
//! its capabilities via `ProviderCapabilities`.

pub mod adapter;
pub mod capabilities;
pub mod capabilities_history;
pub mod controls;
pub mod http_client;
pub mod http_stream;
pub mod model_profile;
pub mod protocol_resolver;
pub mod provider_identity;
pub mod providers;
pub mod redact;
pub mod stream;

pub use adapter::{contract_tests, ProviderAdapter, ProviderStreamEvent};
pub use capabilities::*;
pub use controls::{
    parse_bool_flag, ReasoningEffort, ReasoningRequest, RequestControls, ToolChoice,
    PROMPT_CACHE_ENV,
};
pub use model_profile::{ModelFamily, ModelProfile, PromptCacheMode, ReasoningControl};
pub use protocol_resolver::{
    candidate_protocols, parse_protocol, should_retry_next_candidate, Protocol, ProtocolContext,
};
pub use provider_identity::{ModelCapabilities, ProviderType};
pub use providers::register_all;
pub use redact::redact_secrets;
pub use stream::ProviderEvent;
