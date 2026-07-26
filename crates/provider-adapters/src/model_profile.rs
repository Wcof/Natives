//! Per-model request-side capability profile.
//!
//! [`ProviderCapabilities`](crate::capabilities::ProviderCapabilities) answers
//! "what can this vendor do". This module answers the narrower question the
//! body builders actually need: **what can *this model id* accept in a request** —
//! its output ceiling, its prompt-cache mechanism, its reasoning knob, and
//! whether it still accepts sampling parameters.
//!
//! # Honesty contract
//!
//! This table is a **hand-maintained snapshot of vendor documentation**, not a
//! live capability feed. It is deliberately built so that being wrong fails
//! *safe*:
//!
//! - An unrecognised model resolves to [`ModelProfile::unknown`] for its family,
//!   which carries `max_output_tokens: None` — the adapters then keep whatever
//!   the caller asked for (or omit the field entirely), i.e. exactly today's
//!   behaviour. New models therefore never regress; they simply do not get the
//!   raised ceiling until a row is added here.
//! - Ceilings are only ever used to **clamp down** an explicit caller value,
//!   never to raise one.
//! - Every capability flag is set from documented vendor behaviour. Where a
//!   value could not be confirmed, the conservative option was chosen and is
//!   marked with a `CONSERVATIVE:` comment on the row.
//!
//! The long-term source of truth is provider model discovery
//! ([`ProviderAdapter::discover_models`](crate::capabilities::ProviderAdapter::discover_models)
//! already returns [`ModelInfo::max_output`](crate::capabilities::ModelInfo)).
//! This table is the offline fallback for the request path, which cannot make a
//! discovery round-trip per request.

use serde::{Deserialize, Serialize};

/// Vendor wire dialect a model id belongs to.
///
/// Resolved even when the exact model row is unknown, so family-level defaults
/// (for example "every Claude model accepts `cache_control`") still apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelFamily {
    Anthropic,
    OpenAi,
    Gemini,
    DeepSeek,
    /// Self-hosted or third-party model behind an OpenAI-compatible endpoint.
    Unknown,
}

/// How (or whether) prompt caching is requested for a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptCacheMode {
    /// No prompt cache. Nothing to send, nothing to read back.
    None,
    /// Caller marks breakpoints with `cache_control: {"type": "ephemeral"}`.
    /// Anthropic Messages API.
    ExplicitBreakpoints,
    /// Provider caches the longest common prefix on its own. There is no
    /// request-side parameter; usage is still reported in the response.
    /// OpenAI (`prompt_tokens_details.cached_tokens`), DeepSeek
    /// (`prompt_cache_hit_tokens`), Gemini implicit caching
    /// (`usageMetadata.cachedContentTokenCount`).
    AutomaticPrefix,
}

/// Request-side reasoning/thinking control a model accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningControl {
    /// No request-side reasoning knob at all.
    None,
    /// Anthropic `thinking: {"type": "enabled", "budget_tokens": N}`.
    /// `budget_tokens` must be strictly less than `max_tokens`.
    AnthropicBudget,
    /// Anthropic `thinking: {"type": "adaptive"}`. `budget_tokens` is rejected
    /// with a 400 on these models.
    AnthropicAdaptive,
    /// OpenAI `reasoning_effort` (chat completions) / `reasoning: {effort}`
    /// (responses).
    OpenAiEffort,
    /// Gemini `generationConfig.thinkingConfig.thinkingBudget`.
    GeminiThinkingBudget,
    /// The model always reasons and the request cannot influence it
    /// (for example `deepseek-reasoner`).
    AlwaysOnNotConfigurable,
}

/// Request-side profile for one model id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelProfile {
    pub family: ModelFamily,
    /// Total context window in tokens. `None` when unknown.
    pub context_window: Option<u64>,
    /// Maximum tokens the model may emit in one response.
    ///
    /// `None` means "not known" — callers must then pass the request through
    /// unchanged rather than inventing a ceiling.
    pub max_output_tokens: Option<u64>,
    pub prompt_cache: PromptCacheMode,
    /// Shortest prefix the provider will cache, in tokens. Prefixes below this
    /// are silently not cached (no error, no extra cost).
    pub prompt_cache_min_tokens: Option<u64>,
    pub reasoning: ReasoningControl,
    /// Whether `temperature` / `top_p` are still accepted. Anthropic Opus 4.7+
    /// and Sonnet 5 reject them with a 400.
    pub sampling_params: bool,
    /// `false` when this profile came from a family fallback rather than a
    /// table row.
    pub known: bool,
}

impl ModelProfile {
    /// Family-level fallback used for model ids with no table row.
    ///
    /// Deliberately carries `max_output_tokens: None` so an unrecognised model
    /// behaves exactly as it does today.
    pub fn unknown(family: ModelFamily) -> Self {
        ModelProfile {
            family,
            context_window: None,
            max_output_tokens: None,
            prompt_cache: match family {
                // Every Claude model on the Messages API accepts `cache_control`
                // breakpoints; below-minimum prefixes are a silent no-op rather
                // than an error, so this is safe for unreleased model ids.
                ModelFamily::Anthropic => PromptCacheMode::ExplicitBreakpoints,
                // CONSERVATIVE: automatic-prefix caching is not universal across
                // OpenAI-compatible endpoints, so an unknown id claims nothing.
                _ => PromptCacheMode::None,
            },
            prompt_cache_min_tokens: match family {
                ModelFamily::Anthropic => Some(1024),
                _ => None,
            },
            reasoning: ReasoningControl::None,
            sampling_params: true,
            known: false,
        }
    }

    fn anthropic(
        context_window: u64,
        max_output_tokens: u64,
        cache_min: u64,
        reasoning: ReasoningControl,
        sampling_params: bool,
    ) -> Self {
        ModelProfile {
            family: ModelFamily::Anthropic,
            context_window: Some(context_window),
            max_output_tokens: Some(max_output_tokens),
            prompt_cache: PromptCacheMode::ExplicitBreakpoints,
            prompt_cache_min_tokens: Some(cache_min),
            reasoning,
            sampling_params,
            known: true,
        }
    }

    fn simple(
        family: ModelFamily,
        context_window: u64,
        max_output_tokens: u64,
        prompt_cache: PromptCacheMode,
        reasoning: ReasoningControl,
        sampling_params: bool,
    ) -> Self {
        ModelProfile {
            family,
            context_window: Some(context_window),
            max_output_tokens: Some(max_output_tokens),
            prompt_cache,
            prompt_cache_min_tokens: match prompt_cache {
                PromptCacheMode::AutomaticPrefix => Some(1024),
                _ => None,
            },
            reasoning,
            sampling_params,
            known: true,
        }
    }

    /// Whether the caller should emit `cache_control` breakpoints.
    pub fn wants_explicit_cache_breakpoints(&self) -> bool {
        self.prompt_cache == PromptCacheMode::ExplicitBreakpoints
    }
}

/// Strip a routing prefix (`anthropic/claude-…`, `openai/gpt-4o`) and lowercase.
///
/// Sub2API-style pools and OpenRouter-style gateways both prefix the vendor
/// onto the model id; the profile lookup must see through that.
fn normalize(model: &str) -> String {
    let trimmed = model.trim();
    let tail = trimmed.rsplit('/').next().unwrap_or(trimmed);
    tail.to_ascii_lowercase()
}

/// Detect the wire dialect from a model id.
pub fn family_of(model: &str) -> ModelFamily {
    let id = normalize(model);
    if id.starts_with("claude") {
        return ModelFamily::Anthropic;
    }
    if id.starts_with("deepseek") {
        return ModelFamily::DeepSeek;
    }
    if id.starts_with("gemini") {
        return ModelFamily::Gemini;
    }
    if id.starts_with("gpt-")
        || id.starts_with("chatgpt")
        || id.starts_with("o1")
        || id.starts_with("o3")
        || id.starts_with("o4")
        || id.starts_with("codex")
    {
        return ModelFamily::OpenAi;
    }
    ModelFamily::Unknown
}

/// Resolve the request-side profile for a model id.
///
/// Never panics and never fails: unrecognised ids fall back to
/// [`ModelProfile::unknown`] for their detected family.
pub fn resolve(model: &str) -> ModelProfile {
    use ReasoningControl::*;
    let id = normalize(model);
    let m = id.as_str();

    // --- Anthropic -------------------------------------------------------
    // Ordered most-specific first; dated suffixes match via `starts_with`.
    // Cache minimums are per-model (they are not monotonic across releases).
    if m.starts_with("claude-fable-5") || m.starts_with("claude-mythos-5") {
        return ModelProfile::anthropic(1_000_000, 128_000, 512, AnthropicAdaptive, false);
    }
    if m.starts_with("claude-mythos-preview") {
        return ModelProfile::anthropic(1_000_000, 128_000, 2048, AnthropicAdaptive, false);
    }
    if m.starts_with("claude-opus-5") {
        return ModelProfile::anthropic(1_000_000, 128_000, 512, AnthropicAdaptive, false);
    }
    if m.starts_with("claude-opus-4-8") {
        return ModelProfile::anthropic(1_000_000, 128_000, 1024, AnthropicAdaptive, false);
    }
    if m.starts_with("claude-opus-4-7") {
        return ModelProfile::anthropic(1_000_000, 128_000, 2048, AnthropicAdaptive, false);
    }
    if m.starts_with("claude-opus-4-6") {
        return ModelProfile::anthropic(1_000_000, 128_000, 4096, AnthropicAdaptive, true);
    }
    if m.starts_with("claude-opus-4-5") {
        return ModelProfile::anthropic(200_000, 64_000, 4096, AnthropicBudget, true);
    }
    if m.starts_with("claude-opus-4-1") || m.starts_with("claude-opus-4") {
        return ModelProfile::anthropic(200_000, 32_000, 1024, AnthropicBudget, true);
    }
    if m.starts_with("claude-sonnet-5") {
        return ModelProfile::anthropic(1_000_000, 128_000, 1024, AnthropicAdaptive, false);
    }
    if m.starts_with("claude-sonnet-4-6") {
        return ModelProfile::anthropic(1_000_000, 128_000, 1024, AnthropicAdaptive, true);
    }
    if m.starts_with("claude-sonnet-4-5") || m.starts_with("claude-sonnet-4") {
        return ModelProfile::anthropic(200_000, 64_000, 1024, AnthropicBudget, true);
    }
    if m.starts_with("claude-haiku-4-5") {
        return ModelProfile::anthropic(200_000, 64_000, 4096, AnthropicBudget, true);
    }
    if m.starts_with("claude-3-7-sonnet") {
        return ModelProfile::anthropic(200_000, 64_000, 1024, AnthropicBudget, true);
    }
    if m.starts_with("claude-3-5-sonnet") {
        return ModelProfile::anthropic(200_000, 8_192, 1024, None, true);
    }
    if m.starts_with("claude-3-5-haiku") {
        return ModelProfile::anthropic(200_000, 8_192, 2048, None, true);
    }
    if m.starts_with("claude-3-opus")
        || m.starts_with("claude-3-sonnet")
        || m.starts_with("claude-3-haiku")
    {
        return ModelProfile::anthropic(200_000, 4_096, 2048, None, true);
    }

    // --- OpenAI ----------------------------------------------------------
    // `gpt-5` and the o-series reject `temperature`/`top_p` other than the
    // default, so `sampling_params` is false for them.
    if m.starts_with("gpt-5") {
        return ModelProfile::simple(
            ModelFamily::OpenAi,
            400_000,
            128_000,
            PromptCacheMode::AutomaticPrefix,
            OpenAiEffort,
            false,
        );
    }
    if m.starts_with("o4-mini") || m.starts_with("o3") {
        return ModelProfile::simple(
            ModelFamily::OpenAi,
            200_000,
            100_000,
            PromptCacheMode::AutomaticPrefix,
            OpenAiEffort,
            false,
        );
    }
    if m.starts_with("o1-mini") {
        return ModelProfile::simple(
            ModelFamily::OpenAi,
            128_000,
            65_536,
            PromptCacheMode::AutomaticPrefix,
            None,
            false,
        );
    }
    if m.starts_with("o1-preview") {
        return ModelProfile::simple(
            ModelFamily::OpenAi,
            128_000,
            32_768,
            PromptCacheMode::AutomaticPrefix,
            None,
            false,
        );
    }
    if m.starts_with("o1") {
        return ModelProfile::simple(
            ModelFamily::OpenAi,
            200_000,
            100_000,
            PromptCacheMode::AutomaticPrefix,
            OpenAiEffort,
            false,
        );
    }
    if m.starts_with("gpt-4.1") {
        return ModelProfile::simple(
            ModelFamily::OpenAi,
            1_047_576,
            32_768,
            PromptCacheMode::AutomaticPrefix,
            None,
            true,
        );
    }
    if m.starts_with("gpt-4o") || m.starts_with("chatgpt-4o") {
        return ModelProfile::simple(
            ModelFamily::OpenAi,
            128_000,
            16_384,
            PromptCacheMode::AutomaticPrefix,
            None,
            true,
        );
    }
    if m.starts_with("gpt-4-turbo") {
        return ModelProfile::simple(
            ModelFamily::OpenAi,
            128_000,
            4_096,
            PromptCacheMode::None,
            None,
            true,
        );
    }
    if m.starts_with("gpt-4") {
        return ModelProfile::simple(
            ModelFamily::OpenAi,
            8_192,
            4_096,
            PromptCacheMode::None,
            None,
            true,
        );
    }
    if m.starts_with("gpt-3.5-turbo") {
        return ModelProfile::simple(
            ModelFamily::OpenAi,
            16_385,
            4_096,
            PromptCacheMode::None,
            None,
            true,
        );
    }

    // --- Gemini ----------------------------------------------------------
    if m.starts_with("gemini-2.5-pro") {
        return ModelProfile::simple(
            ModelFamily::Gemini,
            1_048_576,
            65_536,
            PromptCacheMode::AutomaticPrefix,
            GeminiThinkingBudget,
            true,
        );
    }
    if m.starts_with("gemini-2.5-flash") {
        return ModelProfile::simple(
            ModelFamily::Gemini,
            1_048_576,
            65_536,
            PromptCacheMode::AutomaticPrefix,
            GeminiThinkingBudget,
            true,
        );
    }
    if m.starts_with("gemini-2.0-flash") {
        return ModelProfile::simple(
            ModelFamily::Gemini,
            1_048_576,
            8_192,
            PromptCacheMode::None,
            None,
            true,
        );
    }
    if m.starts_with("gemini-1.5-pro") {
        return ModelProfile::simple(
            ModelFamily::Gemini,
            2_097_152,
            8_192,
            PromptCacheMode::None,
            None,
            true,
        );
    }
    if m.starts_with("gemini-1.5-flash") {
        return ModelProfile::simple(
            ModelFamily::Gemini,
            1_048_576,
            8_192,
            PromptCacheMode::None,
            None,
            true,
        );
    }

    // --- DeepSeek --------------------------------------------------------
    // CONSERVATIVE: both rows keep the 8_192 output ceiling already published by
    // `DeepSeekAdapter::list_models`. DeepSeek documents a higher maximum for
    // the reasoner, but an over-large `max_tokens` is a hard 400, so the
    // ceiling is only raised once it can be confirmed against the live API.
    if m.starts_with("deepseek-reasoner") {
        return ModelProfile::simple(
            ModelFamily::DeepSeek,
            64_000,
            8_192,
            PromptCacheMode::AutomaticPrefix,
            AlwaysOnNotConfigurable,
            true,
        );
    }
    if m.starts_with("deepseek") {
        return ModelProfile::simple(
            ModelFamily::DeepSeek,
            64_000,
            8_192,
            PromptCacheMode::AutomaticPrefix,
            None,
            true,
        );
    }

    ModelProfile::unknown(family_of(model))
}

/// Resolve the effective output-token ceiling for a request.
///
/// Rules, in order:
/// 1. An explicit caller value is honoured, clamped down to the model ceiling
///    when one is known. It is **never** raised.
/// 2. With no caller value, the model ceiling is used when known.
/// 3. Otherwise `None` — the adapter must then omit the field (or apply its own
///    protocol-mandated fallback), preserving today's behaviour.
pub fn resolve_max_output(requested: Option<u64>, profile: &ModelProfile) -> Option<u64> {
    match (requested, profile.max_output_tokens) {
        (Some(asked), Some(ceiling)) => Some(asked.min(ceiling)),
        (Some(asked), None) => Some(asked),
        (None, ceiling) => ceiling,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_known_anthropic_ceilings_and_cache_mode() {
        let sonnet = resolve("claude-sonnet-4-5-20250929");
        assert!(sonnet.known);
        assert_eq!(sonnet.max_output_tokens, Some(64_000));
        assert_eq!(sonnet.prompt_cache, PromptCacheMode::ExplicitBreakpoints);
        assert_eq!(sonnet.reasoning, ReasoningControl::AnthropicBudget);
        assert!(sonnet.sampling_params);

        let opus5 = resolve("claude-opus-5");
        assert_eq!(opus5.max_output_tokens, Some(128_000));
        assert_eq!(opus5.reasoning, ReasoningControl::AnthropicAdaptive);
        assert!(
            !opus5.sampling_params,
            "Opus 5 rejects temperature/top_p; the profile must say so"
        );
        assert_eq!(opus5.prompt_cache_min_tokens, Some(512));
    }

    #[test]
    fn sees_through_gateway_model_prefixes() {
        assert_eq!(
            resolve("anthropic/claude-sonnet-4-5").max_output_tokens,
            Some(64_000)
        );
        assert_eq!(family_of("openai/gpt-4o-mini"), ModelFamily::OpenAi);
        assert_eq!(family_of("google/gemini-2.5-pro"), ModelFamily::Gemini);
    }

    #[test]
    fn unknown_model_stays_conservative() {
        let profile = resolve("qwen-max-2099");
        assert!(!profile.known);
        assert_eq!(profile.family, ModelFamily::Unknown);
        assert_eq!(
            profile.max_output_tokens, None,
            "an unknown model must not get an invented ceiling"
        );
        assert_eq!(profile.prompt_cache, PromptCacheMode::None);
        assert_eq!(profile.reasoning, ReasoningControl::None);
    }

    #[test]
    fn unknown_claude_keeps_family_cache_support() {
        let profile = resolve("claude-quintuple-9");
        assert!(!profile.known);
        assert_eq!(profile.family, ModelFamily::Anthropic);
        assert_eq!(profile.max_output_tokens, None);
        assert!(profile.wants_explicit_cache_breakpoints());
    }

    #[test]
    fn max_output_clamps_down_but_never_up() {
        let sonnet = resolve("claude-sonnet-4-5");
        // Caller asked for less than the ceiling: honour the caller.
        assert_eq!(resolve_max_output(Some(256), &sonnet), Some(256));
        // Caller asked for more than the ceiling: clamp to the ceiling.
        assert_eq!(resolve_max_output(Some(999_999), &sonnet), Some(64_000));
        // No caller value: use the ceiling instead of a hardcoded 4096.
        assert_eq!(resolve_max_output(None, &sonnet), Some(64_000));

        let unknown = resolve("some-local-model");
        assert_eq!(resolve_max_output(Some(4096), &unknown), Some(4096));
        assert_eq!(resolve_max_output(None, &unknown), None);
    }

    #[test]
    fn openai_reasoning_models_drop_sampling_params() {
        for model in ["o1", "o3-mini", "o4-mini", "gpt-5"] {
            let profile = resolve(model);
            assert!(
                !profile.sampling_params,
                "{model} rejects non-default temperature"
            );
            assert_eq!(profile.family, ModelFamily::OpenAi);
        }
        assert!(resolve("gpt-4o").sampling_params);
    }

    #[test]
    fn automatic_prefix_providers_take_no_request_parameter() {
        for model in ["gpt-4o", "deepseek-chat", "gemini-2.5-pro"] {
            let profile = resolve(model);
            assert_eq!(profile.prompt_cache, PromptCacheMode::AutomaticPrefix);
            assert!(
                !profile.wants_explicit_cache_breakpoints(),
                "{model} must not receive cache_control breakpoints"
            );
        }
    }
}
