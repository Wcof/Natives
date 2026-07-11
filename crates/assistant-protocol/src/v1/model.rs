use serde::{Deserialize, Serialize};
use super::provider::ModelCapabilities;

/// A model and its capabilities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub provider_id: String,
    pub display_name: Option<String>,
    pub capabilities: ModelCapabilities,
    pub context_window: u64,
    pub max_output: u64,
    pub pricing: Option<ModelPricing>,
}

/// Model pricing information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPricing {
    pub input_per_million_tokens: f64,
    pub output_per_million_tokens: f64,
    pub currency: String,
}

/// Query for model capabilities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListCapabilitiesQuery {
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub require_capability: Option<String>,
}