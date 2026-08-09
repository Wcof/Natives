//! Sub-agent permission allowlist resolution — pure functions.

pub fn default_subagent_tool_allowlist() -> Vec<String> {
    vec!["read_file".into(), "list_dir".into(), "grep".into()]
}

/// Cap a child's permission profile so it never exceeds the parent.
///
/// Privilege order: `readonly` < `ask` < `full_access`.
/// Unknown / empty values normalize to `ask`.
///
/// Child defaults remain readonly/ask even when the parent is `full_access`
/// (callers pass the requested profile; omit or pass `ask` for the default).
pub fn cap_child_permission(parent: &str, requested: &str) -> String {
    fn rank(profile: &str) -> u8 {
        match profile.trim() {
            "readonly" | "read_only" => 0,
            "full_access" | "autonomous" | "full" => 2,
            // ask / confirm_each / empty / unknown
            _ => 1,
        }
    }
    fn label(rank: u8) -> String {
        match rank {
            0 => "readonly".into(),
            2 => "full_access".into(),
            _ => "ask".into(),
        }
    }
    label(rank(parent).min(rank(requested)))
}

/// Does a tool allowlist admit `name`?
///
/// Sole definition of allowlist matching, so the runtime that *enforces* a
/// surface and the resolver that *derives* a child surface can never disagree.
/// MCP tools (`mcp__server__tool`) are admitted by their exact name or by the
/// `mcp_call` capability entry that stands for the whole MCP surface.
pub fn tool_list_allows(list: &[String], name: &str) -> bool {
    if list.iter().any(|tool| tool == name) {
        return true;
    }
    if name.starts_with("mcp__") {
        return list.iter().any(|tool| tool == "mcp_call" || tool == name);
    }
    false
}

/// Resolve a child's permission profile from the request, the selected agent
/// profile, and the parent's own profile.
///
/// Two independent one-way valves, in this order:
///
/// 1. The agent profile can only *tighten* the request. A profile declaring
///    `permissionMode: full_access` never elevates a child whose caller did not
///    explicitly ask for it — otherwise "pick a powerful persona" would be an
///    escalation primitive for a prompt-injected parent.
/// 2. The parent caps whatever survives, via [`cap_child_permission`].
///
/// An absent request floors at `ask`, matching the sub-agent default.
pub fn resolve_child_permission(
    parent: &str,
    requested: Option<&str>,
    profile_mode: Option<&str>,
) -> String {
    let mut requested = requested
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("ask")
        .to_string();
    if let Some(mode) = profile_mode
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        requested = cap_child_permission(&requested, mode);
    }
    cap_child_permission(parent, &requested)
}

/// Resolve a child's tool surface from the request, the selected agent profile,
/// and the parent's own surface.
///
/// Precedence for the starting set: an explicit request, else the profile's
/// `tools`, else the parent's surface, else the readonly default. The profile's
/// `disallowedTools` are then removed, and finally the parent's surface is a
/// hard ceiling — a child can never reach a tool the parent itself cannot call.
///
/// `parent = None` means "unrestricted surface" (a root run), so the ceiling is
/// a no-op there; the permission profile still gates every call.
pub fn resolve_child_tool_allowlist(
    parent: Option<&[String]>,
    requested: Option<&[String]>,
    profile_tools: Option<&[String]>,
    profile_disallowed: Option<&[String]>,
) -> Vec<String> {
    let mut out: Vec<String> = match (requested, profile_tools) {
        (Some(requested), _) => requested.to_vec(),
        (None, Some(tools)) => tools.to_vec(),
        (None, None) => parent
            .map(<[String]>::to_vec)
            .unwrap_or_else(default_subagent_tool_allowlist),
    };
    if let Some(denied) = profile_disallowed {
        out.retain(|tool| !denied.iter().any(|entry| entry == tool));
    }
    if let Some(parent) = parent {
        out.retain(|tool| tool_list_allows(parent, tool));
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|tool| seen.insert(tool.clone()));
    out
}
