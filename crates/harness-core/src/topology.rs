//! The fixed Native execution topology.
//!
//! This is the answer to "what does the execution engine look like at each
//! stage?". It is deliberately a **constant**, not configuration: users may
//! inspect stages, but they may never add, delete, reorder, or reconnect them
//! (design 第 11 节). What varies is which Hooks are attached to a stage's Hook
//! points — and that comes from `HookRegistry::describe()`, never from here.
//!
//! Three concepts the UI must keep apart:
//!
//! - **Stage** — a fixed Engine execution phase.
//! - **Safe point** — where Session queue / interjection coordination is legal.
//! - **Hook point** — where Hook definitions may execute.
//!
//! Each Hook point additionally records its [`TriggerSite`]: the module that
//! actually dispatches it today. That field is the difference between an
//! honest map and a brochure — five of the sixteen events are declared by the
//! Hook contract and three of them are dispatched outside `agent-core`, which
//! a stage table alone cannot express. `harness_topology_truth.rs` in the
//! daemon test suite re-derives every entry from the sources, so a dispatch
//! site that moves or disappears turns the test red instead of silently
//! turning this file into a lie.

use crate::hooks::HookEvent;
use crate::session_actor::SafePoint;
use serde::{Deserialize, Serialize};


/// Bumped whenever stages, their order, or their point assignments change.
///
/// Persisted into every `ResolvedHarnessSnapshot` so an old Run's evidence
/// stays interpretable after the topology evolves.
pub const TOPOLOGY_VERSION: u32 = 1;

/// A fixed Engine execution phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StageId {
    Session,
    Context,
    Provider,
    ToolGate,
    Permission,
    ToolExecute,
    Subagent,
    Compact,
    Stop,
    Terminal,
    /// Not a phase: events that may fire from anywhere in the loop.
    CrossStage,
}

impl StageId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::Context => "context",
            Self::Provider => "provider",
            Self::ToolGate => "tool_gate",
            Self::Permission => "permission",
            Self::ToolExecute => "tool_execute",
            Self::Subagent => "subagent",
            Self::Compact => "compact",
            Self::Stop => "stop",
            Self::Terminal => "terminal",
            Self::CrossStage => "cross_stage",
        }
    }
}

impl std::fmt::Display for StageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where a Hook point is dispatched from today.
///
/// `module` is a Rust module path rather than a line number precisely so it
/// survives ordinary edits; the guard test matches on the source file that
/// path names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TriggerSite {
    /// Fired by the single-Run engine loop.
    Engine { module: &'static str },
    /// Fired by the Daemon tool / permission path, outside the engine loop.
    ToolPath { module: &'static str },
    /// Declared by the Hook contract, dispatched by nothing.
    ///
    /// A Hook registered here is inert. The catalog must say so rather than
    /// letting a user configure a Hook that can never run.
    NotDispatched,
}

impl TriggerSite {
    pub fn module(self) -> Option<&'static str> {
        match self {
            Self::Engine { module } | Self::ToolPath { module } => Some(module),
            Self::NotDispatched => None,
        }
    }

    /// False for [`TriggerSite::NotDispatched`] — the one field a UI must never
    /// paper over.
    pub fn is_dispatched(self) -> bool {
        !matches!(self, Self::NotDispatched)
    }
}

/// One Hook event as it sits in the topology.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct HookPoint {
    pub event: HookEvent,
    pub trigger: TriggerSite,
}

/// A fixed Engine execution phase with its points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Stage {
    pub id: StageId,
    /// Position in the fixed pipeline, starting at 0.
    pub order: u8,
    pub hook_points: &'static [HookPoint],
    pub safe_points: &'static [SafePoint],
}

const ENGINE: &str = "agent-core::engine";
const TOOL_PATH: &str = "natives-agent-daemon::production_tools";

const fn engine(event: HookEvent) -> HookPoint {
    HookPoint {
        event,
        trigger: TriggerSite::Engine { module: ENGINE },
    }
}

const fn tool_path(event: HookEvent) -> HookPoint {
    HookPoint {
        event,
        trigger: TriggerSite::ToolPath { module: TOOL_PATH },
    }
}

const fn inert(event: HookEvent) -> HookPoint {
    HookPoint {
        event,
        trigger: TriggerSite::NotDispatched,
    }
}

/// The topology, in pipeline order. Every [`HookEvent`] appears exactly once.
pub const STAGES: &[Stage] = &[
    Stage {
        id: StageId::Session,
        order: 0,
        hook_points: &[
            engine(HookEvent::SessionStart),
            engine(HookEvent::UserPromptSubmit),
        ],
        safe_points: &[],
    },
    Stage {
        id: StageId::Context,
        order: 1,
        hook_points: &[],
        safe_points: &[],
    },
    Stage {
        id: StageId::Provider,
        order: 2,
        hook_points: &[],
        safe_points: &[SafePoint::ProviderBatchBoundary],
    },
    Stage {
        id: StageId::ToolGate,
        order: 3,
        hook_points: &[engine(HookEvent::PreToolUse)],
        safe_points: &[SafePoint::BeforeTool],
    },
    Stage {
        id: StageId::Permission,
        order: 4,
        hook_points: &[
            tool_path(HookEvent::PermissionRequest),
            tool_path(HookEvent::PermissionDenied),
        ],
        safe_points: &[SafePoint::AfterPermissionResolved],
    },
    Stage {
        id: StageId::ToolExecute,
        order: 5,
        hook_points: &[
            engine(HookEvent::PostToolUse),
            engine(HookEvent::PostToolUseFailure),
        ],
        safe_points: &[SafePoint::AfterTool],
    },
    Stage {
        id: StageId::Subagent,
        order: 6,
        // Dispatched from the daemon's `task` tool, not the engine loop: the
        // parent's engine hands the spawn off and returns, so `SubagentStart`
        // fires before the child run exists (a hook can still refuse it) and
        // `SubagentStop` fires from the watcher once the child reaches any
        // terminal status.
        hook_points: &[
            tool_path(HookEvent::SubagentStart),
            tool_path(HookEvent::SubagentStop),
        ],
        safe_points: &[],
    },
    Stage {
        id: StageId::Compact,
        order: 7,
        hook_points: &[
            engine(HookEvent::PreCompact),
            engine(HookEvent::PostCompact),
        ],
        safe_points: &[],
    },
    Stage {
        id: StageId::Stop,
        order: 8,
        hook_points: &[engine(HookEvent::Stop), engine(HookEvent::StopFailure)],
        safe_points: &[],
    },
    Stage {
        id: StageId::Terminal,
        order: 9,
        hook_points: &[engine(HookEvent::SessionEnd), engine(HookEvent::Error)],
        safe_points: &[],
    },
    Stage {
        id: StageId::CrossStage,
        order: 10,
        hook_points: &[tool_path(HookEvent::Notification)],
        safe_points: &[],
    },
];

/// The stage that owns `event`. Total by construction — see the tests.
pub fn stage_of(event: HookEvent) -> StageId {
    hook_point_of(event).0
}

/// The stage and Hook point for `event`.
pub fn hook_point_of(event: HookEvent) -> (StageId, HookPoint) {
    for stage in STAGES {
        for point in stage.hook_points {
            if point.event == event {
                return (stage.id, *point);
            }
        }
    }
    // Unreachable while `every_hook_event_belongs_to_exactly_one_stage` passes.
    // A panic here would take down a Run for a display concern, so degrade to
    // the cross-stage bucket instead and let the test be the alarm.
    (
        StageId::CrossStage,
        HookPoint {
            event,
            trigger: TriggerSite::NotDispatched,
        },
    )
}

/// Stable snake_case name for a safe point, for wire and UI use.
pub fn safe_point_name(point: SafePoint) -> &'static str {
    match point {
        SafePoint::ProviderBatchBoundary => "provider_batch_boundary",
        SafePoint::BeforeTool => "before_tool",
        SafePoint::AfterTool => "after_tool",
        SafePoint::AfterPermissionResolved => "after_permission_resolved",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn every_hook_event_belongs_to_exactly_one_stage() {
        let mut seen: Vec<HookEvent> = Vec::new();
        for stage in STAGES {
            for point in stage.hook_points {
                assert!(
                    !seen.contains(&point.event),
                    "{:?} is assigned to more than one stage",
                    point.event
                );
                seen.push(point.event);
            }
        }
        let placed: BTreeSet<&str> = seen.iter().map(|e| e.as_str()).collect();
        let all: BTreeSet<&str> = HookEvent::ALL.iter().map(|e| e.as_str()).collect();
        assert_eq!(placed, all, "topology must cover every HookEvent exactly once");
    }

    #[test]
    fn stage_order_is_dense_and_ascending() {
        for (index, stage) in STAGES.iter().enumerate() {
            assert_eq!(stage.order as usize, index, "stage order must be its index");
        }
    }

    #[test]
    fn every_safe_point_variant_is_placed_exactly_once() {
        let mut seen: Vec<SafePoint> = Vec::new();
        for stage in STAGES {
            for point in stage.safe_points {
                assert!(!seen.contains(point), "{point:?} placed twice");
                seen.push(*point);
            }
        }
        let names: BTreeSet<&str> = seen.iter().map(|p| safe_point_name(*p)).collect();
        assert_eq!(
            names,
            BTreeSet::from([
                "provider_batch_boundary",
                "before_tool",
                "after_tool",
                "after_permission_resolved",
            ]),
            "the four SessionCoordinator safe points must all appear"
        );
    }

    #[test]
    fn security_events_sit_in_the_gate_and_permission_stages() {
        for event in HookEvent::ALL {
            if event.is_security_sensitive() {
                assert!(
                    matches!(stage_of(event), StageId::ToolGate | StageId::Permission),
                    "{event:?} is security-sensitive but sits in {:?}",
                    stage_of(event)
                );
            }
        }
    }

    /// The inert pair is a product fact, not an accident: a UI that hides it
    /// would let a user attach a Hook that can never fire.
    /// Every declared event now has a real dispatch site. `NotDispatched` is
    /// kept as a representable state on purpose — a future event should be
    /// allowed to land in the catalog before its call site does, and saying so
    /// out loud beats quietly listing it next to the working ones.
    #[test]
    fn no_declared_hook_point_is_inert() {
        let inert: Vec<&str> = STAGES
            .iter()
            .flat_map(|s| s.hook_points)
            .filter(|p| !p.trigger.is_dispatched())
            .map(|p| p.event.as_str())
            .collect();
        assert!(inert.is_empty(), "inert hook points: {inert:?}");
    }

    #[test]
    fn stage_ids_have_unique_wire_names() {
        let names: BTreeSet<&str> = STAGES.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(names.len(), STAGES.len());
    }

    #[test]
    fn hook_point_lookup_reports_the_real_trigger() {
        let (stage, point) = hook_point_of(HookEvent::PermissionRequest);
        assert_eq!(stage, StageId::Permission);
        assert_eq!(point.trigger.module(), Some(TOOL_PATH));
        // Subagent lifecycle also lives on the tool path: the parent's engine
        // hands the spawn off and returns, so neither event can come from the
        // engine loop.
        let (stage, point) = hook_point_of(HookEvent::SubagentStart);
        assert_eq!(stage, StageId::Subagent);
        assert_eq!(point.trigger.module(), Some(TOOL_PATH));
    }
}
