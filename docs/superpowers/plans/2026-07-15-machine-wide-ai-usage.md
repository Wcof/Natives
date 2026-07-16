# Machine-wide AI Usage Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the personal dashboard aggregate reliable local usage from every supported AI tool found on the computer while explicitly identifying installed tools whose usage cannot be measured.

**Architecture:** Keep the existing read-only snapshot dashboard. Add small filesystem/database adapters that emit the existing daily/activity/session DTOs, merge them in `aggregate.rs`, and invalidate stale snapshots with a schema bump. Known desktop apps without stable token fields emit source status only.

**Tech Stack:** Rust, serde/serde_json, rusqlite, chrono, walkdir, Tauri, React, TypeScript, existing Rust and Vitest test suites.

---

### Task 1: Represent detected-but-unmeasurable applications

**Files:**
- Modify: `src-tauri/src/usage/mod.rs`
- Create: `src-tauri/src/usage/detected.rs`
- Modify: `src/types/usage.ts`
- Modify: `src/components/dashboard/UsageSourcesPanel.tsx`

- [ ] **Step 1: Write failing serialization and detection tests**

Add tests that expect `UsageSourceState::Detected` to serialize as `"detected"` and that a temporary Cursor data directory produces a status with all capabilities disabled.

```rust
assert_eq!(serde_json::to_string(&UsageSourceState::Detected).unwrap(), "\"detected\"");
let status = detected_source_status("cursor", "Cursor", &root).unwrap();
assert!(matches!(status.state, UsageSourceState::Detected));
assert!(!status.capabilities.total_tokens);
```

- [ ] **Step 2: Verify RED**

Run: `rtk cargo test --manifest-path src-tauri/Cargo.toml usage::tests::serialization_contracts_match_typescript usage::detected::tests`

Expected: compilation failure because `Detected` and `detected_source_status` do not exist.

- [ ] **Step 3: Implement the minimal status-only detector**

Add `Detected` to the Rust/TypeScript source-state unions. Implement a static list for Cursor, Claude Desktop, ChatGPT Desktop, and Antigravity using their known macOS data directories. Return only paths that exist, with no usage records and all capabilities false. Render `detected` with the existing neutral source icon treatment.

- [ ] **Step 4: Verify GREEN**

Run: `rtk cargo test --manifest-path src-tauri/Cargo.toml usage::tests::serialization_contracts_match_typescript usage::detected::tests`

Expected: all selected tests pass.

### Task 2: Correct Atomcode and include archived sessions

**Files:**
- Modify: `src-tauri/src/usage/atomcode.rs`

- [ ] **Step 1: Replace the incorrect regression with failing accounting tests**

Use one active fixture whose Snapshot contains two requests and one standalone archived `.json` fixture. Assert cache-normalized current totals and inclusion of the archived total.

```rust
assert_eq!(active.input_tokens, Some((100 - 40) + (150 - 100)));
assert_eq!(active.output_tokens, Some(30));
assert_eq!(active.cache_read_tokens, Some(140));
assert_eq!(active.total_tokens, Some(280));
assert_eq!(archived.total_tokens, Some(900));
assert_eq!(archived.input_tokens, None);
```

- [ ] **Step 2: Verify RED**

Run: `rtk cargo test --manifest-path src-tauri/Cargo.toml usage::atomcode::tests -- --nocapture`

Expected: active total remains cache-inclusive and archived record is absent.

- [ ] **Step 3: Implement normalized active records and archived records**

Use `prompt.saturating_sub(cached)` for active input. Discover `.jsonl` and standalone `.json` files under every Atomcode sessions root. Parse archive `id`, `working_dir`, `created_at`, `updated_at`, and `turn_stats`; emit a total-only daily record and one session record. Deduplicate by Atomcode session ID before aggregation.

- [ ] **Step 4: Verify GREEN**

Run: `rtk cargo test --manifest-path src-tauri/Cargo.toml usage::atomcode::tests -- --nocapture`

Expected: all Atomcode tests pass.

### Task 3: Add OpenCode database usage

**Files:**
- Create: `src-tauri/src/usage/opencode.rs`
- Modify: `src-tauri/src/usage/mod.rs`

- [ ] **Step 1: Write a failing in-memory database test**

Create minimal `session` and `message` tables, insert one completed assistant message with `tokens.input/output/reasoning/cache`, then assert the normalized DTO.

```rust
assert_eq!(result.daily[0].input_tokens, Some(10));
assert_eq!(result.daily[0].output_tokens, Some(5));
assert_eq!(result.daily[0].cache_read_tokens, Some(7));
assert_eq!(result.daily[0].cache_creation_tokens, Some(3));
assert_eq!(result.daily[0].total_tokens, Some(25));
```

- [ ] **Step 2: Verify RED**

Run: `rtk cargo test --manifest-path src-tauri/Cargo.toml usage::opencode::tests`

Expected: module/function missing.

- [ ] **Step 3: Implement read-only SQLite parsing**

Open the discovered `opencode.db` read-only, query completed assistant messages joined to session directory, parse the existing JSON token shape, merge reasoning into output, preserve reported cost, and group into daily/activity/session DTOs. Skip incomplete and all-zero messages.

- [ ] **Step 4: Verify GREEN**

Run: `rtk cargo test --manifest-path src-tauri/Cargo.toml usage::opencode::tests`

Expected: all OpenCode tests pass.

### Task 4: Add Gemini CLI session usage

**Files:**
- Create: `src-tauri/src/usage/gemini.rs`
- Modify: `src-tauri/src/usage/mod.rs`

- [ ] **Step 1: Write a failing Gemini fixture test**

Create `tmp/project/chats/session-1.json` containing one `type: gemini` message with input/output/thoughts/cached fields. Assert thoughts and cache are included once in total.

```rust
assert_eq!(record.input_tokens, Some(10));
assert_eq!(record.output_tokens, Some(6));
assert_eq!(record.cache_read_tokens, Some(8));
assert_eq!(record.total_tokens, Some(24));
```

- [ ] **Step 2: Verify RED**

Run: `rtk cargo test --manifest-path src-tauri/Cargo.toml usage::gemini::tests`

Expected: module/function missing.

- [ ] **Step 3: Implement the published Gemini CLI log parser**

Read `tmp/*/chats/session-*.json`, process only Gemini messages with token objects, use message ID for deduplication, and emit source/model/session/time dimensions. If `.gemini` exists but the CLI chat layout does not, do not misclassify Antigravity as Gemini CLI usage.

- [ ] **Step 4: Verify GREEN**

Run: `rtk cargo test --manifest-path src-tauri/Cargo.toml usage::gemini::tests`

Expected: all Gemini tests pass.

### Task 5: Add Grok CLI session usage

**Files:**
- Create: `src-tauri/src/usage/grok.rs`
- Modify: `src-tauri/src/usage/mod.rs`

- [ ] **Step 1: Write a failing Grok signals fixture test**

Create `sessions/project/session/summary.json` and `signals.json` using documented snake-case usage fields. Assert uncached input, cache, output, session, and model values.

```rust
assert_eq!(record.input_tokens, Some(7_210));
assert_eq!(record.cache_read_tokens, Some(41_000));
assert_eq!(record.output_tokens, Some(1_893));
assert_eq!(record.total_tokens, Some(50_103));
```

- [ ] **Step 2: Verify RED**

Run: `rtk cargo test --manifest-path src-tauri/Cargo.toml usage::grok::tests`

Expected: module/function missing.

- [ ] **Step 3: Implement tolerant documented-field parsing**

Discover `signals.json` recursively below `GROK_HOME/sessions`, read sibling `summary.json`, accept documented snake/camel usage field aliases, and emit no fabricated record when token usage is absent. A Grok install with no sessions remains detected with zero usage.

- [ ] **Step 4: Verify GREEN**

Run: `rtk cargo test --manifest-path src-tauri/Cargo.toml usage::grok::tests`

Expected: all Grok tests pass.

### Task 6: Discover roots, merge adapters, and invalidate old snapshots

**Files:**
- Modify: `src-tauri/src/usage/aggregate.rs`
- Modify: `src-tauri/src/usage/snapshot.rs`
- Modify: `src-tauri/src/usage/claude.rs`
- Modify: `src-tauri/src/usage/codex.rs`
- Modify: `src-tauri/src/usage/atomcode.rs`

- [ ] **Step 1: Write failing root and aggregate tests**

Assert custom Home plus default Home are deduplicated, cache tokens are included in total, and detected-only statuses enter the source list without daily rows.

```rust
assert_eq!(dedupe_paths(vec![root.clone(), root.clone()]), vec![root]);
assert!(sources.iter().any(|s| s.id == "cursor"));
assert!(!daily.iter().any(|d| d.source_id == "cursor"));
```

- [ ] **Step 2: Verify RED**

Run: `rtk cargo test --manifest-path src-tauri/Cargo.toml usage::aggregate::tests usage::tests::deduplicates_source_roots`

Expected: missing root resolver/aggregate behavior.

- [ ] **Step 3: Integrate all sources**

Resolve default and process-visible custom homes (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`, `ATOMCODE_HOME`, `GEMINI_CLI_HOME`, `GROK_HOME`, XDG OpenCode data). Run the five external scanners alongside existing sources, merge DTOs, append only detected desktop statuses, and ensure ccusage does not duplicate native Claude/Codex/Gemini/OpenCode rows.

- [ ] **Step 4: Bump snapshot schema**

Change `SNAPSHOT_SCHEMA_VERSION` to `6` so snapshots using the old total-token formula are rejected.

- [ ] **Step 5: Verify GREEN**

Run: `rtk cargo test --manifest-path src-tauri/Cargo.toml usage::`

Expected: all usage tests pass.

### Task 7: Verify the complete dashboard change

**Files:**
- Modify only files required by failures found below.

- [ ] **Step 1: Format and run Rust checks**

Run: `rtk cargo fmt --manifest-path src-tauri/Cargo.toml -- --check && rtk cargo test --manifest-path src-tauri/Cargo.toml usage:: && rtk cargo check --manifest-path src-tauri/Cargo.toml`

Expected: exit 0, no failed tests.

- [ ] **Step 2: Run frontend and diff checks**

Run: `rtk npm run typecheck && rtk git diff --check`

Expected: exit 0.

- [ ] **Step 3: Run a read-only real-data audit**

Run focused scanner tests against local standard roots without printing chat content. Confirm Atomcode source count includes active and archived sessions, OpenCode is present, and Grok/Gemini with no supported logs do not fabricate usage.

- [ ] **Step 4: Review scope**

Confirm no recursive system-disk scan, no network/API call added, no message-text Token estimation, and no user-owned untracked files changed.
