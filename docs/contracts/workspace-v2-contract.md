# Workspace / Widget / Layout / Theme V2 —— 冻结契约（M-004 + M-006）

> 状态：**已冻结**（2026-08-21）。A/B/C 三方必须以本文类型名与字段为准开发；任何核心字段变更需经 Main Agent 批准并升版。
> **PWSV2 升版（2026-08-23，Main Agent 裁决，见 ADR-0021 修订 §PWSV2 与 plan2 方案）**：布局模式词表统一 `structured | free`（历史词 `compact` 废弃，`structured` 为默认）；旧 `workspace_tabs`（内容 tab 表）降为 legacy，新增 `workspace_open_tabs`（Workspace 会话）与 `workspace_templates`（内置/个人模板）两张表；`workspaces` 软删 `deleted_at` + `default_layout_mode` + `appearance_json` + `template_source_id/template_version`；`workspace_widgets` 增 `config_version/appearance_json/enabled/z_index`（生产读写切 `enabled`，v29 映射 `enabled = NOT hidden`）；`workspace_layouts` 增 `layout_mode/layout_version`，唯一键 `UNIQUE(workspace_id, layout_mode, breakpoint)`，free 模式 breakpoint 取保留值 `'free'`；`workspace_view_states` 增 `state_version`；`workspace_session_snapshot` 为全局轻量模型。matrix §10 的 12 项「A 组 schema 迁移」由增量迁移 **v29**（v28 已被应用中心占用）完成，本文 §3/§4/§5/§7 文本已同步修订。
> **Wave2 冻结确认（I-008，2026-08-21）**：核心字段不再变更；A/B/C Wave2 并行开发。新增能力只做加法：
> A-032 context reorder/batch、A-033 snapshot revision/version、A-034 MCP exposure DTO（不实现 Agent runtime）、
> B-030..B-036 新 metrics/distribution widgets、C-033..C-038 Frame/Group/z-order/canvas keyboard/picker 集成。
> 关联：ADR-0021、`docs/contracts/file-ownership.md`、`docs/contracts/reference-provenance.md`、方案 04/04A/06/07。
> **PWSV2-T01 一致性修订（2026-08-23，T01 文档裁决；仅文本一致性、无字段语义变更、加法性，保留全部既有冻结规则）**：
> - 清除 §6 `WidgetDefinition.supportedLayouts` 与 §3 legacy-home 迁移描述中残留的历史词 `compact`，统一为 `structured | free`（与本文件 PWSV2 升版 §0 词表裁决一致）。
> - 事件命名澄清：Host 事件经 `db-state-changed`（channel `workspace`）/ `workspace` 广播（standards technical/02 R-S9）；plan2 的 `workspace:snapshot-changed` 等自造名**不**作为独立 transport，仅为语义别名。
> - 旧版冻结文本（M-004/M-006、Wave2 I-008、G-007 三概念、V-005 token taxonomy）全部保留，未删除。

## 1. 总体权威链

```
SQLite / Local Files -> Domain Service -> Typed IPC -> Renderer Snapshot/UI
```

- 禁止 IndexedDB / localStorage 成为 Workspace 权威。
- Widget 禁止直接读 SQLite；Renderer 只拿 config/data/loading/error/actions。
- 相同 adapter key 由 WorkspaceDataBroker 去重共享一次查询/订阅。
- 指针移动只写内存；stop/debounce/explicit flush 才持久化。

## 2. 前端依赖冻结（M-006）

只使用现有：React 19 / Framer Motion / react-grid-layout@2.2.4 / Recharts@3 / Zod@3 / lucide-react。
**不新增**前端状态库、图表库、拖拽库。`liquid-glass-react` 仅当 `src/components/ui/LiquidGlass.tsx` 无生产引用时允许移除（B-D04，由 Main 应用）。

## 3. SQLite v27 表契约（A 实现，字段名 serde/ts-rs 命名一致）

| 表 | 关键字段 | 语义 |
|---|---|---|
| `workspaces` | id TEXT PK, name, kind, icon, description, theme CHECK(dark\|light) DEFAULT 'dark', position INT, appearance_json DEFAULT '{}', default_layout_mode CHECK(structured\|free) DEFAULT 'structured', template_source_id NULL, template_version NULL, created_at, updated_at, deleted_at NULL | 软删（list/snapshot 过滤 `deleted_at`）；Close 不写这里 |
| `workspace_open_tabs`（v29 新表） | workspace_id TEXT PK FK ON DELETE CASCADE, sort_order REAL, is_pinned INT, opened_at, last_active_at | row 存在=打开；Close=删 row；Reopen=插 row。实现中同名的旧 `workspace_tabs`（tab_type/title/ref_id/url 内容 tab 表）PWSV2 起降为 legacy 表，v29 后无 production 读/写，death proof 后删除 |
| `workspace_context_items` | id TEXT PK, workspace_id FK, kind CHECK(project_root\|pinned_file\|pinned_folder\|link\|note\|prompt_snippet\|usage_scope\|provider_profile\|proxy_profile), payload_json, sort_order, created_at, updated_at | 只存路径/引用/文本/非敏感配置；Credential 只存 opaque ref |
| `workspace_widgets` | id TEXT PK, workspace_id FK, widget_type, config_version INT DEFAULT 1, config_json, appearance_json DEFAULT '{}'（V-003 白名单：surfaceVariant/header/opacity）, enabled INT DEFAULT 1, z_index INT DEFAULT 0, position INT, created_at, updated_at | 生产读写用 `enabled`（v29 迁移映射 `enabled = NOT hidden`，切换后不再读写 `hidden`，该列保留至 death proof）；appearance 只允许实例级外观，禁止颜色 token 入库 |
| `workspace_layouts` | id TEXT PK, workspace_id FK, layout_mode CHECK(structured\|free) NOT NULL DEFAULT 'structured', breakpoint TEXT NOT NULL（free 模式取保留值 `'free'`）, layout_version INT DEFAULT 1, layout_json DEFAULT '[]', UNIQUE(workspace_id, layout_mode, breakpoint), updated_at | structured: lg/md/sm 各一份；free: breakpoint='free'，layout_json 存 world rect/camera/parent frame/z-index |
| `workspace_view_states` | id TEXT PK, workspace_id FK, view_key, state_version INT DEFAULT 1, state_json DEFAULT '{}', updated_at, UNIQUE(workspace_id, view_key) | filter/sort/group/board/calendar/hidden fields/canvas camera 等 UI state，非业务数据；Data View 模式（list|table|board|calendar）是 Data View Widget 的展示模式，按 view_key 作用域持久化于此 |
| `workspace_tool_profiles` | id TEXT PK, workspace_id FK, tool_kind, tool_ref, profile_json, enabled INT, sort_order REAL, created_at, updated_at | 无 secret 明文 |
| `workspace_templates`（v29 新表） | id TEXT PK, name, origin CHECK(builtin\|personal), schema_version INT, template_version INT, manifest_json TEXT NOT NULL, preview_key TEXT, created_at, updated_at, deleted_at TEXT | Built-in manifest 由代码提供（Classic Personal Dashboard 默认 / Blank / Focus）；表存 personal 模板与 built-in metadata override，Host 对两者暴露统一 read model；Manifest 禁止 Secret、用户绝对路径、历史数据与实时指标 |

- schema version 26→27（已落地）；PWSV2 增量 27→29（v28 为应用中心迁移）：只增列/增表、幂等、`PRAGMA table_info` guard，禁止 DROP/重建；批量 layout 更新单事务提交；旧读路径（`workspace_tabs`、`hidden`）在新 Snapshot 验证通过后同一切片删除，不保留静默 fallback。
- 旧 `settings:home_workspace`（schemaVersion 1）迁移为 Default Workspace：instance→workspace_widgets、lg/md/sm layouts→workspace_layouts（layout_mode='structured'）、hidden 仅审计、建 tabs row 并设 active、写 `settings:home_workspace_migrated_at` + 来源 hash；旧 key 在 Final Gate 通过后才清理。幂等：已迁移标记存在时不得重复创建。

## 4. Rust 模块与 Tauri Command 契约（A 实现）

模块：`src-tauri/src/workspace/{model,repository,service,migration,snapshot,errors,mod}.rs` + `src-tauri/src/commands/workspace.rs`。

必须提供的 Host Service（command 只调用 service，不直连 DB）：

```
list_workspaces / get_workspace / create_workspace / update_workspace / delete_workspace（软删）
duplicate_workspace / open_workspace / close_workspace / reopen_workspace / pin_workspace
reorder_workspace_tabs / set_active_workspace
get_session_snapshot（全局轻量：openedTabs + workspaces metadata + revision）/ get_workspace_snapshot（单 Workspace 全量读模型，不含内容 tab）
upsert_context_item / remove_context_item / batch_update_context_items（Wave2 A-032）
upsert_widget / remove_widget / batch_update_widget_configs
save_layout / get_layouts
save_view_state / get_view_state
upsert_tool_profile / remove_tool_profile
list_templates / get_template / instantiate_template / save_template_from_workspace /
update_template / delete_template / restore_workspace_from_template / reset_widget_from_template
```

所有 mutation 接受 `expectedRevision`（A-033 内容指纹），事务内校验后写入，成功返回新 snapshot/revision；stale 值在写入前以结构化 `Conflict` 失败。`workspace_tab_create/update/close/reorder` 命令族随旧 View Tab Strip 同一切片删除（PWSV2）。

## 5. 前端 TS 契约（A-028 导出；C 消费）

`src/lib/workspace/contracts.ts`（前端唯一契约源，字段与 Rust serde 对齐）：

```ts
// 核心快照 —— 单 Workspace 全量读模型（A-020；PWSV2：不含内容 tab）
interface WorkspaceSnapshot {
  workspace: WorkspaceRecord;       // 含 defaultLayoutMode / appearance / templateSourceId / templateVersion
  contextItems: WorkspaceContextItem[];
  widgets: WorkspaceWidgetRecord[]; // configVersion / appearance（V-003 白名单）/ enabled / zIndex
  layouts: WorkspaceLayoutRecord[]; // layoutMode (structured|free) + layoutVersion；free breakpoint='free'
  viewStates: WorkspaceViewState[]; // stateVersion；Data View 模式与 canvas camera
  toolProfiles: WorkspaceToolProfile[];
  revision: number;            // A-033：48-bit 内容指纹，回传为 expectedRevision
}

// Session 快照 —— 全局轻量模型（A-021 / M-026 裁决）：inactive Workspace 不携带完整 widgets/layouts
interface WorkspaceSessionSnapshot {
  openedTabs: WorkspaceOpenTabRecord[];  // workspaceId(PK) / sortOrder / isPinned / openedAt / lastActiveAt
  activeWorkspaceId: string | null;
  workspaces: WorkspaceRecord[];   // 仅 metadata
  revision: number;
}
```

Renderer 侧新增 `src/lib/workspace/{client,events,session-store,snapshot-store}.ts`：
- `client.ts`：UI 不直接 db.get/set，只经 client facade 调 workspace commands。
- `events.ts`：只发 workspace 域事件。
- 切换 Workspace：先显示 last snapshot cache，再异步 reconcile Host 最新 snapshot（无白屏）。

## 6. Widget Contract（B 实现，code-registered，非 Plugin Runtime）

```ts
type SurfacePolicy = 'bare' | 'surface' | 'raised' | 'glass' | 'crystal' | 'floating';

interface WidgetDefinition<TConfig, TData> {
  type: string;
  titleKey: string;
  configVersion: number;
  defaultConfig: TConfig;
  configSchema: ZodSchema<TConfig>;
  migrateConfig?: (version: number, raw: unknown) => TConfig;
  size: { default: Size; min: Size; max: Size };
  supportedLayouts: ('structured' | 'free')[];
  surfacePolicy: SurfacePolicy[];
  Renderer: ComponentType<WidgetRenderProps<TConfig, TData>>;
  Inspector?: ComponentType<WidgetInspectorProps<TConfig>>;
  adapter?: WidgetDataAdapter<TConfig, TData>;
}

interface WidgetInstance {   // 与 Rust WidgetRecord 对应
  id: string; workspaceId: string; widgetType: string;
  config: TConfig; appearance: { surfaceVariant?: SurfacePolicy; header?: boolean; opacity?: number };
  enabled: boolean; zIndex: number; configVersion: number;
}
```

- Registry：`src/lib/workspace/widgets/registry.ts`，type 唯一校验，注册冲突显式失败。
- Config：`src/lib/workspace/widgets/config.ts`，Zod validate/migrate，非法配置回退 last-valid。
- DataBroker：`src/lib/workspace/widgets/data-broker.ts`，adapter key 去重/缓存/共享订阅。
  key 示例：`usage.summary:30d:timezone:project` / `files.recent:8` / `providers.status` / `apps.recent`。
- Renderer：`src/components/workspace/widgets/WidgetRenderer.tsx`，状态机 loading/error/ready，不直连 IPC。
- WidgetShell：`WidgetShell.tsx`，surfacePolicy/header/edit chrome，**禁止所有 Widget 强制同一种玻璃卡**。

首批 7 个迁移 Widget：Greeting / Recent Files / App Launcher / Today Usage / AI Status / Storage Overview / Token Metrics（B-021..B-028）。

## 7. Design System V2 Token Taxonomy（V-005 冻结；B 实现 token，C 消费）

主题内部 ID 固定 `dark/light`；旧名（terminal-volt / frosted-jasmine）只读兼容，运行时不再产生新值（V-002/V-004）。

### 语义键（dark/light 必须对齐，CSS 变量 `var(--x)`）

```
canvas / surface / surface-raised / surface-floating / surface-inset
material / material-glass / material-crystal
edge / edge-strong / edge-subtle / highlight-specular / highlight-edge
shadow-card / shadow-popup / shadow-modal / shadow-dragging
glow-focus / glow-selected / glow-data / glow-status
text / text-body / text-secondary / text-tertiary / text-disabled
control-bg / control-bg-hover / control-fg / control-selected-bg / control-selected-fg
chart-volume-0..8 / chart-line / chart-area-fill / chart-grid
overlay / overlay-soft / overlay-medium / overlay-strong
motion-easing / transition-fast / transition-normal / transition-slow
```

### Dark Glow（V-008/V-009）

- 低反射深色 canvas，允许极轻环境亮度变化；base/raised/floating/inset 四级 Surface 可辨。
- Glow 只属于 focus / selected / active data / status；克制，禁止霓虹墙。
- Border 优先低对比 edge + highlight，不给每张卡画亮框。
- 数据可视化可有克制 luminous line。

### Liquid Crystal 四条强制法则（V-010..V-013，禁止 dark 机械反色）

1. **顶层 Specular Highlight**：`--highlight-specular` 独立纯白高光线 token，用于晶透卡片/浮层顶部。
2. **分层微阴影**：`--shadow-card/popup/modal` 各含 2–3 层低 alpha 扩散阴影，替代粗黑边框。
3. **文字层级**：Primary/Body 深石墨灰（如 `#2E2E33` 系），Secondary/Tertiary 中性灰，避免大面积纯黑。
4. **图表透气感**：浅色 area fill 使用单色/同色系水彩渐变，透明度约 15% → 0%。

### Surface Policy（V-017/V-018/V-020）

- bare：代码、表格、文件正文、密集列表 —— 稳定不透明。
- surface：标准卡片/面板。
- raised：Inspector/编辑工具。
- glass/crystal：高价值概览、toolbar、小型 metric —— 局部 blur，无 WebGL。
- floating：Popover/Dialog/Command Palette。

组件只消费语义 token，不接受“传 hex 就换色”的 API（R-U1 升级为 V2 语义）。

## 8. Motion 约束（Framer × RGL 冲突规则）

- RGL 活跃 drag/resize 期间：RGL 独占 transform，Framer Motion **不得**机械套到 grid item。
- drag stop 后过渡只作用于 wrapper / 非 transform property。
- Free Canvas：pointer move 由 Canvas engine 驱动；Motion 只用于 selection chrome / inspector。
- Tab / Inspector / Picker / 非拖拽 card mount/unmount 可用 Motion。

## 9. 性能与可访问性

- 无全屏高成本 blur / WebGL；drag 可降级。
- `prefers-reduced-motion` / `prefers-reduced-transparency` 有确定性退化路径（V-005/V-035

## 7. 三概念严格区分（G-007 术语裁决，2026-08-22）

> 本契约与所有后续实现中，以下三概念必须严格区分，禁止互相借用：

1. **Workspace Tab = 打开的 Workspace 会话**（v29 起由 `workspace_open_tabs` 承载）。Tab row 存在 ⇔ 该 Workspace 处于打开状态；Close = 删除 row；Reopen = 插入 row。会话 tab 只承载会话字段（workspaceId PK / sortOrder / isPinned / openedAt / lastActiveAt），**永不表示布局模式或数据视图模式**。旧 `workspace_tabs` 的 tabType/title/refId/url 是内容 tab 字段，v29 起不再是 Workspace 会话语义。
2. **布局模式（Structured/Free）= Workspace 级互斥状态**：`structured`（12 列磁吸 Structured Grid，默认）与 `free`（bounded DOM Free Canvas）。词表 PWSV2（2026-08-23）统一为 `structured | free`，历史词 `compact` 废弃。持久化于 `workspaces.default_layout_mode` 与 `workspace_layouts.layout_mode`（v29 迁移落地）。同一时刻一个 Workspace 只有一种激活布局模式，可持久化切换。
3. **DataView 模式 = Data View Widget 的展示模式**：`list | table | board | calendar`。经 `workspace_view_states`（按 view_key 作用域）持久化，与布局模式正交——同一 Workspace 可同时为 free 画布布局 + board 模式 Data View。

类型化时机（无消费方不预置）：`WorkspaceTab` 已满足定义 1；`WorkspaceLayoutMode` 随 Compact Grid 切片（C-015..020）引入；`DataViewMode` 随 Data View 切片（C-021..025）引入。

**schema 裁决**：本文件 §3 表结构与实现的偏离（TS/Rust/SQLite/frozen 四源矩阵 M-01..M-26）按 `workspace-v2-contract-matrix.md` §10 裁决执行：以实现为准的 12 项由 A 组同步修订本文档文本；A 组 schema 迁移的 12 项（含 M-17/18/19 layout_mode——解除 Grid/Canvas 双布局结构性阻断，最高优先）在迁移完成后同步修订本文档。）。
- 125%/150% 缩放与窄窗口下材质、阴影、高光不破裂（V-055）。
