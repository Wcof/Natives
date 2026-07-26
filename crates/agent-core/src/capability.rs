//! Capability library domain types (ADR-0016).
//!
//! The capability library is the authoritative CRUD surface for skills, MCP
//! connectors and experts / expert teams. These types are shared by the daemon
//! store (`src-agent-daemon/src/capability/`), the resolve step at run start,
//! and — later — the Harness Blueprint, which references the same ids.
//!
//! Wire selection type is [`assistant_protocol::v2::run::CapabilitySelection`];
//! this module holds the domain-side definitions only.

use serde::{Deserialize, Serialize};

use crate::profile::AgentProfile;

/// Where a capability record came from. Records imported from outside the
/// library default to untrusted until the user explicitly promotes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilitySource {
    /// Discovered by the local scanner (skills) or created in the UI.
    Manual,
    Scan,
    ImportZip,
    ImportDir,
    ImportJson,
    ImportMd,
    Hub,
    HostMigration,
}

impl CapabilitySource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Scan => "scan",
            Self::ImportZip => "import_zip",
            Self::ImportDir => "import_dir",
            Self::ImportJson => "import_json",
            Self::ImportMd => "import_md",
            Self::Hub => "hub",
            Self::HostMigration => "host_migration",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "manual" => Self::Manual,
            "scan" => Self::Scan,
            "import_zip" => Self::ImportZip,
            "import_dir" => Self::ImportDir,
            "import_json" => Self::ImportJson,
            "import_md" => Self::ImportMd,
            "hub" => Self::Hub,
            "host_migration" => Self::HostMigration,
            _ => return None,
        })
    }
}

/// Skill metadata row. The skill body stays on disk (`dir_path` + SKILL.md) to
/// preserve interop with external CLI engines; the library only owns metadata
/// and the enable/trust switches.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillMeta {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// "user" | "project"
    pub scope: String,
    #[serde(default)]
    pub project_id: Option<String>,
    pub dir_path: String,
    /// sha256 of SKILL.md for drift detection.
    #[serde(default)]
    pub content_hash: Option<String>,
    /// Single primary category (办公 / 工具 / 投资 / 效率 …).
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub enabled: bool,
    pub trusted: bool,
    pub source: CapabilitySource,
    #[serde(default)]
    pub source_ref: Option<String>,
    /// Engines this skill targets: native | claude_cli | codex_cli.
    #[serde(default)]
    pub engine_targets: Vec<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

/// MCP transport kind for connector configs (matches `McpTransport` variants).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorTransport {
    Stdio,
    Http,
    Sse,
}

impl ConnectorTransport {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Stdio => "stdio",
            Self::Http => "http",
            Self::Sse => "sse",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "stdio" => Self::Stdio,
            "http" => Self::Http,
            "sse" => Self::Sse,
            _ => return None,
        })
    }
}

/// MCP connector configuration draft / record.
///
/// Secrets never live here: `env` values may hold `secret:<id>` references
/// resolved by the host-side encrypted store, and `headers` must never carry a
/// plaintext Authorization value (validated at the RPC boundary).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerDraft {
    pub id: String,
    pub name: String,
    pub transport: ConnectorTransport,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    /// Values may be literal or `secret:<capability_secrets.id>` references.
    #[serde(default)]
    pub env: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub headers: std::collections::BTreeMap<String, String>,
    /// "none" | "bearer" | "oauth"
    #[serde(default = "default_auth_mode")]
    pub auth_mode: String,
    /// authorize_url / token_url / client_id / scopes — never a client secret.
    #[serde(default)]
    pub oauth_config: Option<serde_json::Value>,
    pub trusted: bool,
    pub enabled: bool,
    pub source: CapabilitySource,
    /// Registry entry name when `source == Hub`.
    #[serde(default)]
    pub hub_ref: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

fn default_auth_mode() -> String {
    "none".into()
}

/// Expert definition — the DB-authoritative form of an [`AgentProfile`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpertDefinition {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub system_prompt: String,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub disallowed_tools: Vec<String>,
    #[serde(default)]
    pub permission_mode: Option<String>,
    /// capability_skill ids bound to this expert.
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub provider_id: Option<String>,
    /// Route key id; `'auto'` is rejected at the store boundary.
    #[serde(default)]
    pub key_id: Option<String>,
    #[serde(default)]
    pub model_id: Option<String>,
    /// max_steps / token_budget / context_mode / isolation_mode …
    #[serde(default)]
    pub params: serde_json::Value,
    pub enabled: bool,
    pub source: CapabilitySource,
    #[serde(default)]
    pub source_path: Option<String>,
    #[serde(default)]
    pub content_hash: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

impl ExpertDefinition {
    /// Project this definition into the engine-facing [`AgentProfile`] shape so
    /// downstream consumers (`production.rs`, context assembly) stay unchanged.
    pub fn to_agent_profile(&self) -> AgentProfile {
        fn opt_vec(v: &[String]) -> Option<Vec<String>> {
            if v.is_empty() {
                None
            } else {
                Some(v.to_vec())
            }
        }
        let params = &self.params;
        let param_str =
            |key: &str| -> Option<String> { params.get(key)?.as_str().map(str::to_string) };
        let param_u64 = |key: &str| -> Option<u64> { params.get(key)?.as_u64() };
        AgentProfile {
            id: self.id.clone(),
            name: self.name.clone(),
            description: if self.description.is_empty() {
                None
            } else {
                Some(self.description.clone())
            },
            prompt_mode: param_str("promptMode"),
            system_prompt: Some(self.system_prompt.clone()),
            tools: opt_vec(&self.tools),
            disallowed_tools: opt_vec(&self.disallowed_tools),
            permission_mode: self.permission_mode.clone(),
            skills: opt_vec(&self.skills),
            provider_id: self.provider_id.clone(),
            key_id: self.key_id.clone(),
            model_id: self.model_id.clone(),
            base_url_override: param_str("baseUrlOverride"),
            context_mode: param_str("contextMode"),
            isolation_mode: param_str("isolationMode"),
            max_steps: param_u64("maxSteps").and_then(|v| u32::try_from(v).ok()),
            max_duration: param_u64("maxDuration"),
            token_budget: param_u64("tokenBudget"),
            completion_requirement: param_str("completionRequirement"),
            body: self.system_prompt.clone(),
            source_path: None,
        }
    }
}

/// Team orchestration strategy. The team is a preset (lead persona + member
/// allowlist + failure policy) driven through the existing task tool — it is
/// NOT a new scheduler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamStrategy {
    #[default]
    Parallel,
    Sequential,
    Coordinator,
}

impl TeamStrategy {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Parallel => "parallel",
            Self::Sequential => "sequential",
            Self::Coordinator => "coordinator",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "parallel" => Self::Parallel,
            "sequential" => Self::Sequential,
            "coordinator" => Self::Coordinator,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpertTeamMember {
    pub expert_id: String,
    pub position: u32,
    #[serde(default)]
    pub role_hint: String,
    #[serde(default)]
    pub task_template: String,
}

/// Expert team definition. `failure_policy` uses the same word list as
/// [`crate::subagents::FailurePolicy::parse`]: isolate | fail_fast | require_all.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpertTeamDefinition {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub strategy: TeamStrategy,
    #[serde(default = "default_failure_policy")]
    pub failure_policy: String,
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: u32,
    /// Lead expert running the parent persona; members are spawnable via task.
    #[serde(default)]
    pub coordinator_expert_id: Option<String>,
    #[serde(default)]
    pub members: Vec<ExpertTeamMember>,
    pub enabled: bool,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

fn default_failure_policy() -> String {
    "isolate".into()
}

fn default_max_concurrent() -> u32 {
    3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expert_projects_to_agent_profile() {
        let expert = ExpertDefinition {
            id: "coder".into(),
            name: "Coder".into(),
            description: "writes code".into(),
            system_prompt: "You are a coder.".into(),
            tools: vec!["read_file".into()],
            disallowed_tools: vec![],
            permission_mode: Some("ask".into()),
            skills: vec!["skill-1".into()],
            provider_id: Some("anthropic".into()),
            key_id: Some("key-1".into()),
            model_id: Some("claude-sonnet-5".into()),
            params: serde_json::json!({ "maxSteps": 12, "tokenBudget": 50000 }),
            enabled: true,
            source: CapabilitySource::Manual,
            source_path: None,
            content_hash: None,
            created_at: None,
            updated_at: None,
        };
        let profile = expert.to_agent_profile();
        assert_eq!(profile.id, "coder");
        assert_eq!(profile.system_prompt.as_deref(), Some("You are a coder."));
        assert_eq!(profile.tools.as_deref(), Some(&["read_file".to_string()][..]));
        assert_eq!(profile.disallowed_tools, None);
        assert_eq!(profile.skills.as_deref(), Some(&["skill-1".to_string()][..]));
        assert_eq!(profile.max_steps, Some(12));
        assert_eq!(profile.token_budget, Some(50000));
    }

    #[test]
    fn source_round_trips() {
        for s in [
            "manual",
            "scan",
            "import_zip",
            "import_dir",
            "import_json",
            "import_md",
            "hub",
            "host_migration",
        ] {
            assert_eq!(CapabilitySource::parse(s).unwrap().as_str(), s);
        }
        assert!(CapabilitySource::parse("bogus").is_none());
    }

    #[test]
    fn team_strategy_round_trips() {
        for s in ["parallel", "sequential", "coordinator"] {
            assert_eq!(TeamStrategy::parse(s).unwrap().as_str(), s);
        }
        assert!(TeamStrategy::parse("dag").is_none());
    }
}
