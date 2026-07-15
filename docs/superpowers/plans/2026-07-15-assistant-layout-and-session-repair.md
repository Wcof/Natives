# Assistant Layout and Session Repair Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Assistant a Codex-style project workspace and restore usable conversations.

**Architecture:** Reuse the existing global `Sidebar`, `AssistantSidebarSection`, `AssistantWorkbench`, and Tauri `assistant_service`. The frontend reads conversations from the service's actual array response and Providers from the existing Provider command; no new daemon or data store is introduced.

**Tech Stack:** Next.js/React, Tailwind, Tauri/Rust, existing `tsx --test` suite.

---

### Task 1: Repair the assistant data contract

**Files:**
- Modify: `src/components/assistant/AssistantWorkbench.tsx`
- Test: `src/lib/assistant-project-groups.test.ts`

- [ ] Make the failing assertion describe the service contract: `conversation.list` returns an array, not `{ conversations }`.
- [ ] Change `loadConversations` to accept the returned array and filter archived sessions locally when needed.
- [ ] Change `loadProviders` to use `window.nativesAPI.provider.list()` and map its existing Provider DTO to the local selector type; do not call the missing `provider.list` assistant RPC method.
- [ ] Ensure every RPC response with `{ success: false, error }` rejects through the common `v2call` helper rather than being treated as data.
- [ ] Run `rtk npm run test` and `rtk tsc --noEmit`.

### Task 2: Make Assistant a top-level project launcher

**Files:**
- Modify: `src/components/shell/Sidebar.tsx`
- Modify: `src/components/assistant/AssistantSidebarSection.tsx`
- Modify: `src/components/assistant/AssistantWorkspaceContext.tsx`
- Modify: `src/components/assistant/AssistantWorkbench.tsx`
- Test: `src/components/assistant/AssistantSidebarSection.test.tsx`

- [ ] Replace the global assistant `Plus` action with a project add action; use `actions.addProjectFolder()` only.
- [ ] Add a local collapsed state for the Assistant section; its title row has exactly two icon buttons: expand/collapse and add project.
- [ ] Remove search, global new conversation, global Chat/Agent compatibility overloads, `creatingMode`, and `pickProject` aliases.
- [ ] Make each project row a second-level item with its own expand/collapse control and a `Plus` action calling `createConversation()` after selecting that project.
- [ ] Update tests to inspect exported pure navigation behavior or rendered controls; delete all `assert.ok(true)` tests.

### Task 3: Build the three-column conversation workspace

**Files:**
- Modify: `src/components/assistant/AssistantWorkbench.tsx`
- Modify: `src/components/assistant/RunInspector.tsx` only if a compact right-panel wrapper is needed
- Test: `src/components/assistant/AssistantWorkbench.test.tsx` (create)

- [ ] Keep the existing message timeline and `MessageInput` in the central column.
- [ ] Render the existing project/session navigation in a left workbench panel rather than duplicating global controls.
- [ ] Render the existing `RunInspector` in the right panel with the active conversation's Provider, model, run state, usage, file changes, and artifacts.
- [ ] Add top icon controls for independently collapsing the left and right workbench panels; collapse only changes layout and does not clear selected conversation or run state.
- [ ] Add a test for initial three-column layout and both collapse toggles.

### Task 4: Verify and deliver

**Files:**
- Modify only if needed: `src/i18n/*.json`

- [ ] Add zh/en keys for every new label in the same commit; do not hardcode UI text.
- [ ] Run `rtk tsc --noEmit`, `rtk npm run lint`, `rtk npm run test`, `rtk npm run i18n:check`, `rtk cargo check --workspace`, and `rtk git diff --check`.
- [ ] If a Rust check fails, distinguish a pre-existing workspace/dependency failure from a failure introduced by this branch; fix introduced failures before delivery.
- [ ] Commit source changes with `fix: repair assistant project workspace and sessions`.
