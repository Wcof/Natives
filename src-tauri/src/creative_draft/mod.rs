//! Creative draft domain — the workspace an idea lives in before it becomes a module.
//!
//! A draft has no `contract_id`, no `modules` row and no sidebar entry. It gets
//! a real module identity only when the user publishes it, at which point the
//! content goes through the one existing gate, `module_manager::
//! write_generated_module` (ADR-0014 section 9).
//!
//! Layout:
//! - [`paths`] — filesystem layout and the containment boundary draft tools run inside
//! - [`model`] — draft state machine and domain types
//! - [`store`] — SQLite metadata plus on-disk revision content

pub mod model;
pub mod paths;
pub mod store;

pub use model::{CreativeDraft, DraftRevision, DraftState, WriteRevisionOutcome};
