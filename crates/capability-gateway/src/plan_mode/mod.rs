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

mod parse;
mod session;

pub use parse::{
    blocked_error, decision, parse_plan, Plan, PlanDecision, PlanRisk, PlanStep, PlanStepKind,
    MAX_PLAN_STEPS, MAX_TEXT_BYTES,
};
pub use session::{
    approve, changed_event, clear, effective_profile, enter, is_active, record_submission, reject,
    snapshot, PlanSession, PlanState, PlanTransition,
};

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
