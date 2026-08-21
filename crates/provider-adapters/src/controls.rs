//! Request-side controls（P0-B Extract）。
//!
//! 从 `capabilities.rs` 拆出：`ToolChoice` / `ReasoningEffort` /
//! `ReasoningRequest` / `RequestControls` 与 prompt-cache 环境开关。
//! 这些是与消息载荷正交的请求控制类型，保持与旧文件字节级兼容的 serde。

use serde::{Deserialize, Serialize};

use crate::model_profile::ModelProfile;

/// Environment kill switch for prompt-cache breakpoints.
///
/// Read on every body build (once per model turn, so the cost is noise) rather
/// than cached, because the point of an escape hatch is that it works on the
/// next request after someone sets it — including on a long-lived daemon that
/// nobody wants to restart mid-incident.
pub const PROMPT_CACHE_ENV: &str = "NATIVES_PROMPT_CACHE";

/// Parse a permissive on/off flag. Returns `None` for anything unrecognised so
/// a typo falls back to the default instead of silently disabling a feature.
pub fn parse_bool_flag(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "on" | "true" | "yes" | "enabled" => Some(true),
        "0" | "off" | "false" | "no" | "disabled" => Some(false),
        _ => None,
    }
}

fn env_prompt_cache_override() -> Option<bool> {
    parse_bool_flag(&std::env::var(PROMPT_CACHE_ENV).ok()?)
}

/// Model may call zero or more tools, possibly constrained.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum ToolChoice {
    /// Model decides freely (provider default).
    Auto,
    /// Model must not call a tool.
    None,
    /// Model must call at least one tool, its choice which.
    Required,
    /// Model must call this exact tool.
    Tool { name: String },
}

impl ToolChoice {
    /// Anthropic Messages API `tool_choice` object.
    ///
    /// `disable_parallel_tool_use` rides on the same object for Anthropic, so
    /// it is passed in here rather than emitted as a sibling field.
    pub fn to_anthropic(&self, parallel_tool_calls: Option<bool>) -> serde_json::Value {
        let mut value = match self {
            ToolChoice::Auto => serde_json::json!({ "type": "auto" }),
            ToolChoice::None => serde_json::json!({ "type": "none" }),
            ToolChoice::Required => serde_json::json!({ "type": "any" }),
            ToolChoice::Tool { name } => serde_json::json!({ "type": "tool", "name": name }),
        };
        if let Some(false) = parallel_tool_calls {
            value["disable_parallel_tool_use"] = serde_json::json!(true);
        }
        value
    }

    /// OpenAI chat-completions / Responses `tool_choice` value.
    pub fn to_openai(&self) -> serde_json::Value {
        match self {
            ToolChoice::Auto => serde_json::json!("auto"),
            ToolChoice::None => serde_json::json!("none"),
            ToolChoice::Required => serde_json::json!("required"),
            ToolChoice::Tool { name } => serde_json::json!({
                "type": "function",
                "function": { "name": name },
            }),
        }
    }

    /// Gemini `toolConfig.functionCallingConfig` object.
    pub fn to_gemini(&self) -> serde_json::Value {
        match self {
            ToolChoice::Auto => serde_json::json!({ "mode": "AUTO" }),
            ToolChoice::None => serde_json::json!({ "mode": "NONE" }),
            ToolChoice::Required => serde_json::json!({ "mode": "ANY" }),
            ToolChoice::Tool { name } => serde_json::json!({
                "mode": "ANY",
                "allowedFunctionNames": [name],
            }),
        }
    }
}

/// Coarse reasoning depth, mapped onto whatever knob the model exposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningEffort {
    Low,
    Medium,
    High,
}

impl ReasoningEffort {
    /// Parse the coarse level carried by `run.start`'s `effort` field.
    ///
    /// The protocol types it as a free-form `Option<String>` (it is
    /// provider-specific by design), so this is the single place that decides
    /// what the workbench's vocabulary means. Anything else returns `None` —
    /// an unknown level must not be rounded to a guess, because the difference
    /// between `low` and `high` is a tenfold thinking budget.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "low" | "minimal" => Some(ReasoningEffort::Low),
            "medium" | "default" | "standard" => Some(ReasoningEffort::Medium),
            "high" | "max" | "maximum" => Some(ReasoningEffort::High),
            _ => None,
        }
    }

    /// OpenAI `reasoning_effort` string.
    pub fn as_openai_str(self) -> &'static str {
        match self {
            ReasoningEffort::Low => "low",
            ReasoningEffort::Medium => "medium",
            ReasoningEffort::High => "high",
        }
    }

    /// Default thinking budget in tokens for providers that take a number
    /// instead of a level. Callers may override via
    /// [`ReasoningRequest::budget_tokens`].
    pub fn default_budget_tokens(self) -> u64 {
        match self {
            ReasoningEffort::Low => 4_096,
            ReasoningEffort::Medium => 16_384,
            ReasoningEffort::High => 32_768,
        }
    }
}

/// Caller-requested reasoning configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReasoningRequest {
    pub effort: ReasoningEffort,
    /// Explicit thinking budget. Ignored by models whose only knob is a level
    /// (`ReasoningControl::OpenAiEffort`, `ReasoningControl::AnthropicAdaptive`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_tokens: Option<u64>,
}

impl ReasoningRequest {
    pub fn new(effort: ReasoningEffort) -> Self {
        ReasoningRequest {
            effort,
            budget_tokens: None,
        }
    }

    /// Budget to send, honouring an explicit value over the effort default.
    pub fn budget(&self) -> u64 {
        self.budget_tokens
            .unwrap_or_else(|| self.effort.default_budget_tokens())
    }
}

/// Request-side controls that are orthogonal to the message payload.
///
/// [`Default`] is exactly today's behaviour: no forced tool, provider-default
/// parallelism, no reasoning parameter, and prompt caching left to the
/// per-model default (enabled wherever the model supports explicit
/// breakpoints).
///
/// This travels inside [`ProviderRequest::controls`], so every path that
/// already builds a request carries it without a second argument. The
/// `build_*_body_with_controls` functions remain as an explicit override seam
/// for callers that want to build a body with controls other than the
/// request's own (they ignore `request.controls` entirely).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestControls {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    /// `Some(false)` forces one tool call per assistant turn. `None` leaves the
    /// provider default (parallel calls allowed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parallel_tool_calls: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ReasoningRequest>,
    /// `None` = per-model default. `Some(false)` disables prompt-cache
    /// breakpoints for this request (incident escape hatch).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_cache: Option<bool>,
}

impl RequestControls {
    /// Whether this is the zero-configuration form, i.e. wire-identical to the
    /// behaviour that predates the field.
    pub fn is_default(&self) -> bool {
        *self == RequestControls::default()
    }

    /// Whether explicit prompt-cache breakpoints should be emitted for a model.
    ///
    /// Precedence, most authoritative first:
    ///
    /// 1. `NATIVES_PROMPT_CACHE` — the operator kill switch. Set it to `0` and
    ///    no request emits a breakpoint, whatever any caller asked for. This
    ///    exists so a provider-side cache incident can be worked around without
    ///    shipping a build.
    /// 2. [`RequestControls::prompt_cache`] — the per-request opt-out.
    /// 3. The per-model default (on wherever the model supports breakpoints).
    pub fn prompt_cache_enabled(&self, profile: &ModelProfile) -> bool {
        if let Some(false) = env_prompt_cache_override() {
            return false;
        }
        profile.wants_explicit_cache_breakpoints() && self.prompt_cache.unwrap_or(true)
    }

    /// Reasoning controls for a coarse effort string (`"low"`/`"medium"`/`"high"`).
    ///
    /// Unrecognised input yields `None` so an unknown level degrades to
    /// "provider default" rather than silently picking a depth for the user.
    pub fn with_effort_str(mut self, effort: Option<&str>) -> Self {
        self.reasoning = effort
            .and_then(ReasoningEffort::parse)
            .map(ReasoningRequest::new);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effort_strings_map_to_levels_and_unknown_stays_unset() {
        assert_eq!(ReasoningEffort::parse("high"), Some(ReasoningEffort::High));
        assert_eq!(ReasoningEffort::parse(" MAX "), Some(ReasoningEffort::High));
        assert_eq!(
            ReasoningEffort::parse("minimal"),
            Some(ReasoningEffort::Low)
        );
        assert_eq!(ReasoningEffort::parse("turbo"), None);
        assert_eq!(ReasoningEffort::parse(""), None);
    }

    #[test]
    fn with_effort_str_only_sets_reasoning_for_a_known_level() {
        assert_eq!(
            RequestControls::default()
                .with_effort_str(Some("low"))
                .reasoning,
            Some(ReasoningRequest::new(ReasoningEffort::Low))
        );
        // An unknown or absent level leaves the request wire-identical to one
        // that never mentioned reasoning at all.
        assert!(RequestControls::default()
            .with_effort_str(Some("wharrgarbl"))
            .is_default());
        assert!(RequestControls::default()
            .with_effort_str(None)
            .is_default());
    }

    #[test]
    fn bool_flag_parsing_ignores_typos() {
        assert_eq!(parse_bool_flag("0"), Some(false));
        assert_eq!(parse_bool_flag(" OFF "), Some(false));
        assert_eq!(parse_bool_flag("disabled"), Some(false));
        assert_eq!(parse_bool_flag("1"), Some(true));
        assert_eq!(parse_bool_flag("true"), Some(true));
        assert_eq!(parse_bool_flag("maybe"), None);
    }

    #[test]
    fn per_request_prompt_cache_opt_out_beats_the_model_default() {
        let profile = crate::model_profile::resolve("claude-sonnet-4-5");
        assert!(profile.wants_explicit_cache_breakpoints());
        assert!(RequestControls::default().prompt_cache_enabled(&profile));
        assert!(!RequestControls {
            prompt_cache: Some(false),
            ..Default::default()
        }
        .prompt_cache_enabled(&profile));
    }
}
