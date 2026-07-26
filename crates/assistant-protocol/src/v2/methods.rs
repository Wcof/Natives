//! Canonical RPC method names for Protocol v2.

/// Full Protocol v2 method catalogue (target surface).
/// Prefer [`IMPLEMENTED_METHODS`] for capability advertisement.
pub const ALL_METHODS: &[&str] = &[
    "daemon.getCapabilities",
    "daemon.getStatus",
    "daemon.ping",
    "provider.list",
    "provider.discoverModels",
    "provider.test",
    "conversation.create",
"conversation.list",
"conversation.listPage",
    "conversation.get",
    "conversation.fork",
    "conversation.update",
"conversation.getMessages",
"conversation.getMessagesPage",
    "conversation.appendMessage",
    "conversation.rename",
    "conversation.update_model",
    "conversation.update_permission",
    "conversation.archive",
    "conversation.delete",
    "run.create",
    "run.start",
    "run.cancel",
    "run.retry",
    "run.subscribe",
    "run.replay",
    "run.list",
    "run.getEvents",
    "run.listChildren",
    "run.finish",
    "run.rewind",
    "run.rewindPreview",
    "workspace.restore",
    "workspace.restorePreview",
    "permission.respond",
    "permission.listPending",
    "interaction.listPending",
    "interaction.respond",
    "promptQueue.list",
    "promptQueue.enqueue",
    "promptQueue.update",
    "promptQueue.remove",
    "promptQueue.reorder",
    "promptQueue.sendNow",
    "promptQueue.interject",
    "tool.list",
    "agent.list",
    "subagent.list",
    "subagent.touch",
    "subagent.switchRoute",
    "extension.list",
    "extension.enable",
    "skill.list",
    "memory.search",
    "memory.add",
    "mcp.list",
    "mcp.start",
    "mcp.stop",
    "mcp.call",
    "mcp.liveness",
    "mcp.reconnect",
    "mcp.auth.set",
    "mcp.auth.status",
    "mcp.auth.clear",
    "mcp.auth.oauthStart",
    "mcp.auth.oauthCallback",
    "artifact.list",
    "artifact.open",
    "artifact.reveal",
    "conversation.getContextUsage",
    "run.getActivity",
    "task.list",
    "task.cancel",
    "task.wait",
    "scheduler.list",
    "scheduler.create",
    "scheduler.update",
    "scheduler.delete",
    "scheduler.history",
    "scheduler.tick",
    "engine.rateLimit.get",
    "engine.rateLimit.update",
    "engine.rateLimit.acquire",
    "engine.rateLimit.cooldown",
];

/// Methods actually handled by the Agent Daemon RPC (must match `rpc.rs`).
/// Capability advertisement must only list these — never the full catalogue.
pub const IMPLEMENTED_METHODS: &[&str] = &[
    "daemon.getCapabilities",
    "daemon.getStatus",
    "daemon.ping",
    "provider.list",
    "provider.discoverModels",
    "provider.test",
    "conversation.create",
"conversation.list",
"conversation.listPage",
    "conversation.get",
    "conversation.fork",
    "conversation.update",
"conversation.getMessages",
"conversation.getMessagesPage",
    "conversation.appendMessage",
    "conversation.rename",
    "conversation.update_model",
    "conversation.update_permission",
    "conversation.archive",
    "conversation.delete",
    "run.create",
    "run.start",
    "run.cancel",
    "run.retry",
    "run.subscribe",
    "run.replay",
    "run.list",
    "run.getEvents",
    "run.listChildren",
    "run.finish",
    "run.getActivity",
    "permission.respond",
    "permission.listPending",
    "tool.list",
    "agent.list",
    "mcp.list",
    "mcp.start",
    "mcp.stop",
    // mcp.call intentionally NOT implemented (task-06): direct RPC transport bypass disabled.
    "mcp.liveness",
    "mcp.reconnect",
    "mcp.auth.set",
    "mcp.auth.status",
    "mcp.auth.clear",
    "scheduler.list",
    "scheduler.create",
    "scheduler.update",
    "scheduler.delete",
    "scheduler.history",
    "scheduler.tick",
    "extension.list",
    "extension.enable",
    "skill.list",
    "memory.search",
    "memory.add",
    "artifact.list",
    "artifact.open",
    // Phase 2/3 daemon surface
    "promptQueue.list",
    "promptQueue.enqueue",
    "promptQueue.update",
    "promptQueue.remove",
    "promptQueue.reorder",
    "promptQueue.sendNow",
    "promptQueue.interject",
    "run.rewindPreview",
    "run.rewind",
    "workspace.restorePreview",
    "workspace.restore",
    "conversation.getContextUsage",
    "task.list",
    "task.cancel",
    "task.wait",
    "interaction.listPending",
    "interaction.respond",
    "subagent.list",
    "subagent.touch",
    "subagent.switchRoute",
    "engine.rateLimit.get",
    "engine.rateLimit.update",
    "engine.rateLimit.acquire",
    "engine.rateLimit.cooldown",
];

/// Methods the Tauri host still owns after Phase 0 cutover.
///
/// Honesty contract: this list is *unioned into the advertised capability set* by
/// [`super::DaemonCapabilities::host_mediated`]. An entry here is a promise that the
/// **host** intercepts the call before it reaches the daemon — it must therefore stay in
/// sync with `src-tauri/src/assistant_service.rs::is_host_owned_method`. Anything listed
/// here that the host does *not* intercept falls through to the daemon RPC, and if the
/// daemon has no dispatch arm the caller gets `internal_error` instead of an honest
/// `unsupported` (the advertisement lied). Prefer implementing on the daemon and listing
/// in [`IMPLEMENTED_METHODS`]; keep this list to genuinely OS-bound seams.
///
/// Production (`NATIVES_DAEMON_MODE=uds|sidecar|remote`): conversation / promptQueue /
/// interaction / permission / run / task are daemon-owned and must NOT be re-advertised
/// here as host-primary.
pub const HOST_IMPLEMENTED_METHODS: &[&str] = &[
    // Run preflight + host projection boundary (host intercepts, then delegates).
    "run.start",
    "run.subscribe",
    // OS-bound artifact actions: only the host can talk to the desktop shell.
    // `artifact.reveal` is host-ONLY — the daemon deliberately has no dispatch arm,
    // because "show in file manager" is not a daemon capability.
    "artifact.open",
    "artifact.reveal",
];

pub mod names {
    pub const DAEMON_GET_CAPABILITIES: &str = "daemon.getCapabilities";
    pub const DAEMON_GET_STATUS: &str = "daemon.getStatus";
    pub const DAEMON_PING: &str = "daemon.ping";
    pub const PROVIDER_LIST: &str = "provider.list";
    pub const PROVIDER_DISCOVER_MODELS: &str = "provider.discoverModels";
    pub const PROVIDER_TEST: &str = "provider.test";
    pub const CONVERSATION_CREATE: &str = "conversation.create";
    pub const CONVERSATION_LIST: &str = "conversation.list";
    pub const CONVERSATION_LIST_PAGE: &str = "conversation.listPage";
    pub const CONVERSATION_GET_MESSAGES_PAGE: &str = "conversation.getMessagesPage";
    pub const CONVERSATION_GET: &str = "conversation.get";
    pub const CONVERSATION_FORK: &str = "conversation.fork";
    pub const CONVERSATION_UPDATE: &str = "conversation.update";
    pub const CONVERSATION_GET_MESSAGES: &str = "conversation.getMessages";
    pub const CONVERSATION_APPEND_MESSAGE: &str = "conversation.appendMessage";
    pub const CONVERSATION_RENAME: &str = "conversation.rename";
    pub const CONVERSATION_UPDATE_MODEL: &str = "conversation.update_model";
    pub const CONVERSATION_UPDATE_PERMISSION: &str = "conversation.update_permission";
    pub const CONVERSATION_ARCHIVE: &str = "conversation.archive";
    pub const CONVERSATION_DELETE: &str = "conversation.delete";
    pub const RUN_CREATE: &str = "run.create";
    pub const RUN_START: &str = "run.start";
    pub const RUN_CANCEL: &str = "run.cancel";
    pub const RUN_RETRY: &str = "run.retry";
    pub const RUN_SUBSCRIBE: &str = "run.subscribe";
    pub const RUN_REPLAY: &str = "run.replay";
    pub const RUN_LIST: &str = "run.list";
    pub const RUN_GET_EVENTS: &str = "run.getEvents";
    pub const RUN_LIST_CHILDREN: &str = "run.listChildren";
    pub const RUN_FINISH: &str = "run.finish";
    pub const RUN_GET_ACTIVITY: &str = "run.getActivity";
    pub const RUN_REWIND: &str = "run.rewind";
    pub const RUN_REWIND_PREVIEW: &str = "run.rewindPreview";
    pub const WORKSPACE_RESTORE: &str = "workspace.restore";
    pub const WORKSPACE_RESTORE_PREVIEW: &str = "workspace.restorePreview";
    pub const PERMISSION_RESPOND: &str = "permission.respond";
    pub const PERMISSION_LIST_PENDING: &str = "permission.listPending";
    pub const TOOL_LIST: &str = "tool.list";
    pub const PROMPT_QUEUE_LIST: &str = "promptQueue.list";
    pub const PROMPT_QUEUE_ENQUEUE: &str = "promptQueue.enqueue";
    pub const PROMPT_QUEUE_UPDATE: &str = "promptQueue.update";
    pub const PROMPT_QUEUE_REMOVE: &str = "promptQueue.remove";
    pub const PROMPT_QUEUE_REORDER: &str = "promptQueue.reorder";
    pub const PROMPT_QUEUE_SEND_NOW: &str = "promptQueue.sendNow";
    pub const PROMPT_QUEUE_INTERJECT: &str = "promptQueue.interject";
    pub const CONVERSATION_GET_CONTEXT_USAGE: &str = "conversation.getContextUsage";
    pub const AGENT_LIST: &str = "agent.list";
    pub const SUBAGENT_LIST: &str = "subagent.list";
    pub const SUBAGENT_TOUCH: &str = "subagent.touch";
    pub const SUBAGENT_SWITCH_ROUTE: &str = "subagent.switchRoute";
    pub const EXTENSION_LIST: &str = "extension.list";
    pub const EXTENSION_ENABLE: &str = "extension.enable";
    pub const SKILL_LIST: &str = "skill.list";
    pub const MEMORY_SEARCH: &str = "memory.search";
    pub const MEMORY_ADD: &str = "memory.add";
    pub const MCP_LIST: &str = "mcp.list";
    pub const MCP_START: &str = "mcp.start";
    pub const MCP_STOP: &str = "mcp.stop";
    pub const MCP_CALL: &str = "mcp.call";
    pub const MCP_LIVENESS: &str = "mcp.liveness";
    pub const MCP_RECONNECT: &str = "mcp.reconnect";
    pub const MCP_AUTH_SET: &str = "mcp.auth.set";
    pub const MCP_AUTH_STATUS: &str = "mcp.auth.status";
    pub const MCP_AUTH_CLEAR: &str = "mcp.auth.clear";
    pub const MCP_AUTH_OAUTH_START: &str = "mcp.auth.oauthStart";
    pub const MCP_AUTH_OAUTH_CALLBACK: &str = "mcp.auth.oauthCallback";
    pub const ARTIFACT_LIST: &str = "artifact.list";
    pub const ARTIFACT_OPEN: &str = "artifact.open";
    pub const TASK_LIST: &str = "task.list";
    pub const TASK_CANCEL: &str = "task.cancel";
    pub const TASK_WAIT: &str = "task.wait";
    pub const INTERACTION_LIST_PENDING: &str = "interaction.listPending";
    pub const INTERACTION_RESPOND: &str = "interaction.respond";
    pub const SCHEDULER_LIST: &str = "scheduler.list";
    pub const SCHEDULER_CREATE: &str = "scheduler.create";
    pub const SCHEDULER_UPDATE: &str = "scheduler.update";
    pub const SCHEDULER_DELETE: &str = "scheduler.delete";
    pub const SCHEDULER_HISTORY: &str = "scheduler.history";
    pub const SCHEDULER_TICK: &str = "scheduler.tick";
    pub const ENGINE_RATE_LIMIT_GET: &str = "engine.rateLimit.get";
    pub const ENGINE_RATE_LIMIT_UPDATE: &str = "engine.rateLimit.update";
    pub const ENGINE_RATE_LIMIT_ACQUIRE: &str = "engine.rateLimit.acquire";
    pub const ENGINE_RATE_LIMIT_COOLDOWN: &str = "engine.rateLimit.cooldown";
}

/// Returns true if `method` is a known v2 RPC method.
pub fn is_known_method(method: &str) -> bool {
    ALL_METHODS.contains(&method)
}

/// Returns true if `method` is currently implemented and advertised.
pub fn is_implemented_method(method: &str) -> bool {
    is_daemon_method(method) || is_host_method(method)
}

pub fn is_daemon_method(method: &str) -> bool {
    IMPLEMENTED_METHODS.contains(&method)
}

pub fn is_host_method(method: &str) -> bool {
    HOST_IMPLEMENTED_METHODS.contains(&method)
}

/// Classify a method for fail-closed RPC handling.
///
/// - Known + implemented → `Implemented`
/// - Known but not implemented → `Unsupported`
/// - Unknown name → treat as unsupported catalogue miss (still fail-closed)
pub fn method_status(method: &str) -> super::envelope::MethodStatus {
    use super::envelope::MethodStatus;
    if is_implemented_method(method) {
        MethodStatus::Implemented
    } else {
        MethodStatus::Unsupported
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_required_methods_are_present() {
        for required in [
            "daemon.getCapabilities",
            "provider.list",
            "provider.discoverModels",
            "provider.test",
            "conversation.create",
            "run.create",
            "run.start",
            "run.cancel",
            "run.retry",
            "run.subscribe",
            "run.replay",
            "permission.respond",
            "tool.list",
            "agent.list",
            "subagent.list",
            "extension.list",
            "extension.enable",
            "mcp.list",
            "artifact.list",
            "scheduler.list",
            "engine.rateLimit.get",
            "engine.rateLimit.update",
            "engine.rateLimit.acquire",
            "engine.rateLimit.cooldown",
        ] {
            assert!(is_known_method(required), "missing method: {required}");
        }
    }

    #[test]
    fn methods_are_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for method in ALL_METHODS {
            assert!(seen.insert(*method), "duplicate method: {method}");
        }
    }

    #[test]
    fn oauth_browser_redirect_is_explicitly_unsupported_until_handler_exists() {
        assert!(is_known_method("mcp.auth.oauthStart"));
        assert!(is_known_method("mcp.auth.oauthCallback"));
        assert!(!is_implemented_method("mcp.auth.oauthStart"));
        assert!(!is_implemented_method("mcp.auth.oauthCallback"));
        assert!(matches!(
            method_status("mcp.auth.oauthStart"),
            super::super::envelope::MethodStatus::Unsupported
        ));
    }

    #[test]
    fn known_but_unimplemented_methods_are_not_advertised() {
        // Catalogued-but-deliberately-unimplemented surface. `mcp.call` is a closed
        // transport bypass (task-06); the oauth pair is an explicit red line.
        for method in ["mcp.call", "mcp.auth.oauthStart", "mcp.auth.oauthCallback"] {
            assert!(is_known_method(method), "missing known: {method}");
            assert!(
                !is_implemented_method(method),
                "should not advertise: {method}"
            );
        }
        // Phase 3 methods are now implemented on daemon
        assert!(is_implemented_method("run.rewind"));
        assert!(is_implemented_method("run.rewindPreview"));
        assert!(is_implemented_method("conversation.getContextUsage"));
        assert!(is_implemented_method("promptQueue.interject"));
        assert!(is_implemented_method("task.list"));
        assert!(is_implemented_method("task.cancel"));
        assert!(is_implemented_method("task.wait"));
        assert!(is_implemented_method("interaction.listPending"));
        assert!(is_implemented_method("interaction.respond"));
        // Previously advertised via HOST but never intercepted by the host and never
        // dispatched by the daemon (callers got `internal_error`). Now daemon-owned.
        assert!(is_daemon_method("run.listChildren"));
        assert!(is_daemon_method("run.finish"));
        assert!(is_daemon_method("run.getActivity"));
        assert!(is_daemon_method("permission.listPending"));
        assert!(is_daemon_method("agent.list"));
        assert!(is_daemon_method("conversation.update"));
    }

    /// Every advertised method must be catalogued. A capability ad for a name that is
    /// not in `ALL_METHODS` is unreachable by definition.
    #[test]
    fn advertised_methods_are_all_catalogued() {
        for method in IMPLEMENTED_METHODS.iter().chain(HOST_IMPLEMENTED_METHODS) {
            assert!(is_known_method(method), "not in ALL_METHODS: {method}");
        }
    }

    /// Host-only entries (not also daemon-implemented) are the ones that MUST be
    /// intercepted by `src-tauri`'s `is_host_owned_method`. Keep the set tiny and
    /// obviously OS-bound so the sync burden stays reviewable.
    #[test]
    fn host_only_surface_is_os_bound_and_minimal() {
        let host_only: Vec<&str> = HOST_IMPLEMENTED_METHODS
            .iter()
            .copied()
            .filter(|m| !IMPLEMENTED_METHODS.contains(m))
            .collect();
        assert_eq!(
            host_only,
            vec!["artifact.reveal"],
            "host-only surface changed — update src-tauri is_host_owned_method and the \
             daemon dispatch contract test together"
        );
    }
}
