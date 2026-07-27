//! Hook definitions — the inspectable data behind the opaque runtime registry.
//!
//! Before this module, a registered Hook was a `Box<dyn HookHandler>`: the
//! runtime could dispatch it but could not answer "which Hook is this, where
//! did it come from, and why did it run in this order?". `HookDefinition`
//! answers those questions without changing how Hooks execute.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

/// Lifecycle hook events (compatible with Claude / Grok hook names).
///
/// The wire representation is PascalCase and is part of the `hooks.json`
/// contract — do not rename variants without a migration.
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
    /// Every event, in the canonical order used for registry assembly and UI.
    pub const ALL: [HookEvent; 16] = [
        Self::SessionStart,
        Self::SessionEnd,
        Self::UserPromptSubmit,
        Self::PreToolUse,
        Self::PostToolUse,
        Self::PostToolUseFailure,
        Self::PermissionRequest,
        Self::PermissionDenied,
        Self::Notification,
        Self::SubagentStart,
        Self::SubagentStop,
        Self::PreCompact,
        Self::PostCompact,
        Self::Stop,
        Self::StopFailure,
        Self::Error,
    ];

    /// Security-sensitive events default fail-closed when no handler allows.
    pub fn is_security_sensitive(self) -> bool {
        matches!(
            self,
            Self::PreToolUse | Self::PermissionRequest | Self::PermissionDenied
        )
    }

    /// Stable identifier used in `hooks.json`, persistence, and the UI.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SessionStart => "SessionStart",
            Self::SessionEnd => "SessionEnd",
            Self::UserPromptSubmit => "UserPromptSubmit",
            Self::PreToolUse => "PreToolUse",
            Self::PostToolUse => "PostToolUse",
            Self::PostToolUseFailure => "PostToolUseFailure",
            Self::PermissionRequest => "PermissionRequest",
            Self::PermissionDenied => "PermissionDenied",
            Self::Notification => "Notification",
            Self::SubagentStart => "SubagentStart",
            Self::SubagentStop => "SubagentStop",
            Self::PreCompact => "PreCompact",
            Self::PostCompact => "PostCompact",
            Self::Stop => "Stop",
            Self::StopFailure => "StopFailure",
            Self::Error => "Error",
        }
    }

    /// Parse a `hooks.json` key, accepting the historical aliases that
    /// `production_hooks` has always honoured.
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "PreToolUse" | "pre_tool_use" => Self::PreToolUse,
            "PostToolUse" | "post_tool_use" => Self::PostToolUse,
            "PostToolUseFailure" | "post_tool_use_failure" => Self::PostToolUseFailure,
            "Stop" | "stop" => Self::Stop,
            "StopFailure" | "stop_failure" => Self::StopFailure,
            "SessionStart" | "session_start" => Self::SessionStart,
            "SessionEnd" | "session_end" => Self::SessionEnd,
            "UserPromptSubmit" | "user_prompt_submit" => Self::UserPromptSubmit,
            "PermissionRequest" | "permission_request" => Self::PermissionRequest,
            "PermissionDenied" | "permission_denied" => Self::PermissionDenied,
            "SubagentStart" | "subagent_start" => Self::SubagentStart,
            "SubagentStop" | "SubagentEnd" | "subagent_stop" => Self::SubagentStop,
            "CompactStart" | "PreCompact" | "pre_compact" => Self::PreCompact,
            "CompactEnd" | "PostCompact" | "post_compact" => Self::PostCompact,
            "Notification" | "notification" => Self::Notification,
            "Error" | "error" => Self::Error,
            _ => return None,
        })
    }
}

impl std::fmt::Display for HookEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where a Hook came from. Drives trust decisions and the UI source filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookScope {
    /// Built into the engine; always present, never user-removable.
    Builtin,
    /// Discovered from a file under the project root.
    Project,
    /// Discovered from a file under the user's home directory.
    User,
    /// Configured through a process environment variable.
    Env,
}

/// Provenance: enough to point a user at the exact line that created a Hook.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookSource {
    pub scope: HookScope,
    /// Project-relative path, home-relative path, env var name, or builtin name.
    ///
    /// Project paths are stored relative to the project root so that a Hook
    /// identity is stable across machines and checkouts.
    pub origin: String,
    /// Index of the matcher group within the event's array, when file-sourced.
    pub group_index: Option<usize>,
    /// Index of the handler within its group, when file-sourced.
    pub entry_index: Option<usize>,
}

impl HookSource {
    pub fn builtin(name: impl Into<String>) -> Self {
        Self {
            scope: HookScope::Builtin,
            origin: name.into(),
            group_index: None,
            entry_index: None,
        }
    }

    pub fn env(var: impl Into<String>) -> Self {
        Self {
            scope: HookScope::Env,
            origin: var.into(),
            group_index: None,
            entry_index: None,
        }
    }

    pub fn file(scope: HookScope, path: impl Into<String>, group: usize, entry: usize) -> Self {
        Self {
            scope,
            origin: path.into(),
            group_index: Some(group),
            entry_index: Some(entry),
        }
    }
}

/// A Hook's identity.
///
/// Deliberately a readable path rather than a hash: it is stable by
/// construction, collision-free, and doubles as provenance in logs and the UI,
/// so a user never has to reverse-lookup an opaque digest.
///
/// ```text
/// builtin/allow-all#PreToolUse
/// env/NATIVES_HOOK_CMD#PreToolUse
/// project/.claude/settings.json#PreToolUse[0]/1
/// user/.claude/settings.json#PostToolUse[2]/0
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct HookId(String);

impl HookId {
    pub fn native(uuid: &str, event: HookEvent) -> Self {
        Self(format!("native/{uuid}#{}", event.as_str()))
    }

    pub fn new(source: &HookSource, event: HookEvent) -> Self {
        let prefix = match source.scope {
            HookScope::Builtin => "builtin",
            HookScope::Project => "project",
            HookScope::User => "user",
            HookScope::Env => "env",
        };
        let mut id = format!("{prefix}/{}#{}", source.origin, event.as_str());
        if let (Some(group), Some(entry)) = (source.group_index, source.entry_index) {
            id.push_str(&format!("[{group}]/{entry}"));
        }
        Self(id)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for HookId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// What a Hook actually invokes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HookKind {
    /// An engine-provided handler, identified by name.
    Builtin { name: String },
    /// An external process invoked with argv (never through a shell by the
    /// caller's own splitting — see the adapter for shell wrapping rules).
    ///
    /// `trusted` is a hard gate, not a label: an untrusted command Hook is
    /// denied without ever being spawned.
    Command {
        program: String,
        args: Vec<String>,
        #[serde(default)]
        trusted: bool,
    },
    /// An HTTP endpoint, optionally restricted to an explicit host allowlist.
    Http {
        url: String,
        #[serde(default)]
        allow_hosts: Vec<String>,
    },
}

/// What happens when an ordinary Hook times out or errors.
///
/// Security-sensitive events ignore this and always fail closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookFailurePolicy {
    /// Deny, failing the surrounding operation.
    #[default]
    Fail,
    /// Skip this Hook and continue with the remaining ones.
    Skip,
    /// Treat the failure as an Allow decision and continue.
    Default,
}

/// How a condition compares a field against a pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConditionOperator {
    #[default]
    RegexMatch,
    Contains,
    NotContains,
    Equals,
    PathMatch,
}

/// A typed predicate over a Hook's tool input.
///
/// Typed on purpose: a UI can render an operator dropdown plus a pattern field,
/// which it cannot do for a free-form expression string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Condition {
    /// Key to read from the tool input, e.g. `command`, `file_path`, `content`.
    pub field: String,
    #[serde(default)]
    pub operator: ConditionOperator,
    pub pattern: String,
}

impl Condition {
    /// Evaluate against a tool input object.
    ///
    /// A missing field yields an empty string, so `not_contains` holds and the
    /// positive operators do not.
    pub fn matches(&self, input: &Value) -> bool {
        let value = input
            .get(&self.field)
            .map(|v| match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            })
            .unwrap_or_default();

        match self.operator {
            ConditionOperator::RegexMatch => regex::Regex::new(&self.pattern)
                .map(|re| re.is_match(&value))
                .unwrap_or(false),
            ConditionOperator::Contains => value.contains(&self.pattern),
            ConditionOperator::NotContains => !value.contains(&self.pattern),
            ConditionOperator::Equals => value == self.pattern,
            ConditionOperator::PathMatch => glob_matches(&self.pattern, &value),
        }
    }
}

/// A Hook as configuration: everything needed to describe it in the UI and to
/// compile it into an executable handler.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookDefinition {
    pub id: HookId,
    pub event: HookEvent,
    pub source: HookSource,
    /// Dispatch order within an event. Lower runs first; ties keep discovery
    /// order. Builtin defaults use `0` so they always precede file Hooks.
    pub order: i32,
    /// Tool-name matcher, `|`-separated with `*` wildcards. `None` matches all.
    pub matcher: Option<String>,
    /// Additional predicates over tool input; all must hold.
    #[serde(default)]
    pub conditions: Vec<Condition>,
    pub timeout_ms: u64,
    #[serde(default)]
    pub failure_policy: HookFailurePolicy,
    pub kind: HookKind,
}

impl HookDefinition {
    pub fn timeout(&self) -> Duration {
        Duration::from_millis(self.timeout_ms)
    }

    /// Whether this Hook applies to a dispatch, matcher and conditions together.
    pub fn applies_to(&self, tool_name: Option<&str>, input: &Value) -> bool {
        tool_pattern_matches(self.matcher.as_deref(), tool_name)
            && self.conditions.iter().all(|c| c.matches(input))
    }
}

/// Match a tool name against a `|`-separated pattern list.
///
/// An empty or absent pattern matches everything. `*` matches everything and
/// `pre*` matches by prefix.
///
/// Moved verbatim from `agent-core` — semantics are deliberately unchanged so
/// the extraction stays behaviour-preserving. Two known quirks are preserved
/// rather than fixed here, because changing a security-relevant matcher belongs
/// in its own reviewable change (task T5):
///
/// - a `None` tool name only matches the whole pattern `"*"`, so `"Bash|*"`
///   does not match it;
/// - `*suffix` is not supported, only `prefix*`.
pub fn tool_pattern_matches(pattern: Option<&str>, tool_name: Option<&str>) -> bool {
    let Some(pattern) = pattern.filter(|pattern| !pattern.trim().is_empty()) else {
        return true;
    };
    let Some(tool_name) = tool_name else {
        return pattern == "*";
    };
    pattern.split('|').any(|candidate| {
        let candidate = candidate.trim();
        candidate == "*"
            || candidate == tool_name
            || (candidate.ends_with('*') && tool_name.starts_with(candidate.trim_end_matches('*')))
    })
}

/// Glob match supporting `*` (any run of characters) and `?` (one character).
fn glob_matches(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    // Iterative backtracking: linear in the common case, no recursion depth risk.
    let (mut p, mut t) = (0usize, 0usize);
    let (mut star, mut resume) = (None, 0usize);
    while t < text.len() {
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == text[t]) {
            p += 1;
            t += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some(p);
            resume = t;
            p += 1;
        } else if let Some(sp) = star {
            p = sp + 1;
            resume += 1;
            t = resume;
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|&c| c == '*')
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn event_string_round_trips_for_every_variant() {
        for event in HookEvent::ALL {
            assert_eq!(HookEvent::parse(event.as_str()), Some(event));
        }
    }

    #[test]
    fn event_parse_accepts_historical_aliases() {
        assert_eq!(
            HookEvent::parse("pre_tool_use"),
            Some(HookEvent::PreToolUse)
        );
        assert_eq!(
            HookEvent::parse("CompactStart"),
            Some(HookEvent::PreCompact)
        );
        assert_eq!(HookEvent::parse("CompactEnd"), Some(HookEvent::PostCompact));
        assert_eq!(
            HookEvent::parse("SubagentEnd"),
            Some(HookEvent::SubagentStop)
        );
        assert_eq!(HookEvent::parse("NotAnEvent"), None);
    }

    #[test]
    fn only_three_events_are_security_sensitive() {
        let sensitive: Vec<_> = HookEvent::ALL
            .into_iter()
            .filter(|e| e.is_security_sensitive())
            .collect();
        assert_eq!(
            sensitive,
            vec![
                HookEvent::PreToolUse,
                HookEvent::PermissionRequest,
                HookEvent::PermissionDenied
            ]
        );
    }

    #[test]
    fn hook_id_is_readable_and_encodes_provenance() {
        assert_eq!(
            HookId::new(&HookSource::builtin("allow-all"), HookEvent::PreToolUse).as_str(),
            "builtin/allow-all#PreToolUse"
        );
        assert_eq!(
            HookId::new(&HookSource::env("NATIVES_HOOK_CMD"), HookEvent::PreToolUse).as_str(),
            "env/NATIVES_HOOK_CMD#PreToolUse"
        );
        assert_eq!(
            HookId::new(
                &HookSource::file(HookScope::Project, ".claude/settings.json", 0, 1),
                HookEvent::PreToolUse
            )
            .as_str(),
            "project/.claude/settings.json#PreToolUse[0]/1"
        );
    }

    #[test]
    fn hook_ids_are_distinct_across_group_and_entry() {
        let a = HookId::new(
            &HookSource::file(HookScope::Project, ".claude/hooks.json", 0, 1),
            HookEvent::PreToolUse,
        );
        let b = HookId::new(
            &HookSource::file(HookScope::Project, ".claude/hooks.json", 1, 0),
            HookEvent::PreToolUse,
        );
        assert_ne!(a, b);
    }

    #[test]
    fn tool_pattern_matches_wildcards_and_alternatives() {
        assert!(tool_pattern_matches(None, Some("anything")));
        assert!(tool_pattern_matches(Some("  "), Some("anything")));
        assert!(tool_pattern_matches(Some("*"), Some("anything")));
        assert!(tool_pattern_matches(
            Some("Bash|run_*"),
            Some("run_command")
        ));
        assert!(tool_pattern_matches(Some("Bash|run_*"), Some("Bash")));
        assert!(!tool_pattern_matches(Some("Bash|run_*"), Some("read_file")));
    }

    /// Pins the two quirks carried over verbatim from `agent-core`. Task T5 may
    /// change these, but only as an explicit, separately reviewed decision.
    #[test]
    fn tool_pattern_preserves_legacy_quirks() {
        assert!(tool_pattern_matches(Some("*"), None));
        assert!(
            !tool_pattern_matches(Some("Bash|*"), None),
            "a None tool name matches only the whole pattern \"*\""
        );
        assert!(!tool_pattern_matches(Some("Bash"), None));
        assert!(
            !tool_pattern_matches(Some("*_file"), Some("read_file")),
            "suffix wildcards are not supported"
        );
    }

    #[test]
    fn conditions_cover_every_operator() {
        let input = json!({"command": "rm -rf /tmp/x", "file_path": "src/main.rs"});

        let cond = |field: &str, op, pattern: &str| Condition {
            field: field.into(),
            operator: op,
            pattern: pattern.into(),
        };

        assert!(cond("command", ConditionOperator::RegexMatch, r"rm\s+-rf").matches(&input));
        assert!(!cond("command", ConditionOperator::RegexMatch, r"^git").matches(&input));
        assert!(cond("command", ConditionOperator::Contains, "rm -rf").matches(&input));
        assert!(cond("command", ConditionOperator::NotContains, "git push").matches(&input));
        assert!(cond("file_path", ConditionOperator::Equals, "src/main.rs").matches(&input));
        assert!(cond("file_path", ConditionOperator::PathMatch, "src/*.rs").matches(&input));
        assert!(!cond("file_path", ConditionOperator::PathMatch, "tests/*.rs").matches(&input));
    }

    #[test]
    fn invalid_regex_does_not_match_rather_than_panicking() {
        let cond = Condition {
            field: "command".into(),
            operator: ConditionOperator::RegexMatch,
            pattern: "[unclosed".into(),
        };
        assert!(!cond.matches(&json!({"command": "anything"})));
    }

    #[test]
    fn missing_field_is_empty_string() {
        let input = json!({});
        assert!(Condition {
            field: "command".into(),
            operator: ConditionOperator::NotContains,
            pattern: "rm".into(),
        }
        .matches(&input));
        assert!(!Condition {
            field: "command".into(),
            operator: ConditionOperator::Contains,
            pattern: "rm".into(),
        }
        .matches(&input));
    }

    #[test]
    fn non_string_field_is_compared_by_json_text() {
        let input = json!({"count": 42});
        assert!(Condition {
            field: "count".into(),
            operator: ConditionOperator::Equals,
            pattern: "42".into(),
        }
        .matches(&input));
    }

    #[test]
    fn glob_handles_multiple_stars_and_question_marks() {
        assert!(glob_matches("*", "anything"));
        assert!(glob_matches("src/*/mod.rs", "src/hooks/mod.rs"));
        assert!(glob_matches("*.rs", "main.rs"));
        assert!(glob_matches("a*b*c", "axxbyyc"));
        assert!(glob_matches("?at", "cat"));
        assert!(!glob_matches("?at", "at"));
        assert!(!glob_matches("a*b*c", "axxbyy"));
        assert!(glob_matches("", ""));
        assert!(!glob_matches("", "x"));
    }

    #[test]
    fn applies_to_requires_matcher_and_every_condition() {
        let def = HookDefinition {
            id: HookId::new(&HookSource::builtin("t"), HookEvent::PreToolUse),
            event: HookEvent::PreToolUse,
            source: HookSource::builtin("t"),
            order: 0,
            matcher: Some("Bash".into()),
            conditions: vec![Condition {
                field: "command".into(),
                operator: ConditionOperator::Contains,
                pattern: "git".into(),
            }],
            timeout_ms: 10_000,
            failure_policy: HookFailurePolicy::Fail,
            kind: HookKind::Builtin { name: "t".into() },
        };

        assert!(def.applies_to(Some("Bash"), &json!({"command": "git push"})));
        assert!(!def.applies_to(Some("Bash"), &json!({"command": "ls"})));
        assert!(!def.applies_to(Some("read_file"), &json!({"command": "git push"})));
    }

    #[test]
    fn definition_survives_a_json_round_trip() {
        let def = HookDefinition {
            id: HookId::new(
                &HookSource::file(HookScope::Project, ".claude/hooks.json", 0, 0),
                HookEvent::PostToolUse,
            ),
            event: HookEvent::PostToolUse,
            source: HookSource::file(HookScope::Project, ".claude/hooks.json", 0, 0),
            order: 10,
            matcher: Some("Edit|Write".into()),
            conditions: vec![],
            timeout_ms: 5_000,
            failure_policy: HookFailurePolicy::Skip,
            kind: HookKind::Http {
                url: "https://example.test/hook".into(),
                allow_hosts: vec!["example.test".into()],
            },
        };
        let text = serde_json::to_string(&def).unwrap();
        assert_eq!(serde_json::from_str::<HookDefinition>(&text).unwrap(), def);
    }

    #[test]
    fn failure_policy_defaults_to_fail() {
        assert_eq!(HookFailurePolicy::default(), HookFailurePolicy::Fail);
    }
}
