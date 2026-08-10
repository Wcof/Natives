//! Prepared-session discovery/digest/revision helpers (W9 split from
//! prepared_session.rs). Pure free functions that mirror
//! `agent_core::assemble_context` file reads; the cache impl + tests stay in
//! prepared_session.rs.

use crate::prepared_session::{
    InstructionSource, PreparedAgentSessionCache, MAX_ENTRIES,
};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Project instruction directories from `project_root` up to (and including)
/// the nearest `.git` ancestor — a mirror of
/// `crates/agent-core/src/context.rs::project_instruction_dirs`.
pub fn project_instruction_dirs(start: &Path) -> Vec<PathBuf> {
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
