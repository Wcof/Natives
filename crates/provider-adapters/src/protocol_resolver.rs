//! ProtocolResolver — 唯一协议解析器（问题 8）
//!
//! 为当前上游决定 Chat Completions / Responses / Anthropic Messages /
//! Gemini / Ollama 的候选协议顺序。决策优先级：
//! 强供应商/认证约束 → 最近成功协议（缓存）→ endpoint/metadata → 模型族 →
//! 供应商保守默认。
//!
//! 纯逻辑、无 IO、无密钥。Renderer 不做协议转换；Daemon/provider-adapters
//! 内部继续使用 canonical `ProviderRequest/ProviderEvent`。

/// 协议标识（与 Host `normalize_api_protocol` 的规范名对齐）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Protocol {
    OpenAiChatCompletions,
    OpenAiResponses,
    AnthropicMessages,
    GeminiGenerateContent,
    OllamaChat,
    OpenAiCompatible,
}

impl Protocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Protocol::OpenAiChatCompletions => "openai_chat_completions",
            Protocol::OpenAiResponses => "openai_responses",
            Protocol::AnthropicMessages => "anthropic_messages",
            Protocol::GeminiGenerateContent => "gemini_generate_content",
            Protocol::OllamaChat => "ollama_chat",
            Protocol::OpenAiCompatible => "openai_compatible",
        }
    }
}

/// 协议解析输入（全部来自既有 ProviderConfig / run 上下文，不含密钥）。
pub struct ProtocolContext<'a> {
    /// 供应商类型（openai / anthropic / gemini / deepseek / compatible / ollama）。
    pub provider_type: &'a str,
    /// base URL（endpoint/metadata 启发用）。
    pub base_url: Option<&'a str>,
    /// 显式配置的协议（开关关闭时原样沿用）。
    pub explicit: Option<&'a str>,
    /// 模型名（模型族启发）。
    pub model: &'a str,
    /// 该 provider+baseURL+model 最近一次成功的协议（缓存，不含密钥）。
    pub previous_success: Option<&'a str>,
}

/// 候选协议顺序。开关关闭（无 explicit 且 routing disabled）时调用方应直接
/// 用供应商默认；本函数只生成「自动路由」的候选。
pub fn candidate_protocols(ctx: &ProtocolContext<'_>) -> Vec<Protocol> {
    // 1) 强供应商/认证约束：explicit 配置优先。
    if let Some(explicit) = ctx.explicit {
        if let Some(p) = parse_protocol(explicit) {
            return vec![p];
        }
    }

    // 2) 最近成功协议缓存（该 provider+baseURL+model 已证明可用）。
    let cached = ctx.previous_success.and_then(parse_protocol);

    // 3) endpoint/metadata：URL 关键字启发。
    let url_hint = ctx
        .base_url
        .map(|u| u.to_ascii_lowercase())
        .unwrap_or_default();
    let url_suggests_responses = url_hint.contains("responses");
    let url_suggests_anthropic = url_hint.contains("anthropic") || url_hint.contains("claude");
    let url_suggests_gemini = url_hint.contains("gemini") || url_hint.contains("googleapis");
    let url_suggests_ollama = url_hint.contains("ollama");

    // 4) 模型族启发。
    let model = ctx.model.to_ascii_lowercase();
    let model_suggests_responses = model.contains("o1")
        || model.contains("o3")
        || model.contains("o4")
        || model.starts_with("gpt-5")
        || model.contains("responses");

    // 5) 供应商保守默认。
    let provider = ctx.provider_type.trim().to_ascii_lowercase();
    let mut candidates: Vec<Protocol> = Vec::new();
    let mut push = |p: Protocol| {
        if !candidates.contains(&p) {
            candidates.push(p);
        }
    };

    if provider.contains("anthropic") || provider.contains("claude") || url_suggests_anthropic {
        push(Protocol::AnthropicMessages);
        push(Protocol::OpenAiChatCompletions);
        return candidates;
    }
    if provider.contains("gemini") || provider.contains("google") || url_suggests_gemini {
        push(Protocol::GeminiGenerateContent);
        push(Protocol::OpenAiChatCompletions);
        return candidates;
    }
    if provider.contains("ollama") || url_suggests_ollama {
        push(Protocol::OllamaChat);
        push(Protocol::OpenAiChatCompletions);
        return candidates;
    }
    // openai / openai-compatible / deepseek 等 OpenAI 协议族。
    if let Some(c) = cached {
        push(c);
    }
    if url_suggests_responses || model_suggests_responses {
        push(Protocol::OpenAiResponses);
        push(Protocol::OpenAiChatCompletions);
    } else {
        push(Protocol::OpenAiChatCompletions);
        push(Protocol::OpenAiResponses);
    }
    if provider.contains("compatible") && !candidates.contains(&Protocol::OpenAiCompatible) {
        candidates.push(Protocol::OpenAiCompatible);
    }
    candidates
}

/// 把协议规范名解析为 [`Protocol`]（未知 → None）。
pub fn parse_protocol(value: &str) -> Option<Protocol> {
    match value.trim().to_ascii_lowercase().as_str() {
        "openai" | "openai_compatible" | "openai-compatible" | "openai_chat_completions" => {
            Some(Protocol::OpenAiChatCompletions)
        }
        "openai_responses" | "responses" => Some(Protocol::OpenAiResponses),
        "anthropic" | "claude" | "anthropic-native" | "anthropic_messages" => {
            Some(Protocol::AnthropicMessages)
        }
        "gemini" | "google" | "gemini_generate_content" => Some(Protocol::GeminiGenerateContent),
        "ollama" | "ollama_chat" => Some(Protocol::OllamaChat),
        _ => None,
    }
}

/// 协议回退边界（问题 8）：
/// 只在首个有效 text/reasoning/tool delta 之前，对 404/405 或协议形状不兼容
/// 才允许尝试下一候选；401/403/429、配额、权限、网络故障一律不换协议。
pub fn should_retry_next_candidate(category: &str, code: &str) -> bool {
    let lower = code.to_ascii_lowercase();
    if lower.contains("401") || lower.contains("403") || lower.contains("429") {
        return false;
    }
    match category {
        "RateLimit" | "Auth" | "Network" => false,
        "ModelNotFound" | "BadRequest" => {
            // 404/405（endpoint/协议不存在）或形状不兼容 → 可回退；
            // 其它 BadRequest（如参数非法）不换协议。
            lower.contains("404") || lower.contains("405") || lower.contains("unsupported")
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx<'a>(
        provider_type: &'a str,
        base_url: Option<&'a str>,
        explicit: Option<&'a str>,
        model: &'a str,
        previous_success: Option<&'a str>,
    ) -> ProtocolContext<'a> {
        ProtocolContext {
            provider_type,
            base_url,
            explicit,
            model,
            previous_success,
        }
    }

    #[test]
    fn explicit_protocol_wins_over_everything() {
        let c = ctx(
            "openai",
            Some("https://x.example"),
            Some("anthropic_messages"),
            "gpt-4o",
            Some("openai_responses"),
        );
        assert_eq!(candidate_protocols(&c), vec![Protocol::AnthropicMessages]);
    }

    #[test]
    fn anthropic_provider_defaults_to_messages() {
        let c = ctx("anthropic", None, None, "claude-3-5-sonnet", None);
        assert_eq!(candidate_protocols(&c)[0], Protocol::AnthropicMessages);
    }

    #[test]
    fn cached_success_is_second_priority() {
        let c = ctx("openai", None, None, "gpt-4o", Some("openai_responses"));
        assert_eq!(candidate_protocols(&c)[0], Protocol::OpenAiResponses);
    }

    #[test]
    fn url_hint_picks_responses() {
        let c = ctx("openai", Some("https://responses.example/v1"), None, "gpt-4o", None);
        assert_eq!(candidate_protocols(&c)[0], Protocol::OpenAiResponses);
    }

    #[test]
    fn model_family_picks_responses() {
        let c = ctx("openai", None, None, "o3-mini", None);
        assert_eq!(candidate_protocols(&c)[0], Protocol::OpenAiResponses);
    }

    #[test]
    fn conservative_default_is_chat_completions_first() {
        let c = ctx("openai", None, None, "gpt-4o", None);
        assert_eq!(candidate_protocols(&c)[0], Protocol::OpenAiChatCompletions);
    }

    #[test]
    fn retry_only_for_404_405_shape() {
        assert!(should_retry_next_candidate("BadRequest", "http_404"));
        assert!(should_retry_next_candidate("ModelNotFound", "http_405"));
        assert!(!should_retry_next_candidate("BadRequest", "http_401"));
        assert!(!should_retry_next_candidate("BadRequest", "http_403"));
        assert!(!should_retry_next_candidate("RateLimit", "http_429"));
        assert!(!should_retry_next_candidate("Network", "connect_timeout"));
        assert!(!should_retry_next_candidate("BadRequest", "http_400"));
    }
}
