//! Plan artifact parsing and the latch gate decision.
//!
//! Owns the structured `Plan` model (`PlanStep`/`PlanRisk`), parsing from tool
//! input, and the [`decision`] that classifies a tool call under the latch.

use crate::{PermissionClass, SideEffect, ToolError};
use serde::{Deserialize, Serialize};

use super::{ENTER_PLAN_MODE_TOOL, EXIT_PLAN_MODE_TOOL};

/// Network tools that only *read* a remote resource.
///
/// `SideEffect::Network` cannot tell "GET a public page" apart from "call an
/// arbitrary MCP tool", and `PermissionClass::ExternalWrite` covers both, so the
/// distinction has to be made by name. These two perform a request and hand back
/// bytes; neither can mutate the user's machine, and research is the whole point
/// of planning. They are *not* auto-approved — the normal profile gate still
/// runs after the latch, so on `ask` the user is still asked.
const NETWORK_READ_TOOLS: &[&str] = &["web_fetch", "web_search"];

/// What Plan Mode does with one tool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanDecision {
    /// The latch does not object. The permission profile still decides.
    Allow,
    /// Blocked while planning. Returned to the model, never shown as a prompt.
    Deny,
    /// A Plan Mode control tool; the runtime handles it before the profile gate.
    Control,
}

/// Decide whether a tool may run while the plan latch is closed.
///
/// Fail-closed by construction: an unrecognised tool with any non-read side
/// effect lands in [`PlanDecision::Deny`], so a newly registered write tool is
/// blocked in Plan Mode on the day it is added rather than the day someone
/// remembers to classify it.
pub fn decision(
    tool_name: &str,
    side_effect: SideEffect,
    permission_class: PermissionClass,
) -> PlanDecision {
    if tool_name == EXIT_PLAN_MODE_TOOL || tool_name == ENTER_PLAN_MODE_TOOL {
        return PlanDecision::Control;
    }
    if NETWORK_READ_TOOLS.contains(&tool_name) {
        return PlanDecision::Allow;
    }
    match (side_effect, permission_class) {
        (SideEffect::ReadOnly, PermissionClass::AlwaysAllowed | PermissionClass::ProjectRead) => {
            PlanDecision::Allow
        }
        _ => PlanDecision::Deny,
    }
}

/// The refusal handed back to the model for a blocked tool.
pub fn blocked_error(tool_name: &str) -> ToolError {
    ToolError {
        code: "plan_mode_blocked".into(),
        message: format!(
            "`{tool_name}` cannot run in Plan Mode. Finish researching, then call \
             `{EXIT_PLAN_MODE_TOOL}` with your plan and wait for the user to approve it. \
             Only the user can leave Plan Mode."
        ),
        retryable: false,
    }
}

// ---------------------------------------------------------------------------
// Plan artifact
// ---------------------------------------------------------------------------

/// Upper bound on steps in one plan. A plan longer than this is not a plan.
pub const MAX_PLAN_STEPS: usize = 40;
/// Upper bound on any single free-text field, in UTF-8 bytes.
pub const MAX_TEXT_BYTES: usize = 4_000;

/// What kind of work a step is. Drives the GUI icon and the risk read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStepKind {
    /// Read/inspect only; produces understanding, not changes.
    Research,
    /// Creates or modifies files.
    Edit,
    /// Runs a command / process.
    Command,
    /// Checks the result (tests, build, lint).
    Verify,
}

impl PlanStepKind {
    fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "edit" | "write" | "patch" => PlanStepKind::Edit,
            "command" | "run" | "terminal" | "shell" => PlanStepKind::Command,
            "verify" | "test" | "check" => PlanStepKind::Verify,
            _ => PlanStepKind::Research,
        }
    }
}

/// Coarse risk band, rendered as a badge next to the step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanRisk {
    Low,
    Medium,
    High,
}

impl PlanRisk {
    fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "high" | "danger" | "destructive" => PlanRisk::High,
            "medium" | "med" | "moderate" => PlanRisk::Medium,
            _ => PlanRisk::Low,
        }
    }
}

/// One proposed step. Structured, not prose, so the GUI can render a checklist
/// with per-step targets and risk badges instead of a wall of markdown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanStep {
    /// Stable id within the plan (`s1`, `s2`, ...). Assigned when absent so the
    /// GUI has a key and a future per-step approval has something to name.
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub detail: String,
    pub kind: PlanStepKind,
    /// Files, directories, or commands the step touches.
    #[serde(default)]
    pub targets: Vec<String>,
    pub risk: PlanRisk,
    /// Whether the step can be undone by the checkpoint system. A `false` here
    /// is the single most important thing for a user to see before approving.
    pub reversible: bool,
}

/// A plan submitted for approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub title: String,
    #[serde(default)]
    pub summary: String,
    pub steps: Vec<PlanStep>,
    /// Things that could go wrong, in the model's own words.
    #[serde(default)]
    pub risks: Vec<String>,
    /// Anything the model needs the user to decide before executing.
    #[serde(default)]
    pub open_questions: Vec<String>,
    /// Explicit non-goals, so approval has a boundary.
    #[serde(default)]
    pub out_of_scope: Vec<String>,
}

impl Plan {
    /// True when any step declares an irreversible effect.
    pub fn has_irreversible_step(&self) -> bool {
        self.steps.iter().any(|s| !s.reversible)
    }

    /// Highest risk band across steps, for the summary badge.
    pub fn peak_risk(&self) -> PlanRisk {
        self.steps
            .iter()
            .map(|s| s.risk)
            .max_by_key(|r| match r {
                PlanRisk::Low => 0u8,
                PlanRisk::Medium => 1,
                PlanRisk::High => 2,
            })
            .unwrap_or(PlanRisk::Low)
    }
}

fn invalid(message: impl Into<String>) -> ToolError {
    ToolError {
        code: "invalid_plan".into(),
        message: message.into(),
        retryable: false,
    }
}

fn text_field(value: Option<&serde_json::Value>, field: &str) -> Result<String, ToolError> {
    let raw = value
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if raw.len() > MAX_TEXT_BYTES {
        return Err(invalid(format!(
            "plan field `{field}` exceeds {MAX_TEXT_BYTES} bytes"
        )));
    }
    Ok(raw)
}

fn string_list(value: Option<&serde_json::Value>, field: &str) -> Result<Vec<String>, ToolError> {
    let Some(arr) = value.and_then(|v| v.as_array()) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for item in arr.iter().take(MAX_PLAN_STEPS) {
        let s = item.as_str().unwrap_or("").trim().to_string();
        if s.is_empty() {
            continue;
        }
        if s.len() > MAX_TEXT_BYTES {
            return Err(invalid(format!(
                "plan field `{field}` has an entry over {MAX_TEXT_BYTES} bytes"
            )));
        }
        out.push(s);
    }
    Ok(out)
}

/// Parse and validate the `exit_plan_mode` input into a renderable [`Plan`].
///
/// Rejects rather than repairs anything load-bearing: an empty plan, a plan with
/// no steps, or a step with no title would all render as an approval card the
/// user cannot actually evaluate, and approving what you cannot read is the
/// failure mode this whole gear exists to prevent.
pub fn parse_plan(input: &serde_json::Value) -> Result<Plan, ToolError> {
    let root = input.get("plan").unwrap_or(input);

    let title = text_field(root.get("title"), "title")?;
    if title.is_empty() {
        return Err(invalid("plan requires a non-empty `title`"));
    }
    let summary = text_field(root.get("summary"), "summary")?;

    let raw_steps = root
        .get("steps")
        .and_then(|v| v.as_array())
        .ok_or_else(|| invalid("plan requires a `steps` array"))?;
    if raw_steps.is_empty() {
        return Err(invalid("plan requires at least one step"));
    }
    if raw_steps.len() > MAX_PLAN_STEPS {
        return Err(invalid(format!(
            "plan has {} steps, limit is {MAX_PLAN_STEPS}",
            raw_steps.len()
        )));
    }

    let mut steps = Vec::with_capacity(raw_steps.len());
    for (idx, raw) in raw_steps.iter().enumerate() {
        let step_title = text_field(raw.get("title"), "steps[].title")?;
        if step_title.is_empty() {
            return Err(invalid(format!("step {} requires a `title`", idx + 1)));
        }
        let id = raw
            .get("id")
            .and_then(|v| v.as_str())
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("s{}", idx + 1));
        let kind = PlanStepKind::parse(raw.get("kind").and_then(|v| v.as_str()).unwrap_or(""));
        let risk = raw
            .get("risk")
            .and_then(|v| v.as_str())
            .map(PlanRisk::parse)
            // No declared risk: infer from the kind rather than defaulting to
            // Low. A step that runs a command is never "low risk by omission".
            .unwrap_or(match kind {
                PlanStepKind::Research => PlanRisk::Low,
                PlanStepKind::Verify => PlanRisk::Low,
                PlanStepKind::Edit => PlanRisk::Medium,
                PlanStepKind::Command => PlanRisk::Medium,
            });
        // Same rule for reversibility: only Edit steps are covered by the
        // checkpoint system, so anything else is irreversible unless the model
        // says otherwise *and* the kind supports it.
        let reversible = raw
            .get("reversible")
            .and_then(|v| v.as_bool())
            .unwrap_or(matches!(
                kind,
                PlanStepKind::Edit | PlanStepKind::Research | PlanStepKind::Verify
            ));
        steps.push(PlanStep {
            id,
            title: step_title,
            detail: text_field(raw.get("detail"), "steps[].detail")?,
            kind,
            targets: string_list(raw.get("targets"), "steps[].targets")?,
            risk,
            reversible,
        });
    }

    // Duplicate ids would collide as GUI keys and make a future per-step
    // approval ambiguous about which step was approved.
    let mut seen = std::collections::HashSet::new();
    for step in &steps {
        if !seen.insert(step.id.as_str()) {
            return Err(invalid(format!("duplicate step id `{}`", step.id)));
        }
    }

    Ok(Plan {
        title,
        summary,
        steps,
        risks: string_list(root.get("risks"), "risks")?,
        open_questions: string_list(root.get("open_questions"), "open_questions")?,
        out_of_scope: string_list(root.get("out_of_scope"), "out_of_scope")?,
    })
}

// ---------------------------------------------------------------------------
// Per-run latch
// ---------------------------------------------------------------------------
