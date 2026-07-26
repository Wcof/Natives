//! # Provider Adapters
//!
//! Unified interface for LLM provider implementations.
//! Each adapter implements the `ProviderAdapter` trait and declares
//! its capabilities via `ProviderCapabilities`.

pub mod capabilities;
pub mod http_client;
pub mod http_stream;
pub mod model_profile;
pub mod providers;
pub mod stream;

pub use capabilities::*;
pub use model_profile::{ModelFamily, ModelProfile, PromptCacheMode, ReasoningControl};
pub use providers::register_all;
pub use stream::ProviderEvent;
