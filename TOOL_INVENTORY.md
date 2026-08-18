# Native Tool Inventory

This inventory records the production tool surface reached through
`Renderer -> Tauri Host -> Agent Daemon -> AgentEngine -> PermissionGatedTools`.
It is an execution contract, not a roadmap. Generated bindings are not an
authority for tool behavior.

| Tool surface | Availability | Enforcement and durable feedback | Current verification |
| --- | --- | --- | --- |
| `read_file`, `search_files`, `list_dir`, `grep` | Built in | Gateway schema, project path scope, requested/prepared/completed events | Gateway suite |
| `write_file`, `edit_file`, `apply_patch` | Built in | Project-write permission, path preflight, side-effect ledger, atomic replace; `apply_patch` accepts legacy, `files`, and patch-text forms | Gateway suite, concurrent atomic-write regression |
| `run_terminal` | Built in | Project permission, supervised process group, bounded output, cancellation and TERM-to-KILL escalation | Gateway and daemon process tests |
| `web_fetch` | Built in | Network permission, scheme/DNS/private-IP and redirect revalidation, streamed 512 KiB maximum | Gateway bounded-stream regressions |
| `web_search` | Configured Brave or Tavily backend only | Backend URL validation, redirect disabled, result URL SSRF filtering | Configuration-dependent |
| `todo_write` | Built in | Always-allowed control tool with normal tool events | Gateway suite |
| `notification` | Built in | Always-allowed; frozen Harness notification hooks apply | Daemon hook tests |
| `memory_search`, `memory_get` | Built in | Read-only gateway memory lookup | Scope authority audit pending |
| `enter_plan_mode`, `exit_plan_mode` | Built in; visibility changes with plan latch | Enter is a gateway control; exit requires persisted human approval | Daemon plan-mode suite |
| `task`, `task_output`, `kill_task` | Built in daemon orchestration | Child Run authority, parent ceilings, reservation, cancellation tree, durable watcher result | Daemon subagent tests |
| `skill` | Built in daemon orchestration | Run snapshot selection and parent tool-surface ceiling; unselected skills fail closed | Skill selection regressions |
| `mcp_call`, `mcp__<server>__<tool>` | Selected trusted MCP servers only | Dynamic schema, permission, run selection, per-request cancellation, 256,000-byte returned-result limit | Real stdio MCP regressions |
| `create_creative_draft` | Built in | Draft store authority | Gateway suite |
| `creative_proposal` | Built in | Structural validation plus mandatory durable Host approval fact; persistence failure is a tool error | Daemon persistence regression |
| `write_draft_module`, `read_draft_module`, `rollback_draft_revision`, `lint_draft_module` | Explicit creative allowlist only | Draft-ID containment, conversation ownership, linting, SQLite transaction | Creative tool tests |

## Shared invariants

- The model sees only schemas selected for the Run's frozen tool plan.
- Permission and Harness decisions may narrow authority; they never widen it.
- Tool completion is persisted before post-execution observation can fail the Run.
- Live text deltas are not durable facts; completed messages and tool results are.
- Cancellation is request/run scoped. Shared MCP server lifetime is not owned by
  one tool request.
- Output limits are enforced at the actual return seam, including daemon-special
  MCP dispatch.

## Known verification boundaries

- `web_search` needs a configured external backend.
- Real-provider execution needs valid Host-brokered credentials.
- Memory project-scope enforcement is blocked until RPC sessions carry an
  authoritative project identity; cooperative caller-provided paths are not
  treated as a security boundary.
