//! Run-level capability resolution (ADR-0016).
//!
//! Sits exactly at the Harness control-plane's frozen insertion point
//! (`run.start → RunManager validation → resolve → runtime dispatch`): merges
//! the run-level selection override with the conversation default, validates
//! every referenced capability, and produces a `ResolvedCapabilitySnapshot`
//! that all three runtimes consume. Fail-closed: an explicit selection that
//! cannot be honoured fails the run before any provider call (ADR-0011 —
//! silent degradation is indistinguishable from mock success).

use agent_core::profile::AgentProfile;
use assistant_protocol::v2::CapabilitySelection;
use rusqlite::OptionalExtension;
use serde_json::{json, Value};
use std::path::Path;

/// Structured resolve failure: (error_code, human reason).
#[derive(Debug, Clone)]
pub struct ResolveError {
    pub code: String,
    pub reason: String,
}

impl ResolveError {
    fn new(sub_code: &str, reason: impl Into<String>) -> Self {
        Self {
            code: format!("CAPABILITY_RESOLVE_FAILED:{sub_code}"),
            reason: reason.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedTeamMember {
    pub expert_id: String,
    pub name: String,
    pub description: String,
    pub role_hint: String,
}

#[derive(Debug, Clone)]
pub struct ResolvedTeam {
    pub team_id: String,
    pub lead_expert_id: String,
    pub members: Vec<ResolvedTeamMember>,
    pub failure_policy: String,
    pub max_concurrent: u32,
}

/// Audit-honest resolution result. Ids and prompts only — never secrets or
/// connector config bodies.
#[derive(Debug, Clone, Default)]
pub struct ResolvedCapabilitySnapshot {
    /// True when an explicit selection (run or conversation level) was applied.
    pub selection_active: bool,
    pub agent_profile_id: Option<String>,
    /// Fully resolved profile (DB authority; file fallback).
    pub profile: Option<AgentProfile>,
    pub team: Option<ResolvedTeam>,
    pub skill_ids: Vec<String>,
    /// Some(_) = selection-scoped skill prompt replaces the legacy global
    /// injection; None = legacy `prompt_for_project` behaviour.
    pub skill_prompt: Option<String>,
    pub mcp_servers: Vec<String>,
    /// Namespaced `mcp__{server}__{tool}` schemas for the selected servers,
    /// surfaced to the model by the tool runtime.
    pub mcp_tool_schemas: Vec<agent_core::engine::ToolSchema>,
    /// Team roster / delegation instructions appended to the system prompt.
    pub extra_system_prompt: Option<String>,
}

impl ResolvedCapabilitySnapshot {
    /// Audit projection persisted on the run row (`capability_snapshot_json`).
    pub fn to_audit_json(&self) -> Value {
        json!({
            "selectionActive": self.selection_active,
            "agentProfileId": self.agent_profile_id,
            "teamId": self.team.as_ref().map(|t| t.team_id.clone()),
            "teamMembers": self.team.as_ref().map(|t| {
                t.members.iter().map(|m| m.expert_id.clone()).collect::<Vec<_>>()
            }),
            "skillIds": self.skill_ids,
            "mcpServers": self.mcp_servers,
        })
    }
}

/// Which capabilities each runtime can actually honour. Advertised through
/// daemon.getCapabilities and enforced here — UI gating alone is not enough.
pub fn runtime_supports(runtime_id: &str, capability: &str) -> bool {
    let _ = capability;
    match runtime_id {
        "native" => true,
        // claude_cli honours the selection through its own flags
        // (--append-system-prompt / --agents / --mcp-config); executed by the
        // CLI's harness, honestly labelled in the capability matrix below.
        "claude_cli" => true,
        _ => false,
    }
}

/// Per-runtime capability matrix for daemon.getCapabilities. Mechanism labels
/// keep the advertisement honest: claude_cli runs capabilities through its own
/// harness — tool approval does NOT pass through the native capability gateway.
pub fn runtime_capability_matrix() -> Value {
    json!({
        "native": {
            "expert": true, "team": true, "skills": true, "mcp": true,
            "mechanism": "native_gateway",
        },
        "claude_cli": {
            "expert": true, "team": true, "skills": true, "mcp": true,
            "mechanism": "cli_flags",
            "executionBackend": "claude_cli_harness",
            "note": "capabilities injected via CLI flags; approvals handled by the Claude CLI, not the native capability gateway",
        },
        "codex_cli": {
            "expert": false, "team": false, "skills": false, "mcp": false,
            "mechanism": "unsupported",
        },
    })
}

/// Resolve the effective selection for a run start.
///
/// Priority: run-level override > conversation default > None (legacy).
/// `explicit_profile_id` is the seam-A `agent_profile_id` field, kept for
/// callers that address an expert directly without a selection object.
pub fn resolve(
    run_selection: Option<&CapabilitySelection>,
    conversation_id: &str,
    explicit_profile_id: Option<&str>,
    project_root: Option<&Path>,
    runtime_id: &str,
) -> Result<ResolvedCapabilitySnapshot, ResolveError> {
    let conversation_default = get_conversation_selection(conversation_id);
    let selection = run_selection
        .cloned()
        .or(conversation_default)
        .filter(|s| !s.is_empty());

    let mut snapshot = ResolvedCapabilitySnapshot::default();

    // Seam A without a selection object: expert addressed directly. Expert-
    // bound skills still apply (their declaration is part of the persona);
    // subagents therefore get exactly their member skills, never the parent
    // conversation's selection (skill_store isolation principle).
    if selection.is_none() {
        if let Some(profile_id) = explicit_profile_id.filter(|s| !s.is_empty()) {
            snapshot.agent_profile_id = Some(profile_id.to_string());
            snapshot.profile = load_profile(profile_id, project_root);
            let Some(profile) = snapshot.profile.as_ref() else {
                return Err(ResolveError::new(
                    "EXPERT_NOT_FOUND",
                    format!("agent profile not found: {profile_id}"),
                ));
            };
            if let Some(skills) = profile.skills.clone().filter(|s| !s.is_empty()) {
                match crate::capability::skills::prompt_for_selection(&skills) {
                    Ok(prompt) => {
                        snapshot.skill_prompt = Some(prompt);
                        snapshot.skill_ids = skills;
                    }
                    Err(missing) => {
                        return Err(ResolveError::new(
                            "SKILL_NOT_FOUND",
                            format!(
                                "expert '{profile_id}' declares unavailable skills: {}",
                                missing.join(", ")
                            ),
                        ));
                    }
                }
            }
        }
        return Ok(snapshot);
    }
    let selection = selection.unwrap();
    selection
        .validate()
        .map_err(|e| ResolveError::new("INVALID_SELECTION", e))?;

    // Runtime capability matrix: fail-closed before any provider work.
    if runtime_id != "native" {
        let wanted: Vec<&str> = [
            selection.expert_id.as_ref().map(|_| "expert"),
            selection.team_id.as_ref().map(|_| "team"),
            selection
                .skills
                .as_ref()
                .filter(|s| !s.is_empty())
                .map(|_| "skills"),
            selection
                .mcp_servers
                .as_ref()
                .filter(|s| !s.is_empty())
                .map(|_| "mcp"),
        ]
        .into_iter()
        .flatten()
        .collect();
        for capability in wanted {
            if !runtime_supports(runtime_id, capability) {
                return Err(ResolveError {
                    code: format!("CAPABILITY_UNSUPPORTED_BY_RUNTIME:{capability}@{runtime_id}"),
                    reason: format!(
                        "runtime '{runtime_id}' cannot honour the selected {capability}; run with the native engine or clear the selection"
                    ),
                });
            }
        }
    }
    snapshot.selection_active = true;

    // Expert or team → lead persona.
    if let Some(team_id) = selection.team_id.as_deref() {
        let team = resolve_team(team_id, project_root)?;
        snapshot.agent_profile_id = Some(team.lead_expert_id.clone());
        snapshot.profile = load_profile(&team.lead_expert_id, project_root);
        if snapshot.profile.is_none() {
            return Err(ResolveError::new(
                "EXPERT_NOT_FOUND",
                format!("team lead expert not found: {}", team.lead_expert_id),
            ));
        }
        snapshot.extra_system_prompt = Some(team_roster_prompt(&team));
        snapshot.team = Some(team);
    } else if let Some(expert_id) = selection
        .expert_id
        .as_deref()
        .or(explicit_profile_id)
        .filter(|s| !s.is_empty())
    {
        snapshot.agent_profile_id = Some(expert_id.to_string());
        snapshot.profile = load_profile(expert_id, project_root);
        if snapshot.profile.is_none() {
            return Err(ResolveError::new(
                "EXPERT_NOT_FOUND",
                format!("expert not found: {expert_id}"),
            ));
        }
    }

    // Effective skills = expert-bound skills ∪ session selection. Both are
    // explicit declarations; missing/untrusted/disabled ids fail the run.
    let mut skill_ids: Vec<String> = Vec::new();
    if let Some(profile_skills) = snapshot.profile.as_ref().and_then(|p| p.skills.clone()) {
        skill_ids.extend(profile_skills);
    }
    if let Some(selected) = &selection.skills {
        skill_ids.extend(selected.iter().cloned());
    }
    skill_ids.sort();
    skill_ids.dedup();
    if selection.skills.is_some() || !skill_ids.is_empty() {
        match crate::capability::skills::prompt_for_selection(&skill_ids) {
            Ok(prompt) => {
                snapshot.skill_prompt = Some(prompt);
                snapshot.skill_ids = skill_ids;
            }
            Err(missing) => {
                return Err(ResolveError::new(
                    "SKILL_NOT_FOUND",
                    format!(
                        "skills unavailable (missing/disabled/untrusted): {}",
                        missing.join(", ")
                    ),
                ));
            }
        }
    }

    // MCP servers: rows must exist, be enabled and trusted; then the runtime
    // must actually be able to start them and expose their tools. Any failure
    // fails the run before provider work (selection is a contract).
    if let Some(servers) = &selection.mcp_servers {
        for server_id in servers {
            let row = crate::capability::mcp::get(&json!({ "id": server_id }))
                .map_err(|e| ResolveError::new("MCP_NOT_FOUND", e))?;
            let server = &row["server"];
            if server["enabled"] != json!(true) {
                return Err(ResolveError::new(
                    "MCP_DISABLED",
                    format!("mcp server disabled: {server_id}"),
                ));
            }
            if server["trusted"] != json!(true) {
                return Err(ResolveError::new(
                    "MCP_UNTRUSTED",
                    format!("mcp server not trusted: {server_id}; review it in the capability hub first"),
                ));
            }
        }
        // Only the native engine consumes daemon-hosted MCP sessions; the
        // claude CLI spawns its own servers from the generated --mcp-config.
        if !servers.is_empty() && runtime_id == "native" {
            let runtime = crate::mcp_runtime::global_mcp();
            for server_id in servers {
                if runtime.server_status(server_id).as_deref() != Some("running") {
                    runtime.start(server_id).map_err(|e| {
                        ResolveError::new(
                            "MCP_START_FAILED",
                            format!("mcp server '{server_id}' failed to start: {e}"),
                        )
                    })?;
                }
            }
            // Namespaced schemas for the selected servers only.
            snapshot.mcp_tool_schemas = runtime
                .namespaced_tools()
                .into_iter()
                .filter(|(name, _, _)| {
                    name.strip_prefix("mcp__")
                        .and_then(|rest| rest.split("__").next())
                        .map(|server| servers.iter().any(|s| s == server))
                        .unwrap_or(false)
                })
                .map(
                    |(name, description, input_schema)| agent_core::engine::ToolSchema {
                        name,
                        description,
                        input_schema,
                    },
                )
                .collect();
        }
        snapshot.mcp_servers = servers.clone();
    }

    Ok(snapshot)
}

/// DB-authoritative profile load with file fallback (ADR-0016 decision 3).
pub fn load_profile(id: &str, project_root: Option<&Path>) -> Option<AgentProfile> {
    if let Some(profile) = load_expert_profile_from_db(id) {
        return Some(profile);
    }
    agent_core::load_agent_profile(id, project_root)
}

fn load_expert_profile_from_db(id: &str) -> Option<AgentProfile> {
    let expert = crate::capability::experts::get(&json!({ "id": id })).ok()?;
    let expert = expert.get("expert")?;
    if expert.get("enabled") != Some(&json!(true)) {
        return None;
    }
    let as_vec = |v: &Value| -> Option<Vec<String>> {
        let items: Vec<String> = v
            .as_array()?
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
        if items.is_empty() {
            None
        } else {
            Some(items)
        }
    };
    let params = expert.get("params").cloned().unwrap_or_else(|| json!({}));
    let param_str = |key: &str| params.get(key).and_then(Value::as_str).map(str::to_string);
    let param_u64 = |key: &str| params.get(key).and_then(Value::as_u64);
    Some(AgentProfile {
        id: expert.get("id")?.as_str()?.to_string(),
        name: expert
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        description: expert
            .get("description")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        prompt_mode: param_str("promptMode"),
        system_prompt: expert
            .get("systemPrompt")
            .and_then(Value::as_str)
            .map(str::to_string),
        tools: expert.get("tools").and_then(|v| as_vec(v)),
        disallowed_tools: expert.get("disallowedTools").and_then(|v| as_vec(v)),
        permission_mode: expert
            .get("permissionMode")
            .and_then(Value::as_str)
            .map(str::to_string),
        skills: expert.get("skills").and_then(|v| as_vec(v)),
        provider_id: expert
            .get("providerId")
            .and_then(Value::as_str)
            .map(str::to_string),
        key_id: expert
            .get("keyId")
            .and_then(Value::as_str)
            .map(str::to_string),
        model_id: expert
            .get("modelId")
            .and_then(Value::as_str)
            .map(str::to_string),
        base_url_override: param_str("baseUrlOverride"),
        context_mode: param_str("contextMode"),
        isolation_mode: param_str("isolationMode"),
        max_steps: param_u64("maxSteps").and_then(|v| u32::try_from(v).ok()),
        max_duration: param_u64("maxDuration"),
        token_budget: param_u64("tokenBudget"),
        completion_requirement: param_str("completionRequirement"),
        body: expert
            .get("systemPrompt")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        source_path: None,
    })
}

fn resolve_team(team_id: &str, _project_root: Option<&Path>) -> Result<ResolvedTeam, ResolveError> {
    let team = crate::capability::experts::team_get(&json!({ "id": team_id }))
        .map_err(|e| ResolveError::new("TEAM_NOT_FOUND", e))?;
    let team = &team["team"];
    if team["enabled"] != json!(true) {
        return Err(ResolveError::new(
            "TEAM_DISABLED",
            format!("team disabled: {team_id}"),
        ));
    }
    let members_json = team["members"].as_array().cloned().unwrap_or_default();
    if members_json.is_empty() {
        return Err(ResolveError::new(
            "TEAM_MEMBER_INVALID",
            format!("team has no members: {team_id}"),
        ));
    }
    let mut members = Vec::new();
    for member in &members_json {
        let expert_id = member["expertId"].as_str().unwrap_or_default().to_string();
        let expert =
            crate::capability::experts::get(&json!({ "id": expert_id })).map_err(|_| {
                ResolveError::new(
                    "TEAM_MEMBER_INVALID",
                    format!("team member expert missing: {expert_id}"),
                )
            })?;
        let expert = &expert["expert"];
        if expert["enabled"] != json!(true) {
            return Err(ResolveError::new(
                "TEAM_MEMBER_INVALID",
                format!("team member disabled: {expert_id}"),
            ));
        }
        members.push(ResolvedTeamMember {
            expert_id,
            name: expert["name"].as_str().unwrap_or_default().to_string(),
            description: expert["description"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            role_hint: member["roleHint"].as_str().unwrap_or_default().to_string(),
        });
    }
    let lead = team["coordinatorExpertId"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| members[0].expert_id.clone());
    Ok(ResolvedTeam {
        team_id: team_id.to_string(),
        lead_expert_id: lead,
        members,
        failure_policy: team["failurePolicy"]
            .as_str()
            .unwrap_or("isolate")
            .to_string(),
        max_concurrent: team["maxConcurrent"].as_u64().unwrap_or(3) as u32,
    })
}

fn team_roster_prompt(team: &ResolvedTeam) -> String {
    let mut out = String::from(
        "## Team roster\nYou lead this expert team. Delegate subtasks with the `task` tool by \
         setting its `agent` parameter to a member id below. Do the coordination yourself; \
         delegate the specialist work.\n",
    );
    for member in &team.members {
        let hint = if member.role_hint.is_empty() {
            member.description.clone()
        } else {
            member.role_hint.clone()
        };
        out.push_str(&format!(
            "- `{}` — {}: {}\n",
            member.expert_id, member.name, hint
        ));
    }
    out
}

// ---------------------------------------------------------------------------
// Conversation-level default selection (authoritative in assistant.db)
// ---------------------------------------------------------------------------

pub fn get_conversation_selection(conversation_id: &str) -> Option<CapabilitySelection> {
    let data = crate::capability::store().ok()?;
    let conn = data.conn().ok()?;
    let raw: Option<String> = conn
        .query_row(
            "SELECT capability_selection_json FROM conversation WHERE id = ?1",
            rusqlite::params![conversation_id],
            |r| r.get(0),
        )
        .optional()
        .ok()?
        .flatten();
    serde_json::from_str(&raw?).ok()
}

pub fn set_conversation_selection(
    conversation_id: &str,
    selection: Option<&CapabilitySelection>,
) -> Result<(), String> {
    if let Some(sel) = selection {
        sel.validate().map_err(|e| e.to_string())?;
    }
    let data = crate::capability::store()?;
    let conn = data.conn()?;
    let raw = selection
        .map(|s| serde_json::to_string(s).map_err(|e| e.to_string()))
        .transpose()?;
    let changed = conn
        .execute(
            "UPDATE conversation SET capability_selection_json = ?2, updated_at = ?3 WHERE id = ?1",
            rusqlite::params![conversation_id, raw, chrono::Utc::now().to_rfc3339()],
        )
        .map_err(|e| e.to_string())?;
    if changed == 0 {
        return Err(format!("conversation not found: {conversation_id}"));
    }
    Ok(())
}

/// RPC surface: conversation.updateCapabilities / conversation.getCapabilities.
pub fn handle_conversation_rpc(method: &str, params: &Value) -> Result<Value, String> {
    let conversation_id = params
        .get("conversation_id")
        .or_else(|| params.get("conversationId"))
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or("conversation_id required")?;
    match method {
        "conversation.updateCapabilities" => {
            let selection: Option<CapabilitySelection> = match params.get("selection") {
                Some(Value::Null) | None => None,
                Some(raw) => Some(serde_json::from_value(raw.clone()).map_err(|e| e.to_string())?),
            };
            // Validate referenced capability ids up front so the picker gets
            // immediate feedback instead of a failed run later.
            if let Some(sel) = &selection {
                sel.validate().map_err(|e| e.to_string())?;
                if let Some(expert_id) = sel.expert_id.as_deref() {
                    if load_profile(expert_id, None).is_none() {
                        return Err(format!("expert not found: {expert_id}"));
                    }
                }
                if let Some(team_id) = sel.team_id.as_deref() {
                    resolve_team(team_id, None).map_err(|e| e.reason)?;
                }
            }
            set_conversation_selection(conversation_id, selection.as_ref())?;
            Ok(json!({
                "conversation_id": conversation_id,
                "selection": selection,
            }))
        }
        "conversation.getCapabilities" => {
            let selection = get_conversation_selection(conversation_id);
            Ok(json!({
                "conversation_id": conversation_id,
                "selection": selection,
            }))
        }
        other => Err(format!("unsupported method: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn with_temp_db<F: FnOnce()>(f: F) {
        let _guard = crate::storage::DataStore::env_test_lock();
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join(format!("resolve-{}.db", Uuid::new_v4()));
        let art = dir.path().join("artifacts");
        crate::storage::set_test_db_override(Some(db.clone()), Some(art.clone()));
        let _warm = crate::storage::DataStore::new(&db, &art).expect("resolve temp db");
        f();
        crate::storage::set_test_db_override(None, None);
    }

    fn make_expert(id: &str) {
        crate::capability::experts::create(&json!({
            "id": id,
            "name": id,
            "systemPrompt": format!("You are {id}."),
        }))
        .unwrap();
    }

    #[test]
    fn empty_selection_resolves_to_legacy_noop() {
        with_temp_db(|| {
            let snapshot = resolve(None, "conv-x", None, None, "native").unwrap();
            assert!(!snapshot.selection_active);
            assert!(snapshot.profile.is_none());
            assert!(snapshot.skill_prompt.is_none());
        });
    }

    #[test]
    fn expert_selection_resolves_db_profile() {
        with_temp_db(|| {
            make_expert("coder");
            let selection = CapabilitySelection {
                expert_id: Some("coder".into()),
                ..Default::default()
            };
            let snapshot = resolve(Some(&selection), "conv-x", None, None, "native").unwrap();
            assert!(snapshot.selection_active);
            assert_eq!(snapshot.agent_profile_id.as_deref(), Some("coder"));
            assert_eq!(
                snapshot.profile.unwrap().system_prompt.as_deref(),
                Some("You are coder.")
            );
        });
    }

    #[test]
    fn missing_expert_fails_closed() {
        with_temp_db(|| {
            let selection = CapabilitySelection {
                expert_id: Some("ghost".into()),
                ..Default::default()
            };
            let err = resolve(Some(&selection), "conv-x", None, None, "native").unwrap_err();
            assert!(err.code.contains("EXPERT_NOT_FOUND"), "{}", err.code);
        });
    }

    #[test]
    fn missing_skill_fails_closed() {
        with_temp_db(|| {
            let selection = CapabilitySelection {
                skills: Some(vec!["user:ghost".into()]),
                ..Default::default()
            };
            let err = resolve(Some(&selection), "conv-x", None, None, "native").unwrap_err();
            assert!(err.code.contains("SKILL_NOT_FOUND"), "{}", err.code);
        });
    }

    #[test]
    fn untrusted_mcp_fails_closed() {
        with_temp_db(|| {
            crate::capability::mcp::create(&json!({
                "id": "figma",
                "name": "Figma",
                "transport": "http",
                "url": "https://mcp.example.com",
                "trusted": false,
            }))
            .unwrap();
            let selection = CapabilitySelection {
                mcp_servers: Some(vec!["figma".into()]),
                ..Default::default()
            };
            let err = resolve(Some(&selection), "conv-x", None, None, "native").unwrap_err();
            assert!(err.code.contains("MCP_UNTRUSTED"), "{}", err.code);
        });
    }

    #[test]
    fn non_native_runtime_rejects_selection() {
        with_temp_db(|| {
            make_expert("coder");
            let selection = CapabilitySelection {
                expert_id: Some("coder".into()),
                ..Default::default()
            };
            let err = resolve(Some(&selection), "conv-x", None, None, "codex_cli").unwrap_err();
            assert!(
                err.code.contains("CAPABILITY_UNSUPPORTED_BY_RUNTIME"),
                "{}",
                err.code
            );
        });
    }

    #[test]
    fn team_resolves_lead_and_roster() {
        with_temp_db(|| {
            make_expert("lead");
            make_expert("builder");
            crate::capability::experts::team_create(&json!({
                "id": "growth",
                "name": "Growth",
                "coordinatorExpertId": "lead",
                "members": [
                    { "expertId": "builder", "roleHint": "builds features" },
                    { "expertId": "lead" }
                ],
            }))
            .unwrap();
            let selection = CapabilitySelection {
                team_id: Some("growth".into()),
                ..Default::default()
            };
            let snapshot = resolve(Some(&selection), "conv-x", None, None, "native").unwrap();
            assert_eq!(snapshot.agent_profile_id.as_deref(), Some("lead"));
            let roster = snapshot.extra_system_prompt.unwrap();
            assert!(roster.contains("`builder`"));
            assert!(roster.contains("builds features"));
            let team = snapshot.team.unwrap();
            assert_eq!(team.failure_policy, "isolate");
            assert_eq!(team.members.len(), 2);
        });
    }

    #[test]
    fn conversation_selection_round_trip() {
        with_temp_db(|| {
            crate::conversation_store::ensure_conversation_stub(
                "conv-1", "openai", "gpt-4o", None, None,
            )
            .unwrap();
            make_expert("coder");
            let selection = CapabilitySelection {
                expert_id: Some("coder".into()),
                skills: Some(vec![]),
                ..Default::default()
            };
            set_conversation_selection("conv-1", Some(&selection)).unwrap();
            let loaded = get_conversation_selection("conv-1").unwrap();
            assert_eq!(loaded.expert_id.as_deref(), Some("coder"));
            // Clearing works.
            set_conversation_selection("conv-1", None).unwrap();
            assert!(get_conversation_selection("conv-1").is_none());
        });
    }
}
