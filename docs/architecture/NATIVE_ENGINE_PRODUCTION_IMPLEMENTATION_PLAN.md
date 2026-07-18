# Native Engine Production Implementation Plan

Last updated: 2026-07-18
Reference project: `/Users/ldh/Downloads/project/grok-build`
Target project: `/Users/ldh/Downloads/project/AiNative/Natives`

## Goal

Make Native Engine a real production agent runtime:

- GUI talks only to Protocol v2 through Tauri.
- Tauri talks only to the UDS Agent Daemon for production runs.
- Daemon owns run state, history, events, permissions, tools, hooks, MCP, providers, and Subagents.
- Providers are selected with an explicit protocol, not guessed from URL.
- Subagents can use independent provider, key, model, base URL, tools, and permission profile.
- Completion requires offline tests, live provider tests, and GUI headed evidence.

## Non-Negotiables

1. No production embedded fallback.
2. No GUI fake provider fallback unless the request explicitly sets a fixture mode.
3. No provider key in logs, docs, command args, test snapshots, or event payloads.
4. No capability is advertised unless the RPC method is implemented and tested.
5. No "full complete" label unless live verification and GUI headed verification are actually run.

## Phase 1: Single Production Chain

Owner files:

- `src-tauri/src/daemon_authority.rs`
- `src-tauri/src/assistant_service.rs`
- `src-tauri/src/sidecar_supervisor.rs`
- `src-agent-daemon/src/client.rs`
- `src-agent-daemon/src/rpc.rs`
- `src-agent-daemon/src/run_manager.rs`
- `src-agent-daemon/src/production.rs`

Tasks:

- Make `NATIVES_DAEMON_MODE=uds` the production default.
- On GUI startup, supervisor creates or loads:
  - `NATIVES_DAEMON_SOCKET`
  - `NATIVES_DAEMON_BOOTSTRAP`
  - `NATIVES_DB_PATH`
- Health check must verify socket connect, bootstrap auth, `daemon.ping`, protocol version, and DB path.
- On restart, rotate bootstrap and rebuild cached authority/client state.
- Delete or CI-ban every production caller that bypasses `ExecutionAuthority`.
- `run.start` must return immediately and execute in the daemon background.
- `run.cancel`, `run.retry`, `permission.respond`, `run.replay`, and `run.subscribe` must use the same authority path.

Acceptance:

- Starting GUI without daemon env starts sidecar and does not throw `NATIVES_DAEMON_BOOTSTRAP required`.
- Killing daemon shows reconnecting state, then recovers or shows a clear daemon failure.
- `scripts/daemon/audit-old-symbols.sh` passes.
- `cargo test -p natives-agent-daemon --test uds_run_lifecycle` passes.

## Phase 2: Daemon-Owned State And History

Owner files:

- `src-agent-daemon/src/storage/`
- `src-agent-daemon/src/event_log.rs`
- `src-agent-daemon/src/run_manager.rs`
- `crates/agent-core/src/engine.rs`
- `src-tauri/src/assistant_service.rs`
- `src-tauri/src/daemon/data.rs`

Tasks:

- Make daemon `DataStore` the authoritative store for conversations, messages, runs, events, tool calls, permissions, and artifacts.
- Keep Tauri conversation storage only as a compatibility adapter or migrate it away.
- `run.start` must atomically:
  - append the user message,
  - create the run,
  - snapshot provider/key/model/project/permission,
  - return the run id.
- Engine must load full conversation history, not only current `user_content`.
- Persist assistant text, thinking, tool calls, tool results, usage, errors, and final status.
- On daemon startup, mark active runs as `interrupted` and make replay deterministic.

Acceptance:

- Multi-turn test proves turn 2 can answer using turn 1 content.
- Restart replay returns the same persisted event order.
- Persistence failure fails the run before broadcast.
- No duplicate user message on retry/idempotent re-submit.

## Phase 3: True Streaming, Cancel, Retry

Owner files:

- `crates/provider-adapters/src/`
- `src-agent-daemon/src/production.rs`
- `crates/agent-core/src/engine.rs`
- `crates/agent-core/src/event_seq.rs`

Tasks:

- Provider adapter returns an async stream consumed incrementally by Engine.
- Cancel token is checked before request, during stream, between tool calls, and before final persistence.
- Remove any path that collects a full provider stream into memory before emitting deltas.
- Add retry policy:
  - retry network errors, timeout, 408, 429, 5xx, and empty pre-delta response;
  - max 3 attempts;
  - backoff 500ms, 1s, 2s;
  - respect `Retry-After`, capped at 30s;
  - never retry auth errors, schema errors, permission denials, or after a committed tool side effect.
- Emit attempt events:
  - `GenerationAttemptStarted`
  - `GenerationAttemptDiscarded`
  - `GenerationAttemptCommitted`

Acceptance:

- First text delta arrives before full provider completion.
- Cancel stops a long run without later text/tool events.
- Retry test covers one transient 500 then success.
- Empty response becomes a structured provider error, not `Model response missing content`.

## Phase 4: Explicit Provider Protocols

Owner files:

- `crates/provider-adapters/src/providers/`
- `src-tauri/src/commands/provider.rs`
- `src/components/settings/AddProviderDialog.tsx`
- `src/lib/assistant-types.ts`

Provider protocol enum:

- `openai_chat_completions`
- `openai_responses`
- `anthropic_messages`
- `gemini_generate_content`
- `ollama_chat`

Tasks:

- Provider creation UI must require protocol selection.
- Store protocol with provider config.
- Adapter dispatch uses protocol, not URL guessing.
- Provider test sends a real minimal request through the selected protocol.
- Error classification must say exactly what failed:
  - auth rejected,
  - model not found,
  - protocol mismatch,
  - empty provider response,
  - unsupported tool calling,
  - malformed stream.

Acceptance:

- Same base URL can be tested with different protocols and produce different wire payloads.
- OpenAI-compatible SenseNova validates `deepseek-v4-flash` through `openai_chat_completions`.
- Anthropic-native validates through `anthropic_messages`.
- Protocol mismatch shows a useful message in GUI.

## Phase 5: Permission, Tools, Hooks, MCP

Owner files:

- `crates/capability-gateway/src/`
- `crates/agent-core/src/permissions.rs`
- `crates/agent-core/src/hooks.rs`
- `crates/agent-core/src/hook_handlers.rs`
- `src-agent-daemon/src/mcp_runtime.rs`
- `src-agent-daemon/src/production.rs`

Tasks:

- Centralize permission decision in the gateway.
- `readonly` denies side effects.
- `ask` prompts for side effects.
- `full_access` allows normal side effects without Ask, while keeping path scope and dangerous-operation blocks.
- All tools go through the same path:
  - schema validation,
  - path scope,
  - permission,
  - pre-hook,
  - execute,
  - post-hook,
  - persist event.
- Hook events must cover session, prompt, pre/post tool, permission, subagent, stop, compact, notification, and error.
- MCP `mcp.list` and `mcp.call` must work if advertised.
- MCP browser OAuth remains unsupported unless implemented with browser redirect contract and tests.

Acceptance:

- `full_access` no longer asks for ordinary write/terminal tools inside project scope.
- `readonly` blocks write/terminal/task side effects.
- `mcp.list` never returns `Unknown RPC method` if capabilities advertise MCP.
- OAuth methods are either fully tested or clearly `unsupported`.

## Phase 6: Subagent Productization

Owner files:

- `crates/agent-core/src/subagents.rs`
- `src-agent-daemon/src/production.rs`
- `src-agent-daemon/src/run_manager.rs`
- `src/components/assistant/ActivityInspector.tsx`
- `src/components/assistant/ConversationTimeline.tsx`

Tasks:

- Child run is a real run with its own provider, key, model, base URL, tools, hooks, context, permission profile, and event stream.
- Parent cannot silently pass its key to child.
- Child cannot escalate permission above explicit config.
- Parent cancel cascades to child.
- Child output returns through a normal `task` tool result.
- GUI shows parent/child run tree, child provider/model, status, and errors.

Acceptance:

- Parent OpenAI-compatible plus child Anthropic-native live test passes.
- Parent full_access plus child ask still asks inside child where required.
- Cancelling parent cancels child.
- GUI replay shows subagent events after restart.

## Phase 7: GUI Integration

Owner files:

- `src/components/assistant/AssistantWorkbench.tsx`
- `src/components/assistant/ConversationTimeline.tsx`
- `src/components/assistant/RunStatusBar.tsx`
- `src/components/assistant/ConnectionBanner.tsx`
- `src/lib/assistant-gateway/`
- `src/lib/assistant-protocol/`

Tasks:

- Remove silent fixture fallback from production flows.
- Fix React update loops with stable selectors, stable effect deps, and subscription cleanup.
- Require explicit project path before run.
- Surface daemon status, provider status, permission prompts, retries, cancel state, and subagent tree.
- Retry creates a new run from the selected failed run with preserved context.
- Cancel sends `run.cancel` and stops local UI optimistic streaming.
- GUI uses protocol capabilities to hide unsupported actions.

Acceptance:

- Manual headed flow:
  - choose project path,
  - send text,
  - tool Ask approve,
  - tool Ask deny,
  - cancel,
  - retry,
  - provider protocol mismatch,
  - daemon restart,
  - subagent run,
  - replay after app restart.
- No `Maximum update depth exceeded`.
- No fake assistant response appears on provider/daemon failure.

## Phase 8: Verification And Evidence

Owner files:

- `scripts/daemon/verify-native-engine.sh`
- `src-agent-daemon/tests/live_engine_e2e.rs`
- `crates/provider-adapters/tests/`
- `docs/architecture/NATIVE_ENGINE_FULL_REMEDIATION.md`
- `docs/architecture/NATIVE_ENGINE_GAP_CHECKLIST.md`

Tasks:

- Expand verification script so `--live` runs:
  - adapter text,
  - adapter tool call,
  - engine text,
  - engine tool loop,
  - subagent cross-provider,
  - cancel,
  - retry.
- Write sanitized evidence to `NATIVES_TEST_SCRATCH`.
- `not_run` is allowed only as evidence, never as pass.
- Update docs only after tests actually run.

Offline gate:

```bash
scripts/daemon/verify-native-engine.sh
```

Live gate:

```bash
export NATIVES_LIVE_E2E=1
export NATIVES_TEST_SCRATCH=/tmp/natives-live-e2e

export NATIVES_TEST_OPENAI_BASE=...
export NATIVES_TEST_OPENAI_KEY=...
export NATIVES_TEST_MODEL=...
export NATIVES_TEST_OPENAI_PROTOCOL=openai_chat_completions

export NATIVES_TEST_ANTHROPIC_BASE=...
export NATIVES_TEST_ANTHROPIC_KEY=...
export NATIVES_TEST_ANTHROPIC_MODEL=...
export NATIVES_TEST_ANTHROPIC_PROTOCOL=anthropic_messages

scripts/daemon/verify-native-engine.sh --live
```

Do not pass keys as command arguments.

## Done Definition

The Native Engine is complete only when all are true:

- Offline gate passes from a clean checkout plus current work.
- Live OpenAI-compatible provider passes text, tool, cancel, retry, and usage checks.
- Live Anthropic-native provider passes text, tool, cancel, retry, and usage checks.
- Cross-provider Subagent live test passes.
- GUI headed evidence exists for project path, Ask approve/deny, cancel, retry, daemon restart, protocol mismatch, and subagent tree.
- Capabilities exactly match implemented RPC methods.
- Old execution chain has no production imports.
- Docs record exact test date, provider protocol, model names, sanitized evidence path, and failures if any.

