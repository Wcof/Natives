# Long Task 02: Assistant Git Branch UI

## Mission

Add a polished branch selector to the Assistant composer so users can inspect, create, and switch the real branch of the selected project. The file manager and Assistant must immediately agree because both consume Task 1's shared Git adapter.

## Dependency

Task 1 must be complete and verified first. Confirm `window.nativesAPI.git` exposes status, branches, create, checkout, and `onWorkspaceChanged`.

## Required Reading

- `AGENTS.md`
- `docs/standards/README.md`
- `docs/standards/frontend/01-structure.md`
- `docs/standards/frontend/02-state-and-data.md`
- `docs/standards/frontend/03-i18n.md`
- `docs/standards/ui-ux/01-design-tokens.md`
- `docs/standards/ui-ux/02-interaction.md`
- `docs/standards/ui-ux/03-feedback.md`
- CodePilot reference:
  - `/Users/ldh/Downloads/project/AiNative/References/CodePilot/src/components/git/GitBranchSelector.tsx`
  - `/Users/ldh/Downloads/project/AiNative/References/CodePilot/src/hooks/useGitBranches.ts`

## Write Scope

- Create `src/lib/git-branch-state.ts`
- Create `src/lib/git-branch-state.test.ts`
- Create `src/components/assistant/GitBranchSelector.tsx`
- Modify Git/project strip only in `src/components/assistant/MessageInput.tsx`
- Modify `src/components/assistant/AssistantWorkspaceContext.tsx` only if needed to consume existing project path; do not add branch authority there
- Add matching keys to `src/i18n/zh.ts` and `src/i18n/en.ts`
- Focused component tests if the repository's current test setup supports them

Do not edit Rust Git code, provider/model code, Assistant daemon, or database schema.

## Detailed Work

### 1. Build a Pure Branch State Contract

Create helpers for frontend validation and row action state. Rust remains authoritative, but the UI should reject obviously invalid names immediately.

Required exports:

```ts
export type BranchActionState = 'ready' | 'current' | 'dirty' | 'occupied';
export function validateGitBranchName(name: string): null | 'required' | 'invalid';
export function branchActionState(input: {
  dirty: boolean;
  current: boolean;
  occupied: boolean;
}): BranchActionState;
```

Tests must cover valid slash names, whitespace, leading dash, `..`, `.lock`, current branch, dirty worktree, and linked-worktree occupancy.

### 2. Build `GitBranchSelector`

The selector receives `projectPath` and `locale`. It owns only local UI state: open/closed, loading, error, branch list, status, create-modal state, and in-flight action.

Behavior:

- Load status and branches when opened.
- Render distinct loading, error, empty/non-repository, and success states.
- Display local branches first. Remote branches may be shown as informational rows but must not be passed to local checkout unless backend explicitly supports it.
- Mark the current branch.
- Disable a branch that is current, blocked by dirty state, or occupied by another linked worktree.
- Show localized inline explanations for disabled states.
- Subscribe through `window.nativesAPI.git.onWorkspaceChanged`; filter exact `projectPath`; refresh status/branches; clean up on unmount.
- Do not poll.

### 3. Create Branch Flow

Use the existing themed `Modal`, not browser `prompt/confirm/alert`.

- Validate as the user types.
- Disable submit while invalid or in flight.
- Call `window.nativesAPI.git.createBranch(projectPath, name)`.
- On success, close modal and let the shared event drive refresh.
- On classified error, keep input and display actionable inline feedback.
- Never auto-stash, discard, force switch, or create a hidden worktree.

### 4. Checkout Flow

- Call `checkoutBranch` only for an enabled local row.
- Preserve the open menu and show progress while switching.
- On normal success, update through the shared event.
- If backend reports mutation applied but status refresh failed, explain that the branch changed and force a status reload; do not offer a blind retry button.
- Other failures go through `classifyError` before display.

### 5. Integrate Into Composer

Replace the passive `workspace.inspect` branch fetch in `MessageInput.tsx` with the new selector. Keep project identity sourced from `navigation.activeProjectPath`.

Remove touched hard-coded visible text such as `Cloud` and route it through i18n. Preserve fixed desktop workspace hierarchy and avoid expensive blur on changing content.

### 6. Synchronize i18n

Add matching `assistant.git.*` keys in both locales for branch, loading, empty, non-repository, create, invalid, current, dirty, occupied, checkout, branch-changed-refresh-failed, and retry-refresh states.

## Required Verification

```bash
rtk test npm run test -- src/lib/git-branch-state.test.ts
rtk test npm run test -- src/lib/assistant-stream-state.test.ts src/lib/provider-model-selection.test.ts
rtk tsc --noEmit
rtk git diff --check
```

Perform a desktop/manual smoke check if the app can launch:

1. Select a real Git project.
2. Open branch selector and compare with `rtk git branch --show-current` run in that project.
3. Create and switch to a temporary branch.
4. Confirm file manager and Assistant show the same branch.
5. Modify a tracked file and confirm switching is blocked without altering the file.

## Acceptance Criteria

- Branch selector uses only the existing typed Git adapter.
- Assistant and file manager agree after every branch mutation.
- Dirty and occupied safety states are visible and enforced.
- Create and checkout have proper loading/error/success behavior.
- All touched visible strings are bilingual.
- No additional Git or persistent storage implementation exists.

## Handoff Report

Report UI files, state contract, event cleanup, smoke evidence, tests, and any pre-existing failures.

