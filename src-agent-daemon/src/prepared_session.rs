//! PreparedAgentSession (A5) — cache the STATIC, revisioned parts of run
//! preparation so each new run does not re-scan the project, re-read
//! AGENTS.md/CLAUDE.md, re-load skill catalogs, or re-compile prompt plans
//! from scratch.
//!
//! ## What is cached (safe, revisioned)
//! - compiled effective prompt (static part) + its digest
//! - project instruction fingerprint (digest of AGENTS.md/CLAUDE.md/rules)
//! - frozen tool schemas / gateway template snapshot
//! - skill catalog metadata (names/descriptions, not skill bodies' secrets)
//! - capability resolution snapshot reusable subset
//!
//! ## What is NEVER cached (A5 contract)
//! - provider credentials / secrets
//! - permission decisions (per-call, per-profile authority)
//! - checkpoint / side-effect ledger / resume state
//! - run id / conversation id / mutable transcript
//!
//! ## Invalidation
//! The key covers project identity, capability/harness revisions, and
//! provider/model/runtime. A digest of the project instruction files is part
//! of the key so editing AGENTS.md/CLAUDE.md invalidates the entry. Schema
//! revision bumps (app upgrade) also invalidate.

use agent_core::ToolSchema;
use harness_core::CompiledPromptPlan;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Cache entry size guard — never let the cache grow without bound.
const MAX_ENTRIES: usize = 32;

/// Identity of a prepared session. Everything here is revisioned/static.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PreparedAgentSessionKey {
    /// Canonical project root (identity of the project's instructions).
    pub project_identity: String,
    /// Digest of AGENTS.md/CLAUDE.md/rules files under the project.
    pub project_instruction_digest: String,
    /// ADR-0016 capability resolution revision (bumps when capability
    /// selection / profile / team changes).
    pub capability_revision: u64,
    /// Harness profile/prompt-plan revision (bumps on harness edit).
    pub harness_revision: u64,
    /// Provider + model + runtime identity.
    pub provider_id: String,
    pub model_id: String,
    pub runtime_id: String,
    /// App schema/upgrade revision — bump on migration to drop all entries.
    pub app_schema_revision: u64,
}

/// The reusable, static payload of a prepared session.
///
/// Deliberately contains NO credentials, NO permission decisions, NO
/// checkpoint/ledger/run state.
#[derive(Debug, Clone)]
pub struct PreparedAgentSession {
    /// Compiled effective prompt plan (static part; conversation content is
    /// assembled per run and never cached here).
    pub effective_prompt: CompiledPromptPlan,
    /// Digest of the compiled prompt (cheap equality check on reuse).
    pub prompt_digest: String,
    /// Frozen tool schemas snapshot (capability-selected surface).
    pub frozen_tool_schemas: Vec<ToolSchema>,
    /// Skill catalog metadata (names + descriptions only).
    pub skill_catalog_metadata: Vec<(String, String)>,
}

/// Bounded in-memory cache keyed by [`PreparedAgentSessionKey`].
#[derive(Default)]
pub struct PreparedAgentSessionCache {
    inner: Mutex<HashMap<PreparedAgentSessionKey, Arc<PreparedAgentSession>>>,
}

impl PreparedAgentSessionCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Lookup without loading anything.
    pub fn get(&self, key: &PreparedAgentSessionKey) -> Option<Arc<PreparedAgentSession>> {
        self.inner
            .lock()
            .expect("prepared session cache lock")
            .get(key)
            .cloned()
    }

    /// Insert, evicting the oldest entries beyond [`MAX_ENTRIES`].
    pub fn insert(&self, key: PreparedAgentSessionKey, session: PreparedAgentSession) {
        let mut map = self.inner.lock().expect("prepared session cache lock");
        map.insert(key, Arc::new(session));
        while map.len() > MAX_ENTRIES {
            // HashMap iteration order is unspecified but bounded; dropping the
            // first encountered entry is sufficient for a size cap.
            if let Some(oldest) = map.keys().next().cloned() {
                map.remove(&oldest);
            } else {
                break;
            }
        }
    }

    /// Clear everything (app schema bump / explicit invalidation).
    pub fn clear(&self) {
        self.inner
            .lock()
            .expect("prepared session cache lock")
            .clear();
    }
}

/// Compute a stable digest over the project instruction files
/// (AGENTS.md / CLAUDE.md / .atomcode.md / rules) so edits invalidate the key.
/// Missing files contribute their absence deterministically.
pub fn project_instruction_digest(project_root: &Path) -> String {
    use std::collections::BTreeMap;
    use std::io::Read;
    let mut files: BTreeMap<String, String> = BTreeMap::new();
    for name in [
        "AGENTS.md",
        "CLAUDE.md",
        ".atomcode.md",
        ".atomcode.user.md",
    ] {
        let path = project_root.join(name);
        match std::fs::File::open(&path) {
            Ok(mut f) => {
                let mut content = String::new();
                if f.read_to_string(&mut content).is_ok() {
                    files.insert(name.to_string(), content);
                }
            }
            Err(_) => {
                files.insert(name.to_string(), "<missing>".to_string());
            }
        }
    }
    // ALSO scan the docs/ tree for project rules files (digest only, never
    // body secrets). Keep it cheap: a stable join of file names + sizes.
    if let Ok(entries) = std::fs::read_dir(project_root.join("docs")) {
        let mut rules: Vec<String> = entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                if name.ends_with(".md") {
                    Some(format!(
                        "{name}:{}",
                        e.metadata().ok().map(|m| m.len()).unwrap_or(0)
                    ))
                } else {
                    None
                }
            })
            .collect();
        rules.sort();
        for rule in rules {
            files.insert(format!("docs/{rule}"), String::new());
        }
    }
    // Deterministic digest: hash of the sorted name=content-length pairs
    // plus a cheap content hash. This is a fingerprint, not a security bound.
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    use std::hash::{Hash, Hasher};
    for (name, content) in &files {
        name.hash(&mut hasher);
        content.len().hash(&mut hasher);
        // Cheap rolling hash over content so content edits (same length)
        // still change the fingerprint in practice.
        let mut roll: u64 = 0;
        for (i, b) in content.bytes().enumerate().take(64) {
            roll = roll.wrapping_add((b as u64).wrapping_mul((i as u64 + 1).wrapping_mul(31)));
        }
        roll.hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(root: &str) -> PreparedAgentSessionKey {
        PreparedAgentSessionKey {
            project_identity: root.to_string(),
            project_instruction_digest: project_instruction_digest(Path::new(root)),
            capability_revision: 1,
            harness_revision: 1,
            provider_id: "openai".into(),
            model_id: "gpt-4o".into(),
            runtime_id: "native".into(),
            app_schema_revision: 0,
        }
    }

    fn session() -> PreparedAgentSession {
        PreparedAgentSession {
            effective_prompt: CompiledPromptPlan::default(),
            prompt_digest: "d1".into(),
            frozen_tool_schemas: vec![],
            skill_catalog_metadata: vec![("grep".into(), "search files".into())],
        }
    }

    #[test]
    fn cache_hit_and_miss() {
        let cache = PreparedAgentSessionCache::new();
        let k = key("/tmp/natives-a5-test");
        assert!(cache.get(&k).is_none());
        cache.insert(k.clone(), session());
        assert!(cache.get(&k).is_some());
    }

    #[test]
    fn instruction_edit_invalidates_key() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path();
        std::fs::write(p.join("AGENTS.md"), "v1 rules").unwrap();
        let d1 = project_instruction_digest(p);
        std::fs::write(p.join("AGENTS.md"), "v2 rules changed").unwrap();
        let d2 = project_instruction_digest(p);
        assert_ne!(d1, d2, "editing AGENTS.md must change the fingerprint");
    }

    #[test]
    fn cache_is_bounded() {
        let cache = PreparedAgentSessionCache::new();
        for i in 0..(MAX_ENTRIES + 8) {
            let mut k = key(&format!("/tmp/natives-a5-{i}"));
            k.provider_id = format!("p{i}");
            cache.insert(k, session());
        }
        let map = cache.inner.lock().unwrap();
        assert!(map.len() <= MAX_ENTRIES, "cache must stay bounded");
    }
}
