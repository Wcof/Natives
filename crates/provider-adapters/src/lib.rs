//! # Provider Adapters
//!
//! Unified interface for LLM provider implementations.
//! Each adapter implements the `ProviderAdapter` trait and declares
//! its capabilities via `ProviderCapabilities`.

pub mod capabilities;
pub mod http_client;
pub mod http_stream;
pub mod providers;
pub mod stream;

pub use capabilities::*;
pub use providers::register_all;
pub use stream::ProviderEvent;
