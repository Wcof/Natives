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
//! ## PERF-002 warm path (metadata fast path)
//! [`PreparedAgentSessionCache::resolve`] is the two-tier lookup the run path
//! uses:
//! 1. **Fast tier** — [`project_instruction_metadata_fingerprint`]: a hash of
//!    canonical path + mtime + size for each instruction source plus a
//!    metadata-only skill-directory generation (entry name + length + mtime).
//!    No instruction content is read and no skill body is scanned, so a warm
//!    cache hit costs only `stat`/`read_dir` work.
//! 2. **Slow tier** — only when the fast tier misses, [`project_instruction_digest`]
//!    reads and SHA-256-hashes the full instruction content (and folds in skill
//!    names). A content-identical metadata move (e.g. a `touch`) re-hits the
//!    durable content-verified key, and [`PreparedAgentSessionCache::resolve`]
//!    re-indexes it under the metadata key so the next lookup is fast again.
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
    /// Project instruction discriminator (PERF-002): either the metadata
    /// fingerprint (fast tier) or the full content SHA-256 (slow tier). The
    /// two-tier [`PreparedAgentSessionCache::resolve`] lookup indexes a rebuilt
    /// session under BOTH values so a warm hit never needs the content hash.
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

/// Outcome of the PERF-002 two-tier [`PreparedAgentSessionCache::resolve`].
#[derive(Debug)]
pub enum PreparedResolve {
    /// Warm metadata-tier hit — zero instruction content reads, zero skill
    /// body scans.
    Hit(Arc<PreparedAgentSession>),
    /// Metadata moved but the full content digest re-hit a durable entry; the
    /// entry has already been re-indexed under the metadata key so the next
    /// identical lookup is metadata-only.
    ContentUnchanged(Arc<PreparedAgentSession>),
    /// No usable entry exists; the caller must rebuild. Carries the
    /// already-computed full content digest so the caller does not re-hash.
    Miss {
        /// Metadata key to index the rebuilt session under.
        fast_key: PreparedAgentSessionKey,
        /// Full content SHA-256 (already computed during the slow-tier check).
        full_digest: String,
    },
    /// No cache key is applicable (e.g. a per-run child directive is present);
    /// the caller must rebuild without inserting anything.
    NoCache,
}

impl Default for PreparedAgentSessionCache {
    fn default() -> Self {
        Self::new()
    }
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

    /// PERF-002 two-tier warm lookup.
    ///
    /// 1. **Fast tier** — builds a key from
    ///    [`project_instruction_metadata_fingerprint`] (canonical path + mtime
    ///    + size + skill-directory generation). No instruction content is read
    ///    and no skill body is scanned, so a warm hit costs only `stat` /
    ///    `read_dir` work.
    /// 2. **Slow tier** — only when the fast tier misses, the full content
    ///    digest ([`project_instruction_digest`]) is computed and the durable
    ///    content-verified key is consulted; a content-identical metadata move
    ///    still re-hits and is re-indexed under the fast key.
    ///
    /// A [`PreparedResolve::Miss`] carries the already-computed `full_digest`
    /// so the caller never re-hashes the instruction set.
    pub fn resolve(
        &self,
        project_root: &Path,
        project_identity: &str,
        capability_revision: u64,
        harness_revision: u64,
        provider_id: &str,
        model_id: &str,
        runtime_id: &str,
        app_schema_revision: u64,
    ) -> PreparedResolve {
        let fast_key = PreparedAgentSessionKey {
            project_identity: project_identity.to_string(),
            project_instruction_digest: project_instruction_metadata_fingerprint(project_root),
            capability_revision,
            harness_revision,
            provider_id: provider_id.to_string(),
            model_id: model_id.to_string(),
            runtime_id: runtime_id.to_string(),
            app_schema_revision,
        };
        if let Some(session) = self.get(&fast_key) {
            return PreparedResolve::Hit(session);
        }
        let full_digest = project_instruction_digest(project_root);
        let full_key = PreparedAgentSessionKey {
            project_instruction_digest: full_digest.clone(),
            ..fast_key.clone()
        };
        if let Some(session) = self.get(&full_key) {
            // Content unchanged but metadata moved (e.g. a touch): keep the
            // entry warm under the metadata key so the next lookup is
            // metadata-only again.
            self.insert(fast_key, (*session).clone());
            return PreparedResolve::ContentUnchanged(session);
        }
        PreparedResolve::Miss {
            fast_key,
            full_digest,
        }
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

/// Metadata-only fingerprint of the project instruction source set (PERF-002).
///
/// This is the **fast tier** of the prepared-session cache key. Unlike
/// [`project_instruction_digest`] it NEVER reads instruction content: for each
/// discovered source it hashes the canonical path label plus `mtime`/`size`
/// from `metadata()`, and for each skill directory it folds in a generation
/// built from entry names plus each entry's `len`/`mtime` — again metadata
/// only. A content edit that bumps mtime/size, a rules-file add/remove, or a
/// skill add/remove/rename therefore invalidates the fast tier, while a warm
/// hit costs only `stat` / `read_dir` syscalls.
///
/// The full content digest is computed only when this fast tier misses; see
/// [`PreparedAgentSessionCache::resolve`].
pub fn project_instruction_metadata_fingerprint(project_root: &Path) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    for source in discover_instruction_sources(project_root) {
        hasher.update(b"file\0");
        hasher.update(source.label.as_bytes());
        hasher.update(b"\0");
        match std::fs::metadata(&source.path) {
            Ok(meta) => {
                hasher.update(meta.len().to_le_bytes());
                hasher.update(modified_nanos(&meta).to_le_bytes());
            }
            Err(_) => hasher.update(b"missing"),
        }
        hasher.update(b"\0");
    }
    for entry in skill_directory_generation(project_root) {
        hasher.update(b"skill-dir\0");
        hasher.update(entry.as_bytes());
        hasher.update(b"\0");
    }
    format!("{:x}", hasher.finalize())
}

fn modified_nanos(meta: &std::fs::Metadata) -> u128 {
    meta.modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}

/// Metadata-only generation of the effective skill-entry set (PERF-002).
///
/// Mirrors the traversal and name-dedup of [`discover_skill_entry_names`] but
/// folds in each entry's `len`/`mtime` instead of reading any skill content, so
/// a skill body edit (which can change the advertised description) invalidates
/// the fast tier. Shadowed entries (duplicate name in an outer dir) contribute
/// nothing — exactly the entries [`discover_skill_entry_names`] reports.
fn skill_directory_generation(project_root: &Path) -> Vec<String> {
    let root = std::fs::canonicalize(project_root).unwrap_or_else(|_| project_root.to_path_buf());
    let instruction_dirs = project_instruction_dirs(&root);
    let mut generation: Vec<String> = Vec::new();
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
                .filter_map(|entry| entry.ok())
                .filter_map(|entry| {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if !seen.insert(name.clone()) {
                        return None;
                    }
                    let marker = entry
                        .metadata()
                        .ok()
                        .map(|meta| format!("{}\0{}", meta.len(), modified_nanos(&meta)))
                        .unwrap_or_default();
                    Some(format!("{name}\0{marker}"))
                })
                .collect();
            entries.sort();
            generation.extend(entries);
        }
    }
    generation.sort();
    generation
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

/// First 8 bytes of SHA-256 as a little-endian u64. Deterministic and cheap.
fn sha256_prefix_u64(bytes: &[u8]) -> u64 {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    u64::from_le_bytes(digest[..8].try_into().expect("sha256 prefix"))
}

/// Deterministic capability audit revision for the prepare cache key.
///
/// The resolved capability snapshot has no numeric revision field, so we fold
/// its stable audit JSON (selection/profile/team/allowlist — no secrets) into
/// a SHA-256 and take a u64 prefix. Any capability selection change bumps the
/// revision and misses the cache.
pub fn capability_audit_revision(audit: &serde_json::Value) -> u64 {
    sha256_prefix_u64(audit.to_string().as_bytes())
}

/// Deterministic revision of every capability input that contributes to the
/// prepared prompt (NE-P0-04).
///
/// The audit projection alone only carries stable IDs, so a content-only edit
/// of an Expert/Profile prompt, a Skill body, or a Team roster keeps the same
/// IDs and would silently reuse the previous compiled Prompt. This folds the
/// audit AND the prompt-contributing content (profile system prompt, selected
/// skill catalog, team roster text, team member descriptions) into one digest
/// so any content change invalidates the prepared session on the next Run.
pub fn capability_prompt_revision(
    snapshot: &crate::capability_resolution::ResolvedCapabilitySnapshot,
) -> u64 {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(snapshot.to_audit_json().to_string().as_bytes());
    hasher.update(b"\0capability-content\0");
    if let Some(profile) = &snapshot.profile {
        if let Some(prompt) = profile.system_prompt.as_deref() {
            hasher.update(prompt.as_bytes());
        }
    }
    hasher.update(b"\0");
    if let Some(skill) = snapshot.skill_prompt.as_deref() {
        hasher.update(skill.as_bytes());
    }
    hasher.update(b"\0");
    if let Some(team) = snapshot.extra_system_prompt.as_deref() {
        hasher.update(team.as_bytes());
    }
    hasher.update(b"\0");
    if let Some(team) = &snapshot.team {
        hasher.update(team.lead_expert_id.as_bytes());
        for member in &team.members {
            hasher.update(member.expert_id.as_bytes());
            hasher.update(member.name.as_bytes());
            hasher.update(member.description.as_bytes());
            hasher.update(member.role_hint.as_bytes());
        }
    }
    hasher.update(b"\0");
    u64::from_le_bytes(hasher.finalize()[..8].try_into().expect("sha256 prefix"))
}

/// Deterministic harness revision for the prepare cache key (NE-P0-04).
///
/// The harness evidence snapshot has no numeric revision field either; its
/// `canonical_hash` already covers every published layer/version plus the
/// prompt-block / builtin-replacement source digests and the frozen tool plan,
/// so we fold the same SHA-256 hex string into a u64 prefix exactly like
/// [`capability_audit_revision`]. Publishing a new Harness version or editing
/// any Harness-owned prompt content bumps the hash and misses the cache.
pub fn harness_revision(snapshot_canonical_hash: &str) -> u64 {
    sha256_prefix_u64(snapshot_canonical_hash.as_bytes())
}

/// App schema/upgrade revision for the prepare cache key (NE-P0-04).
///
/// This is the highest daemon migration version this binary knows about: adding
/// a migration changes the value, so every cached `PreparedAgentSession` from
/// an older binary misses on the next Run — a migration is exactly the point
/// where the compiled static prompt may change shape.
pub fn app_schema_revision() -> u64 {
    crate::storage::migrations::ALL
        .last()
        .map(|(version, _)| *version as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(root: &str) -> PreparedAgentSessionKey {
        PreparedAgentSessionKey {
            project_instruction_digest: project_instruction_digest(Path::new(root)),
            ..key_base(root)
        }
    }

    /// Key template without the instruction discriminator (the caller picks
    /// the digest/fingerprint it wants).
    fn key_base(root: &str) -> PreparedAgentSessionKey {
        PreparedAgentSessionKey {
            project_identity: root.to_string(),
            project_instruction_digest: String::new(),
            capability_revision: 1,
            harness_revision: 1,
            provider_id: "openai".into(),
            model_id: "gpt-4o".into(),
            runtime_id: "native".into(),
            app_schema_revision: 0,
        }
    }

    /// Pin a file's mtime so the metadata fingerprint sees an unchanged
    /// mtime/size pair across content rewrites of equal length.
    fn pin_mtime(path: &Path, mtime: std::time::SystemTime) {
        let file = std::fs::File::open(path).expect("open for mtime pin");
        let times = std::fs::FileTimes::new().set_modified(mtime);
        file.set_times(times).expect("set pinned mtime");
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

    #[test]
    fn metadata_fingerprint_invalidates_on_content_edit() {
        // PERF-002 fast tier: a size/mtime-moving edit must invalidate the
        // metadata fingerprint without reading the file content.
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path();
        std::fs::write(p.join("AGENTS.md"), "v1 rules").unwrap();
        let fp1 = project_instruction_metadata_fingerprint(p);
        std::fs::write(p.join("AGENTS.md"), "v1 rules with a longer body").unwrap();
        let fp2 = project_instruction_metadata_fingerprint(p);
        assert_ne!(
            fp1, fp2,
            "editing AGENTS.md must change the metadata fingerprint"
        );
    }

    #[test]
    fn metadata_fingerprint_invalidates_on_skill_add() {
        // PERF-002 fast tier: adding a skill must invalidate via the
        // metadata-only skill-directory generation (no skill body read).
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path();
        let fp1 = project_instruction_metadata_fingerprint(p);
        let skills = p.join(".agents").join("skills").join("review");
        std::fs::create_dir_all(&skills).unwrap();
        std::fs::write(skills.join("SKILL.md"), "# Review\n\nsummary").unwrap();
        let fp2 = project_instruction_metadata_fingerprint(p);
        assert_ne!(
            fp1, fp2,
            "adding a skill must change the metadata fingerprint"
        );
    }

    #[test]
    fn metadata_fingerprint_reads_no_content() {
        // PERF-002 fast tier must be metadata-only: two DIFFERENT contents
        // pinned to the same length and mtime yield the SAME fingerprint,
        // while the full content digest still catches the edit (slow tier).
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path();
        let a_path = p.join("AGENTS.md");
        let a: Vec<u8> = vec![b'a'; 512];
        let mut b = a.clone();
        b[400] = b'z';
        let fixed_mtime =
            std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_600_000_000);

        std::fs::write(&a_path, &a).unwrap();
        pin_mtime(&a_path, fixed_mtime);
        let fp_a = project_instruction_metadata_fingerprint(p);
        let d_a = project_instruction_digest(p);

        std::fs::write(&a_path, &b).unwrap();
        pin_mtime(&a_path, fixed_mtime);
        let fp_b = project_instruction_metadata_fingerprint(p);
        let d_b = project_instruction_digest(p);

        assert_eq!(
            fp_a, fp_b,
            "metadata fingerprint must not read file content"
        );

        assert_ne!(
            d_a, d_b,
            "full content digest must still detect the same-length edit"
        );
    }

    #[test]
    fn metadata_fingerprint_is_deterministic() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path();
        std::fs::write(p.join("AGENTS.md"), "stable rules").unwrap();
        let fp1 = project_instruction_metadata_fingerprint(p);
        let fp2 = project_instruction_metadata_fingerprint(p);
        assert_eq!(fp1, fp2, "fingerprint must be deterministic");
        assert_eq!(fp1.len(), 64, "expected a full 256-bit hex digest");
    }

    #[test]
    fn resolve_warm_hit_after_priming_uses_metadata_tier() {
        // PERF-002 two-tier flow: a miss returns the fast key + full digest,
        // the caller indexes under BOTH, and the next resolve is a metadata
        // tier Hit (no content read, no skill scan).
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path();
        std::fs::write(p.join("AGENTS.md"), "v1 rules").unwrap();
        let cache = PreparedAgentSessionCache::new();
        let session = session();
        match cache.resolve(
            p,
            &p.to_string_lossy(),
            1,
            1,
            "openai",
            "gpt-4o",
            "native",
            0,
        ) {
            PreparedResolve::Miss {
                fast_key,
                full_digest,
            } => {
                let full_key = PreparedAgentSessionKey {
                    project_instruction_digest: full_digest,
                    ..fast_key.clone()
                };
                cache.insert(full_key, session.clone());
                cache.insert(fast_key, session.clone());
            }
            other => panic!("expected Miss on a cold cache, got {other:?}"),
        }
        match cache.resolve(
            p,
            &p.to_string_lossy(),
            1,
            1,
            "openai",
            "gpt-4o",
            "native",
            0,
        ) {
            PreparedResolve::Hit(_) => {}
            other => panic!("expected a warm metadata Hit, got {other:?}"),
        }
    }

    #[test]
    fn resolve_reindexes_content_unchanged_hit_under_metadata_key() {
        // PERF-002 slow tier: an entry primed only under the full content
        // digest is re-indexed under the metadata key on the first resolve, so
        // the next identical lookup is metadata-only.
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path();
        std::fs::write(p.join("AGENTS.md"), "v1 rules").unwrap();
        let cache = PreparedAgentSessionCache::new();
        let full_key = PreparedAgentSessionKey {
            project_instruction_digest: project_instruction_digest(p),
            ..key_base(&p.to_string_lossy())
        };
        cache.insert(full_key, session());
        match cache.resolve(
            p,
            &p.to_string_lossy(),
            1,
            1,
            "openai",
            "gpt-4o",
            "native",
            0,
        ) {
            PreparedResolve::ContentUnchanged(_) => {}
            other => panic!("expected ContentUnchanged on the slow tier, got {other:?}"),
        }
        match cache.resolve(
            p,
            &p.to_string_lossy(),
            1,
            1,
            "openai",
            "gpt-4o",
            "native",
            0,
        ) {
            PreparedResolve::Hit(_) => {}
            other => panic!("expected Hit after re-indexing, got {other:?}"),
        }
    }

    /// NE-P0-04: a harness, capability, or app-schema revision change must make
    /// the next Run miss instead of reusing the previous compiled Prompt.
    #[test]
    fn harness_or_capability_or_schema_revision_change_invalidates_the_cache() {
        let dir = tempfile::tempdir().expect("tempdir");
        let p = dir.path();
        std::fs::write(p.join("AGENTS.md"), "v1 rules").unwrap();
        let cache = PreparedAgentSessionCache::new();
        let resolve = |capability_revision, harness_revision, app_schema_revision| {
            cache.resolve(
                p,
                &p.to_string_lossy(),
                capability_revision,
                harness_revision,
                "openai",
                "gpt-4o",
                "native",
                app_schema_revision,
            )
        };

        let fast_key = match resolve(1, 1, 37) {
            PreparedResolve::Miss {
                fast_key,
                full_digest,
            } => {
                let full_key = PreparedAgentSessionKey {
                    project_instruction_digest: full_digest,
                    ..fast_key.clone()
                };
                cache.insert(full_key, session());
                cache.insert(fast_key.clone(), session());
                fast_key
            }
            other => panic!("expected Miss on a cold cache, got {other:?}"),
        };
        assert!(
            cache.get(&fast_key).is_some(),
            "primed entry must be present"
        );

        // Harness edits (a new published version / prompt block) cannot reuse
        // the old prepared Prompt.
        match resolve(1, 2, 37) {
            PreparedResolve::Miss { .. } => {}
            other => panic!("harness revision bump must invalidate, got {other:?}"),
        }
        // Capability content/selection edits cannot reuse the old Prompt.
        match resolve(2, 1, 37) {
            PreparedResolve::Miss { .. } => {}
            other => panic!("capability revision bump must invalidate, got {other:?}"),
        }
        // App schema upgrades (new migration set) drop every cached session.
        match resolve(1, 1, 38) {
            PreparedResolve::Miss { .. } => {}
            other => panic!("app schema revision bump must invalidate, got {other:?}"),
        }
        // Identical revisions still hit.
        match resolve(1, 1, 37) {
            PreparedResolve::Hit(_) => {}
            other => panic!("identical revisions must hit, got {other:?}"),
        }
    }

    /// NE-P0-04: Expert/Skill/Team content edits change the capability prompt
    /// revision even when every stable ID stays the same.
    #[test]
    fn capability_prompt_revision_tracks_content_not_just_ids() {
        use crate::capability_resolution::ResolvedCapabilitySnapshot;
        let base = ResolvedCapabilitySnapshot {
            selection_active: true,
            agent_profile_id: Some("expert".into()),
            profile: Some(agent_core::AgentProfile {
                id: "expert".into(),
                name: "Expert".into(),
                system_prompt: Some("expert prompt v1".into()),
                ..Default::default()
            }),
            skill_ids: vec!["grep".into()],
            skill_prompt: Some("skill catalog v1".into()),
            extra_system_prompt: Some("team roster v1".into()),
            ..Default::default()
        };
        assert_eq!(
            capability_prompt_revision(&base),
            capability_prompt_revision(&base.clone()),
            "identical content must produce the same revision"
        );

        let mut profile_edit = base.clone();
        profile_edit.profile.as_mut().unwrap().system_prompt = Some("expert prompt v2".into());
        assert_ne!(
            capability_prompt_revision(&base),
            capability_prompt_revision(&profile_edit),
            "a profile/expert prompt edit must invalidate the prepared session"
        );

        let mut skill_edit = base.clone();
        skill_edit.skill_prompt = Some("skill catalog v2".into());
        assert_ne!(
            capability_prompt_revision(&base),
            capability_prompt_revision(&skill_edit),
            "a skill catalog content edit must invalidate the prepared session"
        );

        let mut team_edit = base.clone();
        team_edit.extra_system_prompt = Some("team roster v2".into());
        assert_ne!(
            capability_prompt_revision(&base),
            capability_prompt_revision(&team_edit),
            "a team roster content edit must invalidate the prepared session"
        );
    }

    /// NE-P0-04: the harness revision is a deterministic fold of the real
    /// snapshot canonical hash, so a harness edit changes it.
    #[test]
    fn harness_revision_follows_the_snapshot_canonical_hash() {
        let zeros = "0000000000000000000000000000000000000000000000000000000000000000";
        let ffff = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
        assert_eq!(harness_revision(zeros), harness_revision(zeros));
        assert_ne!(harness_revision(zeros), harness_revision(ffff));
    }

    /// NE-P0-04: the app schema revision tracks the daemon migration set.
    #[test]
    fn app_schema_revision_tracks_the_daemon_migration_set() {
        let highest = crate::storage::migrations::ALL
            .last()
            .map(|(version, _)| *version as u64)
            .unwrap_or(0);
        assert_eq!(app_schema_revision(), highest);
        assert!(
            app_schema_revision() >= 1,
            "the daemon schema must be revisioned"
        );
    }
}
