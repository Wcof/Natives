//! Provider adapter implementations for six launch providers.

pub mod anthropic;
pub mod deepseek;
pub mod gemini;
pub mod ollama;
pub mod openai;
pub mod openai_codex;
pub mod openai_compatible;

use crate::capabilities::ProviderAdapter;

/// Register all built-in provider adapters.
pub fn register_all() -> Vec<Box<dyn ProviderAdapter>> {
    vec![
        Box::new(openai::OpenAiAdapter::new()),
        Box::new(anthropic::AnthropicAdapter::new()),
        Box::new(gemini::GeminiAdapter::new()),
        Box::new(deepseek::DeepSeekAdapter::new()),
        Box::new(openai_compatible::OpenAiCompatibleAdapter::new()),
        Box::new(ollama::OllamaAdapter::new()),
    ]
}

/// Error for provider not found.
#[derive(Debug)]
pub struct ProviderNotFoundError;

impl std::fmt::Display for ProviderNotFoundError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Provider not found")
    }
}

impl std::error::Error for ProviderNotFoundError {}
