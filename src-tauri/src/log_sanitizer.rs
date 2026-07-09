//! log_sanitizer.rs — Runtime log sanitizer for Rust backend.
//!
//! Redacts sensitive patterns (API keys, tokens, Bearer auth headers)
//! from log messages before they reach stdout/stderr.
//!
//! Usage:
//!   info!("Provider response: {}", sanitize(&raw_body));
//!   eprintln!("[runtime] error: {}", sanitize(&error_msg));
//!
//! Best-effort regex-based redaction. New patterns should be added
//! as they are discovered in real logs.

use regex::Regex;

/// Sanitize a log message by redacting sensitive patterns.
pub fn sanitize(msg: &str) -> String {
    let s = msg.to_string();
    let s = redact_bearer(&s);
    let s = redact_api_keys(&s);
    let s = redact_long_tokens(&s);
    let s = redact_home_dir(&s);
    s
}

/// Redact `Bearer <token>` patterns.
fn redact_bearer(s: &str) -> String {
    let re = Regex::new(r"Bearer\s+([A-Za-z0-9._\-+=]{16,})").unwrap();
    re.replace_all(s, "Bearer ***").to_string()
}

/// Redact known API key patterns.
fn redact_api_keys(s: &str) -> String {
    let key_patterns: &[(Regex, &str)] = &[
        (Regex::new(r"\bsk-ant-[A-Za-z0-9_-]{8,}").unwrap(), "sk-ant-***"),
        (Regex::new(r"\bsk-[A-Za-z0-9_-]{8,}").unwrap(), "sk-***"),
        (Regex::new(r"\banthropic-[A-Za-z0-9_-]{8,}").unwrap(), "anthropic-***"),
        (Regex::new(r"\bkey-[A-Za-z0-9_-]{8,}").unwrap(), "key-***"),
        (Regex::new(r"\bghp_[A-Za-z0-9]{20,}").unwrap(), "ghp_***"),
        (Regex::new(r"\bgho_[A-Za-z0-9]{20,}").unwrap(), "gho_***"),
        (Regex::new(r"\bhf_[A-Za-z0-9]{20,}").unwrap(), "hf_***"),
        (Regex::new(r"\bxai-[A-Za-z0-9_-]{8,}").unwrap(), "xai-***"),
    ];

    let mut result = s.to_string();
    for (re, replacement) in key_patterns {
        result = re.replace_all(&result, *replacement).to_string();
    }
    result
}

/// Redact long hex/base64 tokens (32+ characters).
fn redact_long_tokens(s: &str) -> String {
    let re = Regex::new(r"\b[a-fA-F0-9]{32,}\b").unwrap();
    re.replace_all(s, |caps: &regex::Captures| {
        let m = caps.get(0).map_or("", |c| c.as_str());
        format!("{}***", &m[..8])
    }).to_string()
}

/// Redact home directory path.
fn redact_home_dir(s: &str) -> String {
    if let Ok(home) = std::env::var("HOME") {
        let escaped = regex::escape(&home);
        let re = Regex::new(&escaped).unwrap();
        re.replace_all(s, "~").to_string()
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redact_sk_key() {
        let result = sanitize("API Key: sk-proj-abc123def456ghi789jkl");
        assert!(!result.contains("sk-proj-abc123def456ghi789jkl"), "Full key should be redacted");
        assert!(result.contains("sk-***"), "Should contain mask");
    }

    #[test]
    fn test_redact_bearer() {
        let result = sanitize("Authorization: Bearer sk-ant-abcdef1234567890abcdef1234567890");
        assert!(!result.contains("Bearer sk-ant-"), "Bearer token should be redacted");
        assert!(result.contains("Bearer ***"), "Bearer should be masked");
    }

    #[test]
    fn test_redact_anthropic_key() {
        let result = sanitize("Using anthropic-key-abcdef1234567890");
        assert!(!result.contains("anthropic-key-abcdef1234567890"), "Anthropic key should be redacted");
        assert!(result.contains("anthropic-***"), "Should contain mask");
    }

    #[test]
    fn test_redact_home_dir() {
        let home = std::env::var("HOME").unwrap_or("/nonexistent".to_string());
        let result = sanitize(&format!("Path: {}/.natives/config.json", home));
        assert!(!result.contains(&home), "Home dir should be redacted");
        assert!(result.contains("~/.natives/config.json"), "Should use tilde");
    }

    #[test]
    fn test_redact_long_hex() {
        let result = sanitize("Token: abcdef1234567890abcdef1234567890abcdef12");
        assert!(result.contains("abcdef12***"), "Long hex should be truncated");
    }

    #[test]
    fn test_clean_message_unchanged() {
        let result = sanitize("File not found: /tmp/test.txt");
        assert_eq!(result, "File not found: /tmp/test.txt");
    }

    #[test]
    fn test_redact_ghp_token() {
        let result = sanitize("GitHub token: ghp_abcdef12345678901234567890");
        assert!(!result.contains("ghp_abcdef12345678901234567890"), "GitHub PAT should be redacted");
        assert!(result.contains("ghp_***"), "Should contain mask");
    }

    #[test]
    fn test_redact_hf_token() {
        let result = sanitize("HF token: hf_abcdefghijklmnopqrstuvwxyz");
        assert!(!result.contains("hf_abcdefghijklmnopqrstuvwxyz"), "HF token should be redacted");
        assert!(result.contains("hf_***"), "Should contain mask");
    }

    #[test]
    fn test_multiple_keys_in_one_line() {
        let result = sanitize("Keys: sk-a1234567890abcdef and sk-b9876543210fedcba");
        assert!(!result.contains("sk-a1234567890abcdef"), "First key redacted");
        assert!(!result.contains("sk-b9876543210fedcba"), "Second key redacted");
        assert_eq!(result.matches("sk-***").count(), 2, "Both keys masked");
    }
}
