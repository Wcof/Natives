//! Draft domain types.
//!
//! Mirrors `src/lib/creative-draft.ts` on the frontend; the state machine is
//! defined once per side and asserted against the same ADR-0014 section 3.3 table.

use crate::{Error, Result};
use serde::{Deserialize, Serialize};

/// Lifecycle of one draft. `lint fail` does not appear here on purpose: a
/// rejected revision never lands, so the draft simply falls back to `Ready`
/// with its previous revision intact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DraftState {
    Drafting,
    Generating,
    Ready,
    Publishing,
    Published,
    Archived,
}

impl DraftState {
    pub fn as_str(&self) -> &'static str {
        match self {
            DraftState::Drafting => "drafting",
            DraftState::Generating => "generating",
            DraftState::Ready => "ready",
            DraftState::Publishing => "publishing",
            DraftState::Published => "published",
            DraftState::Archived => "archived",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "drafting" => Ok(DraftState::Drafting),
            "generating" => Ok(DraftState::Generating),
            "ready" => Ok(DraftState::Ready),
            "publishing" => Ok(DraftState::Publishing),
            "published" => Ok(DraftState::Published),
            "archived" => Ok(DraftState::Archived),
            other => Err(Error::InvalidInput(format!("unknown draft state: {other}"))),
        }
    }

    /// Legal transitions (ADR-0014 section 3.3). Anything absent is rejected by
    /// [`Self::ensure_transition`] before it can reach the database.
    pub fn can_transition_to(self, to: DraftState) -> bool {
        use DraftState::*;
        matches!(
            (self, to),
            (Drafting, Generating)
                | (Generating, Ready)
                | (Ready, Generating)
                | (Ready, Publishing)
                | (Publishing, Published)
                | (Publishing, Ready)
                | (Published, Archived)
        )
    }

    pub fn ensure_transition(self, to: DraftState) -> Result<()> {
        if self.can_transition_to(to) {
            Ok(())
        } else {
            Err(Error::InvalidInput(format!(
                "illegal draft transition: {} -> {}",
                self.as_str(),
                to.as_str()
            )))
        }
    }
}

/// One draft's metadata. Revision *content* lives on disk, not here.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreativeDraft {
    pub draft_id: String,
    pub name: String,
    /// The user's original one-sentence request.
    pub intent: String,
    pub conversation_id: Option<String>,
    /// `None` for a brand-new draft; set when continuing an existing module.
    pub origin_module_id: Option<String>,
    pub current_revision: i64,
    pub state: DraftState,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftRevision {
    pub revision: i64,
    pub content_hash: String,
    pub created_at: String,
}

/// Outcome of writing a revision — mirrors what the draft tool returns to the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteRevisionOutcome {
    pub revision: i64,
    pub content_hash: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_strings_round_trip() {
        for s in [
            DraftState::Drafting,
            DraftState::Generating,
            DraftState::Ready,
            DraftState::Publishing,
            DraftState::Published,
            DraftState::Archived,
        ] {
            assert_eq!(DraftState::parse(s.as_str()).expect("round trip"), s);
        }
        assert!(DraftState::parse("bogus").is_err());
    }

    #[test]
    fn legal_transitions_match_the_adr_table() {
        use DraftState::*;
        for (from, to) in [
            (Drafting, Generating),
            (Generating, Ready),
            (Ready, Generating),
            (Ready, Publishing),
            (Publishing, Published),
            (Publishing, Ready),
            (Published, Archived),
        ] {
            assert!(from.can_transition_to(to), "{from:?} -> {to:?} should pass");
        }
    }

    #[test]
    fn illegal_transitions_are_rejected() {
        use DraftState::*;
        for (from, to) in [
            (Drafting, Ready),
            (Drafting, Published),
            (Generating, Publishing),
            (Ready, Published),
            (Published, Generating),
            (Archived, Generating),
            (Archived, Ready),
            (Ready, Ready),
        ] {
            assert!(
                !from.can_transition_to(to),
                "{from:?} -> {to:?} should be rejected"
            );
            assert!(from.ensure_transition(to).is_err());
        }
    }
}
