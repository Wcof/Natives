//! ToolManifest — unified tool declaration for Protocol v2 / Agent Engine.

use crate::{PermissionClass, SideEffect, PathScope};
use serde::{Deserialize, Serialize};

/// Full tool manifest sent to models and the permission engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolManifest {
    pub id: String,
    pub version: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub side_effect: SideEffect,
    pub permission_class: PermissionClass,
    pub path_scope: PathScope,
    pub timeout_ms: u64,
    pub output_limit: usize,
    pub cancellable: bool,
    pub supports_streaming: bool,
    pub parallel_safe: bool,
}

impl ToolManifest {
    pub fn basic(
        id: impl Into<String>,
        description: impl Into<String>,
        schema: serde_json::Value,
        side_effect: SideEffect,
        permission_class: PermissionClass,
    ) -> Self {
        let side = side_effect;
        Self {
            id: id.into(),
            version: "1.0.0".into(),
            description: description.into(),
            input_schema: schema,
            side_effect,
            permission_class,
            path_scope: PathScope::Any,
            timeout_ms: 30_000,
            output_limit: 256_000,
            cancellable: true,
            supports_streaming: false,
            parallel_safe: matches!(side, SideEffect::ReadOnly),
        }
    }
}
