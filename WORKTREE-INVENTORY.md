# Worktree Inventory（G-001）

- **Task**: G-001（`06-TASK-MASTER.md` · Wave 0 / G0 治理 · P0 · 依赖 `-`）
- **生成**: 2026-08-22
- **分支**: `feat/v2-workspace-design-system`
- **HEAD**: `3127120`（chore(git): 忽略本地编译缓存目录 .cargo-target-local）
- **方法**: `git status --short` 逐字输出 + 按区域归属分类。本任务仅新增 2 个文件：本文档 + `WORKTREE-INVENTORY-STATUS.txt`（git 逐字输出），**无源码写入**。

## 1. 归属图例

| 标记 | 含义 |
|---|---|
| `[既有]` | 本会话（Personal Workspace V2 接手）开始前已存在于 worktree 的未提交改动，属其他 Agent / Design System V2 主线在途产物。**不得覆盖、重置、格式化**，新工作只能在现状之上构建 |
| `[本轮]` | 本会话此前“中英双语 + Design Tokens 修复轮”改动的文件（提示词 i18n、缺失令牌补齐、notification-ui 状态色） |
| `[G-001]` | 本任务新增文件（即本文件） |
| `[工具链]` | 会话工具链本地产物（记忆文件、本地 cargo 配置），不属于产品代码 |

## 2. 区域归属说明

- `src-tauri/**`（13 个修改文件：commands/workspace.rs、commands/theme.rs、commands/mod.rs、db.rs、db/migrations_steps.rs、db/migration_v27.rs、db/db_migrations.rs、handler_registration.rs、sidecar_supervisor_config.rs、workspace/{mod,snapshot,store,types}.rs）— **`[既有]`**。Host Workspace V2 后端：27 个 `workspace_*` Tauri 命令、v27 迁移（7 张 workspace 表）已在 worktree 中，属在途主线产物。
- `src-agent-daemon/**`（1 个文件）— **`[既有]`**，迁移期遗留目录，AGENTS.md 禁止新增功能，仅允许安全/迁移/parity/删除类工作。
- `docs/**`（修改：adr/0021、contracts/workspace-v2-contract.md、standards/ui-ux/01-design-tokens.md；未跟踪：development/ 下 3 个 handoff/checklist 文档）— **`[既有]`**，G0 文档同步工作在途。
- `src/components/**`（含 workspace widgets/views/inspector/layout、ui/design-system、home）— **`[既有]` 为主 + `[本轮]` 部分**：本会话双语修复轮改了 workspace 五个提示词/状态 widget（PromptSnippets、Notes、QuickLinks、ToolStatus、ProxyStatus）与 DataView、FreeCanvasView、WorkspaceInspector、WorkspaceCompositionPage 的文案/颜色引用。
- `src/lib/**`（workspace/{client,contracts,session-store,snapshot-store,events}.ts、home-workspace/persistence.ts、widgets/adapters/*、design-tokens.ts、notification-ui.ts）— **`[既有]` 为主 + `[本轮]` 部分**（design-tokens.ts、notification-ui.ts 为双语轮补齐缺失令牌/状态色）。
- `src/i18n/**`（zh/en 的 app、nav 等 6 文件）— **`[既有]` + `[本轮]`**（双语轮新增 workspace 域键，zh/en 已 1:1 对齐）。
- `src/app/**`（page.tsx、RootClient.tsx、styles/tokens.css）— **`[既有]` + `[本轮]`**（tokens.css 双语轮补齐 `--primary-foreground` / `--primary-subtle` / `--surface-subtle` / `--text-muted` / `--chart-line` 等缺失语义令牌，双主题）。
- 根目录（package.json、package-lock.json、scripts/architecture-debt-manifest.json）— **`[既有]`**。
- 未跟踪新文件（widgets、canvas/snap.ts、docs/development/* 等 22 个）— 保守判定为**在途产物**，不得删除或覆盖。另有 2 个 `[G-001]` 新文件：`WORKTREE-INVENTORY.md`、`WORKTREE-INVENTORY-STATUS.txt`。

## 3. 不可覆盖区（后续所有任务必须遵守）

1. `src-agent-daemon/**`（含 Agent/Harness/Capability、Jobs、Assistant、Plugin Runtime）：迁移期遗留，仅允许安全、迁移、parity、删除工作（AGENTS.md）。
2. `src/types/generated/**`：生成的前端绑定，禁止手改。
3. `extension-host/**`：独立包，仅在任务明确涉及时运行其检查。
4. Workshop iframe 与 Embed WebView 的 sandbox / Bridge 防御：不得削弱。
5. 所有 `[既有]` 未提交文件：不得覆盖、重置（`git checkout/restore/reset` 丢弃类命令）、重排格式；在其现状之上叠加。
6. Secret：仅由 OS Keychain 管理；不得进入 workspace config、Renderer state、日志。
7. 旧数据链路（UDS → Agent Daemon 的 Provider 执行、localStorage 权威快照）：只允许按切片**删除**，不得新增依赖。

## 4. 改动全量清单（逐字）

见 `WORKTREE-INVENTORY-STATUS.txt`（`git status --short` 管道生成，75 行，逐字可复核）：

| 类型 | 数量 | 说明 |
|---|---:|---|
| ` M` 修改 | 50 | 区域归属见 §2 |
| ` D` 删除 | 1 | 见状态文件 |
| `??` 未跟踪 | 24 | 22 个既有在途新文件 + 2 个本任务新增 |

## 5. 复核方法

```sh
cd /Users/ldh/Downloads/project/AiNative/Natives
git status --short | grep -c '^ M'   # 期望 50
git status --short | grep -c '^ D'   # 期望 1
git status --short | grep -c '^??'   # 期望 24（22 既有 + 本任务 2 个）
git diff --stat | tail -1            # 与 §4 清单交叉核对
```

本 inventory 是**时点快照**：其他 Agent 提交或新增改动后，重新执行上述命令并更新本文件（更新，不重置）。
