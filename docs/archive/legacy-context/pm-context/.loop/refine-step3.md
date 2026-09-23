# Step 3：方案候选

## 方案 A：恢复历史可视化 + 保留现有分页表单（保守）

- 做法：恢复旧的执行流卡片、Prompt Stack、Hook 卡片；现有 Profile/绑定/编辑表单继续分 Tab 展示。
- 覆盖：Stage、Hook Point、Hook、Prompt、Run；Skill/Tool 另加列表。
- 优点：前端改动较小，可复用现存 `.engine-*` CSS。
- 代价：理解与配置仍是两套页面；用户需要自己把 Skill/Prompt/Tool 列表对应到流程节点。
- 适用：只要求“比现在更像可视化”，不要求从流程直接操作。

## 方案 B：执行地图优先的 Harness 工作台（推荐）

- 做法：
  - 首屏固定显示一次 Native Run 的主循环：用户输入 → 上下文装配 → 模型推理 → Tool 选择 → Hook/权限门禁 → Tool 执行 → 结果回模型 → 停止。
  - 在主流程下设置三条同步轨道：Prompt、Hook、Capability（Skill/Tool/MCP）。
  - 点击 Stage、Hook Point、Prompt Layer、Skill 或 Tool，在右侧检查器显示“是什么、为什么在这里、来源、当前值、运行证据、可做操作”。
  - “添加 Hook”从 Hook Point 发起并预填 Event；“替换 Prompt”从 Natives-owned Prompt Layer 发起；Profile/Draft/发布状态移到上下文栏和底部发布栏。
  - 运行模式将同一地图覆盖为某个 Run 的实际路径、Hook 状态、Tool 调用和 Prompt Snapshot。
- 覆盖：步骤 2 的全部实体与不变量。
- 优点：查看、解释、配置和诊断共享同一心智模型。
- 代价：需要新增一个 Daemon 的有效执行计划投影，前端也要拆分单体组件。
- 适用：用户确实要“看懂并工程化配置 Harness”。

## 方案 C：自由拖拽工作流/DAG（激进但淘汰）

- 做法：允许用户拖拽 Stage、Prompt、Tool 和 Hook，自由连线。
- 覆盖：表面覆盖所有实体。
- 代价：违反固定拓扑、安全门禁和单 Run 权威；会暗示不存在的执行能力。
- 适用：只有未来执行引擎本身改为用户定义工作流时才成立，本需求不适用。

## 物理约束

- M1 Mac 8GB：采用普通 React/CSS 横向画布和按需详情，不引入通用图编辑器；隐藏 Inspector/Trace 不构建昂贵投影。预计远低于 6GB 可用内存红线。

## 审计三元组

`<依据集: [Step 2 全部实体与不变量, 历史可视化 CSS, 当前工作区协议]> → [工具: /pm-refine Step 3, 技术: 保守/推荐/激进方案发散] → [转换: 对每个方案逐项检查 Stage、Hook、Prompt、Skill、Tool、配置版本与 Run 证据能否在一个用户流程内闭环] → <产出: 3 个候选及适用条件>`

