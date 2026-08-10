//! Skill discovery, trust, and progressive disclosure.
//!
//! Scans `.natives|.grok|.agents|.claude/skills` at project and `$HOME` scope and
//! parses each `SKILL.md` as Markdown + YAML frontmatter (the same minimal YAML
//! subset agent profiles use — see [`agent_core::parse_agent_profile_markdown`]).
//!
//! This module is the aggregate of the skill subsystem. The responsibility
//! boundaries live in sibling submodules, all re-exported here so the public
//! `crate::skill_store::*` surface is unchanged:
//!
//! - `skill_parse` — `SKILL.md` frontmatter + body parsing
//! - `skill_trust` — the durable trust ledger (`<runtime>/skills/trust.json`)
//! - `skill_surface` — the on-demand `skill` tool (load + tool-surface capping)
//!
//! # Progressive disclosure
//!
//! The system prompt carries only `name` + `description` for trusted, enabled
//! skills ([`SkillStore::advertisement`]). Bodies never enter the prompt; the
//! model pulls one on demand through the `skill` tool, which lands on
//! [`load_skill_for_project`]. Prompt cost is therefore O(skill count) in short
//! summary lines instead of O(total skill bytes) on *every* request.
//!
//! # Trust
//!
//! A skill body is unfiltered text that becomes agent instructions, so discovery
//! alone must never confer authority. Trust resolves, in order:
//!
//! 1. A `Blocked` entry in the trust ledger — always wins.
//! 2. A `Trusted` ledger entry whose recorded content hash still matches the file
//!    on disk. Editing an approved skill silently revokes its trust
//!    ([`SkillTrustBasis::ContentChanged`]) until the user re-approves.
//! 3. `$HOME/.natives/skills` — this product's own namespace in the user's home
//!    directory. Nothing but Natives (or the user) writes there, and it cannot
//!    arrive with a `git clone`.
//! 4. Otherwise `Untrusted`: not advertised, not loadable, cannot be enabled.
//!
//! Project-scope skills are deliberately *not* auto-trusted: a project tree is
//! cloned content, so `<repo>/.claude/skills/*.md` is attacker-authored text in
//! exactly the way `~/.natives/skills` is not. Grants are keyed by canonical
//! path, so trusting a skill in one location never transfers to a same-named
//! skill anywhere else.
//!
//! The ledger is a JSON file (`<runtime>/skills/trust.json`, atomic replace) so a
//! trust decision survives daemon restarts — an in-memory decision would silently
//! re-open the hole on every relaunch.
//!
//! # Tool surface
//!
//! `allowed-tools:` in frontmatter narrows the tools the model should use while
//! following a skill. It is resolved through
//! [`agent_core::resolve_child_tool_allowlist`], the single definition of tool
//! allowlist matching, so a skill can only ever name a *subset* of the surface
//! its run already has. A skill can never add a tool.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

mod skill_parse;
mod skill_surface;
mod skill_trust;

pub use skill_parse::{parse_skill_markdown, ParsedSkill};
pub use skill_surface::{
    load_skill_for_project, load_skill_for_project_with_surface, prompt_for_project,
    resolve_skill_tool_surface,
};
use skill_trust::{
    content_hash, ledger_key, load_ledger, resolve_trust, save_ledger, TrustEntry, TrustLedger,
};

/// Skill directories, highest precedence first. First hit for a name wins.
const SKILL_ROOTS: [&str; 4] = [
    ".natives/skills",
    ".grok/skills",
    ".agents/skills",
    ".claude/skills",
];

/// Descriptions are summaries; a skill cannot buy prompt real estate with a
/// 10 KB "description".
const MAX_DESCRIPTION_CHARS: usize = 200;
/// Upper bound on advertised skills so a directory full of packs cannot grow the
/// system prompt without bound. Overflow is reported honestly.
const MAX_ADVERTISED: usize = 50;
/// Upper bound on a body handed back through the `skill` tool.
const MAX_LOADED_BODY_CHARS: usize = 64_000;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillScope {
    Project,
    User,
}

/// Whether a skill may act as agent instructions.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SkillTrust {
    /// Advertised in the system prompt, loadable, may be enabled.
    Trusted,
    /// Discovered and listed for the user, but inert.
    #[default]
    Untrusted,
    /// Explicitly denied by the user; never auto-trusted by any rule.
    Blocked,
}

/// Why a skill has the trust level it has — so the GUI can explain itself
/// instead of showing a bare boolean.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillTrustBasis {
    /// Lives in `$HOME/.natives/skills`; trusted by rule, no grant needed.
    HomeNamespace,
    /// Explicit user grant, content hash still matching.
    Grant,
    /// A grant exists but the file changed since it was made.
    ContentChanged,
    /// Explicitly blocked by the user.
    Blocked,
    /// Never reviewed by the user.
    Unreviewed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillRecord {
    pub id: String,
    pub name: String,
    pub description: String,
    pub scope: SkillScope,
    /// Compatibility mirror of `trust == Trusted` for existing consumers.
    pub trusted: bool,
    pub trust: SkillTrust,
    pub trust_basis: SkillTrustBasis,
    pub enabled: bool,
    pub path: String,
    /// Tools declared by `allowed-tools:` frontmatter, exactly as declared.
    /// Advisory until intersected with the caller's surface at load time.
    pub allowed_tools: Option<Vec<String>>,
    /// SHA-256 of the file that produced this record. The trust pin.
    pub content_hash: String,
    /// Prompt body preview for list views (never used for prompt injection).
    pub body_preview: String,
}

// ─── Store ───

#[derive(Default)]
pub struct SkillStore {
    items: Mutex<HashMap<String, SkillRecord>>,
}

impl SkillStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Discover every skill visible to `project` plus the `$HOME` scope.
    ///
    /// Within a scope the roots are scanned in [`SKILL_ROOTS`] order and the
    /// first record for an id wins, so `.natives/skills/foo` shadows
    /// `.claude/skills/foo` — the same precedence agent profiles use.
    pub fn discover_for_project(&self, project: Option<&Path>) {
        let ledger = load_ledger();
        if let Some(root) = project {
            for rel in SKILL_ROOTS {
                // Project trees are cloned content: no root here is ever
                // auto-trusted, not even `.natives`.
                self.scan_dir(&root.join(rel), SkillScope::Project, false, &ledger);
            }
        }
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            let home = PathBuf::from(home);
            for rel in SKILL_ROOTS {
                self.scan_dir(
                    &home.join(rel),
                    SkillScope::User,
                    rel == ".natives/skills",
                    &ledger,
                );
            }
        }
    }

    fn scan_dir(&self, dir: &Path, scope: SkillScope, home_namespace: bool, ledger: &TrustLedger) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
        paths.sort();
        for path in paths {
            // `SKILL.md`/`skill.md` inside a directory, or a bare `.md` file.
            let skill_md = if path.is_dir() {
                let upper = path.join("SKILL.md");
                let lower = path.join("skill.md");
                if upper.exists() {
                    upper
                } else if lower.exists() {
                    lower
                } else {
                    continue;
                }
            } else if path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("md"))
                .unwrap_or(false)
            {
                path.clone()
            } else {
                continue;
            };
            let Ok(raw) = std::fs::read_to_string(&skill_md) else {
                continue;
            };
            // Name falls back to the skill *directory* for `<dir>/SKILL.md`,
            // otherwise every packaged skill would be called "SKILL".
            let parsed = parse_skill_markdown(&raw, Some(&path));
            let id = format!(
                "{}:{}",
                match scope {
                    SkillScope::Project => "project",
                    SkillScope::User => "user",
                },
                parsed.name
            );
            let hash = content_hash(&raw);
            let key = ledger_key(&skill_md);
            let (trust, basis, enabled) = resolve_trust(ledger, &key, &hash, home_namespace);
            let record = SkillRecord {
                id: id.clone(),
                name: parsed.name,
                description: parsed.description,
                scope,
                trusted: trust == SkillTrust::Trusted,
                trust,
                trust_basis: basis,
                enabled,
                path: skill_md.display().to_string(),
                allowed_tools: parsed.allowed_tools,
                content_hash: hash,
                body_preview: parsed.body.chars().take(400).collect(),
            };
            if let Ok(mut items) = self.items.lock() {
                items.entry(id).or_insert(record);
            }
        }
    }

    pub fn list(&self) -> Vec<SkillRecord> {
        let mut out: Vec<SkillRecord> = self
            .items
            .lock()
            .map(|items| items.values().cloned().collect())
            .unwrap_or_default();
        out.sort_by(|left, right| left.id.cmp(&right.id));
        out
    }

    pub fn get(&self, id: &str) -> Option<SkillRecord> {
        self.items.lock().ok()?.get(id).cloned()
    }

    /// Enable / disable a skill. Persisted, because a fresh [`SkillStore`] is
    /// built for every run — an in-memory flag would never reach a prompt.
    ///
    /// Enabling an untrusted skill stays an error: `enabled` is a preference,
    /// `trusted` is the authority gate, and preference cannot grant authority.
    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<SkillRecord, String> {
        let record = self
            .get(id)
            .ok_or_else(|| format!("skill not found: {id}"))?;
        if enabled && record.trust != SkillTrust::Trusted {
            return Err(format!(
                "cannot enable untrusted skill: {id} ({:?})",
                record.trust_basis
            ));
        }
        self.write_ledger_entry(&record, record.trust, enabled)?;
        self.update_local(id, |item| item.enabled = enabled)
    }

    /// Record a durable trust decision for `id` at its current content.
    ///
    /// `Trusted` pins the hash observed right now, so approving a skill approves
    /// *that* text and nothing later written in its place.
    pub fn set_trust(&self, id: &str, trust: SkillTrust) -> Result<SkillRecord, String> {
        let record = self
            .get(id)
            .ok_or_else(|| format!("skill not found: {id}"))?;
        // Re-read: approving a stale in-memory snapshot would pin the wrong bytes.
        let raw = std::fs::read_to_string(&record.path).map_err(|error| error.to_string())?;
        let hash = content_hash(&raw);
        if trust == SkillTrust::Trusted && hash != record.content_hash {
            return Err(format!(
                "skill changed on disk since it was listed: {id}; re-list before trusting"
            ));
        }
        let enabled = record.enabled && trust == SkillTrust::Trusted;
        self.write_ledger_entry(&record, trust, enabled)?;
        self.update_local(id, |item| {
            item.trust = trust;
            item.trusted = trust == SkillTrust::Trusted;
            item.trust_basis = match trust {
                SkillTrust::Trusted => SkillTrustBasis::Grant,
                SkillTrust::Blocked => SkillTrustBasis::Blocked,
                SkillTrust::Untrusted => SkillTrustBasis::Unreviewed,
            };
            item.enabled = enabled;
        })
    }

    fn write_ledger_entry(
        &self,
        record: &SkillRecord,
        trust: SkillTrust,
        enabled: bool,
    ) -> Result<(), String> {
        // Read-modify-write the file rather than a cached copy: another daemon
        // component may have recorded a decision since discovery.
        let mut ledger = load_ledger();
        ledger.version = 1;
        ledger.entries.insert(
            ledger_key(Path::new(&record.path)),
            TrustEntry {
                level: trust,
                content_hash: if trust == SkillTrust::Blocked {
                    String::new()
                } else {
                    record.content_hash.clone()
                },
                enabled,
                granted_at: chrono::Utc::now().to_rfc3339(),
                name: record.name.clone(),
            },
        );
        save_ledger(&ledger)
    }

    fn update_local(
        &self,
        id: &str,
        mutate: impl FnOnce(&mut SkillRecord),
    ) -> Result<SkillRecord, String> {
        let mut items = self.items.lock().map_err(|e| e.to_string())?;
        let item = items
            .get_mut(id)
            .ok_or_else(|| format!("skill not found: {id}"))?;
        mutate(item);
        Ok(item.clone())
    }

    /// Skills eligible to be advertised, in deterministic order
    /// (project before user, then by name).
    fn advertisable(&self) -> Vec<SkillRecord> {
        let Ok(items) = self.items.lock() else {
            return Vec::new();
        };
        let mut skills: Vec<SkillRecord> = items
            .values()
            .filter(|skill| skill.enabled && skill.trust == SkillTrust::Trusted)
            .cloned()
            .collect();
        skills.sort_by(|left, right| {
            let rank = |scope: SkillScope| match scope {
                SkillScope::Project => 0,
                SkillScope::User => 1,
            };
            rank(left.scope)
                .cmp(&rank(right.scope))
                .then_with(|| left.name.cmp(&right.name))
        });
        let mut seen = std::collections::HashSet::new();
        skills.retain(|skill| seen.insert(skill.name.clone()));
        skills
    }

    /// System prompt section: `name` + one-line summary only.
    ///
    /// Deliberately never contains a skill body. The body is a tool call away,
    /// which keeps every request's fixed prompt cost proportional to the number
    /// of skills rather than to their total size.
    pub fn advertisement(&self) -> String {
        let skills = self.advertisable();
        if skills.is_empty() {
            return String::new();
        }
        let overflow = skills.len().saturating_sub(MAX_ADVERTISED);
        let mut out = String::from(
            "# Available skills\n\
             Skills are on-demand procedural instructions. Only the name and summary appear here.\n\
             Call the `skill` tool with `{\"name\": \"<name>\"}` to read the full instructions \
             before doing work a skill covers, and follow them; never infer a skill's contents \
             from its summary.\n",
        );
        for skill in skills.iter().take(MAX_ADVERTISED) {
            let scope = match skill.scope {
                SkillScope::Project => "project",
                SkillScope::User => "user",
            };
            let description = if skill.description.is_empty() {
                "(no summary provided)"
            } else {
                skill.description.as_str()
            };
            out.push_str(&format!("- {} ({scope}): {description}\n", skill.name));
        }
        if overflow > 0 {
            out.push_str(&format!(
                "- ... and {overflow} more skills not listed here; call `skill` with an exact name if you know it.\n"
            ));
        }
        out.trim_end().to_string()
    }
}

static GLOBAL_SKILLS: std::sync::OnceLock<SkillStore> = std::sync::OnceLock::new();

pub fn global_skills() -> &'static SkillStore {
    GLOBAL_SKILLS.get_or_init(|| {
        let store = SkillStore::new();
        store.discover_for_project(std::env::current_dir().ok().as_deref());
        store
    })
}

#[cfg(test)]
pub(super) mod test_support {
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, MutexGuard};

    use super::SkillStore;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Each test owns its own `$HOME` and runtime dir, so the trust ledger never
    /// leaks between tests and discovery never reads the developer's real
    /// `~/.claude/skills` (which would make these assertions machine-dependent).
    pub struct Fixture {
        base: PathBuf,
        pub root: PathBuf,
        home: PathBuf,
        pub runtime: PathBuf,
        previous_home: Option<std::ffi::OsString>,
        previous_userprofile: Option<std::ffi::OsString>,
        _guard: MutexGuard<'static, ()>,
    }

    impl Fixture {
        pub fn new() -> Self {
            let guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let base = std::env::temp_dir().join(format!("natives-skill-{}", uuid::Uuid::new_v4()));
            let runtime = base.join("runtime");
            let home = base.join("home");
            std::fs::create_dir_all(&runtime).unwrap();
            std::fs::create_dir_all(&home).unwrap();
            let previous_home = std::env::var_os("HOME");
            let previous_userprofile = std::env::var_os("USERPROFILE");
            std::env::set_var("NATIVES_RUNTIME_DIR", &runtime);
            std::env::set_var("HOME", &home);
            std::env::set_var("USERPROFILE", &home);
            Self {
                root: base.join("project"),
                home,
                runtime,
                base,
                previous_home,
                previous_userprofile,
                _guard: guard,
            }
        }

        pub fn write_skill(&self, rel_root: &str, name: &str, body: &str) -> PathBuf {
            self.write_skill_under(&self.root, rel_root, name, body)
        }

        pub fn write_home_skill(&self, rel_root: &str, name: &str, body: &str) -> PathBuf {
            self.write_skill_under(&self.home.clone(), rel_root, name, body)
        }

        fn write_skill_under(
            &self,
            base: &Path,
            rel_root: &str,
            name: &str,
            body: &str,
        ) -> PathBuf {
            let dir = base.join(rel_root).join(name);
            std::fs::create_dir_all(&dir).unwrap();
            let path = dir.join("SKILL.md");
            std::fs::write(&path, body).unwrap();
            path
        }

        pub fn store(&self) -> SkillStore {
            let store = SkillStore::new();
            store.discover_for_project(Some(&self.root));
            store
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            match self.previous_home.take() {
                Some(home) => std::env::set_var("HOME", home),
                None => std::env::remove_var("HOME"),
            }
            match self.previous_userprofile.take() {
                Some(profile) => std::env::set_var("USERPROFILE", profile),
                None => std::env::remove_var("USERPROFILE"),
            }
            std::env::remove_var("NATIVES_RUNTIME_DIR");
            let _ = std::fs::remove_dir_all(&self.base);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::Fixture;
    use super::*;

    // ─── Discovery contract (carried over from the pre-frontmatter store) ───

    #[test]
    fn discovers_skill_md() {
        let fx = Fixture::new();
        fx.write_skill(
            ".natives/skills",
            "demo",
            "# Demo\n\nDo the demo thing carefully.\n",
        );
        let store = fx.store();
        let list = store.list();
        let demo = list.iter().find(|x| x.name == "demo").expect("demo listed");
        // Body-only file still yields a truthful description.
        assert_eq!(demo.description, "Do the demo thing carefully.");
        // Project trees are never auto-trusted, so nothing is advertised yet.
        assert_eq!(demo.trust, SkillTrust::Untrusted);
        assert!(store.advertisement().is_empty());

        store.set_trust(&demo.id, SkillTrust::Trusted).unwrap();
        let ad = store.advertisement();
        assert!(
            ad.contains("demo (project): Do the demo thing carefully."),
            "{ad}"
        );
    }

    #[test]
    fn discovers_agents_skill_md() {
        let fx = Fixture::new();
        fx.write_skill(
            ".agents/skills",
            "review",
            "# Review\n\nCheck the actual diff before reporting completion.\n",
        );
        let store = fx.store();
        let review = store
            .list()
            .into_iter()
            .find(|skill| skill.name == "review")
            .expect("review listed");
        store.set_trust(&review.id, SkillTrust::Trusted).unwrap();

        // A fresh store (as every run builds) sees the persisted grant.
        let loaded = load_skill_for_project(&fx.root, "review").unwrap();
        assert_eq!(loaded["loaded"], true);
        assert!(loaded["content"].as_str().unwrap().contains("actual diff"));
    }

    #[test]
    fn directory_skill_is_named_after_its_directory_not_the_file() {
        let fx = Fixture::new();
        fx.write_skill(".natives/skills", "deploy-staging", "Body only.\n");
        let names: Vec<String> = fx.store().list().into_iter().map(|s| s.name).collect();
        assert!(names.contains(&"deploy-staging".to_string()), "{names:?}");
        assert!(!names.contains(&"SKILL".to_string()), "{names:?}");
    }

    // ─── Progressive disclosure ───

    #[test]
    fn advertisement_carries_summaries_never_bodies() {
        let fx = Fixture::new();
        fx.write_skill(
            ".natives/skills",
            "deploy",
            "---\nname: deploy\ndescription: Ship the staging build.\n---\nSECRET_BODY_MARKER: run the rollout script.\n",
        );
        let store = fx.store();
        let id = store.list()[0].id.clone();
        store.set_trust(&id, SkillTrust::Trusted).unwrap();

        let ad = store.advertisement();
        assert!(ad.contains("deploy"), "{ad}");
        assert!(ad.contains("Ship the staging build."), "{ad}");
        assert!(!ad.contains("SECRET_BODY_MARKER"), "{ad}");
        assert!(ad.contains("`skill` tool"), "{ad}");

        // The body is reachable only through the tool.
        let loaded = load_skill_for_project(&fx.root, "deploy").unwrap();
        assert!(loaded["content"]
            .as_str()
            .unwrap()
            .contains("SECRET_BODY_MARKER"));
    }

    #[test]
    fn prompt_for_project_is_bounded_by_summaries_not_body_size() {
        let fx = Fixture::new();
        let huge = "x".repeat(50_000);
        fx.write_skill(
            ".natives/skills",
            "big",
            &format!("---\nname: big\ndescription: A big skill.\n---\n{huge}\n"),
        );
        let store = fx.store();
        let id = store.list()[0].id.clone();
        store.set_trust(&id, SkillTrust::Trusted).unwrap();

        let prompt = prompt_for_project(&fx.root);
        assert!(prompt.contains("A big skill."));
        assert!(!prompt.contains(&huge));
        assert!(
            prompt.len() < 1_000,
            "prompt grew with body: {}",
            prompt.len()
        );
    }

    #[test]
    fn no_trusted_skills_yields_no_prompt_section() {
        let fx = Fixture::new();
        fx.write_skill(".claude/skills", "untrusted", "Body.\n");
        assert!(prompt_for_project(&fx.root).is_empty());
    }
}
