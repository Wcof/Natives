//! 日志/传输脱敏工具（P0-B Extract）。
//!
//! 从 `assistant_protocol::v2::run_event::redact_secrets` 本地化，
//! 行为与旧实现一致（字节级兼容）；解除 provider-adapters 对
//! `assistant-protocol` 的依赖。Host 侧 `log_sanitizer.rs` 仍是更广的
//! 日志脱敏权威，本模块只覆盖传输层需要的子集。

/// 对含密钥/凭证的文本做脱敏（Authorization/Bearer/api_key/sk- 前缀）。
pub fn redact_secrets(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let rest = &input[i..];
        let lower = rest.to_ascii_lowercase();
        let redact_from = if lower.starts_with("authorization:") {
            Some(("Authorization:".len(), true))
        } else if lower.starts_with("bearer ") {
            Some(("Bearer ".len(), true))
        } else if lower.starts_with("api_key=") || lower.starts_with("api-key=") {
            Some((8, true))
        } else if rest.starts_with("sk-ant-") {
            Some(("sk-ant-".len(), false))
        } else if rest.starts_with("sk-") {
            Some(("sk-".len(), false))
        } else {
            None
        };

        if let Some((prefix_len, stop_at_ws)) = redact_from {
            out.push_str(&input[i..i + prefix_len]);
            i += prefix_len;
            if stop_at_ws {
                // skip spaces then secret token
                while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
            }
            while i < bytes.len() {
                let c = bytes[i] as char;
                if stop_at_ws {
                    if c.is_whitespace() || c == '"' || c == '\'' {
                        break;
                    }
                } else if !(c.is_ascii_alphanumeric() || c == '_' || c == '-') {
                    break;
                }
                i += 1;
            }
            out.push_str("[REDACTED]");
            continue;
        }

        // copy one UTF-8 char unchanged
        let ch = rest.chars().next().unwrap_or('\u{FFFD}');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_bearer_and_authorization() {
        // 与旧 assistant-protocol::redact_secrets 行为一致：`authorization:`
        // 前缀后第一个 token（"Bearer"）被脱敏，随后独立的 sk-ant- 前缀
        // 再被第二次脱敏。保持字节级兼容，不改变既有行为。
        assert_eq!(
            redact_secrets("Authorization: Bearer sk-ant-abcdef1234567890"),
            "Authorization:[REDACTED] sk-ant-[REDACTED]"
        );
        assert_eq!(redact_secrets("Bearer abc123"), "Bearer [REDACTED]");
    }

    #[test]
    fn redacts_api_key_forms() {
        assert_eq!(redact_secrets("api_key=secret-value"), "api_key=[REDACTED]");
        assert_eq!(redact_secrets("api-key=secret-value"), "api-key=[REDACTED]");
    }

    #[test]
    fn redacts_sk_prefixes_without_breaking_words() {
        assert_eq!(redact_secrets("sk-ant-abc123"), "sk-ant-[REDACTED]");
        assert_eq!(redact_secrets("sk-abc123"), "sk-[REDACTED]");
        // 非密钥前缀文本原样保留
        assert_eq!(redact_secrets("hello world"), "hello world");
    }
}
