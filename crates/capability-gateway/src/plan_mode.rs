//! Plan Mode — a research-only gear for high-risk work.
//!
//! # What it is
//!
//! Plan Mode is **not** a fourth permission profile. It is a latch that sits in
//! front of the profile ladder and can only ever *subtract* capability:
//!
//! ```text
//!   tool call ──► plan latch ──► permission profile ──► path/injection policy ──► handler
//!                 (this file)    (policy::check_permission)
//! ```
//!
//! Two reasons it is orthogonal rather than a profile variant:
//!
//! 1. "How far do I trust this agent" (`readonly` / `ask` / `autonomous`) and
//!    "is this agent currently allowed to act at all" are independent questions.
//!    A user on `ask` and a user on `autonomous` both want Plan Mode to mean the
//!    same thing: explore freely, change nothing, come back with a proposal.
//! 2. Because the latch only subtracts, releasing it can never grant more than
//!    the run already had. The worst outcome of a bogus release is the profile
//!    the run started with — never an escalation. That property is what makes
//!    the exit path safe to expose to a tool call at all.
//!
//! # Read-only tools stay open
//!
//! Exploration is the precondition for a good plan, so anything that cannot
//! change the machine keeps running: see [`decision`]. Everything else is
//! refused *without prompting the user*. A blocked write must teach the model
//! to finish planning, not train the user to click through approval cards.
//!
//! # Exit is a human action
//!
//! The model may enter Plan Mode on its own (that is monotonically restrictive,
//! so it is safe). It can never leave on its own. `exit_plan_mode` submits a
//! structured plan; the daemon turns that into a real approval interaction and
//! only [`approve`] — reached solely from a user response — clears the latch.
//! No prompt text, no tool output, and no model assertion can substitute.

use crate::{PermissionClass, SideEffect, ToolError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Profile string that marks a run as being in Plan Mode.
pub const PLAN_PROFILE: &str = "plan";

/// Profile a run falls back to when it was *started* in Plan Mode and the plan
/// is approved. Deliberately the confirm-each gear: a run that asked to be
/// planned has not earned autonomy by having its plan accepted.
pub const PLAN_DEFAULT_FALLBACK: &str = "ask";

/// Tool that submits a plan for human approval.
pub const EXIT_PLAN_MODE_TOOL: &str = "exit_plan_mode";
/// Tool that puts the current run into Plan Mode.
pub const ENTER_PLAN_MODE_TOOL: &str = "enter_plan_mode";

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

/// Where a run stands with respect to Plan Mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanState {
    /// Latch closed: writes blocked, waiting for a plan and a human.
    Planning,
    /// A user approved the plan; the run executes under `fallback_profile`.
    Approved,
}

/// Plan Mode record for one run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanSession {
    pub run_id: String,
    pub state: PlanState,
    /// Profile the run reverts to once the plan is approved. Captured at entry
    /// so approval restores what the run already had and never more.
    pub fallback_profile: String,
    /// The most recently submitted plan (kept after approval for the timeline).
    pub plan: Option<Plan>,
    /// How many times a plan was rejected. Surfaced so a loop is visible.
    pub rejections: u32,
    pub entered_at: chrono::DateTime<chrono::Utc>,
    pub approved_at: Option<chrono::DateTime<chrono::Utc>>,
}

fn sessions() -> &'static Mutex<HashMap<String, PlanSession>> {
    static SESSIONS: OnceLock<Mutex<HashMap<String, PlanSession>>> = OnceLock::new();
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Put a run into Plan Mode.
///
/// Idempotent, and deliberately refuses to re-enter a run whose plan was already
/// approved: re-entering would let a model that has just been granted write
/// access hand the user a *second* approval card, which is a phishing shape.
/// A new plan needs a new run.
pub fn enter(run_id: &str, fallback_profile: &str) -> PlanSession {
    let fallback = normalize_fallback(fallback_profile);
    let mut guard = sessions().lock().expect("plan sessions poisoned");
    evict_settled(&mut guard);
    guard
        .entry(run_id.to_string())
        .or_insert_with(|| PlanSession {
            run_id: run_id.to_string(),
            state: PlanState::Planning,
            fallback_profile: fallback,
            plan: None,
            rejections: 0,
            entered_at: chrono::Utc::now(),
            approved_at: None,
        })
        .clone()
}

/// How many settled sessions to keep before the oldest are dropped.
const MAX_RETAINED_SESSIONS: usize = 256;

/// Bound the session map without ever touching a live latch.
///
/// Nothing here may evict a `Planning` session. Dropping one would silently
/// reopen write access for a run that never got its plan approved, which is the
/// exact failure this module exists to prevent — so when every session is live
/// the map is simply allowed to grow.
fn evict_settled(map: &mut HashMap<String, PlanSession>) {
    if map.len() <= MAX_RETAINED_SESSIONS {
        return;
    }
    let mut settled: Vec<(String, chrono::DateTime<chrono::Utc>)> = map
        .iter()
        .filter(|(_, s)| s.state == PlanState::Approved)
        .map(|(id, s)| (id.clone(), s.approved_at.unwrap_or(s.entered_at)))
        .collect();
    settled.sort_by_key(|(_, at)| *at);
    let excess = map.len() - MAX_RETAINED_SESSIONS;
    for (id, _) in settled.into_iter().take(excess) {
        map.remove(&id);
    }
}

/// A run started in Plan Mode falls back to confirm-each, never to the plan
/// pseudo-profile (which would trap it) and never to autonomous.
fn normalize_fallback(profile: &str) -> String {
    let trimmed = profile.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case(PLAN_PROFILE) {
        PLAN_DEFAULT_FALLBACK.to_string()
    } else {
        trimmed.to_string()
    }
}

/// True while the latch is closed for this run.
pub fn is_active(run_id: &str) -> bool {
    sessions()
        .lock()
        .ok()
        .and_then(|g| g.get(run_id).map(|s| s.state == PlanState::Planning))
        .unwrap_or(false)
}

/// Current session record, if the run ever entered Plan Mode.
pub fn snapshot(run_id: &str) -> Option<PlanSession> {
    sessions().lock().ok()?.get(run_id).cloned()
}

/// Record a submitted plan without releasing the latch.
///
/// Called before the user is asked. Keeping submission and approval separate is
/// what makes "the model proposed X" and "the user accepted X" two distinct,
/// separately auditable facts.
pub fn record_submission(run_id: &str, plan: Plan) -> Result<(), ToolError> {
    let mut guard = sessions().lock().expect("plan sessions poisoned");
    match guard.get_mut(run_id) {
        Some(session) if session.state == PlanState::Planning => {
            session.plan = Some(plan);
            Ok(())
        }
        Some(_) => Err(ToolError {
            code: "plan_already_approved".into(),
            message: "this run already left Plan Mode".into(),
            retryable: false,
        }),
        None => Err(not_planning()),
    }
}

/// Release the latch. **Only reachable from a user approval.**
///
/// Returns the profile the run now executes under.
pub fn approve(run_id: &str) -> Result<String, ToolError> {
    let mut guard = sessions().lock().expect("plan sessions poisoned");
    let Some(session) = guard.get_mut(run_id) else {
        return Err(not_planning());
    };
    if session.state == PlanState::Approved {
        return Ok(session.fallback_profile.clone());
    }
    if session.plan.is_none() {
        // Approving nothing is not approval. This is unreachable through the
        // daemon path (which records first) and is a guard against a future
        // caller wiring approval up without a plan.
        return Err(ToolError {
            code: "plan_missing".into(),
            message: "cannot approve: no plan was submitted for this run".into(),
            retryable: false,
        });
    }
    session.state = PlanState::Approved;
    session.approved_at = Some(chrono::Utc::now());
    Ok(session.fallback_profile.clone())
}

/// Record a rejection. The latch stays closed and the model keeps planning.
pub fn reject(run_id: &str) -> Result<u32, ToolError> {
    let mut guard = sessions().lock().expect("plan sessions poisoned");
    let Some(session) = guard.get_mut(run_id) else {
        return Err(not_planning());
    };
    if session.state == PlanState::Approved {
        return Err(ToolError {
            code: "plan_already_approved".into(),
            message: "this run already left Plan Mode".into(),
            retryable: false,
        });
    }
    session.rejections = session.rejections.saturating_add(1);
    Ok(session.rejections)
}

/// Effective profile for a run: the plan gear while planning, the captured
/// fallback afterwards, otherwise the run's declared profile unchanged.
pub fn effective_profile(run_id: &str, declared: &str) -> String {
    match snapshot(run_id) {
        Some(s) if s.state == PlanState::Planning => PLAN_PROFILE.to_string(),
        Some(s) => s.fallback_profile,
        // A run declared as `plan` that never called `enter` is still in Plan
        // Mode — the host asked for it. Treat the declaration as the latch.
        None if declared.trim().eq_ignore_ascii_case(PLAN_PROFILE) => PLAN_PROFILE.to_string(),
        None => declared.to_string(),
    }
}

/// Drop a run's session (run finished / cancelled).
pub fn clear(run_id: &str) {
    if let Ok(mut guard) = sessions().lock() {
        guard.remove(run_id);
    }
}

// ---------------------------------------------------------------------------
// Timeline events
// ---------------------------------------------------------------------------

/// A latch movement worth putting on the run timeline.
///
/// Transitions, not states. `Submitted` and `Rejected` both leave the latch
/// closed, so a state-only event would render the two indistinguishably — and
/// "the model proposed something" versus "you turned something down" is exactly
/// the distinction a user is scrolling the timeline to find.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanTransition {
    /// The run entered Plan Mode.
    Entered,
    /// A plan was submitted and the user is being asked.
    Submitted,
    /// The user approved; the latch is open.
    Approved,
    /// The user rejected; the latch stays closed.
    Rejected,
    /// The session was discarded (run finished or cancelled while planning).
    Cleared,
}

impl PlanTransition {
    /// Wire value for [`RunEventKind::PlanModeChanged::transition`].
    pub fn as_str(self) -> &'static str {
        match self {
            PlanTransition::Entered => "entered",
            PlanTransition::Submitted => "submitted",
            PlanTransition::Approved => "approved",
            PlanTransition::Rejected => "rejected",
            PlanTransition::Cleared => "cleared",
        }
    }
}

/// Build the timeline event for a latch movement.
///
/// Takes the session by reference rather than re-reading it by id so the event a
/// caller emits describes the state it actually acted on, not whatever a
/// concurrent transition left behind between the mutation and the publish.
///
/// The plan rides along on every transition that has one so the timeline stays
/// readable after the approval card is dismissed: an `approved` entry that says
/// only "approved" answers none of the questions someone scrolls back to ask.
pub fn changed_event(
    session: &PlanSession,
    transition: PlanTransition,
    reason: Option<&str>,
) -> assistant_protocol::v2::RunEventKind {
    let effective_profile = match session.state {
        PlanState::Planning => PLAN_PROFILE.to_string(),
        PlanState::Approved => session.fallback_profile.clone(),
    };
    let plan = match transition {
        // Entering has no plan yet, and clearing is a teardown notice — attaching
        // the plan there would replay a stale proposal as if it were live.
        PlanTransition::Entered | PlanTransition::Cleared => None,
        _ => session
            .plan
            .as_ref()
            .and_then(|p| serde_json::to_value(p).ok()),
    };
    assistant_protocol::v2::RunEventKind::PlanModeChanged {
        transition: transition.as_str().to_string(),
        effective_profile,
        plan,
        rejections: (session.rejections > 0).then_some(session.rejections),
        reason: reason
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
    }
}

fn not_planning() -> ToolError {
    ToolError {
        code: "not_in_plan_mode".into(),
        message: format!("this run is not in Plan Mode; call `{ENTER_PLAN_MODE_TOOL}` first"),
        retryable: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(name: &str) -> String {
        format!("{name}-{}", uuid::Uuid::new_v4())
    }

    // -- gate ---------------------------------------------------------------

    #[test]
    fn read_only_project_tools_pass_the_latch() {
        for (name, se, pc) in [
            (
                "read_file",
                SideEffect::ReadOnly,
                PermissionClass::ProjectRead,
            ),
            ("grep", SideEffect::ReadOnly, PermissionClass::ProjectRead),
            (
                "skill",
                SideEffect::ReadOnly,
                PermissionClass::AlwaysAllowed,
            ),
        ] {
            assert_eq!(decision(name, se, pc), PlanDecision::Allow, "{name}");
        }
    }

    #[test]
    fn write_process_and_mcp_are_blocked() {
        for (name, se, pc) in [
            (
                "write_file",
                SideEffect::Write,
                PermissionClass::ProjectWrite,
            ),
            (
                "apply_patch",
                SideEffect::Write,
                PermissionClass::ProjectWrite,
            ),
            (
                "run_terminal",
                SideEffect::Process,
                PermissionClass::DestructiveCommand,
            ),
            ("task", SideEffect::Process, PermissionClass::ProjectWrite),
            (
                "mcp_call",
                SideEffect::Network,
                PermissionClass::ExternalWrite,
            ),
        ] {
            assert_eq!(decision(name, se, pc), PlanDecision::Deny, "{name}");
        }
    }

    #[test]
    fn unknown_write_tool_defaults_to_deny() {
        assert_eq!(
            decision(
                "some_future_tool",
                SideEffect::Write,
                PermissionClass::AlwaysAllowed
            ),
            PlanDecision::Deny
        );
    }

    #[test]
    fn network_read_tools_reach_the_profile_gate() {
        assert_eq!(
            decision(
                "web_fetch",
                SideEffect::Network,
                PermissionClass::ExternalWrite
            ),
            PlanDecision::Allow
        );
        assert_eq!(
            decision(
                "web_search",
                SideEffect::Network,
                PermissionClass::ExternalWrite
            ),
            PlanDecision::Allow
        );
    }

    #[test]
    fn control_tools_are_control() {
        assert_eq!(
            decision(
                EXIT_PLAN_MODE_TOOL,
                SideEffect::Write,
                PermissionClass::AlwaysAllowed
            ),
            PlanDecision::Control
        );
        assert_eq!(
            decision(
                ENTER_PLAN_MODE_TOOL,
                SideEffect::ReadOnly,
                PermissionClass::AlwaysAllowed
            ),
            PlanDecision::Control
        );
    }

    // -- plan parsing -------------------------------------------------------

    fn sample_plan_json() -> serde_json::Value {
        serde_json::json!({
            "plan": {
                "title": "Add retry to the uploader",
                "summary": "Wrap the upload call in a bounded retry.",
                "steps": [
                    {"title": "Read uploader.rs", "kind": "research"},
                    {
                        "title": "Add retry loop",
                        "kind": "edit",
                        "targets": ["src/uploader.rs"],
                        "risk": "medium"
                    },
                    {"title": "cargo test", "kind": "command", "reversible": false}
                ],
                "risks": ["Retry could mask a real auth failure"],
                "open_questions": ["Max attempts?"],
                "out_of_scope": ["Changing the transport"]
            }
        })
    }

    #[test]
    fn parses_a_full_plan() {
        let plan = parse_plan(&sample_plan_json()).unwrap();
        assert_eq!(plan.steps.len(), 3);
        assert_eq!(plan.steps[0].id, "s1");
        assert_eq!(plan.steps[1].kind, PlanStepKind::Edit);
        assert_eq!(plan.steps[1].targets, vec!["src/uploader.rs".to_string()]);
        assert_eq!(plan.peak_risk(), PlanRisk::Medium);
        assert!(plan.has_irreversible_step());
        assert_eq!(plan.open_questions.len(), 1);
    }

    #[test]
    fn accepts_an_unwrapped_plan_object() {
        let raw = sample_plan_json();
        let inner = raw.get("plan").unwrap().clone();
        assert_eq!(parse_plan(&inner).unwrap().steps.len(), 3);
    }

    #[test]
    fn rejects_empty_and_untitled_plans() {
        assert!(parse_plan(&serde_json::json!({})).is_err());
        assert!(parse_plan(&serde_json::json!({"title": "x"})).is_err());
        assert!(parse_plan(&serde_json::json!({"title": "x", "steps": []})).is_err());
        assert!(parse_plan(&serde_json::json!({
            "title": "x",
            "steps": [{"detail": "no title"}]
        }))
        .is_err());
    }

    #[test]
    fn rejects_duplicate_step_ids() {
        let err = parse_plan(&serde_json::json!({
            "title": "x",
            "steps": [{"id": "a", "title": "one"}, {"id": "a", "title": "two"}]
        }))
        .unwrap_err();
        assert_eq!(err.code, "invalid_plan");
        assert!(err.message.contains("duplicate"));
    }

    #[test]
    fn rejects_oversized_step_list() {
        let steps: Vec<serde_json::Value> = (0..MAX_PLAN_STEPS + 1)
            .map(|i| serde_json::json!({"title": format!("s{i}")}))
            .collect();
        assert!(parse_plan(&serde_json::json!({"title": "x", "steps": steps})).is_err());
    }

    #[test]
    fn command_steps_are_irreversible_unless_stated() {
        let plan = parse_plan(&serde_json::json!({
            "title": "x",
            "steps": [{"title": "rm build dir", "kind": "command"}]
        }))
        .unwrap();
        assert!(!plan.steps[0].reversible);
        assert_eq!(plan.steps[0].risk, PlanRisk::Medium);
    }

    // -- latch --------------------------------------------------------------

    #[test]
    fn enter_then_approve_restores_declared_profile() {
        let id = run("latch");
        assert!(!is_active(&id));
        enter(&id, "autonomous");
        assert!(is_active(&id));
        assert_eq!(effective_profile(&id, "autonomous"), PLAN_PROFILE);

        record_submission(&id, parse_plan(&sample_plan_json()).unwrap()).unwrap();
        assert_eq!(approve(&id).unwrap(), "autonomous");
        assert!(!is_active(&id));
        assert_eq!(effective_profile(&id, "autonomous"), "autonomous");
        clear(&id);
    }

    #[test]
    fn run_declared_as_plan_falls_back_to_ask_not_plan() {
        let id = run("declared");
        assert_eq!(effective_profile(&id, PLAN_PROFILE), PLAN_PROFILE);
        enter(&id, PLAN_PROFILE);
        record_submission(&id, parse_plan(&sample_plan_json()).unwrap()).unwrap();
        assert_eq!(approve(&id).unwrap(), PLAN_DEFAULT_FALLBACK);
        assert_eq!(effective_profile(&id, PLAN_PROFILE), PLAN_DEFAULT_FALLBACK);
        clear(&id);
    }

    #[test]
    fn approval_without_a_submitted_plan_is_refused() {
        let id = run("no-plan");
        enter(&id, "ask");
        let err = approve(&id).unwrap_err();
        assert_eq!(err.code, "plan_missing");
        assert!(is_active(&id), "latch must stay closed");
        clear(&id);
    }

    #[test]
    fn rejection_keeps_the_latch_closed() {
        let id = run("reject");
        enter(&id, "ask");
        record_submission(&id, parse_plan(&sample_plan_json()).unwrap()).unwrap();
        assert_eq!(reject(&id).unwrap(), 1);
        assert_eq!(reject(&id).unwrap(), 2);
        assert!(is_active(&id));
        assert_eq!(effective_profile(&id, "ask"), PLAN_PROFILE);
        clear(&id);
    }

    #[test]
    fn re_entering_after_approval_does_not_reopen_the_latch() {
        let id = run("reenter");
        enter(&id, "ask");
        record_submission(&id, parse_plan(&sample_plan_json()).unwrap()).unwrap();
        approve(&id).unwrap();
        // A model trying to hand the user a second approval card gets nothing.
        enter(&id, "ask");
        assert!(!is_active(&id));
        assert!(record_submission(&id, parse_plan(&sample_plan_json()).unwrap()).is_err());
        clear(&id);
    }

    #[test]
    fn entering_never_escalates_a_readonly_run() {
        let id = run("readonly");
        enter(&id, "readonly");
        record_submission(&id, parse_plan(&sample_plan_json()).unwrap()).unwrap();
        assert_eq!(approve(&id).unwrap(), "readonly");
        clear(&id);
    }

    #[test]
    fn eviction_never_drops_a_live_latch() {
        let mut map: HashMap<String, PlanSession> = HashMap::new();
        let live = "live-run".to_string();
        map.insert(
            live.clone(),
            PlanSession {
                run_id: live.clone(),
                state: PlanState::Planning,
                fallback_profile: "ask".into(),
                plan: None,
                rejections: 0,
                // Oldest entry in the map: an age-only policy would evict it.
                entered_at: chrono::Utc::now() - chrono::Duration::days(1),
                approved_at: None,
            },
        );
        for i in 0..MAX_RETAINED_SESSIONS + 10 {
            let id = format!("settled-{i}");
            map.insert(
                id.clone(),
                PlanSession {
                    run_id: id,
                    state: PlanState::Approved,
                    fallback_profile: "ask".into(),
                    plan: None,
                    rejections: 0,
                    entered_at: chrono::Utc::now(),
                    approved_at: Some(chrono::Utc::now()),
                },
            );
        }
        evict_settled(&mut map);
        assert!(map.len() <= MAX_RETAINED_SESSIONS + 1);
        assert!(
            map.contains_key(&live),
            "an unapproved run must never lose its latch to eviction"
        );
    }

    // -- timeline events ----------------------------------------------------

    fn event_fields(
        kind: &assistant_protocol::v2::RunEventKind,
    ) -> (&str, &str, bool, Option<u32>, Option<&str>) {
        match kind {
            assistant_protocol::v2::RunEventKind::PlanModeChanged {
                transition,
                effective_profile,
                plan,
                rejections,
                reason,
            } => (
                transition,
                effective_profile,
                plan.is_some(),
                *rejections,
                reason.as_deref(),
            ),
            other => panic!("expected plan_mode_changed, got {}", other.type_name()),
        }
    }

    #[test]
    fn entering_reports_the_plan_gear_and_carries_no_plan() {
        let id = run("event-enter");
        let session = enter(&id, "autonomous");
        let kind = changed_event(
            &session,
            PlanTransition::Entered,
            Some("  wide blast radius  "),
        );
        let (transition, profile, has_plan, rejections, reason) = event_fields(&kind);
        assert_eq!(transition, "entered");
        assert_eq!(profile, PLAN_PROFILE);
        assert!(!has_plan, "there is no plan at entry");
        assert_eq!(rejections, None);
        assert_eq!(reason, Some("wide blast radius"), "reason must be trimmed");
        clear(&id);
    }

    #[test]
    fn approval_reports_the_restored_profile_not_the_plan_gear() {
        let id = run("event-approve");
        enter(&id, "autonomous");
        record_submission(&id, parse_plan(&sample_plan_json()).unwrap()).unwrap();

        let submitted = changed_event(&snapshot(&id).unwrap(), PlanTransition::Submitted, None);
        let (transition, profile, has_plan, _, _) = event_fields(&submitted);
        assert_eq!(transition, "submitted");
        assert_eq!(
            profile, PLAN_PROFILE,
            "submitting must not read as having left Plan Mode"
        );
        assert!(has_plan, "the card content belongs on the timeline");

        approve(&id).unwrap();
        let approved = changed_event(&snapshot(&id).unwrap(), PlanTransition::Approved, None);
        let (transition, profile, has_plan, _, _) = event_fields(&approved);
        assert_eq!(transition, "approved");
        assert_eq!(profile, "autonomous");
        assert!(has_plan, "the timeline must show what was agreed to");
        clear(&id);
    }

    #[test]
    fn rejection_keeps_the_plan_gear_and_surfaces_the_count() {
        let id = run("event-reject");
        enter(&id, "ask");
        record_submission(&id, parse_plan(&sample_plan_json()).unwrap()).unwrap();
        reject(&id).unwrap();
        reject(&id).unwrap();
        let kind = changed_event(&snapshot(&id).unwrap(), PlanTransition::Rejected, None);
        let (transition, profile, has_plan, rejections, _) = event_fields(&kind);
        assert_eq!(transition, "rejected");
        assert_eq!(
            profile, PLAN_PROFILE,
            "a rejected plan leaves the latch shut"
        );
        assert!(has_plan);
        assert_eq!(rejections, Some(2), "a loop has to be countable");
        clear(&id);
    }

    #[test]
    fn clearing_does_not_replay_a_stale_plan() {
        let id = run("event-clear");
        enter(&id, "ask");
        record_submission(&id, parse_plan(&sample_plan_json()).unwrap()).unwrap();
        let kind = changed_event(&snapshot(&id).unwrap(), PlanTransition::Cleared, None);
        let (transition, _, has_plan, _, _) = event_fields(&kind);
        assert_eq!(transition, "cleared");
        assert!(!has_plan);
        clear(&id);
    }

    #[test]
    fn operations_on_an_unknown_run_fail_closed() {
        let id = run("unknown");
        assert_eq!(approve(&id).unwrap_err().code, "not_in_plan_mode");
        assert_eq!(reject(&id).unwrap_err().code, "not_in_plan_mode");
        assert!(record_submission(&id, parse_plan(&sample_plan_json()).unwrap()).is_err());
    }
}
