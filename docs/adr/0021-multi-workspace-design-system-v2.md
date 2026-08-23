# ADR-0021: Multi-Workspace + 双布局 + Design System V2（草稿）

- **状态**: 已接受（Accepted）——全面替代 ADR-0020 对应段落
- **日期**: 2026-08-21
- **决策者**: 产品方（用户）
- **取代（范围）**: ADR-0020 §1「首页就是 V1 唯一 Personal Workspace Home；不新增 Workspace 一级菜单、Workspace CRUD 或多工作台数据域」、ADR-0020 §3「Home 布局复用 settings K/V 单份版本化 JSON」与「禁止 Infinite Canvas 与嵌套容器」中与本文冲突的部分
- **保留**: ADR-0020 的 Host authority、Agent/Assistant/Harness/Planner/Subagent Runtime/Capability Gateway/Jobs/Plugin Runtime 停止建设、Secret 归 OS Keychain、领域边界（Files/Apps/Provider/Proxy/AI Tool/Usage）、旧 iframe 安全防线（ADR-0001/0002/0006）等全部决策
- **关联**: `docs/standards/`、`docs/contracts/workspace-v2-contract.md`、`docs/contracts/file-ownership.md`、`docs/contracts/reference-provenance.md`、方案 V2（`AiNative-Full-Implementation-Plan-V2-20260821`）

## 上下文

ADR-0020 将首页固定为「唯一 Personal Workspace Home」，Widget 固定为「单份版本化 JSON + 版本化 grid 布局」，并禁止 Workspace CRUD 与无限画布。产品演进目标要求：以 **Workspace 组织项目上下文、文件、应用、AI 工具、Provider/Proxy、Usage 与生产力组件**；提供 **Compact Grid 与 Free Canvas 双布局**；并以 **Dark Glow / Liquid Crystal 双材质设计系统（Design System V2）**承载自由可视化空间。这需要在保留 ADR-0020 全部权威边界（Host authority、Agent Death、Secret、领域复用）的前提下，放开 Workspace 实体与自由画布的范围限制。

## 决策

### 1. Multi-Workspace（取代 ADR-0020 §1 与 §3 的“单 Home 文档”部分）

- 引入 `workspaces` 等 7 张表的 SQLite v27 迁移（见 `docs/contracts/workspace-v2-contract.md`）。
- 每个 Workspace 是长期场景容器：Create / Open / Close / Reopen / Pin / Reorder / Rename / Duplicate / Template / Delete；`Close` ≠ `Delete`。
- Workspace 只保存对 Files/Apps/Usage/Provider/Proxy 等领域的**引用或 View State**，不跨域复制业务数据。
- 旧 `settings:home_workspace`（schemaVersion 1）通过幂等迁移成为 `Default Workspace`；迁移标记与来源 hash 落库；旧 key 在 Final Gate 通过后才清理。
- 数据权威仍是 `SQLite / Local Files -> Domain Service -> Typed IPC -> Renderer Snapshot/UI`；IndexedDB / localStorage 不得成为 Workspace 权威。

### 2. 双布局（取代 ADR-0020 §3「禁止 Infinite Canvas 与嵌套容器」）

- **Structured Canvas**（历史名 Compact Grid，PWSV2 词表见修订 §10）: 继续使用 `react-grid-layout`，lg/md/sm = 12/8/4 断点；stop/debounce/flush 才持久化，指针移动只写内存。
- **Free Canvas**: 自研轻量 DOM Canvas，只实现 Workspace 需要的自由布局：Drag / Resize / Pan / Zoom / Snap / Multi-select / Group / Frame / Z-order。
- **明确不做**：绘图工具、Connector、CRDT、BlockSuite/Yjs、多人协同、通用任意实体建模器。
- 布局数据仍是普通 `workspace_layouts` 记录，不引入新 runtime。

### 3. Widget Framework 升级（保留 ADR-0020 “不是 Plugin Runtime”边界）

- 从 `WidgetDescriptor + hard map` 升级为代码注册的一方内置 Widget：Definition / Renderer / Inspector / Data Adapter / Versioned Config / Layout Capability / Surface Policy。
- 仍是代码注册的 React renderer，**不是** Plugin Runtime / Worker / Event Bus / Package Manager / Marketplace。
- 相同 adapter key 由 `WorkspaceDataBroker` 去重共享查询，禁止多个 Widget 各自轮询同一数据源。

### 4. Design System V2（新增，约束见 `docs/standards/ui-ux/`）

- **Dark Glow（暗黑流光）**：低反射深色 canvas、四级 Surface（base/raised/floating/inset）、glow 只属于 focus/selected/active data/status、低对比 edge + highlight。
- **Liquid Crystal（晶透液态/极净霜白）**：独立浅色体系，**禁止 dark 机械反色**。四条强制法则：顶层 Specular Highlight、多层微阴影替代粗边框、深石墨灰/中性灰文字层级、图表约 15%→0% 透明面积渐变。
- 组件只消费语义 token，不接受“传 hex 换色”的 API；旧主题名（terminal-volt/frosted-jasmine）只读兼容，运行时不再产生新值。
- 全部页面、组件、弹窗、浮层、图表、编辑态、空态、错误态与边缘流程统一迁移，禁止 V1/V2 长期并存。
- 性能：不使用全屏 WebGL LiquidGlass；blur 局部使用；reduced motion / reduced transparency 有确定退化路径。

### 5. Legacy Death 范围不变

- 生产态 Agent Runtime / Assistant / Harness / Planner / Subagent Runtime / Capability Gateway / Jobs 自主任务 / Plugin Runtime 不复活；现有旧代码只允许安全、迁移、删除与 parity 工作。
- 迁移完成前保持单一 production path；共享文件（package.json / 根 Cargo / RootClient / ShellLayout / handler_registration）只由 Main Agent 最终落地。

## 后果

### 正面

- Workspace 成为长期上下文容器，Files/Apps/AI/Usage 可被组织与恢复，不新增二级数据权威。
- Free Canvas 提供自由可视化空间，同时不引入绘图/协同复杂度。
- Design System V2 双材质统一全部视觉面，旧主题名与 V1 材质不再作为 runtime authority。

### 成本与约束

- SQLite v27 迁移必须幂等、单向、可回滚；旧 Home 迁移不得重复创建。
- Free Canvas 只做 Workspace 所需能力；任何“扩展成绘图/建模平台”的诉求需新 ADR。
- V2 视觉迁移面大（11 路由 + 20 组件域 + 边缘流程），V1 兼容层不得长期兜底新页面。

## 修订

- ADR-0020 §1、§3 中与本文冲突的范围由本文取代；其余决策（产品身份、停止建设、领域边界、Host authority、Secret、迁移纪律）继续有效。
- **PWSV2（2026-08-23，Personal Workspace V2 完整重设计裁决，对应 plan2 方案）**：
  1. **布局模式词表**：统一为 `structured | free`（冻结契约历史词 `compact` 废弃；DB CHECK 与 wire 值一律 `structured`/`free`）。Structured Canvas 是默认布局模式——默认首页是内容优先 Dashboard，由内置模板 `Classic Personal Dashboard` 实例化，不是硬编码固定页面。
  2. **Tab 语义收敛**：旧 `workspace_tabs`（内容 tab：tab_type/title/ref_id/url）降为 legacy 表，v29 后无 production 读/写；"打开的 Workspace 会话"由新表 `workspace_open_tabs` 承载（`workspace_id` PK + sort_order/is_pinned/opened_at/last_active_at）。Close = 删 row（不删 Workspace），Reopen = 插 row。`workspace_tab_create/update/close/reorder` 命令族与旧 View Tab Strip 在同一切片删除，不保留 fallback。
  3. **Session 快照模型（M-26 裁决）**：`workspace_session_snapshot` 是**全局轻量**读模型 `{openedTabs, activeWorkspaceId, workspaces(仅 metadata), revision}`；`workspace_snapshot` 是单 Workspace 全量读模型（不含内容 tab）。Inactive Workspace 不携带 widgets/layouts。
  4. **Template 系统**：新表 `workspace_templates`（origin ∈ builtin|personal）。Built-in manifest 由代码提供：`Classic Personal Dashboard`（默认）/ `Blank` / `Focus`。实例化在 Host 单事务完成并 template-key → instance-id 重写；`workspaces.template_source_id`/`template_version` 仅溯源，模板升级**不覆盖**已实例化 Workspace；支持恢复整个模板与单 Widget 重置。Manifest 禁止 Secret、用户绝对路径与实时数据。
  5. **Delete 语义（M-05 反转，2026-08-23）**：workspaces 采用软删 `deleted_at`（以冻结契约 §3 与 plan2 为准，覆盖 G-007"硬删"裁决）；list/snapshot 过滤软删行；子行随 workspace 软删一并失效（查询侧级联过滤），物理清理仅发生在 Final Gate 的显式 hard delete。
  6. **Widget 实例字段（M-13..16）**：`workspace_widgets` 增 `config_version INT`、`appearance_json TEXT`（V-003 白名单：surfaceVariant/header/opacity，禁止颜色 token 入库）、`enabled INT`、`z_index INT`。生产读写由 `hidden` 切换为 `enabled`：v29 迁移映射 `enabled = NOT hidden`，切换后代码不再读写 `hidden`；`hidden` 列保留至 death proof。
  7. **Layout 存储（M-17..20）**：`workspace_layouts` 增 `layout_mode TEXT CHECK(structured|free)`、`layout_version INT`，唯一键收敛为 `UNIQUE(workspace_id, layout_mode, breakpoint)`（以保数据增量迁移改表，禁止破坏式重建）；free 模式 breakpoint 取保留值 `'free'`。
  8. **View State 版本化（M-22）**：`workspace_view_states` 增 `state_version INT`。Data View 模式（`list|table|board|calendar`）是 **Data View Widget 的展示模式**，经 `workspace_view_states`（view_key 作用域）持久化，与布局模式正交。
  9. **Browse/Edit 双态（产品硬约束）**：Browse 默认安静——不显示 Inspector、拖拽手柄、网格线、删除按钮、技术 Badge 与底部状态栏；Edit 由"自定义工作空间"显式进入（floating toolbar：添加 / Undo-Redo / Structured-Free 切换 / 模板与恢复 / Workspace 外观 / 完成）。指针移动期间 IPC/DB write = 0，仅 drag/resize stop 或显式 commit 持久化（R-B11 延续）。
  10. **权威收敛与迁移号**：localStorage/IndexedDB 不得成为 Workspace 权威（R-B10）；Renderer 只保留有界、可丢弃的内存 read model。本批 schema 迁移号 **v29**（v28 已被应用中心迁移占用）；迁移增量、幂等、PRAGMA table_info guard，回滚版本忽略新增列/表。
  11. **Widget 边界不变**：仍是内置代码注册 React renderer + versioned config + layout capability + surface policy；**不是** Plugin Runtime / Worker / Event Bus / Marketplace / 第二 Event Bus。
