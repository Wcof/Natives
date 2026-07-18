//! Command and HTTP hook handlers with safety rails.

use crate::hooks::{HookDecision, HookHandler, HookRequest, HookResponse};
use serde_json::Value;
use std::net::IpAddr;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

const MAX_IO_BYTES: usize = 256 * 1024;

/// Command hook — argv only, never shell-string join.
pub struct CommandHook {
    pub program: String,
    pub args: Vec<String>,
    pub timeout: Duration,
    pub trusted: bool,
}

#[async_trait::async_trait]
impl HookHandler for CommandHook {
    async fn handle(&self, request: HookRequest) -> HookResponse {
        if !self.trusted {
            return HookResponse {
                decision: HookDecision::Deny {
                    reason: "untrusted plugin cannot run command hooks".into(),
                },
            };
        }
        let payload = serde_json::to_vec(&request).unwrap_or_default();
        if payload.len() > MAX_IO_BYTES {
            return HookResponse {
                decision: HookDecision::Deny {
                    reason: "hook input too large".into(),
                },
            };
        }

        let mut child = match Command::new(&self.program)
            .args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                return HookResponse {
                    decision: HookDecision::Deny {
                        reason: format!("failed to spawn hook: {e}"),
                    },
                };
            }
        };

        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(&payload).await;
        }

        let result = tokio::time::timeout(self.timeout, async {
            let mut stdout = Vec::new();
            if let Some(mut out) = child.stdout.take() {
                let mut buf = vec![0u8; 8192];
                loop {
                    let n = out.read(&mut buf).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    stdout.extend_from_slice(&buf[..n]);
                    if stdout.len() > MAX_IO_BYTES {
                        break;
                    }
                }
            }
            let status = child.wait().await;
            (status, stdout)
        })
        .await;

        match result {
            Ok((Ok(status), stdout)) if status.success() => parse_hook_stdout(&stdout),
            Ok((Ok(status), _)) => HookResponse {
                decision: HookDecision::Deny {
                    reason: format!("hook exited with {status}"),
                },
            },
            Ok((Err(e), _)) => HookResponse {
                decision: HookDecision::Deny {
                    reason: format!("hook wait failed: {e}"),
                },
            },
            Err(_) => {
                let _ = child.kill().await;
                HookResponse {
                    decision: HookDecision::Deny {
                        reason: "hook timeout".into(),
                    },
                }
            }
        }
    }
}

/// HTTP hook with SSRF protections.
pub struct HttpHook {
    pub url: String,
    pub timeout: Duration,
    pub allow_hosts: Vec<String>,
}

#[async_trait::async_trait]
impl HookHandler for HttpHook {
    async fn handle(&self, request: HookRequest) -> HookResponse {
        if let Err(reason) = validate_http_hook_url(&self.url, &self.allow_hosts) {
            return HookResponse {
                decision: HookDecision::Deny { reason },
            };
        }
        let client = match reqwest::Client::builder().timeout(self.timeout).build() {
            Ok(c) => c,
            Err(e) => {
                return HookResponse {
                    decision: HookDecision::Deny {
                        reason: format!("http client error: {e}"),
                    },
                };
            }
        };
        let response = client.post(&self.url).json(&request).send().await;
        match response {
            Ok(resp) if resp.status().is_success() => {
                let bytes = resp.bytes().await.unwrap_or_default();
                if bytes.len() > MAX_IO_BYTES {
                    return HookResponse {
                        decision: HookDecision::Deny {
                            reason: "hook response too large".into(),
                        },
                    };
                }
                parse_hook_stdout(&bytes)
            }
            Ok(resp) => HookResponse {
                decision: HookDecision::Deny {
                    reason: format!("http hook status {}", resp.status()),
                },
            },
            Err(e) => HookResponse {
                decision: HookDecision::Deny {
                    reason: format!("http hook failed: {e}"),
                },
            },
        }
    }
}

fn parse_hook_stdout(stdout: &[u8]) -> HookResponse {
    let text = String::from_utf8_lossy(stdout);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return HookResponse {
            decision: HookDecision::Allow,
        };
    }
    match serde_json::from_str::<Value>(trimmed) {
        Ok(value) => {
            let decision = value
                .get("decision")
                .and_then(|d| d.as_str())
                .unwrap_or("allow");
            match decision {
                "deny" => HookResponse {
                    decision: HookDecision::Deny {
                        reason: value
                            .get("reason")
                            .and_then(|r| r.as_str())
                            .unwrap_or("denied by hook")
                            .to_string(),
                    },
                },
                "modify" => HookResponse {
                    decision: HookDecision::Modify {
                        payload: value.get("payload").cloned().unwrap_or(Value::Null),
                    },
                },
                "inject" => HookResponse {
                    decision: HookDecision::Inject {
                        messages: value
                            .get("messages")
                            .and_then(|m| m.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str().map(str::to_string))
                                    .collect()
                            })
                            .unwrap_or_default(),
                    },
                },
                "rewake" => HookResponse {
                    decision: HookDecision::Rewake,
                },
                _ => HookResponse {
                    decision: HookDecision::Allow,
                },
            }
        }
        Err(_) => HookResponse {
            decision: HookDecision::Allow,
        },
    }
}

/// SSRF checks: HTTPS preferred, block private IPs, require allowlist when set.
pub fn validate_http_hook_url(url: &str, allow_hosts: &[String]) -> Result<(), String> {
    let parsed = url::Url::parse(url).map_err(|e| format!("invalid url: {e}"))?;
    let scheme = parsed.scheme();
    if scheme != "https" && scheme != "http" {
        return Err("only http/https hooks allowed".into());
    }
    let host = parsed.host_str().ok_or_else(|| "missing host".to_string())?;
    if !allow_hosts.is_empty()
        && !allow_hosts
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(host) || host.ends_with(&format!(".{allowed}")))
    {
        return Err(format!("host '{host}' not in allowlist"));
    }
    // Block literal private IPs.
    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_private_or_loopback(ip) {
            return Err("private/loopback IPs are not allowed for HTTP hooks".into());
        }
    }
    let blocked = [
        "localhost",
        "metadata.google.internal",
        "169.254.169.254",
    ];
    if blocked.iter().any(|b| host.eq_ignore_ascii_case(b)) {
        return Err("blocked host".into());
    }
    Ok(())
}

fn is_private_or_loopback(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.octets()[0] == 169 && v4.octets()[1] == 254
        }
        IpAddr::V6(v6) => v6.is_loopback() || v6.is_unique_local() || v6.is_unicast_link_local(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_private_http_targets() {
        assert!(validate_http_hook_url("http://127.0.0.1/hook", &[]).is_err());
        assert!(validate_http_hook_url("http://10.0.0.5/hook", &[]).is_err());
        assert!(validate_http_hook_url("http://169.254.169.254/latest", &[]).is_err());
        assert!(validate_http_hook_url("http://localhost/x", &[]).is_err());
    }

    #[test]
    fn allowlist_enforced() {
        assert!(validate_http_hook_url("https://hooks.example.com/h", &["hooks.example.com".into()]).is_ok());
        assert!(validate_http_hook_url("https://evil.example.org/h", &["hooks.example.com".into()]).is_err());
    }

    #[tokio::test]
    async fn untrusted_command_hook_denied() {
        let hook = CommandHook {
            program: "echo".into(),
            args: vec![],
            timeout: Duration::from_secs(1),
            trusted: false,
        };
        let resp = hook
            .handle(HookRequest {
                event: crate::hooks::HookEvent::PreToolUse,
                run_id: "r".into(),
                tool_name: Some("read_file".into()),
                input: serde_json::json!({}),
            })
            .await;
        assert!(matches!(resp.decision, HookDecision::Deny { .. }));
    }
}
