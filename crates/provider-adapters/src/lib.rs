//! # Provider Adapters
//!
//! Unified interface for LLM provider implementations.
//! Each adapter implements the `ProviderAdapter` trait and declares
//! its capabilities via `ProviderCapabilities`.

pub mod capabilities;

pub use capabilities::*;