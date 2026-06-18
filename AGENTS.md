# AGENTS.md

## Development Commands

- **Start Next.js dev server**: `npm run dev`
- **Start Electron dev**: `npm run electron:dev` (requires Next.js dev server running first)
- **Production build**: `npm run build` then `npm run electron:build`
- **Rebuild native modules**: `npm run electron:rebuild` (required after install for better-sqlite3, node-pty)

## Verification

- **Type check**: `npm run typecheck` (tsc --noEmit)
- **Lint**: `npm run lint` (next lint)
- **Test**: `npm run test` (tsx --test src/**/*.test.ts)

## Architecture

- Electron + Next.js + SQLite desktop container
- Three-layer architecture: Overall, Infrastructure, Frontend
- Four design pillars: Zero-Code Embedding, Style Self-Definition, Environment Injection, Sub-Application Isolation
- Five defense lines: Watchdog, iframe sandbox, PTY terminal, Database unidirectional bus, FOUC guard

## Standards (必读 — 唯一权威约束)

**[`docs/standards/`](docs/standards/README.md) 是本项目所有约束的唯一权威来源。** 下方「Key Constraints」是其精简摘要——冲突时以 `docs/standards/` 为准。

**任何功能开发、重构、Bug 修复任务开始前，必须先读 [`docs/standards/README.md`](docs/standards/README.md)，并按其文档地图加载与任务相关的规范篇。** 规范按四个维度组织：

- **产品架构** ([`product/`](docs/standards/product))：定位与边界、功能治理、无假数据红线
- **技术架构** ([`technical/`](docs/standards/technical))：分层依赖、五大安全防线、数据与持久化
- **前端架构** ([`frontend/`](docs/standards/frontend))：目录/组件分层、状态与 IPC、i18n 双语同步
- **UI/UE 交互** ([`ui-ux/`](docs/standards/ui-ux))：设计令牌/三皮肤、交互模式、反馈与动效

每条规则用 **MUST / SHOULD / MAY** 标注强度（见 [`00-glossary.md`](docs/standards/00-glossary.md)）。若任务必然违反某条 MUST，先停止、向用户确认，或补一条 ADR 说明豁免理由后再实施。

## Key Constraints（摘要 — 详见 standards/）

- No fake data: All user-visible fields must have real source breadcrumbs → [`product/02`](docs/standards/product/02-feature-spec.md)
- i18n sync: UI text changes must update both language files → [`frontend/03`](docs/standards/frontend/03-i18n.md)
- DB schema changes require migration logic (WAL mode, foreign keys) → [`technical/03`](docs/standards/technical/03-data.md)
- Native modules in `serverExternalPackages` cannot be bundled by Next.js
- Credentials encrypted with `electron.safeStorage` → [`technical/02`](docs/standards/technical/02-security.md)

## File Structure

- `src/main/` - Main process modules (database, shell, module-manager, etc.)
- `src/app/` - Next.js frontend application
- `electron/` - Electron main process entry points
- `~/.natives/` - SQLite data storage location

## Development Notes

- Electron window starts invisible (`show: false`) for FOUC protection
- Wait for `theme-applied-ready` IPC before showing window
- SQLite read/write exclusively by Electron main process
- Config changes broadcast via `db-state-changed` IPC

## Diagnostics & Tooling (RTK & CodeGraph)

- **CodeGraph**:
  - If a `.codegraph/` directory exists at the project root, always use CodeGraph MCP tools (`codegraph_explore`, `codegraph_node`) or CLI commands (`codegraph explore "<query>"`, `codegraph node <symbol-or-file>`) to locate code definitions and understand call chains before resorting to standard grep/ripgrep.
  - If `.codegraph/` is absent, skip it (indexing is the user's decision).
- **RTK (High-Performance CLI Proxy)**:
  - **Type Checking**: Use `rtk tsc --noEmit` instead of plain `npm run typecheck` to view error messages grouped by file in a token-optimized format.
  - **Searching**: Use `rtk grep "<query>" src` instead of plain ripgrep for compact, structured search output.
  - **Dependencies**: Use `rtk deps` to inspect and verify dependencies.
  - **Git Operations**: Use `rtk git` to view condensed change diffs.
  - **Tests**: Use `rtk test` to view only test failures.