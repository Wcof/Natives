# AGENTS.md

## Development Commands

- **Start Next.js dev server**: `npm run dev`
- **Start Tauri dev**: `npm run tauri:dev` (starts Next.js + Tauri together)
- **Production build**: `npm run build` then `npm run tauri:build`
- **Rebuild native modules**: Not needed — Tauri manages native deps via Cargo

## Verification

- **Type check**: `npm run typecheck` (tsc --noEmit)
- **Lint**: `npm run lint` (next lint)
- **Test**: `npm run test` (tsx --test src/**/*.test.ts)
- **Cargo check**: `cd src-tauri && cargo check`
- **Cargo test**: `cd src-tauri && cargo test`

## Architecture

- Tauri v2 + Next.js + SQLite desktop container
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
- Credentials encrypted with AES-256-GCM (in `env_manager.rs`) → [`technical/02`](docs/standards/technical/02-security.md)
- Plugin iframe sandbox must be `allow-scripts allow-forms`, no `allow-same-origin` → [`technical/02`](docs/standards/technical/02-security.md)
- Frontend must access backend only through `window.nativesAPI` (tauri-adapter) → [`technical/01`](docs/standards/technical/01-layering.md)

## File Structure

- `src-tauri/` - Tauri backend (Rust): commands, database, HTTP server, terminal, modules
- `src/app/` - Next.js frontend application
- `src/components/` - React UI components
- `src/lib/` - Shared frontend libraries (tauri-adapter, design-tokens, etc.)
- `~/.natives/` - SQLite data storage location

## Development Notes

- Tauri window starts hidden (`visible: false` in tauri.conf.json) for FOUC protection
- It is shown after the frontend sends `theme_ready_signal` via Tauri command
- SQLite read/write exclusively by Tauri Rust backend (src-tauri/)
- Config changes broadcast via `db-state-changed` IPC event

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
