//! # Provider Adapters
//!
//! Unified interface for LLM provider implementations.
//! Each adapter implements the `ProviderAdapter` trait and declares
//! its capabilities via `ProviderCapabilities`.

pub mod capabilities;
pub mod capabilities_history;
pub mod http_client;
pub mod http_stream;
pub mod model_profile;
pub mod protocol_resolver;
pub mod providers;
pub mod stream;

pub use capabilities::*;
pub use model_profile::{ModelFamily, ModelProfile, PromptCacheMode, ReasoningControl};
pub use protocol_resolver::{
    candidate_protocols, parse_protocol, should_retry_next_candidate, Protocol, ProtocolContext,
};
pub use providers::register_all;
pub use stream::ProviderEvent;
