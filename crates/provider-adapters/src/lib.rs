//! # Provider Adapters
//!
//! Unified interface for LLM provider implementations.
//! Each adapter implements the `ProviderAdapter` trait and declares
//! its capabilities via `ProviderCapabilities`.

pub mod capabilities;
pub mod stream;
pub mod http_stream;
pub mod providers;

pub use capabilities::*;
pub use stream::ProviderEvent;
pub use providers::register_all;