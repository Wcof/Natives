# Assistant Completion Agent Task Pack

This directory splits the Assistant completion objective into six long-running tasks that can be handed to different agents one at a time.

## Critical Scheduling Rule

Run these tasks **sequentially in the numbered order** against the same working tree. Do not run implementation agents in parallel. The repository currently contains a large amount of authoritative, uncommitted Assistant work, and several tasks intentionally touch shared integration files such as `AssistantWorkbench.tsx`, `tauri-adapter.ts`, and the locale files.

Each agent must:

1. Read `AGENTS.md`, `docs/standards/README.md`, and every standards document named by its task.
2. Use CodeGraph first because `.codegraph/` exists.
3. Prefix every shell command with `rtk`.
4. Use `apply_patch` for manual edits.
5. Preserve all unrelated working-tree changes.
6. Never create a second Git backend, hidden repository, automatic stash, or debug persistence store.
7. Run its required verification and leave a precise handoff report.

## Execution Order

| Order | Task | Primary Outcome | Depends On |
|---|---|---|---|
| 1 | [01-shared-git-ipc-hardening.md](01-shared-git-ipc-hardening.md) | Finish the single shared Git backend and typed IPC/event contract | Existing commits through `4f0d7d94` |
| 2 | [02-assistant-git-branch-ui.md](02-assistant-git-branch-ui.md) | Add safe branch list/create/switch UI to Assistant | Task 1 |
| 3 | [03-provider-model-switching.md](03-provider-model-switching.md) | Complete real provider discovery and per-conversation model switching | Task 2 current tree |
| 4 | [04-structured-streaming-and-rendering.md](04-structured-streaming-and-rendering.md) | Complete ChatGPT/Codex-style structured streaming, stop, retry, and rendering | Task 3 |
| 5 | [05-project-session-workspace.md](05-project-session-workspace.md) | Complete project-first sessions, sidebar, engine readiness, and desktop startup | Task 4 |
| 6 | [06-integration-completion-audit.md](06-integration-completion-audit.md) | Requirement-by-requirement audit, fixes, full verification, and smoke evidence | Tasks 1-5 |

## Current Evidence

- Branch inspection and safe mutation are already committed in `f2570a27`, `ea6a0fab`, `d67509e4`, and `4f0d7d94`.
- The current working tree contains uncommitted Git command/adapter integration plus broad provider, daemon, Assistant UI, stream renderer, project grouping, and startup work.
- Focused evidence observed before this task pack was written:
  - Rust branch tests: 16 passed.
  - Rust Git command tests: 7 passed.
  - `rtk tsc --noEmit`: passed.
  - Assistant provider/model, stream-state, and project-group frontend tests passed at baseline.
- Passing focused tests are not proof that the full objective is complete. Task 6 must perform the final requirement-by-requirement audit.

## Commit Guidance

Several target files already contain unrelated uncommitted changes. Agents must not stage entire dirty files merely to satisfy a “commit” ritual. Commit only when exact owned hunks can be isolated safely; otherwise leave changes unstaged and document them. Preserving user work has higher priority than producing one commit per task.

