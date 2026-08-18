//! Shared HTTP streaming helper for OpenAI-compatible chat completions.
//!
//! Responsibilities are split:
//! - `body` — chat-completions / Responses body builders and message shaping.
//! - `transport` — streaming and non-streaming HTTP calls plus status helpers.
//! - `http_stream_tests.rs` — request-body unit tests.

mod body;
mod transport;

pub use body::{
    build_chat_completions_body, build_chat_completions_body_with_controls, build_responses_body,
    build_responses_body_with_controls, message_to_json,
};
pub(crate) use transport::transport_error;
pub use transport::{
    chat_completions, map_http_status, retry_after_ms, stream_chat_completions, stream_responses,
    stream_responses_with_headers,
};

#[cfg(test)]
#[path = "http_stream_tests.rs"]
mod tool_message_tests;
