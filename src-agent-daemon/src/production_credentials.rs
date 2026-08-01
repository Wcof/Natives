//! Credential resolution (extracted from `production.rs`, task-01 structure).
//!
//! Order: 1) authenticated Tauri broker (natives.db, run-bound lease) 2) explicit
//! test/dev env keys 3) fail closed. Never invents offline success; never logs api_key.

use provider_adapters::capabilities::Credential;
use std::sync::Arc;

/// Injected by Tauri host: decrypt from natives.db via Credential Broker.
/// Signature: (provider_id, key_id, run_id) → Credential (memory only).
pub type CredentialBrokerFn =
    Arc<dyn Fn(&str, Option<&str>, &str) -> Result<Credential, String> + Send + Sync>;

static CREDENTIAL_BROKER: std::sync::Mutex<Option<CredentialBrokerFn>> =
    std::sync::Mutex::new(None);

/// Install the authenticated Credential Broker (from Tauri host).
pub fn install_credential_broker(broker: CredentialBrokerFn) {
    if let Ok(mut slot) = CREDENTIAL_BROKER.lock() {
        *slot = Some(broker);
    }
}

/// Clear broker (tests only).
#[cfg(test)]
pub fn clear_credential_broker_for_tests() {
    if let Ok(mut slot) = CREDENTIAL_BROKER.lock() {
        *slot = None;
    }
}

/// Resolve credentials: 1) installed Tauri broker (natives.db) 2) test env 3) fail.
/// Never invent mock completion text. Never log api_key.
pub fn resolve_credential(provider_id: &str, key_id: Option<&str>) -> Result<Credential, String> {
    // Legacy callers without a run must fail closed for lease binding in production
    // broker path; env-based test keys still work via resolve_credential_for_run
    // with an explicit synthetic id only from tests.
    resolve_credential_for_run(provider_id, key_id, "legacy-unbound")
}

pub fn resolve_credential_for_run(
    provider_id: &str,
    key_id: Option<&str>,
    run_id: &str,
) -> Result<Credential, String> {
    if run_id.trim().is_empty() {
        return Err("run_id required for credential lease binding".into());
    }
    // 1. Authenticated broker path (production): Tauri decrypts natives.db.
    //    Child agents must pass their own run_id — never inherit parent lease.
    let broker = CREDENTIAL_BROKER.lock().ok().and_then(|g| g.clone());
    if let Some(broker) = broker {
        match broker(provider_id, key_id, run_id) {
            Ok(cred) if !cred.api_key.trim().is_empty() => {
                // Attach key_id for audit; lease binding is enforced by run_id arg.
                return Ok(Credential {
                    api_key: cred.api_key,
                    base_url: cred.base_url,
                    proxy_url: cred.proxy_url,
                    key_id: cred.key_id.or_else(|| key_id.map(|s| s.to_string())),
                    provider_type: cred.provider_type,
                });
            }
            Ok(_) => {
                return Err(format!(
                    "Broker returned empty key for provider '{provider_id}'"
                ))
            }
            Err(e) => {
                // Fall through to env only when broker reports not-found and tests set env.
                let lower = e.to_ascii_lowercase();
                if !lower.contains("not found") && !lower.contains("no active key") {
                    return Err(redact_cred_err(&e));
                }
            }
        }
    }

    // 2. Explicit test/dev env keys (NATIVES_TEST_*) — never invent offline success text.
    // Anthropic also accepts common local env names used by CLI tooling (never logged).
    let lower = provider_id.to_ascii_lowercase();
    if lower.contains("anthropic") || lower.contains("claude") {
        let api_key = std::env::var("NATIVES_TEST_ANTHROPIC_KEY")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                std::env::var("ANTHROPIC_API_KEY")
                    .ok()
                    .filter(|s| !s.trim().is_empty())
            })
            .or_else(|| {
                std::env::var("ANTHROPIC_AUTH_TOKEN")
                    .ok()
                    .filter(|s| !s.trim().is_empty())
            });
        return match api_key {
            Some(api_key) => Ok(Credential {
                api_key,
                base_url: std::env::var("NATIVES_TEST_ANTHROPIC_BASE")
                    .ok()
                    .or_else(|| std::env::var("ANTHROPIC_BASE_URL").ok()),
                proxy_url: None,
                key_id: key_id.map(str::to_string),
                provider_type: Some("anthropic".into()),
            }),
            None => Err(format!(
                "No credential for provider '{provider_id}' (broker + NATIVES_TEST_ANTHROPIC_KEY/ANTHROPIC_API_KEY unavailable)"
            )),
        };
    }
    let (env_key, env_base) = if lower.contains("gemini") {
        ("NATIVES_TEST_GEMINI_KEY", None)
    } else if lower.contains("deepseek") {
        (
            "NATIVES_TEST_DEEPSEEK_KEY",
            Some("NATIVES_TEST_DEEPSEEK_BASE"),
        )
    } else if lower.contains("compatible") || lower.contains("sensenova") {
        ("NATIVES_TEST_OPENAI_KEY", Some("NATIVES_TEST_OPENAI_BASE"))
    } else if lower.contains("ollama") {
        return Ok(Credential {
            api_key: "ollama".into(),
            base_url: std::env::var("NATIVES_TEST_OLLAMA_BASE").ok(),
            proxy_url: None,
            key_id: key_id.map(str::to_string),
            provider_type: Some("ollama".into()),
        });
    } else {
        ("NATIVES_TEST_OPENAI_KEY", Some("NATIVES_TEST_OPENAI_BASE"))
    };
    // OpenAI-compatible / SenseNova often expose keys via ANTHROPIC_* in local tooling.
    let api_key = std::env::var(env_key)
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            if lower.contains("compatible")
                || lower.contains("sensenova")
                || lower.contains("openai")
            {
                std::env::var("ANTHROPIC_AUTH_TOKEN")
                    .ok()
                    .filter(|s| !s.trim().is_empty())
                    .or_else(|| {
                        std::env::var("ANTHROPIC_API_KEY")
                            .ok()
                            .filter(|s| !s.trim().is_empty())
                    })
            } else {
                None
            }
        });
    match api_key {
        Some(api_key) => {
            let raw_base = env_base
                .and_then(|k| std::env::var(k).ok())
                .or_else(|| std::env::var("NATIVES_TEST_OPENAI_BASE").ok())
                .or_else(|| {
                    if lower.contains("compatible")
                        || lower.contains("sensenova")
                        || lower.contains("openai")
                    {
                        std::env::var("ANTHROPIC_BASE_URL").ok()
                    } else {
                        None
                    }
                });
            Ok(Credential {
                api_key,
                base_url: raw_base.map(normalize_openai_compatible_base),
                proxy_url: None,
                key_id: key_id.map(str::to_string),
                provider_type: Some(
                    if lower.contains("deepseek") {
                        "deepseek"
                    } else if lower.contains("gemini") {
                        "gemini"
                    } else {
                        "openai_compatible"
                    }
                    .into(),
                ),
            })
        }
        None => Err(format!(
            "No credential for provider '{provider_id}' (broker + {env_key} both unavailable)"
        )),
    }
}

/// SenseNova and many gateways require `…/v1` for chat completions; accept host-only env.
pub(crate) fn normalize_openai_compatible_base(base: String) -> String {
    let t = base.trim().trim_end_matches('/').to_string();
    if t.is_empty() {
        return t;
    }
    // Already versioned or clearly a full API root.
    if t.ends_with("/v1") || t.contains("/v1/") || t.ends_with("/openai") {
        return t;
    }
    // token.sensenova.cn style host-only base → append /v1
    format!("{t}/v1")
}

pub(crate) fn redact_cred_err(msg: &str) -> String {
    // Lightweight redaction without pulling regex into the daemon binary.
    let mut out = String::with_capacity(msg.len());
    let bytes = msg.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(b"sk-") || bytes[i..].starts_with(b"SK-") {
            out.push_str("[REDACTED_KEY]");
            i += 3;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'-')
            {
                i += 1;
            }
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod base_url_normalize_tests {
    use super::normalize_openai_compatible_base;

    #[test]
    fn appends_v1_for_host_only() {
        assert_eq!(
            normalize_openai_compatible_base("https://token.sensenova.cn".into()),
            "https://token.sensenova.cn/v1"
        );
        assert_eq!(
            normalize_openai_compatible_base("https://token.sensenova.cn/".into()),
            "https://token.sensenova.cn/v1"
        );
    }

    #[test]
    fn keeps_existing_v1() {
        assert_eq!(
            normalize_openai_compatible_base("https://token.sensenova.cn/v1".into()),
            "https://token.sensenova.cn/v1"
        );
    }
}

#[cfg(test)]
mod run_identity_tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn broker_receives_distinct_run_ids() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_by_broker = seen.clone();
        install_credential_broker(Arc::new(move |_provider, _key, run_id| {
            seen_by_broker.lock().unwrap().push(run_id.to_string());
            Ok(Credential {
                api_key: "test-key".into(),
                base_url: None,
                proxy_url: None,
                key_id: None,
                provider_type: Some("openai".into()),
            })
        }));
        let _ = resolve_credential_for_run("openai", None, "run-a").unwrap();
        let _ = resolve_credential_for_run("openai", None, "run-b").unwrap();
        clear_credential_broker_for_tests();
        assert_eq!(*seen.lock().unwrap(), vec!["run-a", "run-b"]);
    }
}
