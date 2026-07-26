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
    // Capability library (ADR-0016): configuration surface, distinct from the
    // mcp.* / skill.* runtime surfaces above.
    "capability.skill.list",
    "capability.skill.get",
    "capability.skill.update",
    "capability.skill.import",
    "capability.skill.delete",
    "capability.skill.rescan",
    "capability.mcp.list",
    "capability.mcp.get",
    "capability.mcp.create",
    "capability.mcp.update",
    "capability.mcp.delete",
    "capability.mcp.importJson",
    "capability.mcp.hub.search",
    "capability.mcp.hub.get",
    "capability.mcp.hub.install",
    "capability.expert.list",
    "capability.expert.get",
    "capability.expert.create",
    "capability.expert.update",
    "capability.expert.delete",
    "capability.expert.importMd",
    "capability.expert.exportMd",
    "capability.team.list",
    "capability.team.get",
    "capability.team.create",
    "capability.team.update",
    "capability.team.delete",
    "conversation.updateCapabilities",
    "conversation.getCapabilities",
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
    "permission.respond",
    "tool.list",
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
    // Capability library (ADR-0016) — advertisement must never outrun
    // implementation; hub methods joined once capability::hub shipped.
    "capability.skill.list",
    "capability.skill.get",
    "capability.skill.update",
    "capability.skill.import",
    "capability.skill.delete",
    "capability.skill.rescan",
    "capability.mcp.list",
    "capability.mcp.get",
    "capability.mcp.create",
    "capability.mcp.update",
    "capability.mcp.delete",
    "capability.mcp.importJson",
    "capability.mcp.hub.search",
    "capability.mcp.hub.get",
    "capability.mcp.hub.install",
    "capability.expert.list",
    "capability.expert.get",
    "capability.expert.create",
    "capability.expert.update",
    "capability.expert.delete",
    "capability.expert.importMd",
    "capability.expert.exportMd",
    "capability.team.list",
    "capability.team.get",
    "capability.team.create",
    "capability.team.update",
    "capability.team.delete",
    "conversation.updateCapabilities",
    "conversation.getCapabilities",
];

/// Methods the Tauri host still owns after Phase 0 cutover.
///
/// Production (`NATIVES_DAEMON_MODE=uds|sidecar|remote`): conversation / promptQueue /
/// interaction / task are daemon-owned and must NOT be re-advertised here as host-primary.
/// Embedded/test mode may still execute host fallbacks for in-process DataStore tests,
/// but capability ads stay honest: only OS + run preflight / projection seams.
pub const HOST_IMPLEMENTED_METHODS: &[&str] = &[
    // Run preflight + host projection boundary
    "run.start",
    "run.subscribe",
    "run.listChildren",
    "run.finish",
    // Permission UI may hit host while projecting; daemon also implements respond.
    "permission.listPending",
    "permission.respond",
    "interaction.listPending",
    "interaction.respond",
    // OS-bound artifact actions
    "artifact.open",
    "artifact.reveal",
    // MCP OAuth browser flow (ADR-0016 decision 7): the Host owns the system
    // browser + loopback callback; the daemon only receives the access token
    // via mcp.auth.set.
    "mcp.auth.oauthStart",
    "mcp.auth.oauthCallback",
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
    pub const RUN_REWIND: &str = "run.rewind";
    pub const RUN_REWIND_PREVIEW: &str = "run.rewindPreview";
    pub const WORKSPACE_RESTORE: &str = "workspace.restore";
    pub const WORKSPACE_RESTORE_PREVIEW: &str = "workspace.restorePreview";
    pub const PERMISSION_RESPOND: &str = "permission.respond";
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
    pub const CAPABILITY_SKILL_LIST: &str = "capability.skill.list";
    pub const CAPABILITY_SKILL_GET: &str = "capability.skill.get";
    pub const CAPABILITY_SKILL_UPDATE: &str = "capability.skill.update";
    pub const CAPABILITY_SKILL_IMPORT: &str = "capability.skill.import";
    pub const CAPABILITY_SKILL_DELETE: &str = "capability.skill.delete";
    pub const CAPABILITY_SKILL_RESCAN: &str = "capability.skill.rescan";
    pub const CAPABILITY_MCP_LIST: &str = "capability.mcp.list";
    pub const CAPABILITY_MCP_GET: &str = "capability.mcp.get";
    pub const CAPABILITY_MCP_CREATE: &str = "capability.mcp.create";
    pub const CAPABILITY_MCP_UPDATE: &str = "capability.mcp.update";
    pub const CAPABILITY_MCP_DELETE: &str = "capability.mcp.delete";
    pub const CAPABILITY_MCP_IMPORT_JSON: &str = "capability.mcp.importJson";
    pub const CAPABILITY_MCP_HUB_SEARCH: &str = "capability.mcp.hub.search";
    pub const CAPABILITY_MCP_HUB_GET: &str = "capability.mcp.hub.get";
    pub const CAPABILITY_MCP_HUB_INSTALL: &str = "capability.mcp.hub.install";
    pub const CAPABILITY_EXPERT_LIST: &str = "capability.expert.list";
    pub const CAPABILITY_EXPERT_GET: &str = "capability.expert.get";
    pub const CAPABILITY_EXPERT_CREATE: &str = "capability.expert.create";
    pub const CAPABILITY_EXPERT_UPDATE: &str = "capability.expert.update";
    pub const CAPABILITY_EXPERT_DELETE: &str = "capability.expert.delete";
    pub const CAPABILITY_EXPERT_IMPORT_MD: &str = "capability.expert.importMd";
    pub const CAPABILITY_EXPERT_EXPORT_MD: &str = "capability.expert.exportMd";
    pub const CAPABILITY_TEAM_LIST: &str = "capability.team.list";
    pub const CAPABILITY_TEAM_GET: &str = "capability.team.get";
    pub const CAPABILITY_TEAM_CREATE: &str = "capability.team.create";
    pub const CAPABILITY_TEAM_UPDATE: &str = "capability.team.update";
    pub const CAPABILITY_TEAM_DELETE: &str = "capability.team.delete";
    pub const CONVERSATION_UPDATE_CAPABILITIES: &str = "conversation.updateCapabilities";
    pub const CONVERSATION_GET_CAPABILITIES: &str = "conversation.getCapabilities";
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
    fn oauth_browser_redirect_is_host_implemented() {
        // Host-side handler exists (mcp_oauth_start command): the browser +
        // loopback flow is owned by the Tauri host, never the daemon.
        assert!(is_known_method("mcp.auth.oauthStart"));
        assert!(is_known_method("mcp.auth.oauthCallback"));
        assert!(is_host_method("mcp.auth.oauthStart"));
        assert!(is_host_method("mcp.auth.oauthCallback"));
        assert!(!is_daemon_method("mcp.auth.oauthStart"));
        assert!(!is_daemon_method("mcp.auth.oauthCallback"));
        assert!(matches!(
            method_status("mcp.auth.oauthStart"),
            super::super::envelope::MethodStatus::Implemented
        ));
    }

    #[test]
    fn known_but_unimplemented_methods_are_not_advertised() {
        for method in ["run.getActivity"] {
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
    }
}
