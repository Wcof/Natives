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
    "run.continue",
    "run.resume",
    "run.subscribe",
    "run.watch",
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
    "mcp.resources.list",
    "mcp.resources.read",
    "mcp.resources.templates.list",
    "mcp.prompts.list",
    "mcp.prompts.get",
    "mcp.roots.list",
    "mcp.notifications.list",
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
    // Harness control plane. Read surface first (topology / catalog), then the
    // Draft → Validate → Diff → Publish → Rollback lifecycle, bindings, and the
    // per-Run evidence lookup.
    "harness.overview",
    "harness.topology",
    "harness.workspace.get",
    "harness.template.list",
    "harness.hook.catalog",
    "harness.profile.list",
    "harness.profile.get",
    "harness.profile.create",
    "harness.profile.archive",
    "harness.draft.get",
    "harness.draft.save",
    "harness.draft.validate",
    "harness.draft.diff",
    "harness.draft.review",
    "harness.draft.simulate",
    "harness.draft.publish",
    "harness.version.list",
    "harness.version.rollback",
    "harness.binding.get",
    "harness.binding.set",
    "harness.run.getSnapshot",
    "harness.audit.list",
    "harness.prompt.preview",
    "harness.source.list",
    "harness.source.acknowledgeDrift",
    "harness.external.inspect",
    "harness.subscribe",
    "harness.trace.list",
    "harness.audit.export",
    "project.identity.register",
    "project.identity.list",
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
    // Local-creative AI analysis (P0): Host → Daemon controlled provider call.
    "creative.local.analyze",
    // Creative proposal facts (T06): the Daemon persists a typed proposal fact
    // when a `creative_proposal` tool call completes; the Host pulls pending
    // facts over UDS to validate and persist its pending approval inbox.
    "proposal.listPending",
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
    "run.continue",
    "run.resume",
    "run.subscribe",
    "run.watch",
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
    // MCP protocol surface beyond tools. `resources`/`prompts` are read-only
    // discovery plus a gated read; they are advertised because the daemon really
    // dispatches them. `mcp.call` stays absent — tool invocation has side effects
    // and must go through the agent's permission gate, a read does not.
    "mcp.resources.list",
    "mcp.resources.read",
    "mcp.resources.templates.list",
    "mcp.prompts.list",
    "mcp.prompts.get",
    "mcp.roots.list",
    "mcp.notifications.list",
    "extension.list",
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
    // Harness control plane (design Phase 2). Every one of these has a real
    // dispatch arm backed by `assistant.db` rows, the fixed topology constant,
    // or on-disk Hook discovery — `rpc_dispatch_contract.rs` proves it by
    // calling each through the live `handle_rpc`.
    "harness.overview",
    "harness.topology",
    "harness.workspace.get",
    "harness.template.list",
    "harness.hook.catalog",
    "harness.profile.list",
    "harness.profile.get",
    "harness.profile.create",
    "harness.profile.archive",
    "harness.draft.get",
    "harness.draft.save",
    "harness.draft.validate",
    "harness.draft.diff",
    "harness.draft.review",
    "harness.draft.simulate",
    "harness.draft.publish",
    "harness.version.list",
    "harness.version.rollback",
    "harness.binding.get",
    "harness.binding.set",
    "harness.run.getSnapshot",
    "harness.audit.list",
    "harness.prompt.preview",
    "harness.source.list",
    "harness.source.acknowledgeDrift",
    "harness.external.inspect",
    "harness.subscribe",
    "harness.trace.list",
    "harness.audit.export",
    "project.identity.register",
    "project.identity.list",
    // Capability library (ADR-0016) — advertisement must never outrun
    // implementation. `rpc.rs` routes the whole family by the `capability.`
    // prefix, guarded by `is_implemented_method`, into `capability::request`,
    // which has a real arm for every name listed here (hub included).
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
    "creative.local.analyze",
    "proposal.listPending",
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
    // Deliberately absent: `mcp.auth.oauthStart` / `mcp.auth.oauthCallback`.
    // ADR-0016 decision 7 is correct that the *browser + loopback callback* is
    // Host-owned, but it ships as the Tauri command `mcp_oauth_start`
    // (`src-tauri/src/commands/mcp_oauth.rs`, registered in `lib.rs`) — invoked
    // directly by the frontend, never routed through this RPC surface.
    // `src-tauri`'s `is_host_owned_method` does NOT match either name, so
    // listing them here would advertise a host interception that does not
    // happen: the call would fall through to the daemon, which has no arm, and
    // the caller would get `internal_error` instead of an honest `unsupported`.
    // Both names stay catalogued in `ALL_METHODS` and unadvertised. To promote
    // them, add them to `is_host_owned_method` + `dispatch_rpc` first, then to
    // `HOST_INTERCEPTED` in `src-agent-daemon/tests/rpc_dispatch_contract.rs`.
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
    pub const RUN_CONTINUE: &str = "run.continue";
    pub const RUN_RESUME: &str = "run.resume";
    pub const RUN_SUBSCRIBE: &str = "run.subscribe";
    pub const RUN_WATCH: &str = "run.watch";
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
    pub const MCP_RESOURCES_LIST: &str = "mcp.resources.list";
    pub const MCP_RESOURCES_READ: &str = "mcp.resources.read";
    pub const MCP_RESOURCES_TEMPLATES_LIST: &str = "mcp.resources.templates.list";
    pub const MCP_PROMPTS_LIST: &str = "mcp.prompts.list";
    pub const MCP_PROMPTS_GET: &str = "mcp.prompts.get";
    pub const MCP_ROOTS_LIST: &str = "mcp.roots.list";
    pub const MCP_NOTIFICATIONS_LIST: &str = "mcp.notifications.list";
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

    /// Prefix shared by the whole Harness control-plane family.
    ///
    /// `rpc.rs` routes on this prefix rather than on eighteen literals, so a
    /// method added to the daemon's own dispatch table cannot be left
    /// unroutable by a forgotten match arm. The advertisement in
    /// [`super::IMPLEMENTED_METHODS`] stays explicit, so the prefix can never
    /// silently widen what is claimed to be callable.
    pub const HARNESS_PREFIX: &str = "harness.";
    pub const HARNESS_OVERVIEW: &str = "harness.overview";
    pub const HARNESS_TOPOLOGY: &str = "harness.topology";
    pub const HARNESS_WORKSPACE_GET: &str = "harness.workspace.get";
    pub const HARNESS_TEMPLATE_LIST: &str = "harness.template.list";
    pub const HARNESS_HOOK_CATALOG: &str = "harness.hook.catalog";
    pub const HARNESS_PROFILE_LIST: &str = "harness.profile.list";
    pub const HARNESS_PROFILE_GET: &str = "harness.profile.get";
    pub const HARNESS_PROFILE_CREATE: &str = "harness.profile.create";
    pub const HARNESS_PROFILE_ARCHIVE: &str = "harness.profile.archive";
    pub const HARNESS_DRAFT_GET: &str = "harness.draft.get";
    pub const HARNESS_DRAFT_SAVE: &str = "harness.draft.save";
    pub const HARNESS_DRAFT_VALIDATE: &str = "harness.draft.validate";
    pub const HARNESS_DRAFT_DIFF: &str = "harness.draft.diff";
    pub const HARNESS_DRAFT_REVIEW: &str = "harness.draft.review";
    pub const HARNESS_DRAFT_SIMULATE: &str = "harness.draft.simulate";
    pub const HARNESS_DRAFT_PUBLISH: &str = "harness.draft.publish";
    pub const HARNESS_VERSION_LIST: &str = "harness.version.list";
    pub const HARNESS_VERSION_ROLLBACK: &str = "harness.version.rollback";
    pub const HARNESS_BINDING_GET: &str = "harness.binding.get";
    pub const HARNESS_BINDING_SET: &str = "harness.binding.set";
    pub const HARNESS_RUN_GET_SNAPSHOT: &str = "harness.run.getSnapshot";
    pub const HARNESS_AUDIT_LIST: &str = "harness.audit.list";
    pub const HARNESS_PROMPT_PREVIEW: &str = "harness.prompt.preview";
    pub const HARNESS_SOURCE_LIST: &str = "harness.source.list";
    pub const HARNESS_SOURCE_ACKNOWLEDGE_DRIFT: &str = "harness.source.acknowledgeDrift";
    pub const HARNESS_EXTERNAL_INSPECT: &str = "harness.external.inspect";
    pub const HARNESS_SUBSCRIBE: &str = "harness.subscribe";
    pub const HARNESS_TRACE_LIST: &str = "harness.trace.list";
    pub const HARNESS_AUDIT_EXPORT: &str = "harness.audit.export";
    pub const PROJECT_IDENTITY_REGISTER: &str = "project.identity.register";
    pub const PROJECT_IDENTITY_LIST: &str = "project.identity.list";

    // Capability library (ADR-0016).
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
    pub const CREATIVE_LOCAL_ANALYZE: &str = "creative.local.analyze";
    pub const PROPOSAL_LIST_PENDING: &str = "proposal.listPending";
}

/// Every advertised Harness method, in the order the design lists them.
///
/// Named separately from [`IMPLEMENTED_METHODS`] so a test can assert that the
/// prefix-routed family and the advertised family are the same set — the
/// failure mode a prefix route otherwise invites.
pub const HARNESS_METHODS: &[&str] = &[
    names::HARNESS_OVERVIEW,
    names::HARNESS_TOPOLOGY,
    names::HARNESS_WORKSPACE_GET,
    names::HARNESS_TEMPLATE_LIST,
    names::HARNESS_HOOK_CATALOG,
    names::HARNESS_PROFILE_LIST,
    names::HARNESS_PROFILE_GET,
    names::HARNESS_PROFILE_CREATE,
    names::HARNESS_PROFILE_ARCHIVE,
    names::HARNESS_DRAFT_GET,
    names::HARNESS_DRAFT_SAVE,
    names::HARNESS_DRAFT_VALIDATE,
    names::HARNESS_DRAFT_DIFF,
    names::HARNESS_DRAFT_REVIEW,
    names::HARNESS_DRAFT_SIMULATE,
    names::HARNESS_DRAFT_PUBLISH,
    names::HARNESS_VERSION_LIST,
    names::HARNESS_VERSION_ROLLBACK,
    names::HARNESS_BINDING_GET,
    names::HARNESS_BINDING_SET,
    names::HARNESS_RUN_GET_SNAPSHOT,
    names::HARNESS_AUDIT_LIST,
    names::HARNESS_PROMPT_PREVIEW,
    names::HARNESS_SOURCE_LIST,
    names::HARNESS_SOURCE_ACKNOWLEDGE_DRIFT,
    names::HARNESS_EXTERNAL_INSPECT,
    names::HARNESS_SUBSCRIBE,
    names::HARNESS_TRACE_LIST,
    names::HARNESS_AUDIT_EXPORT,
];

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
            "run.continue",
            "run.resume",
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

    /// The OAuth browser flow is real, but it is a **Tauri command**
    /// (`mcp_oauth_start`), not an RPC method: nothing in `is_host_owned_method`
    /// matches these names, so advertising them as host-implemented would route
    /// live calls to a daemon that has no arm. Catalogued, never advertised.
    #[test]
    fn oauth_browser_redirect_is_catalogued_but_not_advertised() {
        assert!(is_known_method("mcp.auth.oauthStart"));
        assert!(is_known_method("mcp.auth.oauthCallback"));
        assert!(!is_host_method("mcp.auth.oauthStart"));
        assert!(!is_host_method("mcp.auth.oauthCallback"));
        assert!(!is_daemon_method("mcp.auth.oauthStart"));
        assert!(!is_daemon_method("mcp.auth.oauthCallback"));
        assert!(matches!(
            method_status("mcp.auth.oauthStart"),
            super::super::envelope::MethodStatus::Unsupported
        ));
        assert!(matches!(
            method_status("mcp.auth.oauthCallback"),
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

    /// The MCP surface beyond tools is advertised only because the daemon really
    /// dispatches it. Kept as an explicit list so shrinking it is a deliberate edit.
    #[test]
    fn mcp_protocol_surface_beyond_tools_is_advertised() {
        for method in [
            "mcp.resources.list",
            "mcp.resources.read",
            "mcp.resources.templates.list",
            "mcp.prompts.list",
            "mcp.prompts.get",
            "mcp.roots.list",
            "mcp.notifications.list",
        ] {
            assert!(is_known_method(method), "not catalogued: {method}");
            assert!(is_daemon_method(method), "not advertised: {method}");
        }
    }

    /// `sampling/createMessage` and `elicitation/create` are the two MCP calls
    /// where the **server** drives the client: one spends local inference on a
    /// remote server's prompt, the other puts a remote server's question in front
    /// of the user. Both are permission-model changes, not protocol coverage, and
    /// neither has a consent surface here yet. They must not appear in the
    /// catalogue at all — a catalogued name is a promise of intent.
    #[test]
    fn server_initiated_sampling_and_elicitation_are_not_catalogued() {
        for method in [
            "mcp.sampling.createMessage",
            "mcp.elicitation.create",
            "mcp.sampling.respond",
        ] {
            assert!(
                !is_known_method(method),
                "{method} appeared in ALL_METHODS — server-driven MCP calls need a \
                 consent design before they are catalogued"
            );
            assert!(
                !is_implemented_method(method),
                "should not advertise: {method}"
            );
        }
    }

    /// Every advertised method must be catalogued. A capability ad for a name that is
    /// not in `ALL_METHODS` is unreachable by definition.
    #[test]
    fn advertised_methods_are_all_catalogued() {
        for method in IMPLEMENTED_METHODS.iter().chain(HOST_IMPLEMENTED_METHODS) {
            assert!(is_known_method(method), "not in ALL_METHODS: {method}");
        }
    }

    /// `rpc.rs` routes the Harness family by prefix. That is only safe while the
    /// prefix and the advertised list describe the same set: a name matching the
    /// prefix but missing from `IMPLEMENTED_METHODS` would be dispatchable yet
    /// unadvertised (the frontend gate stays shut on a working method), and one
    /// advertised but not matching the prefix would fall through to the
    /// fail-closed arm (the gate opens on a dead method).
    #[test]
    fn the_harness_prefix_and_the_advertised_harness_family_agree() {
        for method in HARNESS_METHODS {
            assert!(
                method.starts_with(names::HARNESS_PREFIX),
                "{method} is in HARNESS_METHODS but would not be prefix-routed"
            );
            assert!(is_known_method(method), "not catalogued: {method}");
            assert!(is_daemon_method(method), "not advertised: {method}");
        }
        let advertised: Vec<&str> = IMPLEMENTED_METHODS
            .iter()
            .copied()
            .filter(|m| m.starts_with(names::HARNESS_PREFIX))
            .collect();
        assert_eq!(
            advertised,
            HARNESS_METHODS.to_vec(),
            "the advertised harness surface drifted from HARNESS_METHODS"
        );
        assert!(
            !HOST_IMPLEMENTED_METHODS
                .iter()
                .any(|m| m.starts_with(names::HARNESS_PREFIX)),
            "Harness is daemon-owned; the host must not advertise any of it"
        );
    }

    /// The Harness read surface must cover both questions the control plane
    /// exists to answer. Naming them here makes removing either a deliberate
    /// edit rather than a quiet regression in a UI refactor.
    #[test]
    fn the_harness_inspection_surface_is_present() {
        for method in [
            // "what does the engine look like at each stage?"
            "harness.topology",
            // "which stage uses a hook, and which hook?"
            "harness.hook.catalog",
            // "what did this Run actually run with?"
            "harness.run.getSnapshot",
        ] {
            assert!(is_daemon_method(method), "{method} is no longer advertised");
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
