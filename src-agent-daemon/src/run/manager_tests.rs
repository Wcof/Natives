use super::*;
use agent_core::EngineToolRuntime;
use assistant_protocol::v2::{
    ContinueRunRequest, CreateRunRequest, ReplayRunRequest, RetryRunRequest, StartRunRequest,
};
use std::sync::Mutex as StdMutex;

/// Process-global lock for tests that mutate NATIVES_* env (fixture, runtime dir, keys).
fn with_env_lock<R>(f: impl FnOnce() -> R) -> R {
    let _g = crate::storage::DataStore::env_test_lock();
    f()
}

#[path = "manager_tests_create.rs"]
mod create;
#[path = "manager_tests_start.rs"]
mod start;
#[path = "manager_tests_resume.rs"]
mod resume;
#[path = "manager_tests_persist.rs"]
mod persist;
#[path = "manager_tests_cancel.rs"]
mod cancel;
#[path = "manager_tests_credential.rs"]
mod credential;
#[path = "manager_tests_permission.rs"]
mod permission;
