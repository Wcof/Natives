# Long Task 01: Shared Git IPC Hardening

## Mission

Finish the Git backend and IPC seam so the file manager and Assistant use one authoritative Git implementation. Do not build branch UI in this task.

## Current State You Must Preserve

- `src-tauri/src/git.rs` already implements branch validation, local/remote listing, linked-worktree inspection, safe create/checkout, dirty-worktree rejection, and per-repository mutation locking.
- Commits `f2570a27`, `ea6a0fab`, `d67509e4`, and `4f0d7d94` are completed work. Do not rewrite or revert them.
- The working tree already adds branch commands in `src-tauri/src/commands/git.rs`, command registration in `src-tauri/src/lib.rs`, and adapter methods in `src/lib/tauri-adapter.ts`.
- The latest quality review identified two important remaining defects:
  1. `git:workspaceChanged` has no typed `window.nativesAPI.git.onWorkspaceChanged` subscription.
  2. Recovery after “mutation succeeded but status refresh failed” depends on duplicated exact error-message matching instead of a structural outcome/error type.

## Required Reading

- `AGENTS.md`
- `docs/standards/README.md`
- `docs/standards/technical/01-layering.md`
- `docs/standards/frontend/02-state-and-data.md`
- `docs/superpowers/specs/2026-07-12-assistant-workspace-integration-design.md`
- `docs/superpowers/plans/2026-07-12-assistant-shared-git-branches.md`

## Write Scope

Primary files:

- `src-tauri/src/error.rs`
- `src-tauri/src/git.rs`
- `src-tauri/src/commands/git.rs`
- Git registration lines only in `src-tauri/src/lib.rs`
- Git contract and implementation sections only in `src/lib/tauri-adapter.ts`
- `src/types/file.ts` only if consolidating the duplicate Git status contract
- Focused tests in the files above

Do not edit Assistant components, provider code, daemon code, locale files, or database schema.

## Detailed Work

### 1. Inspect Before Editing

Use CodeGraph to locate `git_create_branch`, `git_checkout_branch`, `run_branch_command`, `GitStatus`, `GitBranchInfo`, and every current consumer of `window.nativesAPI.git`.

Record the current diff for each owned file before touching it. The tree is dirty, so distinguish pre-existing hunks from your own.

### 2. Replace String-Based Mutation Recovery

Introduce a structural representation for the case where Git mutation completed but post-mutation status refresh failed. Acceptable shapes include a dedicated `Error` variant carrying the resulting branch and sanitized detail, or a typed internal mutation outcome consumed by command wrappers.

Requirements:

- Command code must not compare a full human-readable error string.
- A completed mutation must always cause `git:workspaceChanged` emission.
- The caller must still receive truthful information that mutation happened and refresh failed, so it does not retry as if no mutation occurred.
- Unrelated internal failures must not emit a workspace-changed event.
- Do not expose secrets, full environment values, or unbounded stderr.

Add tests for normal success, mutation-applied/refresh-failed, and unrelated failure.

### 3. Add Typed Event Subscription

Extend the existing `git` adapter domain with:

```ts
export interface GitWorkspaceChangedEvent {
  projectPath: string;
  branch: string;
}

onWorkspaceChanged(
  callback: (event: GitWorkspaceChangedEvent) => void,
): () => void;
```

Implementation must use Tauri `listen('git:workspaceChanged', ...)` inside `tauri-adapter.ts`. Consumers must not import `@tauri-apps/api/event` directly for this domain. Cleanup must correctly handle the asynchronous unlisten promise.

### 4. Consolidate Git Types

There are currently incompatible Git status concepts in `src/lib/tauri-adapter.ts` and `src/types/file.ts`. Establish one unambiguous bridge contract without breaking existing file-manager consumers.

At minimum:

- `worktreePath` is required and nullable: `string | null`.
- Git status values use the actual backend union: `modified | added | deleted | renamed | untracked | unknown`.
- `GitStatus` contains `branch`, `entries`, and `dirty` exactly as serialized by Rust.
- If legacy file-manager types represent a different parsed shape, rename them clearly rather than pretending they are identical.

### 5. Verify Registration and IPC Arguments

Confirm the command names and camelCase adapter arguments match Tauri deserialization:

- `git_branches` with `{ dirPath }`
- `git_checkout_branch` with `{ dirPath, branch }`
- `git_create_branch` with `{ dirPath, branch }`

No local HTTP route, Assistant daemon RPC, or second subprocess implementation may be added.

## Required Verification

Run:

```bash
rtk cargo test --manifest-path src-tauri/Cargo.toml commands::git -- --nocapture
rtk cargo test --manifest-path src-tauri/Cargo.toml git_branch_tests -- --test-threads=1
rtk tsc --noEmit
rtk git diff --check
rtk grep "Command::new(\"git\")" src-tauri/src
```

Inspect the final grep manually. Product Git branch operations must remain in `src-tauri/src/git.rs`; unrelated Git probes must be explained.

## Acceptance Criteria

- One Git backend remains authoritative.
- Every completed branch mutation emits exactly one typed workspace event.
- Mutation-applied/refresh-failed is represented structurally, not by duplicated message equality.
- Frontend consumers can subscribe only through `window.nativesAPI.git.onWorkspaceChanged`.
- Git contracts are type-safe and non-duplicative.
- All required tests and type checking pass.

## Forbidden Actions

- Do not implement UI.
- Do not add automatic stash, reset, checkout force, or hidden worktree creation.
- Do not add Git state tables, debug databases, event archives, or localStorage.
- Do not stage unrelated dirty hunks.

## Handoff Report

Report exact files/hunks changed, structural outcome chosen, event contract, test counts, remaining warnings, and whether a safe scoped commit was possible.

