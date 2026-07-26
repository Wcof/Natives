//! Hook domain model.
//!
//! `definition` describes Hooks as inspectable data — identity, provenance,
//! ordering, matching, and failure policy. The runtime in `agent-core` compiles
//! these definitions into executable handlers.

pub mod definition;

pub use definition::{
    Condition, ConditionOperator, HookDefinition, HookEvent, HookFailurePolicy, HookId, HookKind,
    HookScope, HookSource, tool_pattern_matches,
};
