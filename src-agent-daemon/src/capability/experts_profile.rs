//! Agent profile loading (W9 split from capability/experts.rs).

use crate::agent_core_shim::AgentProfile;
use std::path::Path;

pub fn load_agent_profile(id: &str, project_root: Option<&Path>) -> Option<AgentProfile> {
    if let Some(profile) = load_expert_profile_from_db(id) {
        return Some(profile);
    }
    agent_core::load_agent_profile(id, project_root)
}

/// Load an enabled DB Expert as an `AgentProfile` (authoritative source).
pub fn load_expert_profile_from_db(id: &str) -> Option<AgentProfile> {
    let expert = get(&json!({ "id": id })).ok()?;
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
        tools: expert.get("tools").and_then(as_vec),
        disallowed_tools: expert.get("disallowedTools").and_then(as_vec),
        permission_mode: expert
            .get("permissionMode")
            .and_then(Value::as_str)
            .map(str::to_string),
        skills: expert.get("skills").and_then(as_vec),
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

