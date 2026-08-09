//! Creative-session draft tools (ADR-0014 section 8).
//!
//! These four tools are the *only* write surface a creative session gets. They
//! exist so the model can shape a Workshop SPA without ever being handed
//! `write_file` / `apply_patch` / `run_terminal`, which is what keeps ADR-0014
//! invariant #3 ("the model cannot reach the real module directory") true by
//! construction rather than by policy.
//!
//! # Why these run without a per-call permission prompt
//!
//! Every path these tools can produce is `~/.natives/drafts/<draftId>/rev-<n>.html`.
//! `draftId` is validated against the same alphabet the host uses, and the file
//! name is generated from an integer — the model never supplies a path fragment.
//! The sandbox *is* the permission boundary, so the tools are `AlwaysAllowed`
//! rather than asking the user once per generated revision.
//!
//! # Why the revision number comes from SQLite and never from the directory
//!
//! `rollback_draft_revision` moves the pointer back but keeps the newer files, so
//! the *highest* `rev-N.html` on disk is frequently the revision the user just
//! undid. `creative_drafts.current_revision` is the only authority.
//!
//! # Why the linter comes from the shared crate
//!
//! `contract-linter` is the ruler shared with the host's `write_generated_module`.

mod store;
mod tools;

pub(crate) use store::{invalid_input, str_arg};
pub use store::{validate_draft_id, CREATIVE_DRAFT_TOOL_NAMES};
pub use tools::{creative_draft_tools, creative_handoff_tool};

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
