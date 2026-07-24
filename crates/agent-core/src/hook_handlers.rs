//! Command and HTTP hook handlers with safety rails.

use crate::hooks::{
    tool_pattern_matches, HookDecision, HookHandler, HookRequest, HookResponse,
};
use serde_json::Value;
use std::net::IpAddr;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

const MAX_IO_BYTES: usize = 256 * 1024;

/// Command hook — argv only, never shell-string join.
pub struct CommandHook {
    pub program: String,
    pub args: Vec<String>,
    pub timeout: Duration,
    pub trusted: bool,
    pub cwd: Option<PathBuf>,
    pub tool_pattern: Option<String>,
}

#[async_trait::async_trait]
impl HookHandler for CommandHook {
    fn matches_tool(&self, tool_name: Option<&str>) -> bool {
        tool_pattern_matches(self.tool_pattern.as_deref(), tool_name)
    }

    async fn handle(&self, request: HookRequest) -> HookResponse {
        if !self.trusted {
            return HookResponse {
                decision: HookDecision::Deny {
                    reason: "untrusted plugin cannot run command hooks".into(),
                },
            };
        }
        let event_name = format!("{:?}", request.event);
        let payload = serde_json::to_vec(&serde_json::json!({
            "event": request.event,
            "hook_event_name": event_name.clone(),
            "run_id": request.run_id.clone(),
            "session_id": request.run_id.clone(),
            "tool_name": request.tool_name.clone(),
            "input": request.input.clone(),
            "tool_input": request.input.clone(),
            "cwd": self.cwd.as_ref(),
        }))
        .unwrap_or_default();
        if payload.len() > MAX_IO_BYTES {
            return HookResponse {
                decision: HookDecision::Deny {
                    reason: "hook input too large".into(),
                },
            };
        }

        let mut command = Command::new(&self.program);
        command
            .args(&self.args)
            .env("NATIVES_HOOK_EVENT", &event_name)
            .env("NATIVES_RUN_ID", &request.run_id);
        if let Some(cwd) = &self.cwd {
            command
                .current_dir(cwd)
                .env("CLAUDE_PROJECT_DIR", cwd)
                .env("NATIVES_PROJECT_DIR", cwd);
        }
        let mut child = match command
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

        let result = tokio::time::timeout(self.timeout, child.wait_with_output()).await;

        match result {
            Ok(Ok(output))
                if output.stdout.len().saturating_add(output.stderr.len()) > MAX_IO_BYTES =>
            {
                HookResponse {
                    decision: HookDecision::Deny {
                        reason: "hook output too large".into(),
                    },
                }
            }
            Ok(Ok(output)) if output.status.success() => parse_hook_stdout(&output.stdout),
            Ok(Ok(output)) => {
                let reason = String::from_utf8_lossy(&output.stderr)
                    .trim()
                    .chars()
                    .take(4_000)
                    .collect::<String>();
                HookResponse {
                    decision: HookDecision::Deny {
                        reason: if reason.is_empty() {
                            format!("hook exited with {}", output.status)
                        } else {
                            reason
                        },
                    },
                }
            }
            Ok(Err(e)) => HookResponse {
                decision: HookDecision::Deny {
                    reason: format!("hook wait failed: {e}"),
                },
            },
            Err(_) => HookResponse {
                decision: HookDecision::Deny {
                    reason: "hook timeout".into(),
                },
            },
        }
    }
}

/// HTTP hook with SSRF protections.
pub struct HttpHook {
    pub url: String,
    pub timeout: Duration,
    pub allow_hosts: Vec<String>,
    pub tool_pattern: Option<String>,
}

#[async_trait::async_trait]
impl HookHandler for HttpHook {
    fn matches_tool(&self, tool_name: Option<&str>) -> bool {
        tool_pattern_matches(self.tool_pattern.as_deref(), tool_name)
    }

    async fn handle(&self, request: HookRequest) -> HookResponse {
        if let Err(reason) = validate_http_hook_url(&self.url, &self.allow_hosts) {
            return HookResponse {
                decision: HookDecision::Deny { reason },
            };
        }
        let client = match reqwest::Client::builder()
            .timeout(self.timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
        {
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
            if value.get("continue").and_then(Value::as_bool) == Some(false) {
                return HookResponse {
                    decision: HookDecision::Deny {
                        reason: value
                            .get("stopReason")
                            .or_else(|| value.get("systemMessage"))
                            .and_then(Value::as_str)
                            .unwrap_or("blocked by hook")
                            .to_string(),
                    },
                };
            }
            if let Some(output) = value.get("hookSpecificOutput") {
                if output
                    .get("permissionDecision")
                    .and_then(Value::as_str)
                    == Some("deny")
                {
                    return HookResponse {
                        decision: HookDecision::Deny {
                            reason: output
                                .get("permissionDecisionReason")
                                .and_then(Value::as_str)
                                .unwrap_or("denied by hook")
                                .to_string(),
                        },
                    };
                }
                if let Some(updated) = output.get("updatedInput") {
                    return HookResponse {
                        decision: HookDecision::Modify {
                            payload: updated.clone(),
                        },
                    };
                }
                if let Some(context) = output.get("additionalContext").and_then(Value::as_str) {
                    return HookResponse {
                        decision: HookDecision::Inject {
                            messages: vec![context.to_string()],
                        },
                    };
                }
            }
            let decision = value
                .get("decision")
                .and_then(|d| d.as_str())
                .unwrap_or("allow");
            match decision {
                "deny" | "block" => HookResponse {
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

    #[test]
    fn parses_claude_hook_output() {
        let denied = parse_hook_stdout(
            br#"{"hookSpecificOutput":{"permissionDecision":"deny","permissionDecisionReason":"blocked"}}"#,
        );
        assert!(matches!(
            denied.decision,
            HookDecision::Deny { ref reason } if reason == "blocked"
        ));

        let modified = parse_hook_stdout(
            br#"{"hookSpecificOutput":{"updatedInput":{"path":"safe.txt"}}}"#,
        );
        assert!(matches!(
            modified.decision,
            HookDecision::Modify { ref payload } if payload["path"] == "safe.txt"
        ));
    }

    #[tokio::test]
    async fn untrusted_command_hook_denied() {
        let hook = CommandHook {
            program: "echo".into(),
            args: vec![],
            timeout: Duration::from_secs(1),
            trusted: false,
            cwd: None,
            tool_pattern: None,
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
