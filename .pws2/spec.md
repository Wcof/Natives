# Personal Workspace V2 · T01 施工规格（spec）

> 来源：`/Users/ldh/Downloads/project/plan2/`（README / 01 / 02 / 03）全文提取，并对照仓库当前实现校准（`src-tauri/src/workspace/*`、`src-tauri/src/db/migration_v27.rs`、`migration_v29.rs`、`src/lib/workspace/*`、`src/components/workspace/widgets/*`）。
> 基线：分支 `codex/provider-proxy-full` @ `93b3b4d`。schema head = **v29**（v28 = 应用中心）。
> 权威链：`SQLite / Domain Sources → Tauri WorkspaceDomain → typed commands+events → Renderer WorkspaceStore(bounded) → WorkspaceExperience → LayoutEngine → WidgetCatalog → WidgetRuntime → WorkspaceDataBroker → Domain adapters`。
> 裁决顺序：standards > ADR-0021 > contract > plan2。矛盾裁决见 `t01-report.md`。

---

## 1. DB Schema（全表 DDL + 迁移版本 + 幂等禁 DROP）

### 1.1 迁移治理

- 迁移文件：`src-tauri/src/db/migration_v29.rs`（PWSV2 增量），注册于 `migrations_steps.rs`；版本标记写 `settings._schema_version = '29'`。
- **增量、单向、幂等**：所有新增列用 `PRAGMA table_info` guard（`add_column_if_missing` / `table_columns`）；新表用 `CREATE TABLE IF NOT EXISTS`；唯一键用 `CREATE UNIQUE INDEX IF NOT EXISTS`。
- **禁止 `DROP TABLE` / 破坏式重建**（standards technical/03 R-D3）。回滚版本忽略新增列/表。
- 写 migration marker + source hash；重复运行不得创建重复 Widget（幂等 backfill）。
- 旧读路径（`workspace_tabs`、`hidden` 列）在新 Snapshot 验证通过后同一切片删除，不保留静默 fallback。

### 1.2 v27 基线表（`migration_v27.rs`，已落地）

```sql
workspaces (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  kind TEXT NOT NULL DEFAULT 'workspace',          -- 'home'|'workspace'
  icon TEXT, description TEXT,
  theme TEXT NOT NULL DEFAULT 'dark',              -- CHECK(dark|light) 归一
  is_active INTEGER NOT NULL DEFAULT 0,            -- legacy active 标记
  position INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL
);
workspace_tabs (                                   -- 旧内容 tab 表 → v29 后 legacy，无 production 读写
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  tab_type TEXT NOT NULL, title TEXT NOT NULL DEFAULT '', ref_id TEXT, url TEXT,
  position INTEGER NOT NULL DEFAULT 0, is_active INTEGER NOT NULL DEFAULT 0,
  pinned INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL
);
workspace_context_items (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  item_kind TEXT NOT NULL, ref_id TEXT NOT NULL, title TEXT NOT NULL DEFAULT '',
  meta_json TEXT NOT NULL DEFAULT '{}',
  position INTEGER NOT NULL DEFAULT 0, created_at TEXT NOT NULL
);
workspace_widgets (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  widget_type TEXT NOT NULL,
  config_json TEXT NOT NULL DEFAULT '{}',
  hidden INTEGER NOT NULL DEFAULT 0,               -- legacy → v29 映射 enabled，保留至 death proof
  position INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL
);
workspace_layouts (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  breakpoint TEXT NOT NULL,                        -- lg/md/sm | 'free'
  layout_json TEXT NOT NULL DEFAULT '[]',
  is_active INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
  UNIQUE(workspace_id, breakpoint)
);
workspace_view_states (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  view_key TEXT NOT NULL, state_json TEXT NOT NULL DEFAULT '{}',
  updated_at TEXT NOT NULL, UNIQUE(workspace_id, view_key)
);
workspace_tool_profiles (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  profile_id TEXT NOT NULL, tool_key TEXT, config_json TEXT NOT NULL DEFAULT '{}',
  enabled INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL, UNIQUE(workspace_id, profile_id)
);
```

### 1.3 v29 增量（`migrate_v29`，与 plan2 §4.2 / ADR 修订 §10 / contract 一致）

```sql
-- workspaces（增量列）
ALTER TABLE workspaces ADD COLUMN default_layout_mode TEXT NOT NULL DEFAULT 'structured'; -- CHECK(structured|free)
ALTER TABLE workspaces ADD COLUMN appearance_json     TEXT NOT NULL DEFAULT '{}';
ALTER TABLE workspaces ADD COLUMN template_source_id  TEXT;                               -- 仅溯源
ALTER TABLE workspaces ADD COLUMN template_version    INTEGER;                            -- 仅溯源
ALTER TABLE workspaces ADD COLUMN deleted_at          TEXT;                               -- 软删

-- workspace_widgets（增量列）
ALTER TABLE workspace_widgets ADD COLUMN config_version INTEGER NOT NULL DEFAULT 1;
ALTER TABLE workspace_widgets ADD COLUMN appearance_json  TEXT NOT NULL DEFAULT '{}';     -- V-003 白名单
ALTER TABLE workspace_widgets ADD COLUMN enabled          INTEGER NOT NULL DEFAULT 1;     -- 生产读写切此
ALTER TABLE workspace_widgets ADD COLUMN z_index          INTEGER NOT NULL DEFAULT 0;
-- backfill: enabled = NOT hidden（幂等；仅 enabled=0 的行未映射时执行）

-- workspace_layouts（增量列 + 语义唯一键）
ALTER TABLE workspace_layouts ADD COLUMN layout_mode    TEXT NOT NULL DEFAULT 'structured';
ALTER TABLE workspace_layouts ADD COLUMN layout_version INTEGER NOT NULL DEFAULT 1;
-- 保留原 UNIQUE(workspace_id,breakpoint)；另建语义唯一索引（两模式共存：structured=lg/md/sm，free='free'，不冲突）
CREATE UNIQUE INDEX IF NOT EXISTS uq_workspace_layouts_mode_breakpoint
  ON workspace_layouts(workspace_id, layout_mode, breakpoint);

-- workspace_view_states（增量列）
ALTER TABLE workspace_view_states ADD COLUMN state_version INTEGER NOT NULL DEFAULT 1;

-- workspace_context_items（增量列）
ALTER TABLE workspace_context_items ADD COLUMN updated_at TEXT;

-- 新表：真正 Workspace 会话 tabs
CREATE TABLE IF NOT EXISTS workspace_open_tabs (
  workspace_id TEXT PRIMARY KEY REFERENCES workspaces(id) ON DELETE CASCADE,
  sort_order REAL NOT NULL DEFAULT 0,
  is_pinned  INTEGER NOT NULL DEFAULT 0,
  opened_at  TEXT NOT NULL,
  last_active_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_workspace_open_tabs_order
  ON workspace_open_tabs(is_pinned, sort_order);
-- backfill: 当前 active workspace 写入 open_tabs（幂等）

-- 新表：内置/个人模板
CREATE TABLE IF NOT EXISTS workspace_templates (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  origin TEXT NOT NULL DEFAULT 'personal' CHECK(origin IN ('builtin','personal')),
  schema_version INTEGER NOT NULL DEFAULT 1,
  template_version INTEGER NOT NULL DEFAULT 1,
  manifest_json TEXT NOT NULL DEFAULT '{}',
  preview_key TEXT,
  created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
  deleted_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_workspace_templates_origin
  ON workspace_templates(origin, deleted_at);
```

### 1.4 表语义速查（施工要点）

| 表 | 关键字段 | 施工要点 |
|---|---|---|
| `workspaces` | `default_layout_mode`、`appearance_json`、`template_source_id/version`、`deleted_at` | list/snapshot 过滤 `deleted_at IS NULL`；Close 不写此表；子行随软删查询侧级联过滤，物理 hard delete 仅在 Final Gate |
| `workspace_open_tabs` | `workspace_id` PK | row 存在=打开；Close=删 row；Reopen=插 row；**永不**表示布局/数据视图模式 |
| `workspace_widgets` | `enabled`、`config_version`、`appearance_json`、`z_index` | 生产读写 `enabled`，不再读写 `hidden`；appearance 白名单 `surfaceVariant/header/opacity`，禁颜色 token 入库 |
| `workspace_layouts` | `layout_mode`、`layout_version`、`breakpoint` | structured=`lg/md/sm` 各一份；free=`breakpoint='free'` 一份；`layout_mode` 用于校验 |
| `workspace_view_states` | `view_key`、`state_version` | Data View 模式（`list/table/board/calendar`）、canvas camera 等 UI state，按 `view_key` 作用域；非业务数据 |
| `workspace_templates` | `origin`、`schema_version`、`template_version`、`manifest_json` | built-in 由代码 manifest 提供；表存 personal 模板 + built-in metadata override；Host 暴露统一 read model |

---

## 2. Host Tauri Command 清单 + DTO + 事件

### 2.1 Command 清单（`src-tauri/src/commands/workspace.rs`，28 个；command 只调 service，不直连 DB）

| 类别 | Command | 说明 |
|---|---|---|
| Workspace | `workspace_list` | `Vec<WorkspaceSummary>`（过滤软删） |
| | `workspace_get` | 单 workspace snapshot 或 null |
| | `workspace_create` | 入参 `WorkspaceCreateInput`（可带 `templateId`，缺省 `classic-personal-dashboard`） |
| | `workspace_update` | patch + `expectedRevision` |
| | `workspace_delete` | 软删 + `expectedRevision` |
| | `workspace_set_active` | 设 active |
| | `workspace_duplicate` | 复制全部子行为新 id |
| | `workspace_snapshot` | 单 workspace 全量读模型 |
| Context | `workspace_context_add` / `workspace_context_remove` / `workspace_context_batch_update` / `workspace_context_reorder` / `workspace_mcp_exposure` | 均带 `expectedRevision`（exposure 只读） |
| Widget | `workspace_widget_upsert` / `workspace_widget_remove` / `workspace_widget_batch_update` | upsert `WorkspaceWidgetInput`；batch 单事务 |
| Layout | `workspace_layout_save` | `{workspaceId, layoutMode, breakpoint, layoutVersion, layoutJson}` |
| View State | `workspace_view_state_save` | `{workspaceId, viewKey, stateVersion, stateJson}` |
| Tool Profile | `workspace_tool_profile_bind` / `workspace_tool_profile_unbind` | 无 secret 明文 |
| Session | `workspace_session_open` / `workspace_session_close` / `workspace_session_snapshot` / `workspace_session_reorder` | open/close = open_tabs 插/删 row |
| Template | `workspace_template_list` / `workspace_template_save` / `workspace_template_delete` / `workspace_restore_template` / `workspace_widget_reset` | list 返回 built-in + personal 统一 read model |

> 注：contract §4 列出的 `open_workspace/close_workspace/reopen_workspace/pin_workspace/reorder_workspace_tabs` 等语义在实现中由 `workspace_session_*`（open/close/snapshot/reorder）与 `workspace_set_active` 承载；`workspace_tab_*`（旧 View Tab）命令族 PWSV2 起随旧 View Tab Strip 同一切片删除，无 fallback。

### 2.2 关键 DTO（`src/lib/workspace/contracts.ts`，与 Rust serde `camelCase` 对齐）

```ts
type WorkspaceLayoutMode = 'structured' | 'free';      // 词表唯一（compact 废弃）
type WorkspaceBreakpoint = 'lg' | 'md' | 'sm' | 'free';
type ExpectedRevision = number;                          // A-033 48-bit 内容指纹，回传

interface WorkspaceSummary { id; name; kind; icon; description; theme:'dark'|'light';
  isActive; position; defaultLayoutMode: WorkspaceLayoutMode; appearance: Record<string,unknown>;
  templateSourceId: string|null; templateVersion: number|null; createdAt; updatedAt; }
interface WorkspaceWidget { id; workspaceId; widgetType; configVersion; config; appearance;
  enabled:boolean; zIndex; position; createdAt; updatedAt; }
interface WorkspaceLayout { id; workspaceId; layoutMode; breakpoint; layoutVersion;
  layout: unknown; isActive; createdAt; updatedAt; }
interface WorkspaceViewState { id; workspaceId; viewKey; stateVersion; state: Record<string,unknown>; updatedAt; }

interface WorkspaceSnapshot {        // 单 workspace 全量（inactive 不携带 widgets/layouts）
  workspace: WorkspaceSummary; contextItems: WorkspaceContextItem[];
  widgets: WorkspaceWidget[]; layouts: WorkspaceLayout[];
  viewStates: WorkspaceViewState[]; toolProfiles: WorkspaceToolProfile[];
  revision: number;                  // 回传为 expectedRevision
}
interface WorkspaceSessionSnapshot { // 全局轻量
  openedTabs: WorkspaceOpenTab[]; activeWorkspaceId: string|null;
  workspaces: WorkspaceSummary[];    // 仅 metadata
  revision: number;
}
interface WorkspaceTemplateManifestV1 {
  schemaVersion: 1; templateVersion: number; nameKey: string|null;
  appearance: Record<string,unknown>; defaultLayoutMode: WorkspaceLayoutMode;
  widgets: { key; widgetType; configVersion; config; appearance }[];
  layouts: Record<string,unknown>;   // structured: {lg,md,sm} | free: {free}
}
interface WorkspaceTemplate { id; name; origin:'builtin'|'personal'; schemaVersion;
  templateVersion; nameKey; previewKey; manifest; createdAt; updatedAt; }
```

入参：`WorkspaceCreateInput{name,kind?,icon?,description?,theme?,defaultLayoutMode?,templateId?}`、`WorkspaceUpdateInput`、`WorkspaceContextItemInput{itemKind,refId,title?,meta?}`、`WorkspaceContextItemPatch{id,title?,meta?,position?}`、`WorkspaceWidgetInput{id?,widgetType,config?,configVersion?,appearance?,enabled?,zIndex?}`、`WorkspaceWidgetConfigPatch{id,config}`。

### 2.3 事件（Host mutation 成功后广播）

- 传输：standards technical/02 R-S9 强制走 `db-state-changed`（channel `workspace`）；Renderer `src/lib/workspace/events.ts` 同时订阅 `workspace` 与 `db-state-changed`（`channel==='workspace'` 过滤）。
- Payload：`{ workspaceId?, event?, channel? }`，`event` 取 14 种 kind（实现枚举）：
  `created, updated, deleted, activeChanged, sessionOpened, sessionClosed, tabChanged, contextChanged, widgetChanged, layoutChanged, viewStateChanged, toolProfileChanged, templateSaved, templateDeleted, templateRestored`（实际 emit 使用其中 14 项；`templateSaved/templateDeleted/templateRestored` 为模板事件）。
- Renderer 收到更高 revision 后 reconcile；不靠各 Widget 轮询配置。
- **禁止**把 plan2 的自造命名事件（`workspace:snapshot-changed` 等）作为独立 transport——以 `db-state-changed`/`workspace` channel 为准（裁决见 report）。

---

## 3. LayoutEngine：Structured(12/8/4) / Free(bounded)

### 3.1 Interface（`src/lib/workspace/layout/`，两 adapter 实现同一 interface）

```ts
interface LayoutEngine<TDocument> {
  readonly mode: 'structured' | 'free';
  validate(document: unknown, widgets: WidgetInstance[]): TDocument;
  derive(source: LayoutDocument, widgets: WidgetInstance[]): TDocument;
  insert(document: TDocument, widget: WidgetInstance, viewport: Viewport): TDocument;
  remove(document: TDocument, widgetIds: string[]): TDocument;
  duplicate(document: TDocument, mapping: Record<string,string>): TDocument;
  serialize(document: TDocument): PersistedLayout;
}
```
目标文件：`src/lib/workspace/layout/{types.ts, structured.ts, free.ts, conversion.ts, validation.ts}`。React View（`CompactGrid.tsx` / `FreeCanvasView.tsx`）只渲染 + 传 terminal event；算法在 module 内测试。

### 3.2 Structured（`StructuredLayoutEngine`，adapter = `CompactGrid.tsx`）

- 列：`lg=12 / md=8 / sm=4`（`GRID_COLUMNS` / `GRID_BREAKPOINTS`）。
- item 形状（RGL）：`{ i, x, y, w, h, minW, minH }`；compactor `preventCollision: true`（`STABLE_COMPACTOR`）。
- Widget Definition 声明 `default/min/max size`；非法组合不能持久化。
- `normalize`：clamp x/w 到列宽、h≥minH、去重叠；仅 `commitStopped`（drag/resize stop）持久化。
- keyboard move / resize、multi-select batch、`lg→md/sm` 确定性派生（priority + 最小宽 + reading order）；用户手动调整某断点后 `customized=true` 不自动覆盖；删 Widget 同步删所有断点项；新 Widget 每断点生成合法位置。

### 3.3 Free（`FreeLayoutEngine`，adapter = `src/lib/workspace/canvas/*`）

- bounded DOM 画布（standards ui-ux/02 R-U13）：**禁**无限画布/CRDT/协同/Plugin Runtime。
- 常量（`canvas/types.ts`）：`CANVAS_GRID=16`、`CANVAS_MIN_ZOOM=0.25`、`CANVAS_MAX_ZOOM=2.5`、`CANVAS_ZOOM_STEP=0.15`、`CANVAS_DEFAULT_SIZE={w:240,h:160}`。
- 能力：pan / zoom（clamp）/ drag / resize / snap / multi-select / group / frame / z-order / fit-to-content / reset camera。
- `CanvasNode{id,kind:'card'|'note'|'frame'|'group'|'widget',label,x,y,w,h,z,accent?,locked?,members?,frameId?,widgetType?,widgetConfig?}`；`CanvasCamera{x,y,zoom}`。
- `persisted layout_json`（breakpoint='free'）：world rect + camera + parent frame + z-index。
- 节点不可无限移出可恢复区域；>200 nodes 需 viewport culling。

### 3.4 切换映射（structured ↔ free，`conversion.ts`）

- Layout Mode 是 Workspace 级状态（`structured|free`），入口只在 Edit Mode 的 Workspace 设置/toolbar。
- 切换**不删除**另一模式的布局记录（两模式各存一份 `workspace_layouts`）。
- 首次切换用确定性算法生成目标模式初始 document：
  - structured→free：当前可见断点 pixel rect → world rect。
  - free→structured：按 y/x 排序 + Widget min size 生成无碰撞 Grid。
- 目标模式布局已存在时直接恢复，不反复重算；用户可切回原模式恢复之前布局。

---

## 4. DataBroker（key / 缓存 / 并发 / 有界 / subscription / pointer-move 0-IPC）

实现：`src/lib/workspace/widgets/data-broker.ts`（`WorkspaceDataBroker`）。

- **key 去重**：key 含 domain + query + workspace scope + time range + 相关 config；示例 `usage.summary:30d:timezone:project` / `files.recent:8` / `providers.status` / `apps.recent`。同 key 多实例 → 一次 fetch/subscription（in-flight promise 共享）。
- **缓存（LRU 有界）**：`MAX_CACHE_ENTRIES=64`；`MAX_LISTENERS_PER_KEY=32`（超限抛错）；`MAX_CONCURRENT_LOADS=6`；`DEFAULT_STALE_MS=15_000`。按 workspace 失效；**禁**无界 Map（standards technical/04 R-P9）。
- **stale / error 分离**：`BrokerSnapshot{key, status:'idle'|'loading'|'ready'|'error', data, error, refetching, lastUpdated}`；有旧数据且后台刷新 → `refetching`；不覆盖成空/成功（standards product/02 R-F1/R-F3）。
- **事件驱动刷新**：DB change / domain event → invalidate + refetch（不重建）；无订阅者时取消 in-flight 与 domain subscription（AbortController）。
- **pause**：document hidden / workspace inactive / hidden widget 暂停非必要刷新；`syncAll()` 只同步当前 workspace 活跃 entry。
- **错误分类**：raw Error → ClassifiedError（`userMessageKey/actionHintKey/retryable/diagnosticId`），不进 UI。
- **pointer-move = 0 IPC/0 DB write**：指针移动只改内存；drag/resize stop 才一次 layout transaction（standards ui-ux/02 R-U14、product/02 R-F6、contract §1）。

---

## 5. Widget Catalog 全清单

### 5.1 Definition 形状（`src/lib/workspace/widgets/registry.ts` / `types.ts`；type 唯一，注册冲突显式失败）

```ts
interface WidgetDefinition<TConfig, TData> {
  type: string; titleKey: string; descriptionKey: string;
  category: 'overview'|'productivity'|'files'|'apps'|'ai'|'usage';
  configVersion: number; defaultConfig: TConfig;
  configSchema: ZodSchema<TConfig>; migrateConfig?: (version, raw) => TConfig;
  layout: { supported: ('structured'|'free')[]; default; min; max; allowMultiple: boolean };
  presentation: { header:'always'|'auto'|'never'; surfaces: SurfacePolicy[]; defaultSurface };
  data?: { key: (cfg, ctx) => string; loader: (ctx) => Promise<TData> };
  Renderer: ComponentType<...>; Inspector?: ComponentType<...>;
}
```
状态机（WidgetHost）：`unregistered→unsupported`、`invalid config→last-valid/migration error`、`loading→skeleton`、`ready→renderer`、`empty→widget empty`、`error→classified+retry`、`stale→previous+refetch indicator`。

### 5.2 全 Catalog（id / 数据源 / view modes / config 校验 / 复合 / 处置）

现有 16 个注册 type：`greeting, today_usage, token_metrics, cost_metrics, recent_files, app_launcher, tool_status, ai_status, proxy_status, storage_overview, work_time, distribution_chart, data_view, notes, prompt_snippets, quick_links`。

| 分类 | Target id | 数据源（Domain query / broker key） | View modes | config 校验要点 | 复合 | 现有 type / 处置 |
|---|---|---|---|---|---|---|
| overview | `personal_hero` | workspace 名/问候 + 今日摘要 + 最高提醒（聚合，无 secret） | single | 无敏感字段 | 是（复合） | `greeting` 深化，保留 adapter |
| overview | `metric_overview` | 3–6 真实指标（today_usage/token/cost 聚合） | summary band | 指标数 3–6 | 是（聚合现有 adapter key，不重复查询） | 新增；复用 usage/token/cost adapter |
| overview | `activity_timeline` | 真实活动事件域 query | timeline | 无假活动 | 否 | 新增 |
| overview | `system_health` | 多项健康检查（端口/进程/代理/DB） | list | 标注估算/扫描时间 | 是（多检查） | 新增 |
| productivity | `quick_actions` | 用户配置动作（终端/建 workspace/开文件/启动 app/进 AI） | actions | 动作白名单 | 是（多动作） | `quick_links` 扩展为 Actions/Links 两 presentation |
| productivity | `notes` | workspace 内文本（用户文本按安全渲染） | list/edit | 多实例 | 否 | `notes` 保留，完善编辑/空态/多实例 |
| productivity | `prompt_snippets` | 用户 snippet 文本 | list/edit | 多实例 | 否 | `prompt_snippets` 保留 |
| productivity | `quick_links` | 用户链接（非敏感） | list | 多实例 | 否 | `quick_links` 保留 |
| productivity | `work_time` | usage 域工作时间/分布（统一 time range） | chart | time-aware | 否 | `work_time` 保留 |
| files | `recent_files` | files 域 recent（`files.recent:8`） | list | favorite/scope/limit | 否 | `recent_files` 加 favorite/scope/limit |
| files | `favorite_files` | files 域 favorite | list | limit | 否 | 新增（复用 files adapter） |
| files | `storage_overview` | disk 域用量（估算） | summary | 标注估算+扫描时间 | 否 | `storage_overview` 保留 |
| apps | `app_launcher` | apps 域（`apps.recent`） | grid | recent/favorite/running 模式 | 否 | `app_launcher` 加 recent/favorite/running |
| apps | `recent_apps` | apps 域 recent | list | limit | 否 | 新增（复用 apps adapter） |
| apps | `running_apps` | apps RuntimeInstance 状态 | list | 无裸 spawn | 否 | 新增 |
| ai | `ai_resource_status` | Provider/Connection/Key 健康（无 secret） | status | 不展示 Secret | 否 | `ai_status` 统一状态 ViewModel |
| ai | `ai_tool_status` | AI Tool detect/inspect 状态 | status | 无回滚写入 | 否 | `tool_status` 统一 |
| ai | `proxy_status` | proxy 域 health/key pool | status | 无 secret 明文 | 否 | `proxy_status` 保留 |
| ai | `model_connection_summary` | Provider/模型目录/连接 | summary | 不展示 Secret | 否 | 新增 |
| usage | `today_usage` | usage 域（`usage.summary:30d:...`） | metric | time-aware | 否 | `today_usage` 保留 |
| usage | `token_metrics` | usage 域 token | metric/chart | time-aware | 否 | `token_metrics` 保留 |
| usage | `cost_metrics` | usage 域 cost | metric/chart | time-aware；不假 cost | 否 | `cost_metrics` 保留 |
| usage | `usage_trend` | usage 域趋势 | chart | ≤3 主线（R-U2.8） | 否 | 新增（复用 usage adapter） |
| usage | `distribution_chart` | usage 域分布 | chart | 系列/分类受限 | 否 | `distribution_chart` 保留 |
| usage | `data_view` | 任意声明 source 的域（widget 级） | `list|table|board|calendar`（view mode，存 view_state） | 模式枚举校验 | 否（展示能力） | `data_view` 迁为 Widget；四模式经 `workspace_view_states` |

- **复合 Widget** 不重复查询：通过 DataBroker 共享现有 Domain adapter key（plan2 §7.3）。
- 每字段建 source matrix：`Widget|字段|Domain query|真实/估算|refresh/event|error category`；无来源字段删除或显 Unknown/Unavailable。
- Header 由 Definition 决定（`always/auto/never`），指标/Hero/Actions 可无 Header。

---

## 6. Template 系统（builtin 清单 + Classic 精确布局 + 表 + origin + 实例化/恢复/单 widget 重置）

### 6.1 Builtin 清单（`src-tauri/src/workspace/templates.rs`，代码 manifest 提供，origin=builtin，不可删）

- `classic-personal-dashboard`（Classic Personal Dashboard，**默认**，`workspace_create` 缺省实例化）
- `blank`（Blank）
- `focus`（Focus：greeting/notes/usage，强调行动）
- Personal：从当前 workspace 保存（origin=personal，`workspace_template_save`）。

### 6.2 Classic Personal Dashboard 精确布局（12 列，RGL `{i,x,y,w,h}`；实现 `builtin_templates`）

Widget（key → type）：`greeting→greeting, usage→today_usage, apps→app_launcher, recent→recent_files, ai→ai_status, notes→notes, storage→storage_overview, quick→quick_links`。

```text
lg（12 列）:
  greeting  x=0  y=0  w=8  h=2      usage   x=8  y=0  w=4  h=2
  apps      x=0  y=2  w=8  h=4      ai      x=8  y=2  w=4  h=4
  recent    x=0  y=6  w=6  h=4      notes   x=6  y=6  w=6  h=4
  storage   x=0  y=10 w=6  h=3      quick   x=6  y=10 w=6  h=3
md（8 列）:
  greeting  x=0 y=0 w=5 h=2  usage  x=5 y=0 w=3 h=2
  apps      x=0 y=2 w=8 h=4
  ai        x=0 y=6 w=4 h=4  recent x=4 y=6 w=4 h=4
  notes     x=0 y=10 w=8 h=4
  storage   x=0 y=14 w=4 h=3  quick x=4 y=14 w=4 h=3
sm（4 列，单列）:
  greeting  x=0 y=0  w=4 h=2
  usage     x=0 y=2  w=4 h=2
  apps      x=0 y=4  w=4 h=4
  ai        x=0 y=8  w=4 h=4
  recent    x=0 y=12 w=4 h=4
  notes     x=0 y=16 w=4 h=4
  storage   x=0 y=20 w=4 h=3
  quick     x=0 y=23 w=4 h=3
```
> 说明：plan2 §8.1 描述的是目标阅读节奏（Hero/Quick Actions/Metric Overview/…/Activity/System Health）；当前实现 manifest 以 8 个真实 widget 落 lg/md/sm。施工时以本表坐标为准，目标 composite widget（personal_hero/metric_overview/activity_timeline/system_health）落地后按 plan2 视觉重排但保持确定性 derive + 每断点独立保存。

### 6.3 Focus 精确布局

```text
lg: greeting(0,0,8,2)  usage(8,0,4,2)  notes(0,2,12,7)
md: greeting(0,0,5,2)  usage(5,0,3,2)  notes(0,2,8,7)
sm: greeting(0,0,4,2)  usage(0,2,4,2)  notes(0,4,4,7)
```
Blank：三断点均空数组。

### 6.4 Manifest 校验（`validate_manifest`）

- `schema_version` 必须 = 1，否则拒绝。
- 禁 Secret / 用户绝对路径 / 历史数据 / 实时指标：序列化后小写匹配禁止串 `<script, javascript:, secret, password, token", /users/, c:\\users\\`，命中即 `InvalidInput`。
- manifest 只含 widget 类型/config 默认/appearance/布局；实例化后归用户所有。

### 6.5 实例化 / 恢复 / 单 widget 重置（`apply_template` / `reset_widget`）

- **实例化/恢复**（`workspace_restore_template` / create 时）：单事务 `apply_template` —— 删 `workspace_widgets/workspace_layouts/workspace_view_states`（按 workspace_id）→ 为每个 manifest widget 生成新 `wgt` id（key→instance id 重写）→ 逐断点 `rewrite_layout_ids`（重写 `i/id/widgetId`）写 `workspace_layouts`（breakpoint=='free' → layout_mode='free'，否则 'structured'）→ 更新 `workspaces.default_layout_mode/appearance_json/template_source_id/template_version` → commit。
- **模板升级不覆盖**用户已实例化 workspace（`template_source_id/version` 仅溯源）；支持 Restore Entire Template（显式）与 Reset Selected Widget（`workspace_widget_reset`，按 widget_type 找 manifest spec 重置 config/appearance）。
- **另存个人模板**（`capture_workspace`）：仅取 `enabled` widget；key 重写为 `widget-{i}`；layout 同步重写；写 `workspace_templates`（origin=personal），重复 id → `template_version+1`；过 `validate_manifest`。

---

## 7. Revision Conflict 协议

- 所有 content mutation command 接受可选 `expectedRevision: number`（A-033 48-bit 内容指纹，= 上次 snapshot `revision`）；省略 = 不校验（向后兼容）。
- 事务内校验：`expectedRevision` 与当前 fingerprint 不一致 → **写入前**以结构化 `Conflict` 失败（不产生空成功/半写）。
- 成功返回新 snapshot / 新 revision；Renderer 用新 revision 更新本地 `expectedRevision`。
- 冲突恢复（plan2 §6.3）：保留本地编辑、拉取新 Snapshot、提供“重新应用 / 放弃”，**不静默覆盖**。
- 结构化错误集（Host workspace error）：`NotFound / InvalidInput / InvalidConfig / InvalidLayout / Conflict / UnsupportedWidget / MigrationRequired / DatabaseUnavailable / Internal(sanitized)`；Renderer 统一转 `userMessageKey/actionHintKey/retryable/diagnosticId`，不显示 raw SQL/路径/`invoke undefined`。
- 提交时机：pointer move=0 IPC/DB；drag/resize stop=1 layout txn；Inspector 字段=debounce/blur 1 config txn；add/remove/duplicate/reset=1 显式 txn；batch selection=1 batch txn；均带 `expectedRevision`。

---

## 8. Browse / Edit UX（显隐清单 / 进入退出 / 全部快捷键 / Undo-Redo 栈）

### 8.1 Browse Mode（默认安静，内容优先）

**显示**：Widget 内容 + 业务动作；右上角低干扰 Workspace 菜单 + “自定义工作空间”；可选 `Dashboard Filter`（time range，只影响 time-aware widget）；确有需要的手动刷新。
**隐藏（MUST 不显示）**：拖拽手柄、网格线、删除按钮、布局技术标签（Grid/Canvas/Breakpoint/z-index）、底部状态栏、常驻 Workspace Header/View Tab、Inspector、技术 badge、手动同步主控件（降为菜单恢复动作）。
- Canvas 本身不可拖动/缩放/选中；首屏第一视觉焦点是用户内容；10 秒内识别状态/重点/主要动作。

### 8.2 Edit Mode（用户主动进入）

**进入**：“自定义工作空间”入口 → 挂载 Floating Toolbar + Canvas 编辑边界 + Edit overlay（heavy module 此时才 mount）。
**Floating Toolbar**：添加 Widget / Undo / Redo / Structured-Free 切换 / Template / Reset / Workspace 外观 / 完成编辑。**不显示**数据库/Breakpoint/Collision 等内部信息。
**退出**：“完成”只退出编辑态（每个 terminal edit 已持久化，不制造未保存状态）→ 恢复内容优先 Dashboard；清理本次 Edit Session 的 undo/redo 历史。
**编辑 Chrome**：单击选中（克制 focus ring + resize handle）；drag 只从明确 drag area 或选中后触发；文本输入聚焦时不拦截 Delete/Backspace；多选 Free 完整、Structured 支持批量移动/删除。
**Contextual Inspector**：默认不挂载，仅 Edit + 有选中对象时出现；分 Content / Data / Appearance(白名单) / Layout(仅用户可理解字段) / Actions(复制/隐藏/删除/恢复默认)；字段由 Definition 声明并验证，不用通用字符串字段列表替代真配置表单；关闭不退出 Edit Mode。
**Widget Catalog**：可搜索 Drawer/Popover；分类 + 名称/说明/真实或结构预览 + 数据源与可用性 + 支持 Canvas 与尺寸 + 已添加数与是否允许多实例；添加后出现在当前视口首个合法位置并自动选中打开 Inspector。
**反馈**：删除 Widget → toast + Undo（领域数据不删）；删除 Workspace → 模态确认；禁 `alert/prompt/confirm`（standards ui-ux/02 R-U6）；冲突给明确恢复路径。

### 8.3 快捷键（全部；Cmd=mac / Ctrl=Win/Linux，`metaKey||ctrlKey`；输入聚焦不触发）

| 快捷键 | 作用 | 范围 |
|---|---|---|
| `Cmd/Ctrl+D` | 复制选中 Widget | Edit |
| `方向键` | 移动选中 Widget | Edit |
| `Shift+方向键` | 加速移动 | Edit |
| `Delete`/`Backspace` | 删除选中 Widget（文本聚焦不拦截） | Edit |
| `Escape` | 取消选择 / 关闭 Catalog·Inspector / 退出 Edit（视上下文） | Edit |
| `Cmd/Ctrl+Z` | Undo | Edit |
| `Cmd/Ctrl+Shift+Z`（或 `Cmd/Ctrl+Y`） | Redo | Edit |
| `Cmd/Ctrl+K` | 全局命令面板（Shell 层注册，不劫持系统键） | Global（standards R-U11/R-U12） |
| `Tab` / `Shift+Tab` | 阅读顺序导航（Canvas/Widget 正确 region/aria-label） | Global a11y |

### 8.4 Undo/Redo 栈

- 有界（bounded）Undo/Redo，仅本次 Edit Session 内有效；退出 Edit 清理历史。
- 不入持久化（UI-only state，同 selection / Catalog·Inspector 开关 / 临时 drag-resize state / browse-edit mode）。
- 栈元素 = terminal edit 的逆操作（add/remove/move/resize/config/duplicate/reset/switch-mode）；每步对应一次已持久化 transaction。
- 持久化 vs UI-only 分界：持久化 = appearance/layout mode、widget instance/config/appearance/enabled/z-index、structured/free layout document、Data View filter/sort/group/calendar state、free canvas camera(可选)；其余为 UI-only。

---

## 9. 主题（Dark Glow / Liquid Crystal token / 断点 / 缩放 / 无障碍）

### 9.1 Token（组件只消费语义 token，禁传 hex；standards ui-ux/01）

- 源：`src/app/styles/tokens.css`（CSS 语义变量）+ `src/lib/design-tokens.ts`（TS 结构常量 SPACING/FONT_SIZE/BORDER_RADIUS/TRANSITION/LAYOUT/SHADOW，不随主题变）+ `src/lib/theme-engine.ts`（Zod 校验 + 应用，`data-theme` 注入 `html`）。
- 主题内部 ID 固定 `dark`/`light`；旧名 `terminal-volt`/`frosted-jasmine` 只读兼容，运行时不再产生新值。
- 固定 15 级原始中性色阶 `--neutral-0..1000`（深浅同值，只改语义映射）；语义键深浅必须对齐（R-U2）。
- contract §7 语义键集（施工参考，以 tokens.css 实际为准）：`canvas/surface/surface-raised/surface-floating/surface-inset`、`material/material-glass/material-crystal`、`edge/edge-strong/edge-subtle`、specular highlight（standards §10.2 用 `--specular-highlight`）、`shadow-card/popup/modal/dragging`、`glow-focus/selected/data/status`、`text/text-body/secondary/tertiary/disabled`、`control-*`、`chart-volume-0..8 / chart-line / chart-area-fill / chart-grid`、`overlay(-soft/medium/strong)`、`motion-easing/transition-fast/normal/slow`。

### 9.2 Dark Glow

- 低反射深色 canvas（可极轻环境亮度变化）；四级表面 `base/raised/floating/inset` 可辨，不靠粗高亮框。
- **Glow 只属于 focus/selected/active data/status**，禁霓虹墙；border 用低对比 edge + 纯白顶层 Specular Highlight；图表克制 luminous line。

### 9.3 Liquid Crystal（禁 dark 机械反色，四强制法则）

1. 顶层 Specular Highlight：独立纯白高光线 token（`--specular-highlight`）。
2. 分层微阴影：`--shadow-card/popup/modal` 各 2–3 层低 alpha 扩散阴影，替代粗黑边框。
3. 文字层级：Primary/Body 深石墨灰（`#2E2E33` 系 / `--neutral-150` 映射），Secondary/Tertiary 中性灰，避免大面积纯黑。
4. 图表透气：浅色 area fill 单色/同色系水彩渐变，透明度约 15% → 0%。

### 9.4 Surface Policy（V-017/018/020；Definition 声明，禁全 Widget 强制同一玻璃卡）

`bare`（Metric/简洁文本/数据摘要）· `surface`（标准 Widget）· `raised`（Inspector/可操作面板）· `glass/crystal`（Hero/Toolbar/少量概览，局部 blur，无 WebGL）· `floating`（Catalog/Popover/Dialog）。Widget 级 `appearance_json` 白名单：`surfaceVariant / header / opacity`（禁颜色 token 入库）。

### 9.5 动效 / 缩放 / 断点 / 无障碍

- 动效统一 120/200/300ms token（fast/normal/slow，`cubic-bezier(0.16,1,0.3,1)`）；Browse 只留轻量进入/hover/数据更新；RGL transform 与 Framer Motion 不竞争（drag 活跃期 RGL 独占 transform）；`transform/opacity` 优先，禁长动效 >400ms。
- 断点：`lg/md/sm = 12/8/4`；`md` Activity/System 进正文流，`sm` 单列（Hero→Actions→Metrics→工作→次要状态）。
- 缩放：125%/150% 与窄窗口无材质破裂（阴影/高光不破裂，V-055）；Free zoom clamp 0.25–2.5。
- 无障碍：全部操作有键盘路径；Canvas/Widget 正确 region/aria-label；Edit Mode 宣告进入/退出；选中/状态/图表不只靠颜色（分类图表带标签+真实值，≤6 类+其他，折线 ≤3 主线）；`prefers-reduced-motion`/`prefers-reduced-transparency` 完整降级（spinner 停转/静态）；焦点环用 `:focus-visible`；正文对比度 WCAG AA 4.5:1（关键边界 ≥3:1）；中英文 125%/150% 无截断核心操作。
- 图标：全部 `lucide-react` SVG，**禁 Emoji**（R-U5.5）。

---

## 10. T00–T11 验收 bullet（对照 plan2 §15/16/17/18/19/21）

- **T00 决策与基线冻结**：standards/ADR/contract 无冲突（本报告裁决完成）；基线 packaged Tauri 视觉+性能可重复（1440×900 dark/light、1280×800、1024×768、125%/150%）；记录现有 query/IPC/drag write/bundle/FPS/RSS；数据备份与回滚路径明确。
- **T01 文档与规格**：`spec.md` + `t01-report.md` 落盘；矛盾裁决 standards>ADR>contract 全部记录；ADR/contract 一致（contract 升版保留旧版）。
- **T02 Host 领域收敛**：Host 单独完成 Workspace 全生命周期 + 模板实例化；不依赖 Renderer localStorage；迁移 fresh/v27/legacy home/重复/损坏 JSON/冲突 全 fixture 通过；幂等、加法、无数据丢失。
- **T03 Renderer 单权威**：重启后全部从 Host 恢复；清空 localStorage 不影响真实配置；Host 错误不转成功/空数组；browser dev 显式 Host unavailable 不冒充空数据；`workspacePersistence`/本地 View Snapshot/host-sync patch 删除。
- **T04 LayoutEngine**：两 adapter 过同一组 contract tests；pointer move DB write=0；structured↔free 来回切换可恢复原布局；lg/md/sm 确定性 derive。
- **T05 WidgetCatalog + Runtime**：任一 Widget 仅经 WidgetHost 运行；同 key 多实例一次 query/subscription；invalid config 可迁移或明确失败；六态（loading/ready/empty/error/unsupported/stale）覆盖。
- **T06 完整 Widget 组合**：Classic 模板所有区域由真实 widget 组成；source matrix 完整；无硬编码数值/假 app/假活动/假健康；空态构图完整+引导行动。
- **T07 Browse 重建**：默认首屏第一视觉焦点是用户内容；10 秒识别状态/重点/动作；Browse 不暴露编辑实现词汇；常驻 Chrome（View Tab/Inspector/状态栏/badge/主同步）删除。
- **T08 Edit 完整交付**：鼠标+键盘都能完成核心配置；任何操作重启后恢复；Undo/Redo 本次 Edit Session 内可靠且有界；Catalog→add→select→configure→persist 全链路；输入区不触发 drag/delete。
- **T09 Template 系统**：instantiate 单事务完成；template key→instance id 正确重写；模板不携带用户数据/Secret；模板升级不覆盖用户实例；restore/reset/save/rename/delete 通过。
- **T10 视觉/主题收尾**：dark/light 非机械反相；Dark Glow glow 限定；Liquid Crystal 四法则；125%/150% + 窄窗口无材质破裂；reduced-motion/transparency 完整；visual regression 矩阵（Theme×Viewport×Scale×DataState×Mode）通过。
- **T11 Legacy 删除 + 最终切换 + Gate**：localStorage authority/本地 View reducer/顶层 View Tab/假 canvas nodes/旧 tab 读路径删除；legacy 表/字段 death proof + 无 production 读写；新 Snapshot+新 Experience 同 release 切换无 silent fallback；`typecheck/lint/test/perf:check/cargo fmt --check/cargo test --workspace/protocol:check/verify:native-engine` + migration/visual/packaged Tauri/20 Widget soak 全通过。

---

## 【禁止清单】（MUST NOT，合并 plan2 + standards）

- **DB / 迁移**：禁 `DROP TABLE`/破坏式重建 v27 表（R-D3）；禁把 JSON/localStorage 当长期补丁；迁移禁重复创建 Widget（幂等）；禁静默 fallback（旧读路径删除，不留兜底）。
- **权威**：禁 IndexedDB/localStorage 成为 Workspace 权威（R-B10）；禁 Renderer 越过 DataBroker 直调 IPC 为 Widget 取数；禁 Widget 直读 SQLite / 理解 DB / Domain transport；禁双 Snapshot 模型/双写；禁把本地 View Tab 当 Workspace Tab；禁 `Grid|Canvas|Data` 顶层 View Tab 模型。
- **持久化**：pointer move 禁写 DB/IPC（0 次）；禁在 move 中放大 IO（R-U14）；config/layout/template 变更必走 typed IPC + expectedRevision；禁静默覆盖冲突（不空成功）。
- **Widget 边界**：禁 Widget Plugin Runtime / Worker / 第二 Event Bus / Marketplace / 在线市场 / 任意第三方代码执行；禁为 Widget 重写 Files/Apps/AI/Usage/Proxy 领域权威；禁多 Widget 各自轮询同一数据源（必须同 key 去重）；禁用通用字符串字段列表替代真配置表单。
- **数据真实性（R-F1/R-F2/R-F3，无假数据）**：禁用假数据撑满模板/空列表；禁虚构数字/活动/应用/健康；错误禁转空数组/零值/成功 toast；loading 禁显示 empty 态；估算值必须标注；无来源字段删除或显 Unknown/Unavailable。
- **Canvas**：禁无限画布 / CRDT / 多人实时协作 / Connector 绘图 / BlockSuite·Yjs / 通用建模器（R-U13）；Free 节点禁移出可恢复区域；zoom 禁无上限。
- **模板**：manifest 禁 Secret/token/Credential 明文/用户绝对路径/历史数据/实时指标；模板升级禁覆盖已实例化 Workspace。
- **安全（R-S 系列）**：禁 Secret 明文落盘/进 Renderer/事件/日志（R-S12：持久 Secret 归 OS Keychain，DB 只存 opaque ref）；template/config 进 Renderer/CSS 前必须 Zod+Rust 双侧校验；appearance 禁任意 CSS/HTML/hex 入库（只 semantic enum/number）；禁把主题编辑器做成任意 Hex/CSS 注入器；Widget 禁消费 Secret 明文。
- **UI/UX（R-U 系列）**：禁 `alert()/prompt()/confirm()`（R-U6）；禁 Emoji 作 UI 图标（R-U5.5）；禁魔法视觉值/硬编码 hex（R-U1）；禁全局 `outline:none`、焦点环必须 `:focus-visible`（R-U16）；禁 >400ms 非循环动效、禁自定义缓动曲线（R-U13/14）；禁全屏 WebGL/高成本 blur；禁所有 Widget 强制同一玻璃卡；Browse 禁显示拖拽手柄/网格线/删除按钮/技术标签/状态栏/常驻 Header·View Tab/Inspector。
- **性能（R-P 系列 / R-F6）**：禁主线程/同步命令 >16ms 阻塞（R-P2）；禁渲染期副作用（R-P3）；禁无界 Map/数组/Promise 队列（R-P9）；禁 >200 UI 项一次性建 DOM（R-P4）；重型能力禁进初始 JS（R-P7）；Home 禁按 widget 数重复相同 query/timer。
- **范围（plan2 非目标）**：禁跨设备云同步；禁无限画布/CRDT/多人协作/绘图工具；禁把早期首页做成硬编码固定页面；禁 V1/V2 主题长期并存；禁新旧 production path 长期共存。
