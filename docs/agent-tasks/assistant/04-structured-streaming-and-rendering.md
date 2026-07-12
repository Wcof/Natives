# Long Task 04: Structured Streaming and Message Rendering

## Mission

Complete a ChatGPT/Codex-style conversation flow with structured streaming, consistent live/persisted rendering, tool activity, reasoning, stop, cancellation, errors, and retry.

## Current State

The working tree already contains:

- `src/lib/assistant-types.ts`
- `src/lib/assistant-stream-state.ts` and tests
- `ConversationTimeline.tsx`
- structured blocks under `src/components/assistant/blocks/`
- `RunInspector.tsx`
- legacy `src/components/assistant/hooks/useAssistantStream.ts`
- runtime and daemon event infrastructure

Audit which path is authoritative. Do not maintain two independent stream state machines.

## Required Reading

- `AGENTS.md`
- `docs/standards/README.md`
- `docs/standards/frontend/01-structure.md`
- `docs/standards/frontend/02-state-and-data.md`
- `docs/standards/frontend/03-i18n.md`
- `docs/standards/ui-ux/02-interaction.md`
- `docs/standards/ui-ux/03-feedback.md`
- `docs/architecture/ASSISTANT-AGENT-ARCHITECTURE.md`
- CodePilot chat research and components relevant to run status and composer behavior

## Write Scope

- Assistant protocol/runtime event types in `crates/assistant-protocol/**` and `crates/agent-core/**` only as required
- Streaming portions of daemon/Tauri Assistant commands
- `src/lib/assistant-types.ts`
- `src/lib/assistant-stream-state.ts` and tests
- Streaming/run portions of `src/components/assistant/AssistantWorkbench.tsx`
- `src/components/assistant/ConversationTimeline.tsx`
- `src/components/assistant/RunInspector.tsx`
- `src/components/assistant/blocks/**`
- Streaming controls in `src/components/assistant/MessageInput.tsx`
- Remove or reduce legacy `useAssistantStream.ts` only after proving no live consumer needs it
- Matching locale keys

Do not change Git behavior, provider discovery, or project sidebar architecture.

## Detailed Work

### 1. Map Every Runtime Event

Build a table for runtime events and UI state:

- run started;
- content delta;
- reasoning delta;
- tool call started/updated;
- tool result success/failure;
- file/diff/citation reference;
- usage update if real;
- completed;
- cancelled;
- failed;
- self-heal/circuit-break state if supported.

Identify serialization names at protocol, daemon, Tauri adapter, reducer, and renderer. Fix mismatches instead of adding fallback parsing.

### 2. Establish One Reducer

Use one pure reducer for active run state. It must:

- append deltas in order;
- keep reasoning separate from final content;
- update tool items by stable call ID;
- preserve all received output on cancel/failure;
- ignore late events after terminal state;
- reset only when a new run starts;
- avoid parsing `<think>` tags when a structured reasoning event exists;
- handle unknown future event types safely.

Add exhaustive transition tests, including interleaved content/reasoning/tool events and cancellation races.

### 3. Make Live and Stored Rendering Identical

Stored messages and active stream output must feed the same block model and components. A response must not visibly reformat when it moves from live to persisted state.

Verify:

- markdown and code fences;
- copy controls;
- reasoning disclosure;
- tool pending/success/error/result;
- file references;
- citations;
- diffs;
- inline error notice;
- terminal cancelled/failed/completed state.

Never display raw JSON blobs when a structured block is available.

### 4. Stop and Cancellation

- Stop must target the active run/session only.
- UI changes to cancelling immediately and reaches cancelled terminal state.
- Already received content remains visible and is persisted consistently.
- Late deltas after cancellation do not resurrect streaming state.
- Stop button is accessible and localized.

### 5. Retry

- Retry creates a new run using the same conversation context.
- It must not overwrite the failed assistant message or duplicate the user message accidentally.
- Provider/model/project context must match the active conversation unless user changed it explicitly.
- A failed retry remains independently inspectable.

### 6. Remove Duplicate Stream Paths

Locate all imports of legacy `useAssistantStream.ts`. If unused, delete it. If still used by a legacy surface, migrate that surface or clearly isolate it; do not let both reducers listen to the same event and diverge.

### 7. Storage Discipline

Persist only messages and required run metadata. Do not introduce event replay databases, debug transcript files, raw provider response archives, or verbose local logs.

## Required Verification

```bash
rtk test npm run test -- src/lib/assistant-stream-state.test.ts
rtk test npm run test -- src/components/assistant/blocks/blocks.test.tsx src/components/assistant/blocks/markdown-content.test.tsx
rtk cargo test -p assistant-protocol -- --nocapture
rtk cargo test -p agent-core -- --nocapture
rtk tsc --noEmit
rtk git diff --check
```

Run a desktop smoke flow if a provider is available: send, observe content and tool stream, stop midway, retry, reopen conversation, and compare rendering before/after persistence.

## Acceptance Criteria

- Exactly one authoritative stream reducer drives the active Assistant run.
- Structured content, reasoning, tools, results, files, citations, diffs, and errors render consistently live and stored.
- Stop preserves content and reaches a stable cancelled state.
- Retry creates a new run without destructive overwrite.
- No debug/replay persistence is added.
- Tests cover event order and terminal races.

## Handoff Report

Include the event matrix, authoritative reducer path, removed duplicate path, test results, smoke evidence, and any runtime event still unsupported.

