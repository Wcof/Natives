//! Signed embed grants for the DSH main-panel iframe (ADR-0048 D5.2).
//!
//! A grant authorizes exactly one document response to be framed by an explicit
//! ancestor set. The launcher mints it with the state directory's relay secret;
//! the service verifies it before replacing `X-Frame-Options` with
//! `Content-Security-Policy: frame-ancestors`.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

/// Grants are short-lived: the page is opened immediately after minting.
pub const TTL_SECS: u64 = 120;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0)
}

fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    let mut block = [0u8; 64];
    if key.len() > 64 {
        let digest = Sha256::digest(key);
        block[..32].copy_from_slice(&digest);
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    for byte in block {
        inner.update([byte ^ 0x36]);
    }
    inner.update(message);
    let inner = inner.finalize();
    let mut outer = Sha256::new();
    for byte in block {
        outer.update([byte ^ 0x5c]);
    }
    outer.update(inner);
    outer.finalize().into()
}

/// `space.html`, or `app.html?app=fund` — the resource a grant is bound to.
pub fn normalize_target(path: &str) -> String {
    let path = path.trim_start_matches('/');
    let (base, query) = path.split_once('?').unwrap_or((path, ""));
    let app = query.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name == "app" && !value.is_empty()).then_some(value)
    });
    match app {
        Some(app) => format!("{base}?app={app}"),
        None => base.to_string(),
    }
}

fn valid_ancestor(source: &str) -> bool {
    // DSH renders its desktop window on the custom app scheme (measured:
    // `dsh-app://app`); `dsh:` stays accepted for the documented web profile
    // and older hosts. Both are scheme-only sources, never arbitrary hosts.
    if matches!(source, "dsh:" | "dsh-app:") {
        return true;
    }
    let rest = source
        .strip_prefix("http://127.0.0.1:")
        .or_else(|| source.strip_prefix("https://127.0.0.1:"));
    matches!(rest, Some(port) if port.parse::<u16>().is_ok_and(|value| value != 0))
}

/// Accept only the DSH app schemes and explicit loopback origins, normalized to
/// one line.
pub fn normalize_ancestors(value: &str) -> Option<String> {
    let sources: Vec<&str> = value.split_whitespace().collect();
    if sources.is_empty() || !sources.iter().all(|source| valid_ancestor(source)) {
        return None;
    }
    Some(sources.join(" "))
}

fn signature(target: &str, ancestors: &str, expires: u64, secret: &[u8]) -> String {
    let message = format!("{target}\n{ancestors}\n{expires}");
    URL_SAFE_NO_PAD.encode(hmac_sha256(secret, message.as_bytes()))
}

/// `expires.signature`; `ancestors` must already be normalized.
pub fn mint_at(target: &str, ancestors: &str, expires: u64, secret: &str) -> String {
    format!(
        "{expires}.{}",
        signature(target, ancestors, expires, secret.as_bytes())
    )
}

pub fn mint(target: &str, ancestors: &str, secret: &str) -> String {
    mint_at(target, ancestors, now_secs() + TTL_SECS, secret)
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in left.iter().zip(right) {
        diff |= a ^ b;
    }
    diff == 0
}

pub fn verify_at(target: &str, ancestors: &str, grant: &str, now: u64, secret: &str) -> bool {
    let Some((expires, sig)) = grant.split_once('.') else {
        return false;
    };
    let Ok(expires) = expires.parse::<u64>() else {
        return false;
    };
    if expires < now {
        return false;
    }
    let expected = signature(target, ancestors, expires, secret.as_bytes());
    constant_time_eq(sig.as_bytes(), expected.as_bytes())
}

pub fn verify(target: &str, ancestors: &str, grant: &str, secret: &str) -> bool {
    verify_at(target, ancestors, grant, now_secs(), secret)
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok();
            if let Some(byte) = hex.and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        // Query strings are form-encoded: `URLSearchParams` serializes a space
        // as `+`, and the runtime re-serializes the minted URL through it.
        if bytes[index] == b'+' {
            out.push(b' ');
            index += 1;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn query_value(full_path: &str, name: &str) -> Option<String> {
    let (_, query) = full_path.split_once('?')?;
    query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == name).then(|| percent_decode(value))
    })
}

/// `Some("frame-ancestors …")` for a valid grant on this document request.
pub fn frame_policy(full_path: &str, secret: &str) -> Option<String> {
    let grant = query_value(full_path, "embed")?;
    let ancestors = normalize_ancestors(&query_value(full_path, "anc")?)?;
    let target = normalize_target(full_path);
    verify(&target, &ancestors, &grant, secret).then(|| format!("frame-ancestors {ancestors}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grants_bind_target_ancestors_and_expiry() {
        let secret = "relay-secret";
        let target = normalize_target("/app.html?app=fund&theme=volt");
        assert_eq!(target, "app.html?app=fund");
        let ancestors = normalize_ancestors("dsh: http://127.0.0.1:59252").unwrap();
        let grant = mint_at(&target, &ancestors, 1000, secret);

        assert!(verify_at(&target, &ancestors, &grant, 999, secret));
        assert!(verify_at(&target, &ancestors, &grant, 1000, secret));
        assert!(!verify_at(&target, &ancestors, &grant, 1001, secret));
        assert!(!verify_at(
            "app.html?app=tokenusage",
            &ancestors,
            &grant,
            900,
            secret
        ));
        assert!(!verify_at(&target, "dsh:", &grant, 900, secret));
        assert!(!verify_at(&target, &ancestors, &grant, 900, "other"));
        assert!(!verify_at(&target, &ancestors, "garbage", 900, secret));
    }

    #[test]
    fn ancestors_reject_network_and_injection() {
        assert!(normalize_ancestors("dsh:").is_some());
        assert!(normalize_ancestors("dsh-app:").is_some());
        assert!(normalize_ancestors("dsh-app: dsh: http://127.0.0.1:59252").is_some());
        assert!(normalize_ancestors("http://127.0.0.1:59252 https://127.0.0.1:1").is_some());
        assert!(normalize_ancestors("https://example.com").is_none());
        assert!(normalize_ancestors("dsh:; script-src *").is_none());
        assert!(normalize_ancestors("http://127.0.0.1:59252\r\nX-Evil: 1").is_none());
        assert!(normalize_ancestors("http://localhost:59252").is_none());
        assert!(normalize_ancestors("http://127.0.0.1:0").is_none());
        assert!(normalize_ancestors("   ").is_none());
    }

    #[test]
    fn frame_policy_decodes_form_encoded_ancestors() {
        // `URLSearchParams` re-serialization turns the minted `%20` into `+`;
        // the grant must still verify against the form-decoded value.
        let secret = "relay-secret";
        let ancestors = "dsh-app: dsh: http://127.0.0.1:59737";
        let grant = mint("space.html", ancestors, secret);
        let path = format!(
            "/space.html?embed={grant}&anc={}",
            "dsh-app%3A+dsh%3A+http%3A%2F%2F127.0.0.1%3A59737"
        );
        assert_eq!(
            frame_policy(&path, secret),
            Some(format!("frame-ancestors {ancestors}"))
        );
    }

    #[test]
    fn frame_policy_requires_a_matching_grant() {
        let secret = "relay-secret";
        let ancestors = "dsh: http://127.0.0.1:59252";
        let grant = mint("space.html", ancestors, secret);
        let encoded_ancestors = ancestors.replace(' ', "%20").replace(':', "%3A");

        let path = format!("/space.html?theme=volt&embed={grant}&anc={encoded_ancestors}");
        assert_eq!(
            frame_policy(&path, secret),
            Some(format!("frame-ancestors {ancestors}"))
        );
        assert_eq!(frame_policy("/space.html?theme=volt", secret), None);
        assert_eq!(
            frame_policy("/files.html?embed={grant}&anc={encoded_ancestors}", secret),
            None
        );
    }
}
