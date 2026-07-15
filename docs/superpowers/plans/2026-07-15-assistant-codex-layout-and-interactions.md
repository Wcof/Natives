# Assistant Codex-Style Layout and Interactions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build one Codex-style assistant workspace with a single project tree, a functional composer, and recoverable structured conversation/run interactions.

**Architecture:** Keep `AssistantWorkspaceContext` as the bridge between the shell sidebar and workbench. Extend the existing assistant RPC tables instead of adding a parallel store; the composer submits text, attachments, permission profile, provider, and model into the existing run pipeline. Render persisted and live blocks through one timeline model.

**Tech Stack:** React 19, TypeScript, Tailwind CSS, Tauri 2, Rust, rusqlite, Node test runner.

---

### Task 1: Shell sidebar hierarchy and ownership

**Files:**
- Modify: `src/components/shell/Sidebar.tsx`
- Modify: `src/components/shell/Header.tsx`
- Modify: `src/components/assistant/AssistantSidebarSection.tsx`
- Modify: `src/components/assistant/AssistantWorkbench.tsx`
- Test: `src/components/shell/Sidebar.test.tsx`

- [ ] **Step 1: Replace the placeholder sidebar assertions with source-contract tests**

Read the four source files as text and assert that `AssistantSidebarSection` occurs once in the rendered application path, the workbench no longer imports it, the assistant heading uses the same label classes as Quick Access, and `Sidebar` calls `onToggle` from its own top area.

```ts
test('assistant tree is owned only by the shell sidebar', () => {
  assert.equal(count(workbench, 'AssistantSidebarSection'), 0);
  assert.equal(count(sidebar, '<AssistantSidebarSection'), 1);
});
```

- [ ] **Step 2: Run the sidebar test and verify it fails**

Run: `npm test -- src/components/shell/Sidebar.test.tsx`
Expected: FAIL because the workbench still renders `AssistantSidebarSection` and the sidebar heading is a normal navigation row.

- [ ] **Step 3: Implement the single-sidebar layout**

Render “助理” with the same small gray heading contract as Quick Access. Keep only the directory chevron and `FolderPlus`. Render projects below it. Put `PanelLeftClose` in the top of `Sidebar`; in `Header`, render `PanelLeftOpen` only when `sidebarCollapsed` is true. Remove `leftPanelOpen`, the internal `<aside>`, and its imports from `AssistantWorkbench`.

- [ ] **Step 4: Run tests and typecheck**

Run: `npm test -- src/components/shell/Sidebar.test.tsx && npm run typecheck`
Expected: PASS and no TypeScript errors.

- [ ] **Step 5: Commit**

```bash
git add src/components/shell/Sidebar.tsx src/components/shell/Header.tsx src/components/assistant/AssistantSidebarSection.tsx src/components/assistant/AssistantWorkbench.tsx src/components/shell/Sidebar.test.tsx
git commit -m "fix: make assistant tree part of main sidebar"
```

### Task 2: Persist permission profiles and attachment blocks

**Files:**
- Modify: `src-tauri/src/assistant_service.rs`
- Test: `src-tauri/src/assistant_service.rs`

- [ ] **Step 1: Add failing Rust round-trip tests**

Create a conversation with `permission_profile_id: "ask"`, call `conversation.update_permission` with `readonly`, start a run with one file attachment, then assert:

```rust
assert_eq!(conversation["permission_profile_id"], "readonly");
assert_eq!(run["permission_profile"], "readonly");
assert_eq!(messages[0]["content_blocks"][1]["type"], "file_reference");
```

- [ ] **Step 2: Run the tests and verify they fail**

Run: `cargo test -p natives assistant_service::tests::conversation_permission_and_attachments_round_trip --lib -- --nocapture`
Expected: FAIL because the RPC method and attachment persistence do not exist.

- [ ] **Step 3: Implement the minimal RPC changes**

Add `conversation.update_permission` dispatch. Include `permission_profile_id` in conversation list/create responses. Validate profiles against `readonly | ask | full_access`. In `run.start`, read the persisted profile and write it to `assistant_runs.permission_profile`. Accept `attachments` shaped as `{path,name,mime_type,size}` and insert `file_reference` blocks in the same transaction after the text block. Keep the path as a reference; do not copy file data into SQLite.

- [ ] **Step 4: Run Rust tests**

Run: `cargo test -p natives assistant_service::tests::conversation_permission_and_attachments_round_trip --lib -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/assistant_service.rs
git commit -m "feat: persist assistant permissions and attachments"
```

### Task 3: Composer state and file picker

**Files:**
- Create: `src/lib/assistant-composer.ts`
- Create: `src/lib/assistant-composer.test.ts`
- Modify: `src/lib/tauri-adapter.ts`
- Modify: `src/components/assistant/MessageInput.tsx`
- Reuse: `src/components/assistant/ModelSelectorDropdown.tsx`

- [ ] **Step 1: Write failing composer helper tests**

```ts
assert.equal(canSendAssistantMessage('   ', []), false);
assert.equal(canSendAssistantMessage('', [{ path: '/tmp/a.txt', name: 'a.txt', size: 1 }]), true);
assert.deepEqual(normalizePermissionProfile('bad'), 'ask');
```

- [ ] **Step 2: Run the test and verify it fails**

Run: `npm test -- src/lib/assistant-composer.test.ts`
Expected: FAIL because `assistant-composer.ts` does not exist.

- [ ] **Step 3: Implement helpers and the native file picker**

Add the `AssistantAttachment` and `AssistantPermissionProfile` types, `canSendAssistantMessage`, and profile normalization. Extend `window.nativesAPI.dialog` with `pickFiles()` using `@tauri-apps/plugin-dialog` `open({ directory:false, multiple:true })` and file metadata from the installed Tauri fs API where available; reject missing files before send.

- [ ] **Step 4: Rebuild `MessageInput` as the Codex-style composer**

Use a centered max-width container, rounded border, approximately 100px minimum textarea height, upward auto-growth, attachment chips, plus menu, permission selector, reused provider/model dropdown, and circular send/stop button. Props must include current profile, providers, provider/model selection callbacks, attachment-aware send, and model update callback. Preserve draft and attachments when `onSend` rejects.

- [ ] **Step 5: Run tests and typecheck**

Run: `npm test -- src/lib/assistant-composer.test.ts && npm run typecheck`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/lib/assistant-composer.ts src/lib/assistant-composer.test.ts src/lib/tauri-adapter.ts src/components/assistant/MessageInput.tsx
git commit -m "feat: add functional assistant composer"
```

### Task 4: Connect composer controls to the current conversation and run

**Files:**
- Modify: `src/components/assistant/AssistantWorkbench.tsx`
- Modify: `src/components/assistant/AssistantWorkspaceContext.tsx`

- [ ] **Step 1: Extend conversation and send state**

Add `permission_profile_id` to the frontend `Conversation`. Add `handleSelectPermission` that calls `conversation.update_permission` and only commits local state after success. Change `handleSend` to receive `{content, attachments}` and pass attachments plus the active permission profile to `run.start`.

- [ ] **Step 2: Make the stream prompt attachment-aware without leaking file content**

Build a short reference suffix such as `Attached files:\n- /path/name` for the runtime prompt while the authoritative attachment metadata remains in `assistant_message_blocks`. Pass the selected provider/model from the active conversation, not a separate composer-only state.

- [ ] **Step 3: Remove the duplicate header model selector**

Keep the conversation title in the header. Move provider/model selection exclusively into `MessageInput`. Pass provider data, selected IDs, profile, permission callback, send callback, stop callback, and disabled reason.

- [ ] **Step 4: Verify TypeScript**

Run: `npm run typecheck`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/components/assistant/AssistantWorkbench.tsx src/components/assistant/AssistantWorkspaceContext.tsx
git commit -m "feat: connect assistant composer to runs"
```

### Task 5: Make stream events produce recoverable reasoning, tool and usage state

**Files:**
- Modify: `src/lib/assistant-stream-state.ts`
- Create: `src/lib/assistant-stream-state.test.ts`
- Modify: `src/components/assistant/hooks/useAssistantStream.ts`
- Modify: `src/components/assistant/AssistantWorkbench.tsx`

- [ ] **Step 1: Write reducer tests for every supported event**

Cover assistant delta append, reasoning delta append with first/last timestamp, tool started/completed/failed pairing, permission request, usage update, completed, failed, interrupted, and foreign run ID ignore.

```ts
state = reduceAssistantStreamEvent(state, event('reasoning_delta', { text: 'plan' }, '2026-07-15T00:00:01Z'));
assert.equal(state.blocks[0].type, 'reasoning');
assert.equal(state.reasoningStartedAt, '2026-07-15T00:00:01Z');
```

- [ ] **Step 2: Run the reducer tests and verify they fail**

Run: `npm test -- src/lib/assistant-stream-state.test.ts`
Expected: FAIL because the current reducer only handles terminal events.

- [ ] **Step 3: Implement a typed reducer**

Replace `any` with `AssistantRunEvent` and typed block/state interfaces. Append deltas to stable blocks, keep tool calls keyed by tool call ID, expose permission requests and usage, and compute reasoning duration from timestamps. Do not create a second stream state store.

- [ ] **Step 4: Normalize legacy Tauri stream events through the same model**

Keep `useAssistantStream` as the bridge for the current channel, but record `startedAt`, reasoning start/end, tool status, and terminal error. In the workbench, construct one live timeline message from that state and persist both reasoning and text blocks through an extended `conversation.appendMessage` blocks payload.

- [ ] **Step 5: Run tests and typecheck**

Run: `npm test -- src/lib/assistant-stream-state.test.ts && npm run typecheck`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/lib/assistant-stream-state.ts src/lib/assistant-stream-state.test.ts src/components/assistant/hooks/useAssistantStream.ts src/components/assistant/AssistantWorkbench.tsx
git commit -m "fix: preserve assistant streaming state"
```

### Task 6: Render the CodePilot-inspired conversation timeline

**Files:**
- Modify: `src/components/assistant/ConversationTimeline.tsx`
- Modify: `src/components/assistant/blocks/index.tsx`
- Create: `src/lib/assistant-message-view.ts`
- Create: `src/lib/assistant-message-view.test.ts`

- [ ] **Step 1: Write view-model tests**

Test that user messages align right, assistant messages align left, reasoning duration labels format seconds/minutes, failed/interrupted messages are retryable, and tool call/result blocks pair by ID.

- [ ] **Step 2: Run tests and verify they fail**

Run: `npm test -- src/lib/assistant-message-view.test.ts`
Expected: FAIL because the view-model module does not exist.

- [ ] **Step 3: Implement message view helpers**

Add pure helpers for alignment, duration formatting, retry eligibility, block grouping, and token label formatting. Keep DOM behavior in the component.

- [ ] **Step 4: Rebuild the timeline presentation**

Remove avatars. Render right-aligned muted user bubbles and left-aligned full-width assistant content. Add expandable reasoning with live/finished label, collapsible tool rows, inline permission cards, error blocks, attachment cards, copy/time/token hover actions, retry, long user-message expansion, and a bottom-follow button using the scroll container's distance from bottom.

- [ ] **Step 5: Run tests and typecheck**

Run: `npm test -- src/lib/assistant-message-view.test.ts && npm run typecheck`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/components/assistant/ConversationTimeline.tsx src/components/assistant/blocks/index.tsx src/lib/assistant-message-view.ts src/lib/assistant-message-view.test.ts
git commit -m "feat: complete assistant conversation interactions"
```

### Task 7: Full verification and integration

**Files:**
- Modify only files required by failures attributable to this branch.

- [ ] **Step 1: Run focused frontend tests**

Run: `npm test -- src/components/shell/Sidebar.test.tsx src/lib/assistant-composer.test.ts src/lib/assistant-stream-state.test.ts src/lib/assistant-message-view.test.ts`
Expected: all focused tests PASS.

- [ ] **Step 2: Run the full frontend suite and static checks**

Run: `npm test && npm run typecheck && npm run lint && npm run build`
Expected: tests pass, TypeScript has zero errors, lint has zero errors, production build exits 0.

- [ ] **Step 3: Run Rust verification**

Run: `cargo test -p natives assistant_service::tests --lib -- --nocapture && cargo check --workspace`
Expected: assistant service tests pass and workspace check exits 0.

- [ ] **Step 4: Inspect the final diff**

Run: `git diff --check && git status --short && git log --oneline --decorate -10`
Expected: no whitespace errors; only intended files are modified; implementation commits are present.

- [ ] **Step 5: Merge into the original branch and reverify**

Preserve the original branch's dirty files, merge `codex/assistant-codex-layout`, restore the dirty state, then rerun `npm run typecheck` and `cargo check --workspace`. If an unrelated pre-existing failure remains, report it without modifying the user's parallel homepage work.
