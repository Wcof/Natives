# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**Natives** is the "AI Steam Base" — a **"Steam + 创意工坊" ecosystem base** that serves as a cohesive, statically-compiled container (Electron + Next.js + SQLite). It is NOT a monolithic application; it is a base container that users populate with Web page plugin modules (MCP module support planned for future releases).

The sibling projects `CodePilot/` and `fanbox/` are reference implementations whose design patterns Natives will absorb (not directly embed).

### Architecture Documents

- **Architecture Design**: `docs/architecture/ARCHITECTURE.md` — Three-layer architecture specification (Overall / Infrastructure / Frontend)
- **Design Discussion**: `docs/architecture/DESIGN_DISCUSSION.md` — Full Q&A record of architectural decisions with rationale

## Design Philosophy

### Application Death Theory
Future software ecosystems will not be dominated by monolithic applications, but by lightweight MCP (Model Context Protocol), APIs, and microservices. Applications dissolve into composable service units.

### UI Self-Generation
Interface styles and layouts should NOT be hardcoded by developers. They should be dynamically generated or assembled based on user preferences. Computing power is becoming cheap infrastructure (like electricity), making dynamic UI rendering economically viable.

### User Sovereignty ("自己玩自己的")
The final aesthetic and layout decisions belong entirely to the user. The base only provides containers and connection standards — how to arrange, what colors to use, how to combine — all decided by user sovereignty.

### "不造领域轮子" (No Domain Wheel Reinvention)
The "don't write" principle applies to the **plugin layer**: don't build AI clients, code editors, etc. — embed existing tools (Claude Code, VS Code, etc.) directly. The **base layer must be built from scratch**: the container itself is the wheel, with no existing substitute (ADR-0007).

- **CLI layer**: Environment injection — wrap native terminals with credentials (Provider Credentials) to run official CLI wrappers directly.
- **APP layer**: Window stacking, network redirection, or sandbox container technology to embed official Web/desktop apps directly.

## Architecture

### Four Design Pillars

1. **Zero-Code Embedding**: Use iframe + local HTTP server to embed plugin pages. iframe `sandbox` attribute provides security isolation; local HTTP server serves static files and Bridge API. Solve cross-origin restrictions without modifying source application code.

2. **Style Self-Definition**: Base styles (theme accent color, sidebar alignment direction, width) are user-customized in settings. SQLite persistence + dynamic CSS variable injection into DOM enables seamless skinning and layout switching.

3. **Environment Injection & Shell Sandbox**: The base acts as an environment proxy. Users create multiple environment profiles (e.g., "Work", "Personal") with encrypted credential storage. When launching terminals, select which profile to inject. CLI tools automatically receive API Keys and environment variables.

4. **Sub-Application Isolation**: Each subscribed application runs in an independent sandbox, non-interfering.

### Five Production-Grade Defense Lines

These are critical technical decisions that evolved through architectural review:

1. **Subprocess Lifecycle & PID Polling Watchdog**: Independent watchdog `watchdog.ts` injected via `node --require dist/watchdog.js`. Child process probes parent every 2 seconds with `process.kill(parentPid, 0)`; auto-exits on parent death. HTTP service auto-selects free port on startup.

2. **iframe Sandbox Security**: Each plugin runs in an isolated iframe with `sandbox="allow-scripts allow-forms"` (no `allow-same-origin`). Path-prefix isolation (`/modules/{moduleId}/`) + two-phase Session Token handshake (ADR-0001) + MessageEvent.source verification (ADR-0002). Local HTTP server injects CSP headers.

3. **PTY Terminal**: Use `node-pty` for full terminal experience (resize, TUI support). Graceful degradation to `child_process.spawn` if node-pty compilation fails. Frontend `@xterm/xterm` renders character stream; Session Token handshake prevents XSS hijacking IPC.

4. **Database Unidirectional Bus & State Broadcast**: SQLite read/write exclusively by Electron main process. Config changes trigger `db-state-changed` broadcast via IPC; frontend React auto-syncs without refresh.

5. **FOUC Guard & Zod Validation**: Electron window starts invisible (`show: false`); Next.js loads config and mounts CSS variables, then sends `theme-applied-ready` handshake before showing. Zod validates color hex, sidebar pixel ranges, and layout parameters.

### Planned Module Structure

```
src/main/
  main.ts           — Entry point. Window anti-FOUC control, Zod style config validation, dynamic port allocation.
  preload.ts        — IPC isolation bridge. DB CRUD listeners, PTY terminal stream read/write.
  database.ts       — SQLite base library. 7 tables, WAL mode, incremental migration (PRAGMA table_info).
  http-server.ts    — Local HTTP server. Static file serving + Bridge API routes + Session Token auth.
  module-manager.ts — Module lifecycle. Install/uninstall/enable/disable + manifest validation.
  bridge-host.ts    — Bridge API host. Request routing + permission checking + lifecycle events.
  shell.ts          — Shell manager. Multi-session PTY (node-pty) with Token handshake, stream I/O.
  env-injector.ts   — Environment injection. Multi-profile credential management + encrypted storage.
  subprocess.ts     — Subprocess manager. Dynamic port + watchdog params.
  watchdog.ts       — Process guardian. PID polling every 2s, self-destruct on parent death.

src/app/
  page.tsx       — Base shell UI. Three-column layout, dynamic subscribed sidebar, collapsible terminal (xterm.js), workshop panel, persistent settings panel.
  globals.css    — Dark Steam aesthetic with frosted glass and gradients, xterm core typography.
```

## Development Context

### Current State
This directory is currently empty. Implementation is pending. The architecture and module structure above represent the planned design based on extensive architectural review.

### Related Projects
- **`../CodePilot/`** — Reference for AI-native patterns (Claude Agent SDK, SSE streaming, provider abstraction, error classification, DB migration).
- **`../fanbox/`** — Reference for zero-dependency backend patterns (97KB server.js, node-pty terminal, config atomicity, dual preview buffer).

### Key Dependencies (Planned)
- `@xterm/xterm` + `@xterm/addon-fit` — Terminal rendering
- `node-pty` — PTY terminal (native module, graceful degradation to child_process.spawn)
- `zod` — Configuration validation
- `lucide-react` — Icons
- `better-sqlite3` — Database (in `serverExternalPackages`, cannot be bundled by Next.js)

### Runtime Requirements
- Electron with `contextIsolation: true` and no `nodeIntegration`
- Next.js `output: 'standalone'` for Electron packaging
- SQLite data stored at `~/.natives/` (dotfile directory pattern, consistent with sibling projects)

## Standards (必读 — 唯一权威约束源)

**[`docs/standards/`](docs/standards/README.md) 是本项目所有约束的唯一权威来源。** 本文件的「Key Constraints」与「Architecture Review Checklist」是其精简摘要——冲突时一律以 `docs/standards/` 为准。

**接到任何编码任务前，必须先读 [`docs/standards/README.md`](docs/standards/README.md)，并按其文档地图加载与任务相关的 1-3 篇规范。** 规范覆盖四个维度：

- **产品架构** ([`product/`](docs/standards/product))：定位与边界、功能治理、无假数据红线
- **技术架构** ([`technical/`](docs/standards/technical))：分层依赖、五大安全防线、数据与持久化
- **前端架构** ([`frontend/`](docs/standards/frontend))：目录/组件分层、状态与 IPC、i18n 双语同步
- **UI/UE 交互** ([`ui-ux/`](docs/standards/ui-ux))：设计令牌/三皮肤、交互模式、反馈与动效

每条规则用 **MUST / SHOULD / MAY** 标注强度（见 [`00-glossary.md`](docs/standards/00-glossary.md)）。若任务必然违反某条 MUST：**先停止**，向用户确认，或补一条 ADR（`docs/adr/`）说明豁免理由后再实施。

## Key Constraints

- **No fake data**: Every user-visible field must have a real source breadcrumb. No placeholder zeros. Hide or label estimates. → 详见 [`product/02`](docs/standards/product/02-feature-spec.md)
- **i18n sync**: Any UI text change must update both language files. → 详见 [`frontend/03`](docs/standards/frontend/03-i18n.md)
- **DB schema changes**: Must include migration logic. WAL mode with foreign keys. 9 tables total. → 详见 [`technical/03`](docs/standards/technical/03-data.md)
- **Native modules**: `better-sqlite3` and `node-pty` are in `serverExternalPackages` — cannot be bundled by Next.js. Run `npx electron-rebuild -f -w node-pty` after install.
- **Credential encryption**: Use `electron.safeStorage.encryptString()` / `decryptString()` for all credential storage. → 详见 [`technical/02`](docs/standards/technical/02-security.md)
- **Anti-fake data principle**: This is a core design constraint, not a suggestion. Violations break user trust.

## Architecture Review Checklist

Before implementing any new feature, verify:
1. Can it be achieved by embedding an existing official tool (中层策略: "完全不写")?
2. Does it respect the four design pillars?
3. Does it maintain the five defense lines?
4. If it violates any principle, document the reason in an ADR.

<!-- rtk-instructions v2 -->
# RTK (Rust Token Killer) - Token-Optimized Commands

## Golden Rule

**Always prefix commands with `rtk`**. If RTK has a dedicated filter, it uses it. If not, it passes through unchanged. This means RTK is always safe to use.

**Important**: Even in command chains with `&&`, use `rtk`:
```bash
# ❌ Wrong
git add . && git commit -m "msg" && git push

# ✅ Correct
rtk git add . && rtk git commit -m "msg" && rtk git push
```

## RTK Commands by Workflow

### Build & Compile (80-90% savings)
```bash
rtk cargo build         # Cargo build output
rtk cargo check         # Cargo check output
rtk cargo clippy        # Clippy warnings grouped by file (80%)
rtk tsc                 # TypeScript errors grouped by file/code (83%)
rtk lint                # ESLint/Biome violations grouped (84%)
rtk prettier --check    # Files needing format only (70%)
rtk next build          # Next.js build with route metrics (87%)
```

### Test (60-99% savings)
```bash
rtk cargo test          # Cargo test failures only (90%)
rtk go test             # Go test failures only (90%)
rtk jest                # Jest failures only (99.5%)
rtk vitest              # Vitest failures only (99.5%)
rtk playwright test     # Playwright failures only (94%)
rtk pytest              # Python test failures only (90%)
rtk rake test           # Ruby test failures only (90%)
rtk rspec               # RSpec test failures only (60%)
rtk test <cmd>          # Generic test wrapper - failures only
```

### Git (59-80% savings)
```bash
rtk git status          # Compact status
rtk git log             # Compact log (works with all git flags)
rtk git diff            # Compact diff (80%)
rtk git show            # Compact show (80%)
rtk git add             # Ultra-compact confirmations (59%)
rtk git commit          # Ultra-compact confirmations (59%)
rtk git push            # Ultra-compact confirmations
rtk git pull            # Ultra-compact confirmations
rtk git branch          # Compact branch list
rtk git fetch           # Compact fetch
rtk git stash           # Compact stash
rtk git worktree        # Compact worktree
```

Note: Git passthrough works for ALL subcommands, even those not explicitly listed.

### GitHub (26-87% savings)
```bash
rtk gh pr view <num>    # Compact PR view (87%)
rtk gh pr checks        # Compact PR checks (79%)
rtk gh run list         # Compact workflow runs (82%)
rtk gh issue list       # Compact issue list (80%)
rtk gh api              # Compact API responses (26%)
```

### JavaScript/TypeScript Tooling (70-90% savings)
```bash
rtk pnpm list           # Compact dependency tree (70%)
rtk pnpm outdated       # Compact outdated packages (80%)
rtk pnpm install        # Compact install output (90%)
rtk npm run <script>    # Compact npm script output
rtk npx <cmd>           # Compact npx command output
rtk prisma              # Prisma without ASCII art (88%)
```

### Files & Search (60-75% savings)
```bash
rtk ls <path>           # Tree format, compact (65%)
rtk read <file>         # Code reading with filtering (60%)
rtk grep <pattern>      # Search grouped by file (75%). Format flags (-c, -l, -L, -o, -Z) run raw.
rtk find <pattern>      # Find grouped by directory (70%)
```

### Analysis & Debug (70-90% savings)
```bash
rtk err <cmd>           # Filter errors only from any command
rtk log <file>          # Deduplicated logs with counts
rtk json <file>         # JSON structure without values
rtk deps                # Dependency overview
rtk env                 # Environment variables compact
rtk summary <cmd>       # Smart summary of command output
rtk diff                # Ultra-compact diffs
```

### Infrastructure (85% savings)
```bash
rtk docker ps           # Compact container list
rtk docker images       # Compact image list
rtk docker logs <c>     # Deduplicated logs
rtk kubectl get         # Compact resource list
rtk kubectl logs        # Deduplicated pod logs
```

### Network (65-70% savings)
```bash
rtk curl <url>          # Compact HTTP responses (70%)
rtk wget <url>          # Compact download output (65%)
```

### Meta Commands
```bash
rtk gain                # View token savings statistics
rtk gain --history      # View command history with savings
rtk discover            # Analyze Claude Code sessions for missed RTK usage
rtk proxy <cmd>         # Run command without filtering (for debugging)
rtk init                # Add RTK instructions to CLAUDE.md
rtk init --global       # Add RTK to ~/.claude/CLAUDE.md
```

## Token Savings Overview

| Category | Commands | Typical Savings |
|----------|----------|-----------------|
| Tests | vitest, playwright, cargo test | 90-99% |
| Build | next, tsc, lint, prettier | 70-87% |
| Git | status, log, diff, add, commit | 59-80% |
| GitHub | gh pr, gh run, gh issue | 26-87% |
| Package Managers | pnpm, npm, npx | 70-90% |
| Files | ls, read, grep, find | 60-75% |
| Infrastructure | docker, kubectl | 85% |
| Network | curl, wget | 65-70% |

Overall average: **60-90% token reduction** on common development operations.
<!-- /rtk-instructions -->