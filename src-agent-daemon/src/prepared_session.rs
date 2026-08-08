//! PreparedAgentSession (A5) — cache the STATIC, revisioned parts of run
//! preparation so each new run does not re-scan the project, re-read
//! AGENTS.md/CLAUDE.md, re-load skill catalogs, or re-compile prompt plans
//! from scratch.
//!
//! ## What is cached (safe, revisioned)
//! - compiled effective prompt (static part) + its digest
//! - project instruction fingerprint (full-SHA-256 digest of the instruction
//!   source set that [`agent_core::assemble_context`] actually reads)
//! - frozen tool schemas / gateway template snapshot
//! - skill catalog metadata (names/descriptions, not skill bodies' secrets)
//! - capability resolution snapshot reusable subset
//!
//! ## What is NEVER cached (A5 contract — excluded by construction)
//! - provider credentials / secrets
//! - permission decisions (per-call, per-profile authority)
//! - checkpoint / side-effect ledger / resume state
//! - run id / conversation id / mutable transcript
//!
//! ## Invalidation
//! The key covers project identity, capability/harness revisions, and
//! provider/model/runtime. The project instruction digest is a full SHA-256
//! over every instruction source file (AGENTS.md/CLAUDE.md, ancestor-directory
//! copies, `.agents/rules` / `.claude/rules` / `.natives/rules`, user-level
//! `$HOME/.natives` / `.agents` / `.claude` files) **plus** skill entry names.
//! Any content edit anywhere in a file (not just the first 64 bytes)
//! invalidates the entry, as does adding/removing a rules file or a skill.
//! Schema revision bumps (app upgrade) also invalidate.
//!
//! ## Cache policy
//! Bounded LRU: at most [`MAX_ENTRIES`], evicting the least-recently-used key.
//! Eviction is fully deterministic for a given access sequence (an explicit
//! MRU-order `VecDeque`, never HashMap iteration order).
//!
//! ## NEEDS-INTEGRATION (Wave-2)
//! The digest/discovery helpers live here in the daemon, but the real prompt
//! assembly (`assemble_context`) lives in `crates/agent-core/src/context.rs`,
//! and the run path that would consume this cache is `production.rs` (owned by
//! S1). This module only provides the helper + tests. Wave-2 must (a) compute
//! [`project_instruction_digest`] when building the cache key and (b) keep the
//! mirrored discovery in sync with `assemble_context`'s file reads.

use agent_core::ToolSchema;
use harness_core::CompiledPromptPlan;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
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

/// A file that contributes to the assembled project instruction prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstructionSource {
    /// Stable, deterministic label used in the digest: relative to the project
    /// root when possible, otherwise `home/<rel>` for user-level config,
    /// otherwise the absolute path.
    pub label: String,
    /// Path of the source file.
    pub path: PathBuf,
}

/// Bounded LRU cache keyed by [`PreparedAgentSessionKey`].
///
/// Eviction is deterministic (explicit MRU order). The payload never contains
/// credentials / permission decisions / run id / transcript — those are
/// assembled per run and excluded by construction.
pub struct PreparedAgentSessionCache {
    inner: Mutex<PreparedLru>,
}

/// Inner LRU state. `order` front = least-recently-used.
struct PreparedLru {
    entries: HashMap<PreparedAgentSessionKey, Arc<PreparedAgentSession>>,
    order: VecDeque<PreparedAgentSessionKey>,
}

impl PreparedAgentSessionCache {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(PreparedLru {
                entries: HashMap::new(),
                order: VecDeque::new(),
            }),
        }
    }

    /// Lookup without loading anything. Touches the key so it becomes
    /// most-recently-used.
    pub fn get(&self, key: &PreparedAgentSessionKey) -> Option<Arc<PreparedAgentSession>> {
        let mut lru = self.inner.lock().expect("prepared session cache lock");
        let value = lru.entries.get(key).cloned()?;
        lru.order.retain(|k| k != key);
        lru.order.push_back(key.clone());
        Some(value)
    }

    /// Insert, evicting the least-recently-used key beyond [`MAX_ENTRIES`].
    pub fn insert(&self, key: PreparedAgentSessionKey, session: PreparedAgentSession) {
        let mut lru = self.inner.lock().expect("prepared session cache lock");
        if lru.entries.contains_key(&key) {
            lru.order.retain(|k| *k != key);
        }
        lru.entries.insert(key.clone(), Arc::new(session));
        lru.order.push_back(key);
        while lru.entries.len() > MAX_ENTRIES {
            match lru.order.pop_front() {
                Some(oldest) => {
                    lru.entries.remove(&oldest);
                }
                None => break,
            }
        }
    }

    /// Clear everything (app schema bump / explicit invalidation).
    pub fn clear(&self) {
        let mut lru = self.inner.lock().expect("prepared session cache lock");
        lru.entries.clear();
        lru.order.clear();
    }
}

/// Project instruction directories from `project_root` up to (and including)
/// the nearest `.git` ancestor — a mirror of
/// `crates/agent-core/src/context.rs::project_instruction_dirs`.
fn project_instruction_dirs(start: &Path) -> Vec<PathBuf> {
    let root = start
        .ancestors()
        .find(|dir| dir.join(".git").exists())
        .unwrap_or(start);
    let mut dirs = Vec::new();
    let mut current = Some(start);
    while let Some(dir) = current {
        dirs.push(dir.to_path_buf());
        if dir == root {
            break;
        }
        current = dir.parent();
    }
    dirs.reverse();
    dirs
}

/// Discover every instruction/rules file that [`agent_core::assemble_context`]
/// reads when building the system prompt for `project_root`:
///
/// 1. `AGENTS.md` / `agents.md` / `CLAUDE.md` / `Claude.md` in each project
///    directory from `project_root` up to the nearest `.git` ancestor
///    (mirrors `agent_core::context::project_instruction_dirs`).
/// 2. `*.md` rules under `.agents/rules`, `.claude/rules`, `.natives/rules`
///    in each of those directories.
/// 3. User-level `AGENTS.md` / `agents.md` / `CLAUDE.md` / `Claude.md` and
///    `rules/*.md` under `$HOME/.natives`, `$HOME/.agents`, `$HOME/.claude`
///    (the same HOME scan `assemble_context` performs).
///
/// Skill bodies are injected by the daemon SkillStore and are deliberately NOT
/// read here; skill entry *names* (which `assemble_context` advertises) are
/// folded into the digest by [`discover_skill_entry_names`].
///
/// The returned set is sorted by label and deduplicated by canonical path, so
/// the digest is deterministic. **NEEDS-INTEGRATION:** keep this mirror in
/// sync with `assemble_context`'s file reads (Wave-2 must consume this helper
/// when building the cache key).
pub fn discover_instruction_sources(project_root: &Path) -> Vec<InstructionSource> {
    let root = std::fs::canonicalize(project_root).unwrap_or_else(|_| project_root.to_path_buf());
    let mut sources: Vec<InstructionSource> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();

    let instruction_dirs = project_instruction_dirs(&root);
    for dir in &instruction_dirs {
        for name in ["AGENTS.md", "agents.md", "CLAUDE.md", "Claude.md"] {
            push_instruction_source(&mut sources, &mut seen, &dir.join(name), &root);
        }
        for rules_dir in [".agents/rules", ".claude/rules", ".natives/rules"] {
            push_rules_sources(&mut sources, &mut seen, &dir.join(rules_dir), &root);
        }
    }

    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        let home = PathBuf::from(home);
        for config_root in [
            home.join(".natives"),
            home.join(".agents"),
            home.join(".claude"),
        ] {
            for name in ["AGENTS.md", "agents.md", "CLAUDE.md", "Claude.md"] {
                push_instruction_source(&mut sources, &mut seen, &config_root.join(name), &root);
            }
            push_rules_sources(&mut sources, &mut seen, &config_root.join("rules"), &root);
        }
    }

    sources.sort_by(|a, b| a.label.cmp(&b.label));
    sources
}

fn push_instruction_source(
    sources: &mut Vec<InstructionSource>,
    seen: &mut HashSet<PathBuf>,
    path: &Path,
    project_root: &Path,
) {
    if !path.is_file() {
        return;
    }
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if !seen.insert(canonical) {
        return;
    }
    sources.push(InstructionSource {
        label: source_label(path, project_root),
        path: path.to_path_buf(),
    });
}

fn push_rules_sources(
    sources: &mut Vec<InstructionSource>,
    seen: &mut HashSet<PathBuf>,
    rules_dir: &Path,
    project_root: &Path,
) {
    let mut rules: Vec<PathBuf> = std::fs::read_dir(rules_dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
        })
        .collect();
    rules.sort();
    for path in rules {
        push_instruction_source(sources, seen, &path, project_root);
    }
}

fn source_label(path: &Path, project_root: &Path) -> String {
    if let Ok(rel) = path.strip_prefix(project_root) {
        return rel.to_string_lossy().to_string();
    }
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        let home = PathBuf::from(home);
        if let Ok(rel) = path.strip_prefix(&home) {
            return format!("home/{}", rel.to_string_lossy());
        }
    }
    path.to_string_lossy().to_string()
}

/// Skill entry *names* that [`agent_core::assemble_context`] advertises in the
/// system prompt (bodies are injected by the SkillStore and stay out of this
/// digest). Mirrors the skills scan in `assemble_context` across the project
/// instruction dirs, but folds in ALL names (not capped at 20/dir) so any
/// skill addition/removal invalidates the cache key.
pub fn discover_skill_entry_names(project_root: &Path) -> Vec<String> {
    let root = std::fs::canonicalize(project_root).unwrap_or_else(|_| project_root.to_path_buf());
    let instruction_dirs = project_instruction_dirs(&root);
    let mut names: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for dir in instruction_dirs.iter().rev() {
        for skills_dir in [
            ".natives/skills",
            ".grok/skills",
            ".agents/skills",
            ".claude/skills",
        ] {
            let Ok(rd) = std::fs::read_dir(dir.join(skills_dir)) else {
                continue;
            };
            let mut entries: Vec<String> = rd
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|name| seen.insert(name.clone()))
                .collect();
            entries.sort();
            names.extend(entries);
        }
    }
    names.sort();
    names
}

/// Full-SHA-256 fingerprint of the project instruction source set.
///
/// Upgraded from the legacy cheap rolling hash: every discovered file is
/// hashed over its FULL content (length-prefixed), so a same-length edit
/// anywhere in a file — not just the first 64 bytes — invalidates the entry.
/// Skill entry names are folded in (name-only). Missing files contribute
/// nothing to the set (their absence is deterministic: creation changes the
/// set), and unreadable files contribute a fixed marker so a permission flip
/// invalidates.
pub fn project_instruction_digest(project_root: &Path) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    for source in discover_instruction_sources(project_root) {
        hasher.update(b"file\0");
        hasher.update(source.label.as_bytes());
        hasher.update(b"\0");
        match std::fs::read(&source.path) {
            Ok(bytes) => {
                hasher.update((bytes.len() as u64).to_le_bytes());
                hasher.update(&bytes);
            }
            Err(_) => hasher.update(b"unreadable"),
        }
        hasher.update(b"\0");
    }
    for name in discover_skill_entry_names(project_root) {
        hasher.update(b"skill\0");
        hasher.update(name.as_bytes());
        hasher.update(b"\0");
    }
    format!("{:x}", hasher.finalize())
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
    fn instruction_tail_same_length_edit_invalidates() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path();
        let a: Vec<u8> = vec![b'a'; 512];
        let mut b = a.clone();
        // Same length, but edit well past the old 64-byte rolling-hash window —
        // the full-SHA-256 digest must still invalidate.
        b[400] = b'z';
        b[499] = b'Z';
        std::fs::write(p.join("AGENTS.md"), &a).unwrap();
        let d1 = project_instruction_digest(p);
        std::fs::write(p.join("AGENTS.md"), &b).unwrap();
        let d2 = project_instruction_digest(p);
        assert_ne!(d1, d2, "same-length tail edit must invalidate the key");
    }

    #[test]
    fn full_sha256_digest_detects_content_change() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path();
        std::fs::write(p.join("CLAUDE.md"), "identical length line #1").unwrap();
        let d1 = project_instruction_digest(p);
        std::fs::write(p.join("CLAUDE.md"), "identical length line #9").unwrap();
        let d2 = project_instruction_digest(p);
        assert_ne!(d1, d2, "same-length content change must change the digest");
        // Deterministic: same bytes -> same digest across repeated calls.
        let d3 = project_instruction_digest(p);
        assert_eq!(d2, d3, "digest must be deterministic");
        // And a 512-char digest proving it is a real SHA-256, not a u64 hash.
        assert_eq!(d2.len(), 64, "expected a full 256-bit hex digest");
    }

    #[test]
    fn rules_edit_invalidates() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path();
        let rules = p.join(".agents").join("rules");
        std::fs::create_dir_all(&rules).unwrap();
        let rule = rules.join("review.md");
        std::fs::write(&rule, "always run rustfmt").unwrap();
        let d1 = project_instruction_digest(p);
        std::fs::write(&rule, "always run clippy").unwrap();
        let d2 = project_instruction_digest(p);
        assert_ne!(d1, d2, "editing a rules/*.md file must invalidate the key");
    }

    #[test]
    fn discovery_covers_assemble_context_sources() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path();
        std::fs::create_dir_all(p.join(".git")).unwrap();
        let nested = p.join("apps").join("web");
        std::fs::create_dir_all(nested.join(".claude").join("rules")).unwrap();
        std::fs::create_dir_all(nested.join(".natives").join("skills").join("review")).unwrap();
        std::fs::write(p.join("AGENTS.md"), "repo instruction").unwrap();
        std::fs::write(nested.join("CLAUDE.md"), "nested instruction").unwrap();
        std::fs::write(nested.join(".claude").join("rules").join("fmt.md"), "fmt").unwrap();
        std::fs::write(
            nested
                .join(".natives")
                .join("skills")
                .join("review")
                .join("SKILL.md"),
            "# Review",
        )
        .unwrap();

        let ctx = agent_core::assemble_context(None, Some(nested.as_path()), None);
        let sources = discover_instruction_sources(&nested);
        let skill_names = discover_skill_entry_names(&nested);
        // Every .md file read by the real assembly is discovered here.
        for src in ctx.sources.iter().filter(|s| s.contains(".md")) {
            let canonical = std::fs::canonicalize(src).unwrap_or_else(|_| PathBuf::from(src));
            let discovered = sources.iter().any(|s| {
                std::fs::canonicalize(&s.path).unwrap_or_else(|_| s.path.clone()) == canonical
            });
            assert!(
                discovered,
                "assemble_context read {src} but discovery missed it"
            );
        }
        assert!(
            skill_names.iter().any(|n| n == "review"),
            "skill entry names must be discovered"
        );
        assert!(
            ctx.system_prompt.contains("- review"),
            "assemble_context should advertise the review skill"
        );
    }

    #[test]
    fn cache_is_bounded() {
        let cache = PreparedAgentSessionCache::new();
        for i in 0..(MAX_ENTRIES + 8) {
            let mut k = key(&format!("/tmp/natives-a5-{i}"));
            k.provider_id = format!("p{i}");
            cache.insert(k, session());
        }
        let lru = cache.inner.lock().unwrap();
        assert!(lru.entries.len() <= MAX_ENTRIES, "cache must stay bounded");
        assert_eq!(
            lru.entries.len(),
            lru.order.len(),
            "order must track entries"
        );
    }

    #[test]
    fn cache_evicts_least_recently_used_deterministically() {
        let cache = PreparedAgentSessionCache::new();
        let mut keys = Vec::new();
        for i in 0..MAX_ENTRIES {
            let mut k = key(&format!("/tmp/natives-a5-lru-{i}"));
            k.provider_id = format!("p{i}");
            cache.insert(k.clone(), session());
            keys.push(k);
        }
        // Touch the first key so it is now most-recently-used; the next LRU
        // victim is `keys[1]`.
        assert!(cache.get(&keys[0]).is_some());
        let mut overflow = key("/tmp/natives-a5-lru-overflow");
        overflow.provider_id = "overflow".into();
        cache.insert(overflow, session());

        let lru = cache.inner.lock().unwrap();
        assert_eq!(lru.entries.len(), MAX_ENTRIES);
        assert!(
            lru.entries.contains_key(&keys[0]),
            "touched entry must survive eviction"
        );
        assert!(
            !lru.entries.contains_key(&keys[1]),
            "deterministic LRU eviction must drop the least-recently-used key"
        );
    }
}
