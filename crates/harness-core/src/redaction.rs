//! Redaction applied before a Hook ever reaches persistence or the Renderer.
//!
//! Design 第 17 节: "Secret-like fields are removed before persistence, not
//! merely hidden by UI." Hook configuration is user-authored, so it is not
//! *supposed* to contain credentials — but two shapes routinely do anyway:
//!
//! - an HTTP Hook URL with a token in the query string or in userinfo;
//! - a command Hook argv carrying `--token=...`.
//!
//! This module removes both before the value is written to `harness_run_snapshot`
//! or returned from `harness.hook.catalog`. It is deliberately conservative
//! about *shape* rather than clever about detection: an over-redacted argument
//! costs a user one glance at their own config file, an under-redacted one is
//! a credential in a database.

use crate::hooks::HookKind;

/// What replaces a redacted value. Fixed text so a diff never leaks length.
pub const REDACTED: &str = "***";

/// Key fragments that make the value after `=` or `:` a secret.
const SECRET_KEYS: [&str; 10] = [
    "token", "secret", "password", "passwd", "apikey", "api_key", "api-key", "auth", "credential",
    "session",
];

/// Redact a Hook's adapter configuration.
///
/// Returns an owned value; the caller keeps the unredacted original for
/// execution. Nothing in this module is on the dispatch path.
pub fn redact_kind(kind: &HookKind) -> HookKind {
    match kind {
        HookKind::Builtin { name } => HookKind::Builtin { name: name.clone() },
        HookKind::Command {
            program,
            args,
            trusted,
        } => HookKind::Command {
            program: program.clone(),
            args: args.iter().map(|a| redact_argument(a)).collect(),
            trusted: *trusted,
        },
        HookKind::Http { url, allow_hosts } => HookKind::Http {
            url: redact_url(url),
            allow_hosts: allow_hosts.clone(),
        },
    }
}

/// Strip userinfo, query, and fragment from a URL.
///
/// Not a URL parser: it only cuts at the first `?` or `#` and at an `@` inside
/// the authority. Anything it cannot understand is returned whole *only* when
/// it contains no secret-carrying punctuation, so a malformed URL cannot slip
/// a query through.
pub fn redact_url(url: &str) -> String {
    let cut = url
        .find(['?', '#'])
        .map(|i| &url[..i])
        .unwrap_or(url)
        .to_string();
    // `scheme://user:pass@host/path` — drop everything up to the last `@` in
    // the authority segment.
    let Some(scheme_end) = cut.find("://") else {
        return cut;
    };
    let (scheme, rest) = cut.split_at(scheme_end + 3);
    let authority_end = rest.find('/').unwrap_or(rest.len());
    let (authority, path) = rest.split_at(authority_end);
    match authority.rfind('@') {
        Some(at) => format!("{scheme}{REDACTED}@{}{path}", &authority[at + 1..]),
        None => cut,
    }
}

/// Redact one command-line argument.
///
/// Handles `--token=abc`, `token:abc`, and a bare argument that is long and
/// opaque enough to be a key rather than a path or a flag.
pub fn redact_argument(arg: &str) -> String {
    if let Some(split) = arg.find(['=', ':']) {
        let (key, value) = arg.split_at(split);
        let separator = &value[..1];
        let value = &value[1..];
        if !value.is_empty() && key_looks_secret(key) {
            return format!("{key}{separator}{REDACTED}");
        }
    }
    if looks_like_opaque_secret(arg) {
        return REDACTED.to_string();
    }
    arg.to_string()
}

fn key_looks_secret(key: &str) -> bool {
    let lowered = key.to_ascii_lowercase();
    SECRET_KEYS.iter().any(|needle| lowered.contains(needle))
}

/// A bare argument that is long, has no path or whitespace structure, and is
/// made only of key-ish characters.
fn looks_like_opaque_secret(arg: &str) -> bool {
    arg.len() >= 24
        && !arg.contains('/')
        && !arg.contains('\\')
        && !arg.contains(char::is_whitespace)
        && !arg.starts_with('-')
        && arg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
        && arg.chars().any(|c| c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_strings_never_survive() {
        assert_eq!(
            redact_url("https://hooks.example/pre?token=abc123&x=1"),
            "https://hooks.example/pre"
        );
        assert_eq!(
            redact_url("https://hooks.example/pre#frag"),
            "https://hooks.example/pre"
        );
    }

    #[test]
    fn userinfo_is_replaced_not_dropped() {
        assert_eq!(
            redact_url("https://user:hunter2@hooks.example/pre"),
            "https://***@hooks.example/pre"
        );
        assert_eq!(
            redact_url("https://hooks.example/pre"),
            "https://hooks.example/pre"
        );
    }

    #[test]
    fn a_malformed_url_still_loses_its_query() {
        assert_eq!(redact_url("not a url?token=abc"), "not a url");
    }

    #[test]
    fn secret_shaped_arguments_are_masked() {
        assert_eq!(redact_argument("--token=abc123"), "--token=***");
        assert_eq!(redact_argument("API_KEY=sk-live-1"), "API_KEY=***");
        assert_eq!(redact_argument("authorization:Bearer x"), "authorization:***");
    }

    #[test]
    fn ordinary_arguments_are_left_readable() {
        for arg in [
            "-lc",
            "./scripts/audit.sh",
            "/usr/bin/echo",
            "--format=json",
            "hi",
            "/Users/me/.claude/hooks/check.sh",
        ] {
            assert_eq!(redact_argument(arg), arg, "over-redacted {arg}");
        }
    }

    #[test]
    fn a_bare_opaque_token_is_masked() {
        assert_eq!(redact_argument("sk-live-51H7xQ2v9KpL3mN8dE4t"), REDACTED);
        // Long, but structured like a path — keep it readable.
        assert_eq!(
            redact_argument("/opt/natives/hooks/very-long-directory-name.sh"),
            "/opt/natives/hooks/very-long-directory-name.sh"
        );
    }

    #[test]
    fn redact_kind_covers_every_variant() {
        assert_eq!(
            redact_kind(&HookKind::Builtin {
                name: "allow-all".into()
            }),
            HookKind::Builtin {
                name: "allow-all".into()
            }
        );
        assert_eq!(
            redact_kind(&HookKind::Command {
                program: "/bin/sh".into(),
                args: vec!["-lc".into(), "--token=abc123".into()],
                trusted: true,
            }),
            HookKind::Command {
                program: "/bin/sh".into(),
                args: vec!["-lc".into(), "--token=***".into()],
                trusted: true,
            }
        );
        assert_eq!(
            redact_kind(&HookKind::Http {
                url: "https://h.example/x?k=v".into(),
                allow_hosts: vec!["h.example".into()],
            }),
            HookKind::Http {
                url: "https://h.example/x".into(),
                allow_hosts: vec!["h.example".into()],
            }
        );
    }
}
