//! Built-in module registry and per-user product configuration.
#![allow(dead_code)]

mod cleanup;
pub mod mutation;
pub mod product;
pub mod query;
pub mod schema;
pub mod types;

pub use cleanup::{ClearDataReceipt, ClearDataScope};
pub use mutation::AppStore;
