# Assistant Shared Git Branches Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add safe branch listing, creation, and checkout to Assistant while keeping `src-tauri/src/git.rs` as the single Git backend shared with the file manager.

**Architecture:** Extend the existing Rust Git module and Tauri command adapter instead of adding Git to Assistant V2 or its daemon. Add a focused frontend state helper and branch picker that consume `window.nativesAPI.git`; successful mutations emit one workspace event so Assistant and file surfaces invalidate from the same source.

**Tech Stack:** Rust, Tauri v2 events/commands, TypeScript, React, Node test runner.

---

## File Structure

- Modify `src-tauri/src/git.rs`: repository inspection, branch parsing, validation, checkout, create-and-checkout, and focused unit tests.
- Modify `src-tauri/src/commands/git.rs`: typed Tauri commands and shared `git:workspaceChanged` event emission.
- Modify `src-tauri/src/lib.rs`: register the added Git commands without adding Assistant-owned Git handlers.
- Modify `src/lib/tauri-adapter.ts`: expose typed `git.branches`, `git.checkoutBranch`, and `git.createBranch` methods beside existing Git status/diff methods.
- Create `src/lib/git-branch-state.ts`: pure branch picker state and branch-name validation helpers.
- Create `src/lib/git-branch-state.test.ts`: frontend contract tests.
- Create `src/components/assistant/GitBranchSelector.tsx`: loading/error/empty/success branch picker and create-branch modal trigger.
- Modify `src/components/assistant/MessageInput.tsx`: replace passive branch text with the shared Git selector.
- Modify `src/components/assistant/AssistantWorkspaceContext.tsx`: carry the active project path only; no duplicate branch authority.
- Modify `src/i18n/zh.ts` and `src/i18n/en.ts`: synchronized Git branch UI strings.

### Task 1: Rust Git branch domain contract

**Files:**
- Modify: `src-tauri/src/git.rs`

- [ ] **Step 1: Add failing branch validation and parsing tests**

Add tests that create temporary repositories with `git init`, configure a local test identity, commit one file, create `feature/existing`, and assert:

```rust
assert!(validate_branch_name("feature/new-ui").is_ok());
assert!(validate_branch_name("-danger").is_err());
assert!(validate_branch_name("bad..name").is_err());
assert!(validate_branch_name("refs/heads/main").is_err());

let branches = git_branches(repo.to_str().unwrap()).unwrap();
assert!(branches.iter().any(|branch| branch.name == "feature/existing"));
assert_eq!(branches.iter().filter(|branch| branch.current).count(), 1);
```

- [ ] **Step 2: Run the focused tests and verify RED**

Run: `rtk cargo test --manifest-path src-tauri/Cargo.toml git_branch -- --nocapture`

Expected: FAIL because `validate_branch_name` and `git_branches` do not exist.

- [ ] **Step 3: Implement typed branch inspection**

Add:

```rust
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GitBranch {
    pub name: String,
    pub current: bool,
    pub remote: bool,
    pub worktree_path: Option<String>,
}

pub fn validate_branch_name(name: &str) -> Result<()>;
pub fn git_branches(dir_path: &str) -> Result<Vec<GitBranch>>;
```

Validation must reject empty/trimmed mismatch, leading `-`, `refs/`, `HEAD`, whitespace, control characters, `..`, `@{`, `\\`, `~`, `^`, `:`, `?`, `*`, `[`, and names ending in `/`, `.`, or `.lock`. `git_branches` must use `git for-each-ref` with explicit arguments and merge local branch data with `git worktree list --porcelain` so occupied branches include their worktree path.

- [ ] **Step 4: Run the focused tests and verify GREEN**

Run: `rtk cargo test --manifest-path src-tauri/Cargo.toml git_branch -- --nocapture`

Expected: PASS.

### Task 2: Safe branch mutations

**Files:**
- Modify: `src-tauri/src/git.rs`

- [ ] **Step 1: Add failing mutation tests**

Add tests proving:

```rust
git_create_branch(repo_path, "feature/new-ui").unwrap();
assert_eq!(git_status(repo_path).unwrap().branch, "feature/new-ui");

git_checkout_branch(repo_path, "main").unwrap();
assert_eq!(git_status(repo_path).unwrap().branch, "main");

std::fs::write(repo.join("tracked.txt"), "dirty").unwrap();
let error = git_checkout_branch(repo_path, "feature/new-ui").unwrap_err();
assert!(error.to_string().contains("dirty_worktree"));
```

Also assert duplicate creation, unknown checkout, invalid names, non-repositories, and a branch occupied by another worktree fail without modifying HEAD.

- [ ] **Step 2: Run mutation tests and verify RED**

Run: `rtk cargo test --manifest-path src-tauri/Cargo.toml git_branch_mutation -- --nocapture`

Expected: FAIL because mutation functions do not exist.

- [ ] **Step 3: Implement safe mutations**

Add:

```rust
pub fn git_checkout_branch(dir_path: &str, branch: &str) -> Result<GitStatus>;
pub fn git_create_branch(dir_path: &str, branch: &str) -> Result<GitStatus>;
```

Both functions validate the repository and branch, reject a dirty worktree before mutation, and pass all Git arguments separately to `std::process::Command`. Checkout must require an existing local branch and reject another-worktree occupancy. Creation must require a non-existing local branch and execute `git switch -c <branch>`. Return fresh shared `GitStatus` after success.

- [ ] **Step 4: Run focused Rust tests**

Run: `rtk cargo test --manifest-path src-tauri/Cargo.toml git_branch -- --nocapture`

Expected: PASS with no branch test failures.

### Task 3: Tauri and adapter integration

**Files:**
- Modify: `src-tauri/src/commands/git.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/lib/tauri-adapter.ts`

- [ ] **Step 1: Add command-level tests**

Add command tests that call the command functions against a temporary repository and verify their serialized output contains `branch`, `entries`, and branch rows using camelCase `worktreePath`.

- [ ] **Step 2: Add Tauri command wrappers**

Expose:

```rust
#[tauri::command]
pub fn git_branches(dir_path: String) -> Result<Vec<git::GitBranch>>;

#[tauri::command]
pub fn git_checkout_branch(app: tauri::AppHandle, dir_path: String, branch: String) -> Result<git::GitStatus>;

#[tauri::command]
pub fn git_create_branch(app: tauri::AppHandle, dir_path: String, branch: String) -> Result<git::GitStatus>;
```

After a successful mutation, emit `git:workspaceChanged` with `{ projectPath, branch }`. Register all commands in the existing invoke handler.

- [ ] **Step 3: Extend the typed frontend bridge**

Add adapter types and methods:

```ts
export interface GitBranchInfo {
  name: string;
  current: boolean;
  remote: boolean;
  worktreePath?: string | null;
}

git: {
  branches(dirPath: string): Promise<GitBranchInfo[]>;
  checkoutBranch(dirPath: string, branch: string): Promise<GitStatus>;
  createBranch(dirPath: string, branch: string): Promise<GitStatus>;
}
```

Each method must invoke the registered Tauri command; no HTTP or direct subprocess access is allowed.

- [ ] **Step 4: Verify Rust and TypeScript contracts**

Run: `rtk cargo test --manifest-path src-tauri/Cargo.toml commands::git -- --nocapture`

Expected: PASS.

Run: `rtk tsc --noEmit`

Expected: no new bridge type errors.

### Task 4: Pure branch picker state

**Files:**
- Create: `src/lib/git-branch-state.ts`
- Create: `src/lib/git-branch-state.test.ts`

- [ ] **Step 1: Write failing state tests**

Test:

```ts
assert.equal(validateGitBranchName('feature/new-ui'), null);
assert.equal(validateGitBranchName('-danger'), 'invalid');
assert.equal(validateGitBranchName('bad..name'), 'invalid');
assert.equal(branchActionState({ dirty: true, current: false, occupied: false }), 'dirty');
assert.equal(branchActionState({ dirty: false, current: false, occupied: true }), 'occupied');
assert.equal(branchActionState({ dirty: false, current: true, occupied: false }), 'current');
assert.equal(branchActionState({ dirty: false, current: false, occupied: false }), 'ready');
```

- [ ] **Step 2: Verify RED**

Run: `rtk test npm run test -- src/lib/git-branch-state.test.ts`

Expected: FAIL because the module is absent.

- [ ] **Step 3: Implement the pure state module**

Export `validateGitBranchName`, `branchActionState`, and their input/result types. Keep frontend validation aligned with Rust for immediate feedback; Rust remains authoritative.

- [ ] **Step 4: Verify GREEN**

Run: `rtk test npm run test -- src/lib/git-branch-state.test.ts`

Expected: PASS.

### Task 5: Assistant branch selector UI

**Files:**
- Create: `src/components/assistant/GitBranchSelector.tsx`
- Modify: `src/components/assistant/MessageInput.tsx`
- Modify: `src/i18n/zh.ts`
- Modify: `src/i18n/en.ts`

- [ ] **Step 1: Implement stateful loading with event invalidation**

`GitBranchSelector` receives `projectPath`, `locale`, and current dirty state. Opening loads `window.nativesAPI.git.branches(projectPath)`. It renders separate loading, classified error, empty, and success states. Listen to `git:workspaceChanged`, filter by exact project path, and reload branch/status after matching events; clean up the listener on unmount.

- [ ] **Step 2: Implement checkout and branch creation interactions**

Rows display current and occupied status. Disable checkout for current, dirty, or occupied branches with localized reasons. Use the existing themed `Modal` for branch creation, validate while typing, submit through `git.createBranch`, and show classified errors inline. Do not use `prompt`, `confirm`, or `alert`.

- [ ] **Step 3: Integrate the selector into the composer**

Replace `MessageInput`'s passive `workspace.inspect` branch fetch with `GitBranchSelector`. Keep `activeProjectPath` from `AssistantWorkspaceContext` as the only project identity and remove any duplicate branch state from context. The composer must continue to show the project label and provider/model controls within the fixed workspace layout.

- [ ] **Step 4: Add synchronized translations**

Add matching `assistant.git.*` keys for branch label, loading, empty, create, invalid, current, occupied, dirty, checkout failure, create failure, and success in both locale files. Remove touched hard-coded `Cloud` text by routing it through i18n.

- [ ] **Step 5: Verify frontend behavior**

Run: `rtk test npm run test -- src/lib/git-branch-state.test.ts src/lib/assistant-stream-state.test.ts src/lib/provider-model-selection.test.ts`

Expected: PASS.

Run: `rtk tsc --noEmit`

Expected: PASS.

### Task 6: Shared Git regression and completion audit

**Files:**
- Verify all files above.

- [ ] **Step 1: Run Git backend regression tests**

Run: `rtk cargo test --manifest-path src-tauri/Cargo.toml git -- --nocapture`

Expected: all Git tests pass.

- [ ] **Step 2: Run assistant-focused frontend tests**

Run: `rtk test npm run test -- src/lib/git-branch-state.test.ts src/lib/provider-model-selection.test.ts src/lib/assistant-stream-state.test.ts src/lib/assistant-project-groups.test.ts`

Expected: all focused tests pass.

- [ ] **Step 3: Run static verification**

Run: `rtk tsc --noEmit`

Expected: PASS.

Run: `rtk git diff --check`

Expected: no whitespace errors.

- [ ] **Step 4: Audit single-backend and storage invariants**

Run: `rtk grep "Command::new(\"git\")" src-tauri/src`

Expected: Git product operations remain confined to the existing Git module; unrelated build/runtime probes may be documented separately.

Run: `rtk grep "recent-models|debug.*database|debug.*sqlite" src src-tauri/src`

Expected: no newly introduced debug persistence or recent-model localStorage path.
