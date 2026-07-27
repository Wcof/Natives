//! Natives Harness Core — the pure Harness domain model.
//!
//! This crate owns what a Harness *is*, never how it is executed, persisted, or
//! rendered. Execution adapters live in `agent-core`; persistence and RPC live
//! in the Daemon.
//!
//! The modules split along the two questions the control plane must answer:
//!
//! - *what is the engine shaped like?* — [`topology`], a code-fixed constant.
//! - *what is attached to it, and who said so?* — [`hooks`] for identity and
//!   provenance, [`blueprint`] for the typed overlay a user may publish,
//!   [`resolver`] for how layers combine, [`validation`] for what may be
//!   published, [`snapshot`] for what a Run provably used, and [`redaction`]
//!   for what must never be written down.
//!
//! Design: `docs/superpowers/specs/2026-07-26-native-harness-control-plane-design.md`

pub mod blueprint;
pub mod hooks;
pub mod redaction;
pub mod resolver;
pub mod session_actor;
pub mod snapshot;
pub mod topology;
pub mod validation;

pub use session_actor::{
    is_parallel_safe_tool, CoordinatorAction, PromptSource, QueueItem, QueueItemStatus, SafePoint,
    SessionActorSnapshot, SessionCoordinator, PARALLEL_SAFE_MAX_CONCURRENCY,
};

pub use hooks::{
    tool_pattern_matches, Condition, ConditionOperator, HookDefinition, HookEvent,
    HookFailurePolicy, HookId, HookKind, HookScope, HookSource,
};

pub use blueprint::{
    canonical_json, sha256_hex, HarnessBlueprint, HookOverlay, HookSemanticsVersion,
    BLUEPRINT_SCHEMA_VERSION,
};
pub use resolver::{
    is_locked, resolve, FieldOverride, ProfileLayer, Resolution, ResolutionIssue, ResolvedHook,
};
pub use snapshot::{LayerRef, PromptPlanSummary, ResolvedHarnessSnapshot};
pub use topology::{
    hook_point_of, safe_point_name, stage_of, HookPoint, Stage, StageId, TriggerSite, STAGES,
    TOPOLOGY_VERSION,
};
pub use validation::{
    diff, validate, BlueprintChange, Severity, ValidationFinding, ValidationReport,
};
