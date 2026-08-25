# File Ownership 与 Shared Lock（M-005）

> 状态：**已冻结**（2026-08-21）。三个长期 Subagent（A/B/C）只能写各自 exclusive 范围；
> shared-lock 文件只允许提交 **patch intent**，由 Main Agent 最终落地。
> 数据 Source of Truth 唯一：Workspace = SQLite v27 表；Theme = V2 semantic tokens；Layout = workspace_layouts。

## A｜Workspace Host / Persistence / Legacy Host Death

| 范围 | 模式 |
|---|---|
| `src-tauri/src/workspace/**` | exclusive |
| `src-tauri/src/commands/workspace.rs` | exclusive |
| `src-tauri/src/db.rs`、`src-tauri/src/db/db_migrations.rs`、`src-tauri/src/db/migrations_steps.rs`、`src-tauri/src/db/schema.rs` | exclusive（schema/migration） |
| `src/lib/workspace/contracts.ts`、`client.ts`、`events.ts`、`session-store.ts`、`snapshot-store.ts` | exclusive（前端 Host contract/client） |
| `src/lib/home-workspace/model.ts`、`persistence.ts` | exclusive / migration-source（旧 Home 迁移源，可读可改但以迁移语义为准） |
| `src-tauri/src/**theme*`、`src-tauri/src/**appearance*`、`src-tauri/src/commands/theme.rs`、`src-tauri/src/ghostty_config.rs` | exclusive（主题偏好持久化兼容，V-001..V-004；不编辑 CSS/token/page） |
| `crates/provider-adapters/**`、`src-tauri/src/proxy/**`（A-D02 解耦） | exclusive（Legacy Death 阶段） |

## B｜Design System V2 / Shell Visual / Widget Visual

| 范围 | 模式 |
|---|---|
| `src/app/styles/**` | exclusive |
| `src/lib/design-tokens.ts`、`src/lib/theme-engine.ts`、`src/context/ThemeContext.tsx` | exclusive |
| `src/components/ui/**`（含 `design-system/**`） | exclusive |
| `src/components/ui/design-system/**` | exclusive |
| `src/lib/workspace/widgets/**` | exclusive |
| `src/components/workspace/widgets/**` | exclusive |
| `src/components/home/widgets/**` | migration-source（旧 Widget 迁移/清理） |
| `src/components/dashboard/**` | exclusive（chart 迁移） |
| `src/lib/prompt-context-injector.ts`（B-D01） | exclusive |
| `src/i18n/**` 主题命名段（V-030 界面命名） | exclusive-limited |
| `docs/standards/ui-ux/**`（V-063 冻结） | exclusive-limited（B-D02） |

## C｜Workspace UX / Layout / Domain Page Migration

| 范围 | 模式 |
|---|---|
| `src/components/workspace/**`（排除 `widgets/**`） | exclusive |
| `src/lib/workspace/canvas/**`、`src/lib/workspace/views/**` | exclusive |
| `src/components/home/HomeWorkspacePage.tsx` | migration-source（替换为 WorkspaceCompositionPage） |
| `src/components/ui/ResizableRightPanel.tsx` | exclusive-limited（Inspector host 复用） |
| `src/components/home/**`（排除 `widgets/**`）、`src/components/files/**`、`src/components/preview/**`、`src/components/library/**`、`src/components/capabilities/**`、`src/components/apps/**`、`src/components/creative/**`、`src/components/ai/**`、`src/components/jobs/**`、`src/components/settings/**`、`src/components/tools/**`、`src/components/menubar/**`、`src/components/onboarding/**`、`src/components/release/**`、`src/components/screenshot/**`、`src/components/update/**` | exclusive（Domain 页视觉迁移） |
| `src/app/**/page.tsx`（业务路由，排除根 `page.tsx`） | exclusive |
| `src/components/shell/Terminal.tsx`、`TerminalRecorder.tsx`、`src/app/styles/terminal.css`（V-049） | exclusive |

## MAIN｜Shared Lock（最终写权，只收 patch intent）

| 文件 | 说明 |
|---|---|
| `package.json`、`package-lock.json` | 依赖/脚本最终变更（B-D04 移除 liquid-glass-react 等） |
| 根 `Cargo.toml`、`src-tauri/Cargo.toml` | A-D05 Cargo 清理 patch |
| `src-tauri/src/handler_registration.rs` | workspace/legacy command 注册（I-003） |
| `src-tauri/src/lib.rs` / `main.rs` / `setup` 钩子（若需挂 workspace service 状态） | Main 裁决 |
| `src/app/RootClient.tsx`、`src/app/page.tsx`、`src/app/layout.tsx` | 根组合 / SSR 主题初值（V-007） |
| `src/components/shell/ShellLayout.tsx`、`MainContent.tsx`、`Header.tsx`、`Sidebar.tsx` | Shell 集成（V-027/V-028 patch intent） |
| `src/components/shell/sidebar/model.ts`、`settings-navigation.ts` | IA 路由 |

## 规则

1. 无两个 Subagent 对同一 shared 文件直接写；shared 变更以 `Shared-file patch intents:` 段落提交。
2. `src/app/styles/**` 只归 B；C 不得自建颜色体系，缺 token 向 B 提 contract request（handoff 记录）。
3. B 最先冻结 token 名 / primitive props / surfacePolicy enum；C 随即并行接入。
4. 不得为方便建立第二套 Workspace / Theme / Layout / 数据 Source of Truth。
5. 每波结束必须输出 handoff（Task ID / 文件 / Source of Truth 变化 / shared patch / 未验证假设 / Final Gate 风险）。

---

## 2026-08-25 整改专用任务租约与 Patch-Intent 契约（ADR-0022）

### 1. 任务专属文件租约表

| 任务 | 专属租约（Exclusive Scope） | 共享 Patch 目标 |
|---|---|---|
| **TH-01** (Theme Host) | `src-tauri/src/commands/theme.rs`, Workspace model/repo/service 中 theme 段, v30 migration 与对应 Rust tests | `handler_registration.rs`, `lib.rs` |
| **TH-02** (Theme Renderer) | `src/lib/theme-engine.ts`, `src/context/ThemeContext.tsx`, `src/lib/appearance/*` 及 tests | `RootClient.tsx`, `ShellLayout.tsx` |
| **TH-03** (Tokens) | `src/lib/design-tokens.ts`, `src/app/styles/tokens.css`, token tests | `WidgetShell.tsx`, `widgets.css` |
| **WS-01/02** (Grid) | `src/components/workspace/GridWorkspaceView.tsx`, `CompactGrid.tsx`, grid tests | `WidgetShell.tsx`, `widgets.css` |
| **WS-03/04** (Canvas) | `src/components/workspace/FreeCanvasView.tsx`, `src/lib/workspace/canvas/*`, canvas unit tests | `WorkspaceCompositionPage.tsx` |
| **WS-05** (Widget Identity) | `src/lib/workspace/client.ts`, `session-store.ts`, Host workspace add transaction | `WorkspaceCompositionPage.tsx` |
| **APP-01** (App DTO) | `src-tauri/src/apps/model.rs`, `src/types/generated/AppView.ts` (由 APP-01 运行生成命令) | `src/lib/tauri/apps.ts` |
| **APP-02** (App Adapter) | `src/lib/tauri/apps.ts`, adapter tests | `ShellLayout.tsx` |
| **APP-03/04** (App Runtime) | `src-tauri/src/commands/apps.rs`, `src-tauri/src/apps/service.rs`, `presentation/` | `lib.rs` |
| **APP-05** (App UI) | `src/components/apps/**` | `ShellLayout.tsx`, `i18n/**` |
| **MAIN** (主集成者) | Shared-lock 文件（Root, Shell, Layout, Composition, i18n, lib.rs, Cargo/package.json） | — |

### 2. Patch-Intent 格式规范

```text
Target file: <path>
Reason / contract: <ADR-0022 / task description>
Anchor symbol: <symbol name / line context>
Requested change: <minimal diff intent>
Required imports/types: <types list>
Failure if omitted: <consequence>
Owning task: <Task ID>
```

### 3. 固定交接格式规范

```text
Task ID / status: implemented / awaiting-final-gate
Exclusive files changed: <list>
Contract/authority implemented: <summary>
Tests authored but not yet run: <list>
Shared patch intents: <list / none>
Known final-gate risks: <risks / none>
```