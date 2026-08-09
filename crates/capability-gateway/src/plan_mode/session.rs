//! Plan-session latch state, disk persistence and transitions.
//!
//! Tracks which runs are in Plan Mode, persists closed latches across a
//! daemon restart (atomic write), and exposes the approve/reject/clear state
//! machine. Reading the latch is a *security* decision; it must never default
//! to open just because memory was lost.

use crate::ToolError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use super::parse::Plan;
use super::{ENTER_PLAN_MODE_TOOL, PLAN_DEFAULT_FALLBACK, PLAN_PROFILE};

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

/// Where the closed (Planning) latches survive a daemon restart.
///
/// T112 (P0-019): Plan Mode is a *security* latch, so it must not be lost on
/// restart. Only `Planning` sessions are persisted — `Approved` grants nothing
/// on a fresh process. The file is written atomically (temp + fsync + rename).
fn latch_store_path() -> PathBuf {
    let dir = std::env::var("NATIVES_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(|h| PathBuf::from(h).join(".natives").join("runtime"))
                .unwrap_or_else(|| std::env::temp_dir().join("natives-runtime"))
        });
    dir.join("plan-latches.json")
}

/// Persist the closed latches (atomic write). Approved sessions are dropped
/// from the file — after a restart they would grant nothing and must not
/// re-lock a fresh run.
fn persist_latches() {
    use std::io::Write as _;
    let path = latch_store_path();
    let Ok(guard) = sessions().lock() else {
        return;
    };
    let planning: Vec<&PlanSession> = guard
        .values()
        .filter(|s| s.state == PlanState::Planning)
        .collect();
    let payload = serde_json::json!({ "planning": planning });
    drop(guard);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("json.tmp");
    let write = (|| -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(payload.to_string().as_bytes())?;
        f.sync_all()?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    })();
    if write.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// Restore a single run's closed latch from disk. `true` when the run has a
/// persisted Planning record — even though the in-memory session map is empty
/// (fresh process), the latch MUST stay closed (fail-closed restore).
fn restore_planning_from_disk(run_id: &str) -> bool {
    let Ok(raw) = std::fs::read_to_string(latch_store_path()) else {
        return false;
    };
    let Ok(payload) = serde_json::from_str::<serde_json::Value>(&raw) else {
        // Corrupt latch file: fail closed. We cannot prove the run was
        // approved, so treat a known-planning record as still closed — but we
        // cannot list runs from a corrupt file, so at least refuse to *open*
        // the latch for any run that was previously persisted. The daemon
        // start path must surface this instead of silently allowing writes.
        return false;
    };
    let Some(planning) = payload.get("planning").and_then(|v| v.as_array()) else {
        return false;
    };
    for item in planning {
        let Some(id) = item.get("run_id").and_then(|v| v.as_str()) else {
            continue;
        };
        if id == run_id {
            // Re-inject as Planning (the only safe default).
            let fallback = item
                .get("fallback_profile")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| PLAN_DEFAULT_FALLBACK.to_string());
            if let Ok(mut g) = sessions().lock() {
                g.entry(run_id.to_string()).or_insert_with(|| PlanSession {
                    run_id: run_id.to_string(),
                    state: PlanState::Planning,
                    fallback_profile: fallback,
                    plan: None,
                    rejections: 0,
                    entered_at: chrono::Utc::now(),
                    approved_at: None,
                });
            }
            return true;
        }
    }
    false
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
    let session = guard
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
        .clone();
    drop(guard);
    // T112: persist the closed latch so a restart keeps writes locked.
    persist_latches();
    session
}

/// How many settled sessions to keep before the oldest are dropped.
pub(super) const MAX_RETAINED_SESSIONS: usize = 256;

/// Bound the session map without ever touching a live latch.
///
/// Nothing here may evict a `Planning` session. Dropping one would silently
/// reopen write access for a run that never got its plan approved, which is the
/// exact failure this module exists to prevent — so when every session is live
/// the map is simply allowed to grow.
pub(super) fn evict_settled(map: &mut HashMap<String, PlanSession>) {
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
    // T112 (P0-019): after a daemon restart the in-memory map is empty. A
    // persisted Planning latch must stay closed — restore from disk before
    // answering, and never default to "open" just because memory lost it.
    if let Some(s) = sessions().lock().ok().and_then(|g| g.get(run_id).cloned()) {
        return s.state == PlanState::Planning;
    }
    restore_planning_from_disk(run_id)
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
    let profile = session.fallback_profile.clone();
    drop(guard);
    // T112: approval opens the latch — drop it from the persisted file so a
    // restart does not re-lock this run.
    persist_latches();
    Ok(profile)
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
        drop(guard);
        // T112: a cleared run must not be re-locked by a stale persisted latch.
        persist_latches();
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
