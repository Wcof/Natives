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

### "完全不写" (Don't Write At All)
Do NOT reinvent wheels. Official/community tools (Cursor Cloud, Desktop APP, Codex APP, Tree Solo, etc.) are already polished. The core idea is "don't write" — directly invoke or embed official tools within the base container, enabling seamless switching in a unified interface.

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

2. **iframe Sandbox Security**: Each plugin runs in an isolated iframe with `sandbox="allow-scripts allow-forms"` (no `allow-same-origin`). Path-prefix isolation (`/modules/{moduleId}/`) + Session Token authentication. Local HTTP server injects CSP headers.

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

## Key Constraints

- **No fake data**: Every user-visible field must have a real source breadcrumb. No placeholder zeros. Hide or label estimates.
- **i18n sync**: Any UI text change must update both language files.
- **DB schema changes**: Must include migration logic. WAL mode with foreign keys. 9 tables total.
- **Native modules**: `better-sqlite3` and `node-pty` are in `serverExternalPackages` — cannot be bundled by Next.js. Run `npx electron-rebuild -f -w node-pty` after install.
- **Credential encryption**: Use `electron.safeStorage.encryptString()` / `decryptString()` for all credential storage.
- **Anti-fake data principle**: This is a core design constraint, not a suggestion. Violations break user trust.

## Architecture Review Checklist

Before implementing any new feature, verify:
1. Can it be achieved by embedding an existing official tool (中层策略: "完全不写")?
2. Does it respect the four design pillars?
3. Does it maintain the five defense lines?
4. If it violates any principle, document the reason in an ADR.
