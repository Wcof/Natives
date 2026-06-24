use serde::{Deserialize, Serialize};

/// Token estimation and context window management for AI conversations.
/// Handles budget calculation, message trimming, and summary generation.

// Rough token estimation: ~4 characters per token for English, ~1.5 for CJK
const CHARS_PER_TOKEN_EN: usize = 4;
const CHARS_PER_TOKEN_CJK: usize = 2;
const DEFAULT_OUTPUT_RESERVE: usize = 4096;
const MAX_FILE_INJECTION_BYTES: usize = 65536; // 64KB

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrimResult {
    pub messages: Vec<ChatMessage>,
    pub trimmed_count: usize,
    pub total_tokens: usize,
}

/// Estimate the number of tokens in a text string.
/// Uses a simple heuristic: CJK characters count as ~2 chars/token, others ~4.
pub fn estimate_tokens(text: &str) -> usize {
    let mut cjk_chars = 0;
    let mut other_chars = 0;

    for c in text.chars() {
        if c >= '\u{4E00}' && c <= '\u{9FFF}' || c >= '\u{3000}' && c <= '\u{303F}' {
            cjk_chars += 1;
        } else if !c.is_whitespace() {
            other_chars += 1;
        }
    }

    cjk_chars / CHARS_PER_TOKEN_CJK + other_chars / CHARS_PER_TOKEN_EN + 1
}

/// Trim messages to fit within the context window budget.
/// Uses reverse iteration (most recent first) and inserts a summary when trimming.
pub fn trim_messages(
    messages: &[ChatMessage],
    system_prompt: &str,
    summary: &str,
    context_limit: usize,
) -> TrimResult {
    let system_tokens = estimate_tokens(system_prompt);
    let output_reserve = DEFAULT_OUTPUT_RESERVE;

    let budget = context_limit
        .saturating_sub(system_tokens)
        .saturating_sub(output_reserve);

    if budget == 0 {
        return TrimResult {
            messages: vec![],
            trimmed_count: messages.len(),
            total_tokens: 0,
        };
    }

    let mut result = Vec::new();
    let mut used = 0;
    let mut trimmed = 0;

    for msg in messages.iter().rev() {
        let t = estimate_tokens(&msg.content);
        if used + t > budget {
            // Insert summary for trimmed content
            if !summary.is_empty() {
                result.insert(0, ChatMessage {
                    role: "system".to_string(),
                    content: format!("[Earlier conversation summary]: {}", summary),
                });
            }
            trimmed += 1;
            continue;
        }
        result.insert(0, msg.clone());
        used += t;
    }

    TrimResult {
        messages: result,
        trimmed_count: trimmed,
        total_tokens: used + system_tokens,
    }
}

/// Get a truncated version of file content for context injection.
/// Returns the content truncated to MAX_FILE_INJECTION_BYTES with a [truncated] marker.
pub fn truncate_file_content(content: &str) -> String {
    if content.len() <= MAX_FILE_INJECTION_BYTES {
        return content.to_string();
    }

    let truncated: String = content.chars().take(MAX_FILE_INJECTION_BYTES).collect();
    format!(
        "{}\n\n---\n[truncated] File exceeds {}KB. Only the first {}KB are shown.",
        truncated,
        MAX_FILE_INJECTION_BYTES / 1024,
        MAX_FILE_INJECTION_BYTES / 1024
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_tokens_english() {
        let tokens = estimate_tokens("Hello, how are you today?");
        assert!(tokens > 0);
        assert!(tokens < 20);
    }

    #[test]
    fn test_estimate_tokens_cjk() {
        let tokens = estimate_tokens("你好，今天怎么样？");
        assert!(tokens > 0);
        assert!(tokens < 20);
    }

    #[test]
    fn test_trim_messages_within_budget() {
        let messages = vec![
            ChatMessage { role: "user".to_string(), content: "Hello".to_string() },
            ChatMessage { role: "assistant".to_string(), content: "Hi there!".to_string() },
        ];
        let result = trim_messages(&messages, "System prompt", "", 32000);
        assert_eq!(result.messages.len(), 2);
        assert_eq!(result.trimmed_count, 0);
    }

    #[test]
    fn test_trim_messages_exceeds_budget() {
        let messages: Vec<ChatMessage> = (0..100).map(|i| ChatMessage {
            role: "user".to_string(),
            content: format!("Message number {}", i),
        }).collect();
        let result = trim_messages(&messages, "System", "Summary of earlier messages", 1024);
        assert!(result.messages.len() < 100);
        assert!(result.trimmed_count > 0);
    }

    #[test]
    fn test_trim_messages_zero_budget() {
        let messages = vec![
            ChatMessage { role: "user".to_string(), content: "Test".to_string() },
        ];
        let result = trim_messages(&messages, "", "", 0);
        assert!(result.messages.is_empty());
    }

    #[test]
    fn test_truncate_file_content_within_limit() {
        let content = "small file content";
        let result = truncate_file_content(content);
        assert_eq!(result, content);
    }

    #[test]
    fn test_truncate_file_content_exceeds_limit() {
        let content = "a".repeat(MAX_FILE_INJECTION_BYTES + 100);
        let result = truncate_file_content(&content);
        assert!(result.contains("[truncated]"));
        assert!(result.len() < content.len());
    }

    #[test]
    fn test_empty_messages() {
        let result = trim_messages(&[], "System", "", 32000);
        assert!(result.messages.is_empty());
        assert_eq!(result.trimmed_count, 0);
    }

    #[test]
    fn test_summary_injection() {
        // Use messages that exceed the budget (budget ≈ 902 tokens after system + reserve)
        // Each 5000 char message ≈ 1250 tokens, two exceed 902
        let messages = vec![
            ChatMessage { role: "user".to_string(), content: "A".repeat(5000) },
            ChatMessage { role: "user".to_string(), content: "B".repeat(5000) },
        ];
        let result = trim_messages(&messages, "System", "Summary of earlier messages", 5000);
        assert!(result.messages.iter().any(|m| m.content.contains("Summary")),
            "Summary should be injected when messages exceed budget");
        assert!(result.trimmed_count > 0, "Expected some messages to be trimmed");
    }
}
