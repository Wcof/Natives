//! The `HarnessControlPlane` operations behind the `harness.*` methods.
//!
//! The interface the design names (第 8 节) — `validate`, `publish`,
//! `resolve_run`, `inspect` — appears here as four groups of free functions
//! over one repository. Persistence, discovery, and projection stay internal
//! seams: a caller asks for an answer, never for the steps.
//!
//! ## The configuration hierarchy, concretely
//!
//! Decision B (design 第 21 节) is global template → project overlay → session
//! selection. That maps onto storage as:
//!
//! | Layer | Profile row | Binding row |
//! |---|---|---|
//! | Global | `kind='global_template'`, seeded as `harness.global.default` | `scope_type='global'`, `scope_id='global'` |
//! | Project | `kind='project_overlay'`, `project_id=<id>` | `scope_type='project'`, `scope_id=<project id>` |
//! | Session | any published profile — a session never owns one | `scope_type='session'`, `scope_id=<conversation id>` |
//!
//! A session *selects* a published profile; it never forks a private copy
//! (第 9.1 节), which is why there is no session-scoped draft.
//!
//! ## Layout
//!
//! This file is the aggregator: the dispatcher plus the `mod` declarations for
//! the per-responsibility files that implement the operations.
//!
//! | File | Responsibility |
//! |---|---|
//! | `control_profile.rs` | shared resolution helpers + `harness.profile.*` |
//! | `control_draft.rs` | `harness.draft.*` (validate/diff/review/simulate/publish) |
//! | `control_binding.rs` | `harness.version.*` and `harness.binding.*` |
//! | `control_observability.rs` | `harness.audit.*`, `harness.trace.*`, `harness.source.*` |
//! | `control_inspect.rs` | overview/topology/catalog/workspace/prompt preview/run snapshot |
//! | `control_resolve.rs` | the Run start seam (`resolve_run` family, `RunHarnessPlan`) |

use super::HarnessError;
use serde_json::Value;

#[path = "control_binding.rs"]
mod control_binding;
#[path = "control_draft.rs"]
mod control_draft;
#[path = "control_inspect.rs"]
mod control_inspect;
#[path = "control_observability.rs"]
mod control_observability;
#[path = "control_profile.rs"]
mod control_profile;
#[path = "control_resolve.rs"]
mod control_resolve;

use control_binding::*;
use control_draft::*;
use control_inspect::*;
use control_observability::*;
use control_profile::*;
pub use control_resolve::*;

/// Route one `harness.*` method.
pub fn request(method: &str, params: Value) -> Result<Value, HarnessError> {
    match method {
        "harness.overview" => overview(&params),
        "harness.topology" => topology(&params),
        "harness.workspace.get" => {
            let request = serde_json::from_value::<
                assistant_protocol::v2::HarnessWorkspaceGetRequest,
            >(params)
            .map_err(|error| {
                HarnessError::invalid(format!("invalid workspace request: {error}"))
            })?;
            workspace_get(&request)
        }
        "harness.template.list" => template_list(&params),
        "harness.hook.catalog" => hook_catalog(&params),
        "harness.profile.list" => profile_list(&params),
        "harness.profile.get" => profile_get(&params),
        "harness.profile.create" => profile_create(&params),
        "harness.profile.archive" => profile_archive(&params),
        "harness.draft.get" => draft_get(&params),
        "harness.draft.save" => draft_save(&params),
        "harness.draft.validate" => draft_validate(&params),
        "harness.draft.diff" => draft_diff(&params),
        "harness.draft.review" => draft_review(&params),
        "harness.draft.simulate" => draft_simulate(&params),
        "harness.draft.publish" => draft_publish(&params),
        "harness.version.list" => version_list(&params),
        "harness.version.rollback" => version_rollback(&params),
        "harness.binding.get" => binding_get(&params),
        "harness.binding.set" => binding_set(&params),
        "harness.run.getSnapshot" => run_get_snapshot(&params),
        "harness.audit.list" => audit_list(&params),
        "harness.prompt.preview" => prompt_preview(&params),
        "harness.source.list" => source_list(&params),
        "harness.source.acknowledgeDrift" => source_acknowledge_drift(&params),
        "harness.external.inspect" => external_inspect(&params),
        // Async long-poll is handled by `harness::request` before this blocking
        // dispatcher. Keeping it out of SQLite's blocking pool lets the Daemon
        // continue serving cancel and other RPCs while a subscriber waits.
        "harness.trace.list" => trace_list(&params),
        "harness.audit.export" => audit_export(&params),
        "project.identity.register" => project_identity_register(&params),
        "project.identity.list" => project_identity_list(&params),
        other => Err(HarnessError::invalid(format!(
            "unsupported harness method: {other}"
        ))),
    }
}
