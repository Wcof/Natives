//! Natives Harness Core — the pure Harness domain model.
//!
//! This crate owns what a Harness *is*, never how it is executed, persisted, or
//! rendered. Execution adapters live in `agent-core`; persistence and RPC live
//! in the Daemon.

pub mod blueprint;
pub mod hooks;
pub mod prompt_plan;
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
    canonical_json, sha256_hex, CommandMode, CommandWorkingDirPolicy, HarnessBlueprint,
    HookAdapterSpecV3, HookOverlay, HookSemanticsVersion, NativeHookSpecV3, PromptBlockPlacement,
    PromptBlockSpecV3, PromptSemanticsVersion, BLUEPRINT_SCHEMA_VERSION,
};

pub use prompt_plan::{CompiledPromptPlan, PromptLayerKind, PromptLayerSummary, PromptPlanBuilder};

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
