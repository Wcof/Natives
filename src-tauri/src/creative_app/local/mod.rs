//! Local creative projects — third source under Personal Creations.
//!
//! Source directory is reference-only: never copy/move/delete project files.
//! See the local_project upgrade plan + ADR-0013 extension.

pub mod ai;
pub mod deps;
pub mod lifecycle;
pub mod logs;
pub mod path;
pub mod plan;
pub mod risk;
pub mod runtime;
pub mod runtime_utils;
pub mod scan;
pub mod store;

pub use lifecycle::{
    await_start_ready, delete_app as delete_running_app, new_runtime_manager, resolve_orphan,
    shutdown_all, start_app, stop_app, LocalRuntimeHandle,
};
pub use path::{canonical_project_root, device_id, device_name, volume_identity};
pub use plan::{fingerprint_plan, validate_launch_plan};
pub use runtime::LocalRuntimeManager;
pub use scan::inspect_local_project;
pub use store::{
    delete_app, get_app, get_app_by_root, insert_app, list_apps, list_env_keys, set_env, set_state,
    summary_from_local, update_app, LocalStoreError,
};
