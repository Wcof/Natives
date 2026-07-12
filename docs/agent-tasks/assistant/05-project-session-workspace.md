# Long Task 05: Project-First Sessions and Desktop Readiness

## Mission

Finish the Assistant as a project-directory-first workspace with reliable session grouping, creation, selection, engine readiness, and desktop startup.

## Current State

Existing work includes project grouping helpers/tests, active-project persistence, `AssistantWorkspaceContext`, project RPC, daemon supervisor, project-first sidebar changes, and desktop startup script changes. The detailed earlier plan is `docs/superpowers/plans/2026-07-12-assistant-project-workspace-and-startup.md`.

Audit current evidence; do not assume the plan is implemented merely because files exist.

## Required Reading

- `AGENTS.md`
- `docs/standards/README.md`
- `docs/standards/product/01-positioning.md`
- `docs/standards/product/02-feature-spec.md`
- `docs/standards/technical/01-layering.md`
- `docs/standards/frontend/02-state-and-data.md`
- `docs/standards/frontend/03-i18n.md`
- `docs/architecture/ASSISTANT-AGENT-ARCHITECTURE.md`
- `docs/superpowers/plans/2026-07-12-assistant-project-workspace-and-startup.md`

## Write Scope

- `src/lib/assistant-project-groups.ts` and tests
- `src/lib/active-project.ts` and tests
- `src/components/assistant/AssistantWorkspaceContext.tsx`
- Project/session/readiness portions of `AssistantWorkbench.tsx`
- `ConversationSidebar.tsx`
- `AssistantContextPanel.tsx` and `AssistantSidebarSection.tsx` only if needed
- Conversation/project RPC in daemon
- Daemon supervisor/status command
- `package.json`, `src-tauri/tauri.conf.json`, startup assertion script
- Matching locale keys

Do not change Git domain behavior, provider discovery logic, or stream reducer internals.

## Detailed Work

### 1. Prove Project Identity

Project path is the authoritative workspace identity used by conversations, files, terminal context, and Git context.

- Normalize only enough to reject empty/invalid persisted values; do not silently remap distinct real paths.
- Restore active project at startup.
- Persist project selection through the existing controlled settings/DB adapter.
- Folder selection must use the native directory chooser.
- Do not use fake recent projects.

### 2. Complete Project-First Sidebar

- Group conversations by exact project path.
- Show unassigned legacy conversations honestly without creating a fake directory.
- Sort projects and conversations by real update timestamps.
- Support collapse/expand without additional durable storage unless an existing UI preference mechanism already owns it.
- New conversation actions always target the currently selected real project.
- Loading, empty, error, and success states must be distinct.

### 3. Complete Conversation Lifecycle

Verify create, list, select, title update, archive/delete if supported, and reopen flows.

- Conversation create response must include project, provider, model, mode, and timestamps needed by the UI.
- List must not silently truncate project groups at 50 rows unless explicit pagination is implemented end to end.
- Selecting a conversation restores project/provider/model/mode consistently.
- Do not display fabricated token counts, summaries, or message counts.

### 4. Separate Readiness Dimensions

Render independent states for:

- renderer-only/no bridge;
- engine connecting;
- engine unavailable;
- no project;
- provider setup needed;
- model setup needed;
- ready.

Retry for engine failure must actually re-check/start the engine, not merely refresh conversations. Provider/model setup failures must not masquerade as engine failures.

### 5. Make Desktop Startup Reliable

Verify scripts avoid recursion and build/start the daemon sidecar before Tauri development startup.

Required intended shape must remain equivalent to:

```json
{
  "web:dev": "NODE_ENV=development next dev -H 127.0.0.1 -p 3000",
  "daemon:build": "cargo build -p natives-agent-daemon",
  "dev": "npm run daemon:build && tauri dev",
  "tauri:dev": "npm run dev"
}
```

Tauri `beforeDevCommand` must invoke renderer-only startup, not recurse into `dev`.

### 6. Preserve Desktop Layout

Keep fixed sidebar, fixed top navbar/custom window controls, and scrollable main workspace. Preserve drag/no-drag regions and avoid heavy dynamic backdrop blur.

## Required Verification

```bash
rtk test npm run test -- src/lib/assistant-project-groups.test.ts src/lib/active-project.test.ts
rtk node scripts/assert-assistant-dev-contract.mjs
rtk cargo test --manifest-path src-tauri/Cargo.toml conversation_project -- --nocapture
rtk cargo test --manifest-path src-tauri/Cargo.toml daemon -- --nocapture
rtk npm run daemon:build
rtk tsc --noEmit
rtk git diff --check
```

Desktop smoke flow:

1. Start with `rtk npm run dev`.
2. Select a directory.
3. Create chat and agent conversations.
4. Restart the app and verify project/conversation restoration.
5. Verify engine failure and retry UI using a real unavailable-sidecar condition if safely reproducible.

## Acceptance Criteria

- Every new conversation belongs to a real selected project.
- Sidebar grouping is complete and not silently truncated.
- Selection restores all conversation context.
- Readiness states are distinct and actionable.
- Desktop dev startup launches renderer, Tauri, and daemon without recursion.
- No fake project/session data or debug persistence is introduced.

## Handoff Report

Report project identity rules, conversation RPC evidence, readiness matrix, startup commands, tests, and smoke results.

