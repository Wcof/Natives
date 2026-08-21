# Workspace / Widget / Layout / Theme V2 —— 冻结契约（M-004 + M-006）

> 状态：**已冻结**（2026-08-21）。A/B/C 三方必须以本文类型名与字段为准开发；任何核心字段变更需经 Main Agent 批准并升版。
> 关联：ADR-0021、`docs/contracts/file-ownership.md`、`docs/contracts/reference-provenance.md`、方案 04/04A/06/07。

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
| `workspaces` | id TEXT PK, name, icon, appearance_json DEFAULT '{}', default_layout_mode CHECK(compact\|free), template_source_id NULL, created_at, updated_at, deleted_at NULL | soft delete；Close 不写这里 |
| `workspace_tabs` | workspace_id TEXT PK FK ON DELETE CASCADE, sort_order REAL, is_pinned INT, opened_at, last_active_at | row 存在=打开；Close=删 row；Reopen=插 row |
| `workspace_context_items` | id TEXT PK, workspace_id FK, kind CHECK(project_root\|pinned_file\|pinned_folder\|link\|note\|prompt_snippet\|usage_scope\|provider_profile\|proxy_profile), payload_json, sort_order, created_at, updated_at | 只存路径/引用/文本/非敏感配置；Credential 只存 opaque ref |
| `workspace_widgets` | id TEXT PK, workspace_id FK, widget_type, config_version INT, config_json, appearance_json, enabled INT, z_index INT, created_at, updated_at | appearance_json 只允许 surfaceVariant/header/opacity 等实例级外观（V-003 白名单），禁止颜色 token 入库 |
| `workspace_layouts` | workspace_id FK, layout_mode CHECK(compact\|free), breakpoint NULL, layout_version INT, layout_json, updated_at, PRIMARY KEY(workspace_id, layout_mode, breakpoint) | compact: lg/md/sm 各一份；free: breakpoint NULL，layout_json 存 world rect/parent frame/z-index |
| `workspace_view_states` | workspace_id FK, view_key, state_version INT, state_json, updated_at, PRIMARY KEY(workspace_id, view_key) | filter/sort/group/board/calendar/hidden fields/canvas camera 等 UI state，非业务数据 |
| `workspace_tool_profiles` | id TEXT PK, workspace_id FK, tool_kind, tool_ref, profile_json, enabled INT, sort_order REAL, created_at, updated_at | 无 secret 明文 |

- schema version 26→27；迁移单向、幂等；批量 layout 更新单事务提交。
- 旧 `settings:home_workspace`（schemaVersion 1）迁移为 Default Workspace：instance→workspace_widgets、lg/md/sm layouts→workspace_layouts(compact)、hidden 仅审计、建 tabs row 并设 active、写 `settings:home_workspace_migrated_at` + 来源 hash；旧 key 在 Final Gate 通过后才清理。幂等：已迁移标记存在时不得重复创建。

## 4. Rust 模块与 Tauri Command 契约（A 实现）

模块：`src-tauri/src/workspace/{model,repository,service,migration,snapshot,errors,mod}.rs` + `src-tauri/src/commands/workspace.rs`。

必须提供的 Host Service（command 只调用 service，不直连 DB）：

```
list_workspaces / get_workspace / create_workspace / update_workspace / delete_workspace
duplicate_workspace / open_workspace / close_workspace / pin_workspace
reorder_workspace_tabs / set_active_workspace
get_session_snapshot / get_workspace_snapshot
upsert_context_item / remove_context_item / batch_update_context_items（Wave2 A-032）
upsert_widget / remove_widget / batch_update_widget_configs
save_layout / get_layouts
save_view_state / get_view_state
upsert_tool_profile / remove_tool_profile
```

## 5. 前端 TS 契约（A-028 导出；C 消费）

`src/lib/workspace/contracts.ts`（前端唯一契约源，字段与 Rust serde 对齐）：

```ts
// 核心快照 —— 单 workspace 一次读取组成（A-020）
interface WorkspaceSnapshot {
  workspace: WorkspaceRecord;
  contextItems: WorkspaceContextItem[];
  widgets: WorkspaceWidgetRecord[];
  layouts: WorkspaceLayoutRecord[];
  viewStates: WorkspaceViewState[];
  toolProfiles: WorkspaceToolProfile[];
  revision: number;            // Wave2 A-033：stale reconcile
}

// Session 快照 —— inactive 不带完整 widgets（A-021）
interface WorkspaceSessionSnapshot {
  openedTabs: WorkspaceTabRecord[];
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
  supportedLayouts: ('compact' | 'free')[];
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
- `prefers-reduced-motion` / `prefers-reduced-transparency` 有确定性退化路径（V-005/V-035）。
- 125%/150% 缩放与窄窗口下材质、阴影、高光不破裂（V-055）。
