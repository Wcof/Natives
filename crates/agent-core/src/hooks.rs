//! Hook Runtime — Session/Prompt/Tool/Permission/Subagent/Compact/Stop.
//!
//! Security-sensitive events (PreToolUse, PermissionRequest) use fail-closed
//! aggregation: any Deny wins; empty registry for those events denies by default
//! when `fail_closed` is set on the registry.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Lifecycle hook events (compatible with Claude / Grok hook names).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum HookEvent {
    SessionStart,
    SessionEnd,
    UserPromptSubmit,
    PreToolUse,
    PostToolUse,
    PostToolUseFailure,
    /// Alias surface for permission prompts (fail-closed by default).
    PermissionRequest,
    PermissionDenied,
    Notification,
    SubagentStart,
    SubagentStop,
    PreCompact,
    PostCompact,
    Stop,
    StopFailure,
    Error,
}

impl HookEvent {
    /// Security-sensitive events default fail-closed when no handler allows.
    pub fn is_security_sensitive(self) -> bool {
        matches!(
            self,
            Self::PreToolUse | Self::PermissionRequest | Self::PermissionDenied
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookDecision {
    Allow,
    Deny { reason: String },
    Modify { payload: Value },
    Inject { messages: Vec<String> },
    Rewake,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookRequest {
    pub event: HookEvent,
    pub run_id: String,
    pub tool_name: Option<String>,
    pub input: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookResponse {
    pub decision: HookDecision,
}

/// Trait for hook handlers (Rust / command / HTTP adapters implement this).
#[async_trait::async_trait]
pub trait HookHandler: Send + Sync {
    async fn handle(&self, request: HookRequest) -> HookResponse;

    /// Optional tool-name matcher (glob-ish: `*` any, exact otherwise).
    fn matches_tool(&self, _tool_name: Option<&str>) -> bool {
        true
    }
}

/// Registry of hook handlers keyed by event.
#[derive(Default)]
pub struct HookRegistry {
    handlers: HashMap<HookEvent, Vec<Box<dyn HookHandler>>>,
    /// When true, security-sensitive events with zero matching handlers deny.
    pub fail_closed_security: bool,
}

impl HookRegistry {
    /// Empty registry. Security fail-closed is **off** by default so bare
    /// `AgentEngine::new()` tests keep working; call
    /// [`HookRegistry::enable_security_fail_closed`] after registering defaults.
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
            fail_closed_security: false,
        }
    }

    pub fn enable_security_fail_closed(&mut self) {
        self.fail_closed_security = true;
    }

    pub fn register(&mut self, event: HookEvent, handler: Box<dyn HookHandler>) {
        self.handlers.entry(event).or_default().push(handler);
    }

    pub async fn dispatch(&self, request: HookRequest) -> Vec<HookResponse> {
        let mut out = Vec::new();
        if let Some(handlers) = self.handlers.get(&request.event) {
            for handler in handlers {
                if !handler.matches_tool(request.tool_name.as_deref()) {
                    continue;
                }
                out.push(handler.handle(request.clone()).await);
            }
        }
        // Fail-closed for security events with no matching handler.
        if out.is_empty()
            && self.fail_closed_security
            && request.event.is_security_sensitive()
        {
            out.push(HookResponse {
                decision: HookDecision::Deny {
                    reason: format!(
                        "fail-closed: no hook handler allowed {:?} for tool {:?}",
                        request.event, request.tool_name
                    ),
                },
            });
        }
        out
    }

    /// Aggregate: any Deny wins (fail-closed for PreToolUse / PermissionRequest).
    pub fn aggregate_allow(responses: &[HookResponse]) -> Result<(), String> {
        for r in responses {
            if let HookDecision::Deny { reason } = &r.decision {
                return Err(reason.clone());
            }
        }
        Ok(())
    }

    pub fn events_covered(&self) -> Vec<HookEvent> {
        self.handlers.keys().copied().collect()
    }
}

/// Built-in allow-all handler (useful for tests / default).
pub struct AllowAllHook;

#[async_trait::async_trait]
impl HookHandler for AllowAllHook {
    async fn handle(&self, _request: HookRequest) -> HookResponse {
        HookResponse {
            decision: HookDecision::Allow,
        }
    }
}

/// Deny when tool name matches a pattern (`*` = all).
pub struct MatcherDenyHook {
    pub tool_pattern: String,
    pub reason: String,
}

#[async_trait::async_trait]
impl HookHandler for MatcherDenyHook {
    fn matches_tool(&self, tool_name: Option<&str>) -> bool {
        match tool_name {
            None => self.tool_pattern == "*",
            Some(name) => {
                self.tool_pattern == "*"
                    || self.tool_pattern == name
                    || (self.tool_pattern.ends_with('*')
                        && name.starts_with(self.tool_pattern.trim_end_matches('*')))
            }
        }
    }

    async fn handle(&self, _request: HookRequest) -> HookResponse {
        HookResponse {
            decision: HookDecision::Deny {
                reason: self.reason.clone(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn allow_all_hook_fires() {
        let mut reg = HookRegistry::new();
        reg.fail_closed_security = false;
        reg.register(HookEvent::PreToolUse, Box::new(AllowAllHook));
        let responses = reg
            .dispatch(HookRequest {
                event: HookEvent::PreToolUse,
                run_id: "r".into(),
                tool_name: Some("read_file".into()),
                input: serde_json::json!({}),
            })
            .await;
        assert_eq!(responses.len(), 1);
        assert!(matches!(responses[0].decision, HookDecision::Allow));
    }

    #[tokio::test]
    async fn matcher_deny_blocks_matching_tool() {
        let mut reg = HookRegistry::new();
        reg.register(
            HookEvent::PreToolUse,
            Box::new(MatcherDenyHook {
                tool_pattern: "bash*".into(),
                reason: "bash blocked".into(),
            }),
        );
        reg.register(HookEvent::PreToolUse, Box::new(AllowAllHook));
        let responses = reg
            .dispatch(HookRequest {
                event: HookEvent::PreToolUse,
                run_id: "r".into(),
                tool_name: Some("bash".into()),
                input: serde_json::json!({}),
            })
            .await;
        assert!(HookRegistry::aggregate_allow(&responses).is_err());
        let ok = reg
            .dispatch(HookRequest {
                event: HookEvent::PreToolUse,
                run_id: "r".into(),
                tool_name: Some("read_file".into()),
                input: serde_json::json!({}),
            })
            .await;
        // only AllowAll matches read_file
        assert!(HookRegistry::aggregate_allow(&ok).is_ok());
    }

    #[tokio::test]
    async fn permission_request_fail_closed_without_handlers() {
        let mut reg = HookRegistry::new();
        reg.enable_security_fail_closed();
        assert!(reg.fail_closed_security);
        let responses = reg
            .dispatch(HookRequest {
                event: HookEvent::PermissionRequest,
                run_id: "r".into(),
                tool_name: Some("write_file".into()),
                input: serde_json::json!({}),
            })
            .await;
        assert_eq!(responses.len(), 1);
        assert!(matches!(
            responses[0].decision,
            HookDecision::Deny { .. }
        ));
    }
}
