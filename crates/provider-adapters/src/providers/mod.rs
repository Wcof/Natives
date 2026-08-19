//! Provider adapter implementations for six launch providers.

pub mod anthropic;
pub mod antigravity;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advertised_capabilities_match_request_support() {
        for adapter in register_all() {
            let caps = adapter.capabilities();
            for (feature, enabled) in [
                ("streaming", caps.streaming),
                ("tool_calls", caps.tool_calls),
                ("structured_output", caps.structured_output),
                ("image_input", caps.image_input),
                ("file_input", caps.file_input),
                ("reasoning", caps.reasoning),
                ("system_prompt", caps.system_prompt),
                ("function_calling", caps.function_calling),
            ] {
                assert_eq!(
                    caps.features.iter().any(|item| item == feature),
                    enabled,
                    "{:?} capability {feature} disagrees with its feature list",
                    caps.provider_type
                );
            }
        }
    }
}
