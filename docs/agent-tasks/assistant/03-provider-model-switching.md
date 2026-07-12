# Long Task 03: Provider and Model Switching

## Mission

Complete end-to-end provider configuration, real model discovery, Assistant availability, and per-conversation provider/model switching. Never invent models or expose credentials to the renderer.

## Current State

The tree already contains broad uncommitted implementation in provider adapters, Tauri provider commands, daemon RPC, Settings, `AssistantWorkbench`, `ModelSelectorDropdown`, `provider-model-selection.ts`, and tests. Treat it as authoritative work to audit and finish, not code to replace.

The earlier detailed plan is `docs/superpowers/plans/2026-07-11-provider-model-assistant-availability.md`. Use it as a requirement source, but verify current code instead of assuming its checkboxes are complete.

## Required Reading

- `AGENTS.md`
- `docs/standards/README.md`
- `docs/standards/product/02-feature-spec.md`
- `docs/standards/technical/01-layering.md`
- `docs/standards/technical/02-security.md`
- `docs/standards/technical/03-data.md`
- `docs/standards/frontend/02-state-and-data.md`
- `docs/standards/frontend/03-i18n.md`
- CodePilot reference:
  - `/Users/ldh/Downloads/project/AiNative/References/CodePilot/src/components/chat/ModelSelectorDropdown.tsx`
  - `/Users/ldh/Downloads/project/AiNative/References/CodePilot/src/hooks/useProviderModels.ts`
  - `/Users/ldh/Downloads/project/AiNative/References/CodePilot/docs/guardrails/ModelDiscovery.md`

## Write Scope

- `crates/provider-adapters/**`
- `src-tauri/src/commands/provider.rs`
- Provider RPC portions of `src-tauri/src/daemon/rpc_server.rs`
- Provider sections of `src/lib/tauri-adapter.ts`
- `src/types/provider.ts`
- `src/lib/provider-presets.ts`
- `src/lib/provider-model-selection.ts` and its test
- `src/components/settings/AddProviderDialog.tsx`
- Provider/model sections of `src/components/shell/SettingsPage.tsx`
- Provider/model sections of `src/components/assistant/AssistantWorkbench.tsx`
- `src/components/assistant/ModelSelectorDropdown.tsx`
- Provider/model disabled-state portions of `src/components/assistant/MessageInput.tsx`
- Matching locale keys

Do not edit Git backend/UI or streaming reducer in this task.

## Detailed Work

### 1. Build an Evidence Matrix

Trace this full flow with CodeGraph:

```text
Settings form
  -> provider discovery/test
  -> encrypted credential persistence
  -> provider/model cache persistence
  -> Assistant provider.list
  -> valid default selection
  -> conversation create/update
  -> runtime request uses selected pair
```

For every arrow, identify the function, input/output type, test, and current gap.

### 2. Enforce Real Discovery

- Remove any hard-coded test/default models from the touched provider flow.
- Normalize provider-returned models: trim, drop empty IDs, deduplicate, stable sort.
- Invalidate discovery when provider type, base URL, or active API key changes.
- Test/save must require a model from the latest successful discovery result.
- Custom OpenAI-compatible provider must support editable name/base URL and persist the explicit provider type.
- Loading, error, empty, and success states must be distinct.

### 3. Preserve Credential Security

- API keys remain backend-owned and encrypted according to the existing credential path.
- Renderer contracts may report `hasActiveKey`, never key plaintext.
- Error display must be sanitized and classified.
- No key, authorization header, provider response body, or full debug request is persisted.

### 4. Complete Assistant Provider Readiness

`provider.list` must return configured providers joined with active-key availability, default model, and real cached models. The frontend must distinguish:

- no provider;
- provider without active key;
- provider with no discovered model;
- ready;
- runtime/model incompatible.

Do not collapse engine availability and provider readiness into one generic error.

### 5. Complete Model Picker

- Group models by provider.
- Show active and configured default pairs.
- Display capabilities and runtime incompatibility when known.
- Disable unavailable models with a localized reason.
- Do not copy CodePilot's recent-model localStorage behavior.
- Do not synthesize fallback provider/model IDs.

### 6. Persist Per-Conversation Selection

- Changing a pair updates the active conversation through the existing conversation RPC.
- New conversations use the current valid pair.
- Reopening a conversation restores its stored pair if still valid.
- A stale pair is corrected deterministically to configured default or first real valid model; the correction is persisted.
- Verify the runtime request uses the same provider/model pair shown in the composer.

## Required Verification

```bash
rtk test npm run test -- src/lib/provider-model-selection.test.ts
rtk cargo test --manifest-path src-tauri/Cargo.toml provider -- --nocapture
rtk cargo test -p provider-adapters -- --nocapture
rtk tsc --noEmit
rtk git diff --check
rtk grep "recent-models|last-model|last-provider-id" src
```

Manually test one real configured provider if credentials are available. If not, clearly report that runtime authentication was not verified; do not fabricate success.

## Acceptance Criteria

- Only real discovered/manual persisted models are user-visible.
- Credentials never cross to the renderer.
- Settings and Assistant use the same provider/model records.
- Provider/model switching updates and restores the active conversation.
- Runtime receives the pair shown in UI.
- No recent-model or debug persistence is added.
- Chinese and English keys remain identical.

## Forbidden Actions

- No fake model defaults.
- No plaintext key fields in frontend or logs.
- No new provider database outside existing ownership.
- No localStorage recent-model implementation.

## Handoff Report

Include the evidence matrix, fixed gaps, test counts, real-provider verification status, and any unavailable external credential checks.

