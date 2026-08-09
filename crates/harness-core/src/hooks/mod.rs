//! Hook domain model.
//!
//! `definition` describes Hooks as inspectable data — identity, provenance,
//! ordering, matching, and failure policy. The runtime in `agent-core` compiles
//! these definitions into executable handlers.

pub mod definition;

pub use definition::{
    tool_pattern_matches, Condition, ConditionOperator, HookDefinition, HookErrorCategory,
    HookEvent, HookFailurePolicy, HookId, HookInvocationStatus, HookInvocationTrace, HookKind,
    HookScope, HookSource,
};
