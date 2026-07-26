//! Command and HTTP hook handlers with safety rails.

use crate::hooks::{
    tool_pattern_matches, HookDecision, HookHandler, HookOutcome, HookRequest, HookResponse,
    PermissionVerdict,
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
        self.handle_outcome(request).await.into_response()
    }

    /// The real body. `handle` collapses it fail-closed for callers that have
    /// no failure policy to apply.
    async fn handle_outcome(&self, request: HookRequest) -> HookOutcome {
        if !self.trusted {
            // A deliberate refusal, not a failure: no failure policy may soften
            // it, so it stays a Deny rather than becoming `Failed`.
            return HookOutcome::Decided(HookResponse {
                decision: HookDecision::Deny {
                    reason: "untrusted plugin cannot run command hooks".into(),
                },
            });
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
            return HookOutcome::Failed {
                reason: "hook input too large".into(),
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
                return HookOutcome::Failed {
                    reason: format!("failed to spawn hook: {e}"),
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
                HookOutcome::Failed {
                    reason: "hook output too large".into(),
                }
            }
            Ok(Ok(output)) if output.status.success() => parse_hook_stdout(&output.stdout),
            // A non-zero exit is the hook *deciding* to block, per the Claude
            // contract — not an infrastructure failure. It stays a Deny that no
            // failure policy can soften.
            Ok(Ok(output)) => {
                let reason = String::from_utf8_lossy(&output.stderr)
                    .trim()
                    .chars()
                    .take(4_000)
                    .collect::<String>();
                HookOutcome::Decided(HookResponse {
                    decision: HookDecision::Deny {
                        reason: if reason.is_empty() {
                            format!("hook exited with {}", output.status)
                        } else {
                            reason
                        },
                    },
                })
            }
            Ok(Err(e)) => HookOutcome::Failed {
                reason: format!("hook wait failed: {e}"),
            },
            Err(_) => HookOutcome::Failed {
                reason: "hook timeout".into(),
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
        self.handle_outcome(request).await.into_response()
    }

    async fn handle_outcome(&self, request: HookRequest) -> HookOutcome {
        // An SSRF-rejected URL is a configuration refusal, not a transient
        // failure: no failure policy may turn it into an allow.
        if let Err(reason) = validate_http_hook_url(&self.url, &self.allow_hosts) {
            return HookOutcome::Decided(HookResponse {
                decision: HookDecision::Deny { reason },
            });
        }
        let client = match reqwest::Client::builder()
            .timeout(self.timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                return HookOutcome::Failed {
                    reason: format!("http client error: {e}"),
                };
            }
        };
        let response = client.post(&self.url).json(&request).send().await;
        match response {
            Ok(resp) if resp.status().is_success() => {
                let bytes = resp.bytes().await.unwrap_or_default();
                if bytes.len() > MAX_IO_BYTES {
                    return HookOutcome::Failed {
                        reason: "hook response too large".into(),
                    };
                }
                parse_hook_stdout(&bytes)
            }
            Ok(resp) => HookOutcome::Failed {
                reason: format!("http hook status {}", resp.status()),
            },
            Err(e) => HookOutcome::Failed {
                reason: format!("http hook failed: {e}"),
            },
        }
    }
}

fn decided(decision: HookDecision) -> HookOutcome {
    HookOutcome::Decided(HookResponse { decision })
}

fn parse_hook_stdout(stdout: &[u8]) -> HookOutcome {
    let text = String::from_utf8_lossy(stdout);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return decided(HookDecision::Allow);
    }
    match serde_json::from_str::<Value>(trimmed) {
        Ok(value) => {
            if value.get("continue").and_then(Value::as_bool) == Some(false) {
                return decided(HookDecision::Deny {
                    reason: value
                        .get("stopReason")
                        .or_else(|| value.get("systemMessage"))
                        .and_then(Value::as_str)
                        .unwrap_or("blocked by hook")
                        .to_string(),
                });
            }
            if let Some(output) = value.get("hookSpecificOutput") {
                // `permissionDecision` is checked before `updatedInput` /
                // `additionalContext` so an explicit verdict always wins over
                // an incidental payload edit in the same object.
                let permission_reason = || {
                    output
                        .get("permissionDecisionReason")
                        .and_then(Value::as_str)
                        .unwrap_or("no reason given")
                        .to_string()
                };
                match output.get("permissionDecision").and_then(Value::as_str) {
                    Some("deny") => {
                        return decided(HookDecision::Deny {
                            reason: output
                                .get("permissionDecisionReason")
                                .and_then(Value::as_str)
                                .unwrap_or("denied by hook")
                                .to_string(),
                        })
                    }
                    Some("allow") => {
                        return HookOutcome::Permission(PermissionVerdict::Allow {
                            reason: permission_reason(),
                        })
                    }
                    Some("ask") => {
                        return HookOutcome::Permission(PermissionVerdict::Ask {
                            reason: permission_reason(),
                        })
                    }
                    // An unknown verdict is not an approval. Fall through to
                    // the native `decision` field rather than guessing.
                    _ => {}
                }
                if let Some(updated) = output.get("updatedInput") {
                    return decided(HookDecision::Modify {
                        payload: updated.clone(),
                    });
                }
                if let Some(context) = output.get("additionalContext").and_then(Value::as_str) {
                    return decided(HookDecision::Inject {
                        messages: vec![context.to_string()],
                    });
                }
            }
            let decision = value
                .get("decision")
                .and_then(|d| d.as_str())
                .unwrap_or("allow");
            match decision {
                "deny" | "block" => decided(HookDecision::Deny {
                    reason: value
                        .get("reason")
                        .and_then(|r| r.as_str())
                        .unwrap_or("denied by hook")
                        .to_string(),
                }),
                "modify" => decided(HookDecision::Modify {
                    payload: value.get("payload").cloned().unwrap_or(Value::Null),
                }),
                "inject" => decided(HookDecision::Inject {
                    messages: value
                        .get("messages")
                        .and_then(|m| m.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(str::to_string))
                                .collect()
                        })
                        .unwrap_or_default(),
                }),
                // `rewake` used to produce `HookDecision::Rewake`, which every
                // engine match arm ignored: the hook author was told nothing
                // and the Run stopped anyway. Refusing out loud is the only
                // honest answer until a real resume path exists — see the
                // `HookDecision::Rewake` doc comment.
                "rewake" => decided(HookDecision::Deny {
                    reason: "hook requested 'rewake', which this engine does not implement; \
                             the request was refused rather than silently ignored"
                        .into(),
                }),
                _ => decided(HookDecision::Allow),
            }
        }
        // Non-JSON stdout is not a decision — a hook that prints a log line
        // must not be read as an opinion either way.
        Err(_) => decided(HookDecision::Allow),
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

    /// Collapse an outcome to its decision for the tests that only care about
    /// the engine-facing half.
    fn decision_of(outcome: HookOutcome) -> HookDecision {
        outcome.into_response().decision
    }

    #[test]
    fn parses_claude_hook_output() {
        let denied = parse_hook_stdout(
            br#"{"hookSpecificOutput":{"permissionDecision":"deny","permissionDecisionReason":"blocked"}}"#,
        );
        assert!(matches!(
            decision_of(denied),
            HookDecision::Deny { ref reason } if reason == "blocked"
        ));

        let modified = parse_hook_stdout(
            br#"{"hookSpecificOutput":{"updatedInput":{"path":"safe.txt"}}}"#,
        );
        assert!(matches!(
            decision_of(modified),
            HookDecision::Modify { ref payload } if payload["path"] == "safe.txt"
        ));
    }

    #[test]
    fn parses_permission_decision_allow() {
        let allowed = parse_hook_stdout(
            br#"{"hookSpecificOutput":{"permissionDecision":"allow","permissionDecisionReason":"trusted read"}}"#,
        );
        assert!(matches!(
            allowed,
            HookOutcome::Permission(PermissionVerdict::Allow { ref reason }) if reason == "trusted read"
        ));
    }

    #[test]
    fn parses_permission_decision_ask() {
        let ask = parse_hook_stdout(
            br#"{"hookSpecificOutput":{"permissionDecision":"ask","permissionDecisionReason":"needs a human"}}"#,
        );
        assert!(matches!(
            ask,
            HookOutcome::Permission(PermissionVerdict::Ask { ref reason }) if reason == "needs a human"
        ));
    }

    /// An allow with no reason must still be auditable, so the reason is
    /// synthesised rather than left empty.
    #[test]
    fn permission_allow_without_reason_still_carries_one() {
        let allowed =
            parse_hook_stdout(br#"{"hookSpecificOutput":{"permissionDecision":"allow"}}"#);
        match allowed {
            HookOutcome::Permission(PermissionVerdict::Allow { reason }) => {
                assert!(!reason.trim().is_empty())
            }
            other => panic!("expected an allow verdict, got {other:?}"),
        }
    }

    /// A verdict this engine does not know is not an approval.
    #[test]
    fn unknown_permission_decision_is_not_an_approval() {
        let outcome = parse_hook_stdout(
            br#"{"hookSpecificOutput":{"permissionDecision":"maybe"}}"#,
        );
        assert!(matches!(outcome, HookOutcome::Decided(_)));
        assert!(matches!(decision_of(outcome), HookDecision::Allow));
    }

    /// `continue: false` is a hard stop and outranks an allow verdict in the
    /// same object.
    #[test]
    fn continue_false_outranks_permission_allow() {
        let outcome = parse_hook_stdout(
            br#"{"continue":false,"stopReason":"halt","hookSpecificOutput":{"permissionDecision":"allow"}}"#,
        );
        assert!(matches!(
            decision_of(outcome),
            HookDecision::Deny { ref reason } if reason == "halt"
        ));
    }

    /// An explicit verdict wins over an incidental `updatedInput` sibling, so a
    /// hook cannot smuggle an input rewrite past its own deny.
    #[test]
    fn permission_decision_outranks_updated_input() {
        let outcome = parse_hook_stdout(
            br#"{"hookSpecificOutput":{"permissionDecision":"deny","permissionDecisionReason":"no","updatedInput":{"path":"x"}}}"#,
        );
        assert!(matches!(decision_of(outcome), HookDecision::Deny { .. }));
    }

    /// `rewake` was a silent no-op. It must now be visibly refused.
    #[test]
    fn rewake_is_refused_rather_than_ignored() {
        let outcome = parse_hook_stdout(br#"{"decision":"rewake"}"#);
        match decision_of(outcome) {
            HookDecision::Deny { reason } => assert!(reason.contains("rewake")),
            other => panic!("expected a deny explaining the refusal, got {other:?}"),
        }
    }

    /// A permission verdict has no `HookDecision` spelling; degrading it must
    /// land on "no objection", never on something that could skip a prompt.
    #[test]
    fn permission_verdict_degrades_to_allow_not_to_a_bypass() {
        let response = HookOutcome::Permission(PermissionVerdict::Allow {
            reason: "r".into(),
        })
        .into_response();
        assert!(matches!(response.decision, HookDecision::Allow));
    }

    #[test]
    fn failed_outcome_degrades_to_deny() {
        let response = HookOutcome::Failed {
            reason: "boom".into(),
        }
        .into_response();
        assert!(matches!(
            response.decision,
            HookDecision::Deny { ref reason } if reason == "boom"
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

    fn probe_request() -> HookRequest {
        HookRequest {
            event: crate::hooks::HookEvent::PostToolUse,
            run_id: "r".into(),
            tool_name: Some("read_file".into()),
            input: serde_json::json!({}),
        }
    }

    /// A timeout is an infrastructure failure, so the failure policy gets a say.
    #[tokio::test]
    async fn command_hook_timeout_is_a_failure_not_a_decision() {
        let hook = CommandHook {
            program: "sleep".into(),
            args: vec!["5".into()],
            timeout: Duration::from_millis(50),
            trusted: true,
            cwd: None,
            tool_pattern: None,
        };
        let outcome = hook.handle_outcome(probe_request()).await;
        assert!(
            matches!(outcome, HookOutcome::Failed { ref reason } if reason == "hook timeout"),
            "timeout must be reported as a failure so HookFailurePolicy can act"
        );
    }

    /// A missing program is a failure, not the hook's opinion.
    #[tokio::test]
    async fn command_hook_spawn_error_is_a_failure() {
        let hook = CommandHook {
            program: "natives-no-such-hook-binary".into(),
            args: vec![],
            timeout: Duration::from_secs(1),
            trusted: true,
            cwd: None,
            tool_pattern: None,
        };
        assert!(matches!(
            hook.handle_outcome(probe_request()).await,
            HookOutcome::Failed { .. }
        ));
    }

    /// A non-zero exit is the documented way for a hook to block. It must stay
    /// a decision so no failure policy can soften it into an allow.
    #[tokio::test]
    async fn command_hook_nonzero_exit_stays_a_deny() {
        let hook = CommandHook {
            program: "false".into(),
            args: vec![],
            timeout: Duration::from_secs(5),
            trusted: true,
            cwd: None,
            tool_pattern: None,
        };
        let outcome = hook.handle_outcome(probe_request()).await;
        assert!(
            matches!(
                outcome,
                HookOutcome::Decided(HookResponse {
                    decision: HookDecision::Deny { .. }
                })
            ),
            "a blocking exit code must not be downgradable by a failure policy"
        );
    }

    /// `handle` has no failure policy to consult, so it must stay fail-closed.
    #[tokio::test]
    async fn handle_collapses_failures_to_deny() {
        let hook = CommandHook {
            program: "natives-no-such-hook-binary".into(),
            args: vec![],
            timeout: Duration::from_secs(1),
            trusted: true,
            cwd: None,
            tool_pattern: None,
        };
        assert!(matches!(
            hook.handle(probe_request()).await.decision,
            HookDecision::Deny { .. }
        ));
    }
}
