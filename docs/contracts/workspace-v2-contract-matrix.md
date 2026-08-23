# Workspace V2 — TS / Rust / SQLite / Frozen Contract 字段矩阵（G-006）

- **日期**: 2026-08-22
- **Task**: G-006（P0 · 依赖 G-002..004 · owner Main+A）
- **四源路径**:
  1. 冻结契约（最高权威）：`docs/contracts/workspace-v2-contract.md`（178 行，2026-08-21 冻结）
  2. TS：`src/lib/workspace/contracts.ts`（212 行，21 个 type/interface）
  3. Rust：`src-tauri/src/workspace/types.rs`（310 行，19 个 struct）
  4. SQLite：`src-tauri/src/db/migration_v27.rs` L7–100（7 个 CREATE TABLE）
- **权威顺序**: frozen contract > ADR-0021 > TS = Rust wire 对称 > SQLite 持久化形态
- **case 判定**: 全部 Rust DTO 均 `#[serde(rename_all = "camelCase")]` → wire case = camelCase；TS 字段即 wire 形态；SQLite 列名 = snake_case（DB case）。

> **总判定**：TS 与 Rust 的 7 实体字段逐一完全对称（wire case 一致）；但 **SQLite v27 实际 DDL 与冻结契约 §3 的表 schema 存在系统性偏离**（列名、列集、约束均不同），TS/Rust 实际对齐的是 v27 DDL 而非冻结契约文本。差异集中登记于 §9 MISMATCH 清单，归属 G-007/G-008 裁决（改契约文本 or 改 DDL/DTO）。

---

## 1. Workspace（`workspaces` 表 / `WorkspaceSummary`）

| 字段 | 冻结契约 | TS | Rust | SQLite | source | default | nullable | case | 状态 |
|---|---|---|---|---|---|---|---|---|---|
| id | id TEXT PK | id: string | id: String | id TEXT PRIMARY KEY | frozen | — | 否 | camelCase | OK |
| name | name | name: string | name: String | name TEXT NOT NULL | frozen | — | 否 | camelCase | OK |
| kind | —（契约未列） | kind: WorkspaceKind | kind: String | kind TEXT NOT NULL | SQLite | 'workspace' | 否 | camelCase | DB_ONLY（契约缺） |
| icon | icon | icon: string\|null | icon: Option\<String\> | icon TEXT | frozen | — | 是 | camelCase | OK |
| description | —（契约未列） | description: string\|null | description: Option\<String\> | description TEXT | SQLite | — | 是 | camelCase | DB_ONLY（契约缺） |
| theme | appearance_json DEFAULT '{}' | theme: WorkspaceTheme | theme: String | theme TEXT NOT NULL | 实际实现 | 'dark' | 否 | camelCase | M-01（见 §9） |
| isActive | — | isActive: boolean | is_active: bool | is_active INTEGER NOT NULL | 实际实现 | 0 | 否 | camelCase | DB_ONLY（契约缺；契约用 deleted_at 软删模型） |
| position | — | position: number | position: i64 | position INTEGER NOT NULL | 实际实现 | 0 | 否 | camelCase | DB_ONLY（契约缺） |
| createdAt | created_at | createdAt: string | created_at: String | created_at TEXT NOT NULL | frozen | — | 否 | camelCase | OK |
| updatedAt | updated_at | updatedAt: string | updated_at: String | updated_at TEXT NOT NULL | frozen | — | 否 | camelCase | OK |
| appearance_json | appearance_json DEFAULT '{}' | — | — | — | frozen | '{}' | 否 | snake_case(DB) | M-02：契约有、TS/Rust/DDL 三处均无 |
| default_layout_mode | CHECK(compact\|free) | — | — | — | frozen | — | 否 | snake_case(DB) | M-03：仅契约有（双布局字段缺失） |
| template_source_id | NULL（模板来源） | — | — | — | frozen | NULL | 是 | snake_case(DB) | M-04：仅契约有（复制/模板功能字段缺失） |
| deleted_at | NULL（软删） | — | — | — | frozen | NULL | 是 | snake_case(DB) | M-05：仅契约有（Close≠Delete 语义依赖此列） |

索引（SQLite 实有）：`idx_workspaces_active ON workspaces(is_active, position)`。

## 2. WorkspaceTab（`workspace_tabs` 表 / `WorkspaceTab`）

| 字段 | 冻结契约 | TS | Rust | SQLite | source | default | nullable | case | 状态 |
|---|---|---|---|---|---|---|---|---|---|
| id | —（契约以 workspace_id 为 PK） | id: string | id: String | id TEXT PRIMARY KEY | 实际实现 | — | 否 | camelCase | M-06（见 §9：PK 模型冲突） |
| workspaceId | workspace_id TEXT PK FK CASCADE | workspaceId: string | workspace_id: String | workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE | frozen | — | 否 | camelCase | OK |
| tabType | — | tabType: string | tab_type: String | tab_type TEXT NOT NULL | 实际实现 | — | 否 | camelCase | DB_ONLY（契约缺） |
| title | — | title: string | title: String | title TEXT NOT NULL | 实际实现 | '' | 否 | camelCase | DB_ONLY（契约缺） |
| refId | — | refId: string\|null | ref_id: Option\<String\> | ref_id TEXT | 实际实现 | — | 是 | camelCase | DB_ONLY（契约缺） |
| url | — | url: string\|null | url: Option\<String\> | url TEXT | 实际实现 | — | 是 | camelCase | DB_ONLY（契约缺） |
| position | sort_order REAL | position: number | position: i64 | position INTEGER NOT NULL | 实际实现 | 0 | 否 | camelCase | M-07（列名/类型 REAL vs INTEGER） |
| isActive | — | isActive: boolean | is_active: bool | is_active INTEGER NOT NULL | 实际实现 | 0 | 否 | camelCase | DB_ONLY（契约用 last_active_at 表达） |
| pinned | is_pinned INT | pinned: boolean | pinned: bool | pinned INTEGER NOT NULL | frozen | 0 | 否 | camelCase | OK |
| createdAt | — | createdAt: string | created_at: String | created_at TEXT NOT NULL | 实际实现 | — | 否 | camelCase | DB_ONLY（契约缺） |
| updatedAt | opened_at / last_active_at | updatedAt: string | updated_at: String | updated_at TEXT NOT NULL | 实际实现 | — | 否 | camelCase | M-08（契约双时间戳 vs 实现单 updated_at） |

索引（SQLite 实有）：`idx_workspace_tabs_workspace ON workspace_tabs(workspace_id, position)`。

语义对齐：契约「row 存在=打开；Close=删 row；Reopen=插 row」与实现的 tab row 模型一致（`workspace_tab_create/close/reorder` 命令存在）。

## 3. WorkspaceContextItem（`workspace_context_items` 表 / `WorkspaceContextItem`）

| 字段 | 冻结契约 | TS | Rust | SQLite | source | default | nullable | case | 状态 |
|---|---|---|---|---|---|---|---|---|---|
| id | id TEXT PK | id: string | id: String | id TEXT PRIMARY KEY | frozen | — | 否 | camelCase | OK |
| workspaceId | workspace_id FK | workspaceId: string | workspace_id: String | workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE | frozen | — | 否 | camelCase | OK |
| itemKind | kind CHECK(project_root\|pinned_file\|pinned_folder\|link\|note\|prompt_snippet\|usage_scope\|provider_profile\|proxy_profile) | itemKind: string | item_kind: String | item_kind TEXT NOT NULL | 实际实现 | — | 否 | camelCase | M-09（列名 kind→item_kind；CHECK 约束缺失，枚举未入库校验） |
| refId | —（契约为 payload_json 模型） | refId: string | ref_id: String | ref_id TEXT NOT NULL | 实际实现 | — | 否 | camelCase | M-10（契约 payload_json 单体 vs 实现 ref_id+meta 拆分） |
| title | — | title: string | title: String | title TEXT NOT NULL | 实际实现 | '' | 否 | camelCase | DB_ONLY（契约缺） |
| meta | payload_json | meta: Record<string, unknown> | meta: serde_json::Value | meta_json TEXT NOT NULL | 实际实现 | '{}' | 否 | camelCase | M-10 同上 |
| position | sort_order | position: number | position: i64 | position INTEGER NOT NULL | 实际实现 | 0 | 否 | camelCase | M-11（列名 sort_order→position） |
| createdAt | created_at | createdAt: string | created_at: String | created_at TEXT NOT NULL | frozen | — | 否 | camelCase | OK |
| updatedAt | updated_at | — | — | — | frozen | — | — | — | M-12（契约有、TS/Rust/DDL 均无；context item 不可 update，仅 batch patch title/meta/position 时 updated_at 语义缺失） |

索引（SQLite 实有）：`idx_workspace_context_workspace ON workspace_context_items(workspace_id, position)`。
安全对齐：契约「只存路径/引用/文本/非敏感配置；Credential 只存 opaque ref」与实现一致（refId 语义为 opaque host reference，types.rs L41 注释明确 never a secret）。

## 4. WorkspaceWidget（`workspace_widgets` 表 / `WorkspaceWidget`）

| 字段 | 冻结契约 | TS | Rust | SQLite | source | default | nullable | case | 状态 |
|---|---|---|---|---|---|---|---|---|---|
| id | id TEXT PK | id: string | id: String | id TEXT PRIMARY KEY | frozen | — | 否 | camelCase | OK |
| workspaceId | workspace_id FK | workspaceId: string | workspace_id: String | workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE | frozen | — | 否 | camelCase | OK |
| widgetType | widget_type | widgetType: string | widget_type: String | widget_type TEXT NOT NULL | frozen | — | 否 | camelCase | OK |
| config | config_json | config: Record<string, unknown> | config: serde_json::Value | config_json TEXT NOT NULL | frozen | '{}' | 否 | camelCase | OK |
| hidden | enabled INT | hidden: boolean | hidden: bool | hidden INTEGER NOT NULL | 实际实现 | 0 | 否 | camelCase | M-13（enabled vs hidden 反义列名；语义等价需 G-007 统一术语） |
| position | —（契约 widget 无排序列，z_index 表达） | position: number | position: i64 | position INTEGER NOT NULL | 实际实现 | 0 | 否 | camelCase | DB_ONLY（契约缺） |
| configVersion | config_version INT | — | — | — | frozen | — | 否 | snake_case(DB) | M-14：仅契约有（config 版本化缺失，migrateConfig 依赖此列） |
| appearance | appearance_json（V-003 白名单 surfaceVariant/header/opacity） | — | — | — | frozen | — | — | snake_case(DB) | M-15：仅契约有（WidgetInstance.appearance 在 TS/Rust 均无落点） |
| zIndex | z_index INT | — | — | — | frozen | — | 否 | snake_case(DB) | M-16：仅契约有（Free Canvas z-order 依赖此列） |
| createdAt | created_at | createdAt: string | created_at: String | created_at TEXT NOT NULL | frozen | — | 否 | camelCase | OK |
| updatedAt | updated_at | updatedAt: string | updated_at: String | updated_at TEXT NOT NULL | frozen | — | 否 | camelCase | OK |

索引（SQLite 实有）：`idx_workspace_widgets_workspace ON workspace_widgets(workspace_id, position)`。
关联契约 §6：WidgetInstance（appearance/enabled/zIndex/configVersion）与 WidgetDefinition（surfacePolicy 等）为 B 域 code-registered 结构，不落 SQLite 列即上述 M-14/15/16。

## 5. WorkspaceLayout（`workspace_layouts` 表 / `WorkspaceLayout`）

| 字段 | 冻结契约 | TS | Rust | SQLite | source | default | nullable | case | 状态 |
|---|---|---|---|---|---|---|---|---|---|
| id | —（契约 PK = workspace_id+layout_mode+breakpoint） | id: string | id: String | id TEXT PRIMARY KEY | 实际实现 | — | 否 | camelCase | M-17（见 §9：PK 模型冲突） |
| workspaceId | workspace_id FK | workspaceId: string | workspace_id: String | workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE | frozen | — | 否 | camelCase | OK |
| breakpoint | breakpoint NULL | breakpoint: WorkspaceBreakpoint | breakpoint: String | breakpoint TEXT NOT NULL | 实际实现 | — | 否（契约 NULL 表示 free 模式） | camelCase | M-18（free canvas 的 breakpoint NULL 表示法在实现中无落点） |
| layout | layout_json | layout: unknown[] | layout: serde_json::Value | layout_json TEXT NOT NULL | frozen | '[]' | 否 | camelCase | OK |
| isActive | — | isActive: boolean | is_active: bool | is_active INTEGER NOT NULL | 实际实现 | 1 | 否 | camelCase | DB_ONLY（契约缺） |
| createdAt | — | createdAt: string | created_at: String | created_at TEXT NOT NULL | 实际实现 | — | 否 | camelCase | DB_ONLY（契约缺） |
| updatedAt | updated_at | updatedAt: string | updated_at: String | updated_at TEXT NOT NULL | frozen | — | 否 | camelCase | OK |
| layoutMode | layout_mode CHECK(compact\|free) | — | — | — | frozen | — | 否 | snake_case(DB) | M-19：仅契约有（Grid/Canvas 双布局核心字段缺失） |
| layoutVersion | layout_version INT | — | — | — | frozen | — | 否 | snake_case(DB) | M-20：仅契约有 |

索引/约束（SQLite 实有）：`UNIQUE(workspace_id, breakpoint)` + `idx_workspace_layouts_workspace ON workspace_layouts(workspace_id)`。
**注意**：实际 UNIQUE 为 (workspace_id, breakpoint)，契约为 PRIMARY KEY (workspace_id, layout_mode, breakpoint)——同一 workspace 的 free 模式布局在实现 schema 下无处安放（M-18/M-19 的结构性后果）。

## 6. WorkspaceViewState（`workspace_view_states` 表 / `WorkspaceViewState`）

| 字段 | 冻结契约 | TS | Rust | SQLite | source | default | nullable | case | 状态 |
|---|---|---|---|---|---|---|---|---|---|
| id | —（契约 PK = workspace_id+view_key） | id: string | id: String | id TEXT PRIMARY KEY | 实际实现 | — | 否 | camelCase | M-21（PK 模型冲突，同 M-17 类型） |
| workspaceId | workspace_id FK | workspaceId: string | workspace_id: String | workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE | frozen | — | 否 | camelCase | OK |
| viewKey | view_key | viewKey: string | view_key: String | view_key TEXT NOT NULL | frozen | — | 否 | camelCase | OK |
| state | state_json | state: Record<string, unknown> | state: serde_json::Value | state_json TEXT NOT NULL | frozen | '{}' | 否 | camelCase | OK |
| updatedAt | updated_at | updatedAt: string | updated_at: String | updated_at TEXT NOT NULL | frozen | — | 否 | camelCase | OK |
| stateVersion | state_version INT | — | — | — | frozen | — | 否 | snake_case(DB) | M-22：仅契约有 |

索引/约束（SQLite 实有）：`UNIQUE(workspace_id, view_key)` + `idx_workspace_view_states_workspace ON workspace_view_states(workspace_id)`。
语义对齐：契约「filter/sort/group/board/calendar/hidden fields/canvas camera 等 UI state，非业务数据」——当前 Renderer 侧 Data View 展示态仍在 localStorage（`views/dataViewState.ts`），即此表的**消费方未接线**，属 C-021..025 收编范围（与 G-009 R3 一致）。

## 7. WorkspaceToolProfile（`workspace_tool_profiles` 表 / `WorkspaceToolProfile`）

| 字段 | 冻结契约 | TS | Rust | SQLite | source | default | nullable | case | 状态 |
|---|---|---|---|---|---|---|---|---|---|
| id | id TEXT PK | id: string | id: String | id TEXT PRIMARY KEY | frozen | — | 否 | camelCase | OK |
| workspaceId | workspace_id FK | workspaceId: string | workspace_id: String | workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE | frozen | — | 否 | camelCase | OK |
| profileId | —（契约为 tool_kind+tool_ref 模型） | profileId: string | profile_id: String | profile_id TEXT NOT NULL | 实际实现 | — | 否 | camelCase | M-23（契约 tool_kind/tool_ref vs 实现 profile_id/tool_key，模型名不同） |
| toolKey | — | toolKey: string\|null | tool_key: Option\<String\> | tool_key TEXT | 实际实现 | — | 是 | camelCase | M-23 同上 |
| config | profile_json | config: Record<string, unknown> | config: serde_json::Value | config_json TEXT NOT NULL | frozen（语义） | '{}' | 否 | camelCase | OK（列名 profile_json→config_json） |
| enabled | enabled INT | enabled: boolean | enabled: bool | enabled INTEGER NOT NULL | frozen | 1 | 否 | camelCase | OK |
| createdAt | created_at | createdAt: string | created_at: String | created_at TEXT NOT NULL | frozen | — | 否 | camelCase | OK |
| updatedAt | updated_at | updatedAt: string | updated_at: String | updated_at TEXT NOT NULL | frozen | — | 否 | camelCase | OK |
| sortOrder | sort_order REAL | — | — | — | frozen | — | 否 | snake_case(DB) | M-24：仅契约有 |

索引/约束（SQLite 实有）：`UNIQUE(workspace_id, profile_id)` + `idx_workspace_tool_profiles_workspace ON workspace_tool_profiles(workspace_id)`。
安全对齐：契约「无 secret 明文」与实现一致；`WorkspaceMcpExposure`（A-034）DTO 只携带 profile_id + tool_key 引用，注释明确 never config secrets / credential plaintext。

## 8. Input / Patch / Snapshot 聚合（简表）

| 类型 | 冻结契约 | TS | Rust | 对齐状态 |
|---|---|---|---|---|
| Create | —（§4 service 名 create_workspace） | WorkspaceCreateInput {name, kind?, icon?, description?, theme?} | WorkspaceCreateRequest（同字段，Option + serde(default)） | OK（wire camelCase；可选字段三源一致） |
| Update | — | WorkspaceUpdateInput {name?, icon?, description?, theme?, position?} | WorkspaceUpdateRequest（同字段，struct-level default；空串清列） | OK |
| TabInput | — | WorkspaceTabInput {tabType, title?, refId?, url?} | WorkspaceTabInput（同） | OK |
| TabUpdate | — | WorkspaceTabUpdateInput {title?, refId?, url?, isActive?, pinned?, position?} | WorkspaceTabUpdate（同） | OK |
| ContextItemInput | — | {itemKind, refId, title?, meta?} | 同 | OK |
| ContextItemPatch（A-032） | — | {id, title?, meta?, position?} | 同（meta 整对象替换） | OK |
| WidgetInput（upsert） | — | {id?, widgetType, config?, hidden?} | 同 | OK |
| WidgetConfigPatch（batch） | — | {id, config} | 同（config 整对象替换；缺失 widget 忽略） | OK |
| McpToolProfileExposure | —（A-034 后增） | {profileId, toolKey, enabled} | 同 | OK |
| WorkspaceMcpExposure | —（A-034 后增） | {workspaceId, contextItemKinds, widgetTypes, toolProfiles, layoutBreakpoints, generatedAt} | 同 | OK |
| WorkspaceSnapshot | §5：{workspace, contextItems, widgets, layouts, viewStates, toolProfiles, revision} | **多 `tabs: WorkspaceTab[]`**；revision 无（TS 无此字段） | {同契约 + tabs, revision: i64}（48-bit FNV-1a，A-033） | M-25（见 §9） |
| WorkspaceSessionSnapshot | §5：{openedTabs, activeWorkspaceId, workspaces(metadata only), revision} | {workspaceId, activeTabId, tabs, contextItems, widgets, layouts, viewStates, toolProfiles} | 同 TS + revision: i64 | M-26（见 §9：契约「inactive 不带完整 widgets + 多 workspace metadata 列表」的轻量 session 模型 vs 实现「单 workspace 全量集合」模型） |

命令面核对（frozen §4 的 24 个 service vs `commands/workspace.rs` 实际 27 个 `workspace_*` 命令）：
- 对齐（改名实现，语义 1:1）：`list_workspaces`→workspace_list、`get_workspace`→workspace_get、`create/update/delete/duplicate/open(→session_open)/close(→session_close)/set_active`、`get_session_snapshot`/`get_workspace_snapshot`、`upsert_context_item`→workspace_context_add、`remove_context_item`、`batch_update_context_items`→workspace_context_batch_update、`upsert_widget`→workspace_widget_upsert、`remove_widget`、`batch_update_widget_configs`→workspace_widget_batch_update、`save_layout`→workspace_layout_save、`save_view_state`→workspace_view_state_save、`upsert_tool_profile`→workspace_tool_profile_bind、`remove_tool_profile`→workspace_tool_profile_unbind。
- 实现新增（契约外）：workspace_context_reorder（A-032 语义，契约 §4 未列）、workspace_tab_create/close/reorder/update（契约 §3 表语义隐含、§4 service 列表缺）、workspace_mcp_exposure（A-034）。
- 契约有而命令面未见单列：`get_layouts`、`get_view_state`、`reorder_workspace_tabs`、`pin_workspace`——**待 A 组切片核实**是否由 session_open/snapshot 聚合返回覆盖（记为待验证项 V-1，不在 G-006 裁决范围）。

---

## 9. MISMATCH 清单（集中登记）

> 只记录、不裁决。裁决归属：术语/模型统一 → **G-007**；revision/冲突语义 → **G-008**；schema 迁移（改 DDL）→ A 组（A-001..）在裁决后执行。

### 9.1 结构性差异（TS=Rust 对称，但与冻结契约 §3 schema 偏离）

| 编号 | 字段/模型 | 差异 | 影响 | 建议归属 |
|---|---|---|---|---|
| M-01 | workspaces.theme | 实现为 `theme TEXT NOT NULL`（'dark'/'light'）；契约用 `appearance_json DEFAULT '{}'` | 主题持久化形态不同 | G-007 术语裁决 |
| M-02 | appearance_json | 仅契约有；TS/Rust/DDL 三处均无 | 无运行时影响（未被消费） | G-007：删除契约字段或补列 |
| M-03 | default_layout_mode | 仅契约有（CHECK compact\|free） | Grid/Canvas 双布局核心字段缺失 | G-007 + A 组补列 |
| M-04 | template_source_id | 仅契约有 | 复制/模板创建（产品目标）无落点 | G-007 + A 组补列 |
| M-05 | workspaces.deleted_at | 仅契约有（软删模型）；实现为硬删 | Close≠Delete 的 Delete 语义=物理删除；与契约"软删"冲突 | G-007 裁决 |
| M-06 | workspace_tabs PK | 实现 `id TEXT PK`；契约以 `workspace_id` 为 PK | 幂等/重开语义不同 | G-007 |
| M-07 | tab 排序列 | 实现 `position INTEGER`；契约 `sort_order REAL` | 无功能影响 | G-007 |
| M-08 | tab 时间戳 | 实现单 `updated_at`；契约 `opened_at`/`last_active_at` 双戳 | LRU 重开排序语义缺失 | G-007 |
| M-09 | context item 种类列 | 实现 `item_kind` 无 CHECK；契约 `kind` 有 9 值 CHECK | 脏数据可入库 | G-007 + A 组补 CHECK |
| M-10 | context item 载荷 | 实现 `ref_id NOT NULL + meta_json`；契约 `payload_json` 单体 | 字段读写路径不同 | G-007 |
| M-11 | context 排序列名 | 实现 `position`；契约 `sort_order` | 无功能影响 | G-007 |
| M-12 | context item updated_at | 仅契约有 | batch patch 无更新时间 | G-007 |
| M-13 | widget 显隐列 | 实现 `hidden INTEGER`；契约 `enabled INT`（反义） | 语义取反，易错 | G-007 术语统一 |
| M-14 | widget config_version | 仅契约有 | config 版本化/migrateConfig 无落点 | G-007 + A 组补列 |
| M-15 | widget appearance | 仅契约有（V-003 白名单） | Widget 流光色/透明度配置无落点（B 域依赖） | G-007 + A 组补列 |
| M-16 | widget z_index | 仅契约有 | Free Canvas z-order 无落点（C-026..032 依赖） | G-007 + A 组补列 |
| M-17 | workspace_layouts PK | 实现 `id TEXT PK + UNIQUE(workspace_id, breakpoint)`；契约 `PK(workspace_id, layout_mode, breakpoint)` | free 模式布局无处安放（结构性阻断 C-015..020） | G-007 裁决 + A 组迁移 |
| M-18 | layout.breakpoint | 实现 `NOT NULL`；契约 NULL 表示 free 模式 | 同 M-17 | G-007 |
| M-19 | layout.layout_mode | 仅契约有 | 双布局核心字段缺失 | G-007 + A 组补列 |
| M-20 | layout.layout_version | 仅契约有 | 布局版本化缺失 | G-007 |
| M-21 | view_states PK | 实现 `id PK + UNIQUE(workspace_id, view_key)`；契约 `PK(workspace_id, view_key)` | 无功能影响 | G-007 |
| M-22 | view state_version | 仅契约有 | 状态版本化缺失 | G-007 |
| M-23 | tool profile 模型 | 实现 `profile_id NOT NULL + tool_key NULL`；契约 `tool_kind + tool_ref` | MCP 暴露模型命名不同（A-034 依赖实现形态） | G-007 术语统一 |
| M-24 | tool profile sort_order | 仅契约有（REAL） | 无功能影响 | G-007 |
| M-25 | WorkspaceSnapshot.revision | 实现含 `revision: i64`（48-bit FNV-1a，A-033）；契约 §5 snapshot 亦列 revision —— 对齐；但 TS 端 snapshot 类型无 revision 字段（Rust-only） | 冲突检测（G-008）在 TS 侧无承载 | G-008 |
| M-26 | SessionSnapshot 模型 | 契约：`{openedTabs, activeWorkspaceId, workspaces[](metadata only), revision}`（轻量、多 workspace）；实现：单 workspace 全量集合 `{workspaceId, activeTabId, tabs, contextItems, widgets, layouts, viewStates, toolProfiles(+revision)}` | 多 Workspace 切换/重开的轻量 session 模型未实现（C-001..014 依赖） | G-008 + C 组 |

### 9.2 一致项确认（无差异，抽样记录）

- 7 实体主键/外键/workspace_id FK 级联：TS=Rust=DDL=契约 一致（camelCase↔snake_case 映射由 `rename_all="camelCase"` 统一）。
- Create/Update/Tab/Context/Widget/McpExposure 全部 Input/Patch DTO：TS 与 Rust 字段逐一 1:1，wire case = camelCase。
- 安全边界：ContextItem refId 为 opaque host reference、McpExposure 只携带引用——两源注释与契约"无 secret 明文"一致。

### 9.3 对后续任务的阻断判定

1. **C-015..020（Compact Grid）与 C-026..032（Free Canvas）被 M-17/M-18/M-19 结构性阻断**：free 模式布局在当前 DDL 下无存储位置 → A 组必须先裁决并迁移 schema。
2. **复制/模板（产品目标）被 M-04 阻断**。
3. **Data View 收编（C-021..025）被 M-21/M-22 软性影响**（表已存在且 UNIQUE 可用，消费方未接线）。
4. **冲突/revision 语义（G-008）被 M-25/M-26 影响**：revision 已在 Rust 快照中生成，TS 侧无字段承载。

---

## 10. 术语裁决（G-007，2026-08-22）

> **裁决原则**：实现（v27 DDL + TS/Rust 对称 DTO）= 当前运行事实，**以实现为准**；冻结契约 §3 文本按本表修订（文档工作随 A 组 DDL 迁移同步完成，避免二次不一致）。产品目标需要而实现缺失的结构性字段 → **A 组 schema 迁移清单**。本裁决**不改任何代码**（Wave 0 边界）。

### 10.1 三概念严格区分（固化为契约定义，见契约文档 §7）

1. **Workspace Tab** = 当前打开的 Workspace 的**会话**（row 存在=打开；Close=删 row；Reopen=插 row）。Tab 类型只含会话字段，**永不表示布局模式或数据视图模式**。
2. **布局模式（Grid/Canvas）** = 同一 Workspace 的两种互斥布局：`compact`（12 列磁吸 Compact Grid）| `free`（bounded DOM Free Canvas）。是 Workspace 级状态（`workspaces.default_layout_mode`，A 组补列），不是 Tab 属性。
3. **DataView 模式** = Data View Widget 的展示模式：`list | table | board | calendar`，经 `workspace_view_states`（view_key 作用域）持久化。与布局模式无关：同一 Workspace 可 free 画布 + board 模式 Data View 并存。

### 10.2 二十六项差异裁决

| 编号 | 字段/模型 | 裁决 | 执行方 |
|---|---|---|---|
| M-01 | workspaces.theme | 实现为准：`theme TEXT NOT NULL DEFAULT 'dark'`；契约 §3 改 | A 组同步文档 |
| M-02 | appearance_json | **删除**（实现未采用，无消费方）；契约 §3 删行 | A 组同步文档 |
| M-03 | default_layout_mode | **A 组迁移补列** `default_layout_mode TEXT NOT NULL DEFAULT 'compact'`（CHECK compact\|free） | A 组（C-015..020 前置） |
| M-04 | template_source_id | **A 组迁移补列** `template_source_id TEXT NULL` | A 组（复制/模板前置） |
| M-05 | workspaces.deleted_at | 实现为准：**硬删**（无软删）；契约软删表述删除，Delete=物理删除+级联 tab | A 组同步文档 |
| M-06 | tabs PK | 实现为准：`id TEXT PK + workspace_id NOT NULL` | A 组同步文档 |
| M-07 | tab 排序列 | 实现为准：`position INTEGER`；契约 sort_order REAL 删 | A 组同步文档 |
| M-08 | tab 时间戳 | 实现为准：单 `updated_at`；契约 opened_at/last_active_at 删 | A 组同步文档 |
| M-09 | context 种类列 | 实现为准 `item_kind`；**A 组迁移补 9 值 CHECK**（契约枚举保留为 DB 约束） | A 组 |
| M-10 | context 载荷 | 实现为准：`ref_id NOT NULL + meta_json`；契约 payload_json 模型删（ref_id=opaque host reference，无 secret） | A 组同步文档 |
| M-11 | context 排序 | 实现为准：`position INTEGER` | A 组同步文档 |
| M-12 | context updated_at | **A 组迁移补列**（batch patch 需写更新时间） | A 组 |
| M-13 | widget 显隐 | 实现为准：`hidden INTEGER DEFAULT 0`；术语统一为 **hidden**（契约 enabled 删） | A 组同步文档 |
| M-14 | widget config_version | **A 组迁移补列**（migrateConfig 依赖） | A 组 |
| M-15 | widget appearance | **A 组迁移补列** `appearance_json TEXT NOT NULL DEFAULT '{}'`（V-003 白名单） | A 组（B 组 Inspector 前置） |
| M-16 | widget z_index | **A 组迁移补列** `z_index INTEGER NOT NULL DEFAULT 0` | A 组（C-026..032 前置） |
| M-17 | layouts PK | **A 组迁移**：加 `layout_mode TEXT NOT NULL CHECK(compact\|free)` + `UNIQUE(workspace_id, layout_mode, breakpoint)` | A 组（最高优先，解除 C 组结构性阻断） |
| M-18 | breakpoint | 随 M-17：`breakpoint TEXT NOT NULL`，free 模式取固定值 `'free'`（避免 NULL 歧义） | A 组 |
| M-19 | layout_mode | = M-17 同一次迁移 | A 组 |
| M-20 | layout_version | **A 组迁移补列** | A 组 |
| M-21 | view_states PK | 实现为准：`id PK + UNIQUE(workspace_id, view_key)` | A 组同步文档 |
| M-22 | state_version | **A 组迁移补列**（可选项，DataView 收编时执行） | A 组 |
| M-23 | tool profile 模型 | 实现为准：`profile_id NOT NULL + tool_key NULL`（A-034 依赖实现形态）；契约 tool_kind/tool_ref 改 | A 组同步文档 |
| M-24 | tool profile sort_order | 实现为准：**无排序列**（无消费方）；契约行删 | A 组同步文档 |
| M-25 | TS snapshot revision | **已解决（G-008，2026-08-22）**：TS `WorkspaceSnapshot`/`WorkspaceSessionSnapshot` 补 `revision: number`，input 类型补 `expectedRevision?`，client 透传；测试 `revision_tests` 覆盖 stale→Conflict | G-008 已完成 |
| M-26 | SessionSnapshot 模型 | **C 组**（C-001..014）：按契约 §5 建多 Workspace 轻量 session 模型；现单 workspace 全量快照保留为 workspace 级 API | C 组 |

### 10.3 三概念类型化时机

| 概念 | TS 类型 | 落地时机 |
|---|---|---|
| Tab | `WorkspaceTab`（已存在，字段已是纯会话语义，无需改动） | 已满足 G-007 验收 |
| 布局模式 | `WorkspaceLayoutMode = 'compact' \| 'free'` | 随 C-015 双布局切片引入（无消费方不预置，AGENTS.md） |
| DataView 模式 | `DataViewMode = 'list' \| 'table' \| 'board' \| 'calendar'` | 随 C-021 Data View 切片引入（同上） |

### 10.4 汇总

- **实现为准（契约文本修订）**：M-01/02/05/06/07/08/10/11/13/21/23/24（12 项，文档工作随 A 组迁移同步）
- **A 组 schema 迁移（补列/约束）**：M-03/04/09/12/14/15/16/17/18/19/20/22（12 项，其中 M-17/18/19 最高优先——解除 Grid/Canvas 双布局结构性阻断）
- **G-008**：M-25（1 项）
- **C 组**：M-26（1 项）
