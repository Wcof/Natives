# Personal Workspace V2 · T01 报告（矛盾裁决 / 文档编辑 / 遗留风险）

> 范围：对照 `/Users/ldh/Downloads/project/plan2/`（README/01/02/03）与仓库 `docs/adr/0021`、`docs/contracts/workspace-v2-contract.md`、`docs/standards/ui-ux/01-03`、`docs/standards/technical/02-04`、`docs/standards/product/02-feature-spec.md`。
> 裁决顺序：standards > ADR > contract > plan2。contract 需改则升版、保留旧版。
> 基线：`codex/provider-proxy-full` @ `93b3b4d`，编辑前 `git diff` 确认工作树干净；未提交他人改动：**无**。
> 配套施工规格：`.pws2/spec.md`。

---

## 【矛盾裁决】（逐条：矛盾 / 裁决 / 理由）

### C1. 布局模式词表 `compact` 残留 vs `structured|free`
- **矛盾**：`workspace-v2-contract.md` §6 冻结 TS 类型 `WidgetDefinition.supportedLayouts: ('compact'|'free')[]` 与 §3 legacy-home 迁移描述 `workspace_layouts(compact)` 仍用历史词 `compact`；而 plan2（01/02）、ADR-0021 修订 §10、**同文件 §0 PWSV2 升版**（"历史词 `compact` 废弃"）、standards ui-ux/02 R-U13（"词表统一为 `structured|free`，structured 为默认"）均为 `structured|free`。contract 自身 §0 与 §6 自相矛盾。
- **裁决**：统一 `structured|free`，清除 contract 内功能性 `compact` 残留。
- **理由**：standards（R-U13，MUST 级方向）> ADR（修订 §10）> contract；且 contract 冻结文本 §0 已明文"compact 废弃、structured 为默认"，§6 type 是未同步的残留。属文本一致性修复，无字段语义变更。
- **落点**：已编辑 `workspace-v2-contract.md`（§6 type、§3 迁移行）。

### C2. Host 事件命名（plan2 自造通道 vs standards 强制 `db-state-changed`）
- **矛盾**：plan2 §5.3 定义事件 `workspace:session-changed / workspace:snapshot-changed{workspaceId,revision,reason} / workspace:deleted / workspace:template-changed`；standards technical/02 R-S9（MUST）规定"任何影响 Renderer 展示的 DB 变更必须经 `db-state-changed` IPC 广播，禁止依赖轮询"；实现（`events.ts` + `commands/workspace.rs`）用 `db-state-changed`（channel `workspace`）+ 独立 `workspace` 事件，payload `{workspaceId,event,channel}`，14 种 event kind（含 `templateSaved/templateDeleted/templateRestored`）。
- **裁决**：以 standards R-S9 为准 —— 事件经 `db-state-changed`(channel `workspace`) / `workspace` 广播；plan2 的 `workspace:*` 自造名**不**作为独立 transport，仅作语义别名。
- **理由**：R-S9 为 MUST 且实现已落地；plan2 是外部只读方案（本次不改），其事件名与实现/standards 不符，按裁决顺序让位。
- **落点**：已编辑 `workspace-v2-contract.md`（§0 增澄清条目）。

### C3. `workspace_layouts` 唯一键机制（ADR/contract 文本 vs 实现）
- **矛盾**：ADR-0021 修订 §7 与 contract §3 均称唯一键"收敛为 `UNIQUE(workspace_id, layout_mode, breakpoint)`"；实现 `migration_v29.rs` 保留原 v27 `UNIQUE(workspace_id, breakpoint)`，另建 `CREATE UNIQUE INDEX uq_workspace_layouts_mode_breakpoint(workspace_id, layout_mode, breakpoint)`（free 用保留 breakpoint='free'，两模式不冲突）。
- **裁决**：以实现的加法方案为准（保留原唯一键 + 语义唯一索引）。
- **理由**：standards technical/03 R-D3（MUST）只要求"增量、禁 DROP/重建"，实现满足且更保守（不改动既有唯一约束，避免数据迁移风险）；语义等价（两模式各占 breakpoint 空间不冲突）。ADR/contract 文本是"目标唯一性"描述，非强制机制措辞。不破坏冻结语义，故不改 ADR/contract。
- **落点**：不编辑（记录为遗留风险 R2）。

### C4. Session 命令面（contract §4 理想清单 vs 实现命名）
- **矛盾**：contract §4 列出 `open_workspace / close_workspace / reopen_workspace / pin_workspace / reorder_workspace_tabs / set_active_workspace` 等；实现以 `workspace_session_open / workspace_session_close / workspace_session_snapshot / workspace_session_reorder` + `workspace_set_active` 承载，且无独立 `pin_workspace` 命令（pinned 由 `workspace_open_tabs.is_pinned` 列 + reorder 承载）。
- **裁决**：以实现 `workspace_session_*` 命令面为准；contract §4 视为语义能力清单（open/close/reopen/pin/reorder 的语义在实现中已满足），命令命名以 `commands/workspace.rs` + `client.ts` 为唯一对齐源。
- **理由**：contract 前缀"必须提供的 Host Service"是能力描述，实现已覆盖全部语义（open/close=snapshot/reorder/set_active；pin=列字段）。plan2 未强制具体命令名。避免大改冻结契约 §4 命令表，故暂不改；对齐动作入 T02。
- **落点**：不编辑（记录为遗留风险 R3；spec.md §2 已注明映射）。

### C5. `workspaces.theme` CHECK 约束（contract 文本 vs 实现）
- **矛盾**：contract §3 写 `theme CHECK(dark|light) DEFAULT 'dark'`；v27 实现为 `theme TEXT NOT NULL DEFAULT 'dark'`，靠 `normalize_legacy_theme` 运行时归一，无硬 CHECK 约束。
- **裁决**：以实现（TEXT + 归一函数）为准；contract 目标约束视为可接受的文档表述。
- **理由**：standards product/02 R-F1（状态可验证）与技术/03 R-D3（增量、禁破坏）不强制加 CHECK；加 CHECK 属破坏性约束变更（需回填校验），收益低。plan2 未涉及。
- **落点**：不编辑（记录为遗留风险 R1）。

> 一致性确认（无矛盾，未列为裁决项）：Data View 四模式 `list/table/board/calendar`（经 `workspace_view_states`，三向一致）；widget appearance 白名单 `surfaceVariant/header/opacity`（ADR §6 = contract §3 = plan2 §10）；template manifest `nameKey` 可选（三向一致）；三概念分离（Tab/LayoutMode/DataView，ADR §7 G-007 = contract = plan2）；host 单权威 / pointer-move 0-IPC（standards R-S9/R-U14/R-F6 = plan2 = ADR）。

---

## 【文档编辑】（每文件 ≤5 条）

### `docs/contracts/workspace-v2-contract.md`（3 条；加法性文本一致性修订，保留全部既有冻结规则）
1. **§6 `WidgetDefinition.supportedLayouts`**：`('compact'|'free')[]` → `('structured'|'free')[]`（C1）。
2. **§3 legacy-home 迁移行**：`workspace_layouts(compact)` → `workspace_layouts（layout_mode='structured'）`（C1）。
3. **文首 §0 新增「PWSV2-T01 一致性修订（2026-08-23）」条目**：声明 C1 词表残留清理 + C2 事件命名澄清（`db-state-changed`/`workspace` 为准，plan2 自造名仅语义别名），并声明旧版冻结文本（M-004/M-006、Wave2 I-008、G-007、V-005）全部保留、未删除（C2）。

### `docs/adr/0021-multi-workspace-design-system-v2.md`（0 条）
- 唯一 `compact` 出现于"历史词 `compact` 废弃"的**说明性措辞**，非 live 类型/约束，与 C1 裁决一致，无需改动。ADR 修订 §PWSV2 全部与实现/standards 对齐。

> standards（ui-ux/01-03、technical/02-04、product/02）本次**只读**，未改动。

---

## 【遗留风险】（≤5 条）

- **R1 · `workspaces.theme` 无硬 CHECK**：contract 文本写 `CHECK(dark|light)`，实现为 TEXT + `normalize_legacy_theme` 归一。风险：脏 theme 值需靠归一函数兜底，未来直插可能漏校验。建议 T02 评估补 CHECK 或在校验层强制（非本次范围）。
- **R2 · `workspace_layouts` 唯一键机制措辞差异**：实现 = 保留原 `UNIQUE(workspace_id,breakpoint)` + 语义索引 `uq_workspace_layouts_mode_breakpoint`；ADR/contract 文本称"收敛为 `UNIQUE(workspace_id,layout_mode,breakpoint)`"。语义等价（free 用保留 breakpoint），但文档与实现机制描述不一致，后续维护者可能误以为已改表。建议 T02 起在 contract §3 注明"以语义唯一索引实现"。
- **R3 · session 命令命名对齐**：contract §4 列理想命令名（open/close/reopen/pin_workspace…），实现以 `workspace_session_*` 承载且无独立 `pin` 命令。风险：三方按 §4 字面名开发会调不到命令。建议 T02 把 contract §4 命令表与 `commands/workspace.rs`/`client.ts` 做一次显式对齐（spec.md §2 已给映射）。
- **R4 · plan2 在仓库外、未版本化**：plan2 的 4 个自造事件名与 §8.1 目标 composite 布局（personal_hero/metric_overview/activity_timeline/system_health）未回写仓库，仅由 `.pws2/spec.md` 落盘。风险：plan2 若被他人再改将偏离 spec。建议后续把 plan2 关键决策并入仓库（或 spec.md 标注以 spec 为准）。
- **R5 · Classic 模板当前 manifest 仅 8 个真实 widget**：plan2 §8.1 目标 composite（personal_hero/metric_overview/activity_timeline/system_health）尚未实现（当前 = greeting/today_usage/app_launcher/recent_files/ai_status/notes/storage_overview/quick_links）。风险：T06 前默认首页视觉与 plan2 目标节奏有差距。spec.md §6.2 已给过渡说明与目标坐标；T06 落地后按 plan2 重排（保持确定性 derive + 每断点独立保存）。
