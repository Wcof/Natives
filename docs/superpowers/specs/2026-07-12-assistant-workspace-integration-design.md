# Assistant Workspace Integration Design

**Date:** 2026-07-12
**Status:** Approved

## Goal

Complete the Natives assistant by adapting the useful interaction and workflow patterns from CodePilot while preserving Natives' existing architecture. The result must support real project Git branches, provider and model switching, structured streaming responses, and useful conversation controls without adding a second Git implementation or debug-oriented persistent storage.

## Architectural Boundaries

1. `src-tauri/src/git.rs` remains the single Git implementation used by both the file manager and Assistant. Assistant-specific daemon or database code must not execute a parallel Git workflow.
2. Renderer code accesses Git, filesystem, provider, model, and assistant state only through `window.nativesAPI`.
3. Provider credentials remain encrypted and backend-owned. The frontend receives provider readiness and discovered model metadata, never decrypted keys.
4. User-visible provider and model choices come from real configured providers and discovered/cache-backed model records. No fake providers, placeholder models, or hard-coded availability claims are allowed.
5. Streaming is represented as structured events rather than reparsing one combined text blob wherever the runtime can provide structure.
6. Only product data is persisted: projects, conversations, messages, provider configuration, model discovery cache, and required run metadata. No debug transcript database, duplicate event archive, or localStorage-based recent-model history is introduced.

## Git Workspace Design

The selected project directory is the shared workspace identity. Git status, branch listing, branch creation, and checkout all operate against that directory through the existing Tauri Git module.

The Git API will expose:

- repository status, including current branch and dirty state;
- local and remote branch metadata, including branches occupied by another worktree;
- safe checkout of an existing local branch;
- creation and checkout of a new local branch from the current HEAD.

Checkout is rejected when the current worktree is dirty, the target branch is already checked out in another worktree, the branch name is invalid, or the path is not a Git repository. Natives will not automatically stash, discard changes, or create hidden worktrees. After a successful mutation, a Git workspace event refreshes every consumer, including Assistant and the file manager.

The Assistant composer shows the current branch and opens a compact branch selector. Branch creation uses a themed modal with validation. Errors are classified before display. Loading, empty, and error states remain distinct.

## Provider and Model Design

The Assistant uses the existing provider configuration and Assistant V2 model cache. The picker groups models by provider and only lists models backed by provider discovery or an explicit persisted manual model. It shows the active pair, default pair, model capabilities when known, and incompatibility reasons when a selected runtime cannot serve a model.

Changing a provider/model pair updates the active conversation through the existing conversation RPC. Creating a new conversation uses the currently selected valid pair. A stale selection is corrected to the configured provider default or first valid discovered model, but no synthetic fallback model is invented.

No recent-model localStorage list is copied from CodePilot. Session UI state stays in memory; durable defaults use the existing settings/database path.

## Conversation and Streaming Design

The conversation surface follows a structured timeline similar to ChatGPT and Codex:

- user messages;
- assistant markdown content;
- collapsible reasoning content;
- tool activity with pending, success, failure, and result states;
- file references, citations, code blocks, and diffs;
- inline recoverable errors;
- run completion, cancellation, and retry actions.

The stream reducer owns the current run state. Runtime events update content, reasoning, tool calls, tool results, usage, and terminal status independently. Cancellation preserves all received output and marks the run cancelled. Retrying starts a new run from the same conversation context rather than overwriting the failed message.

Stored messages and live stream output use the same rendering blocks so a completed response does not visually change when it moves from live state to persisted state.

## State and Data Flow

`AssistantWorkbench` remains the page-level coordinator. Focused hooks or pure reducers own provider/model selection, Git workspace state, and stream state. Cross-surface Git updates use a Tauri event rather than polling. Expensive project reads may use session memory caching, but branch mutations always invalidate the cache.

The Assistant remains project-first: a conversation belongs to a project directory, and that directory determines file context, terminal context, and Git context. Git branch is live workspace state and is not copied into a second Assistant-owned database record as an authority.

## Error Handling and Safety

All Git subprocess arguments are passed as separate arguments, never through a shell. Repository paths and branch names are validated. Destructive operations are outside this scope. Dirty-worktree checkout is blocked with actionable UI.

Provider and runtime errors are classified into setup, authentication, rate limit, network, cancellation, tool, and internal categories. Raw secrets and unfiltered stderr are not shown or persisted.

## Testing

- Rust unit/integration tests cover branch parsing, invalid names, dirty checkout rejection, occupied worktree rejection, successful create/checkout, and non-repository errors.
- TypeScript tests cover provider/model selection, stream transitions, cancellation preservation, project grouping, and Git UI state mapping.
- Type checking and focused Rust/TypeScript suites run before broader verification.
- A desktop smoke test verifies that selecting a project, switching branch, switching provider/model, sending a prompt, receiving structured streaming output, stopping a run, and reopening the conversation all use the same underlying state.

## Migration Strategy

Existing in-progress Assistant V2, daemon, provider adapter, project grouping, and structured message-block work is retained. Changes extend those seams rather than replacing them. CodePilot code is treated as interaction reference only; Next API routes, duplicate Git abstractions, localStorage recents, and debug persistence are not transplanted.

## Acceptance Criteria

1. Assistant and file manager report the same branch for the same project and use the same Tauri Git backend.
2. Users can list, create, and safely switch branches from Assistant without hidden repositories or automatic stashing.
3. Users can switch among real configured providers and discovered models, and the selected pair is applied to the active conversation.
4. Live and persisted responses consistently render markdown, reasoning, tools, results, files, citations, errors, and terminal states.
5. Stop and retry flows preserve received content and produce clear status.
6. No new debug-oriented persistent store or recent-model localStorage data is added.
7. New user-visible text is present in both Chinese and English.
8. Focused frontend and backend tests, type checking, and the relevant desktop smoke flow pass.
