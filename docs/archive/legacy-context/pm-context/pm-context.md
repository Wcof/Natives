# PMContext: Native Harness 可视化引擎工程

## 概述

### 问题与目标

- 事实：当前《引擎工程》把 Profile、Scope、Draft、matcher、timeout 等内部配置放在执行过程之前，用户无法建立用途认知。← 来源：用户反馈、`NativeHarnessPanel.tsx:358`
- 事实：Native Daemon 已有固定 Stage、Hook Point、Prompt Plan 类型、Capability Snapshot 和 Run Trace 等真实来源。← 来源：项目扫描 M3/M6/M7
- 目标：让用户先看懂一次 Native Run，再从执行节点查看和配置未来 Run。

### 现状平替与摩擦力

- 用户目前只能在“执行拓扑 / Hooks 工程 / 提示词工程 / 实时运行”间自行建立对应关系。
- 当前横向 Stage 卡片只显示内部枚举与 Hook 数量，不解释阶段职责，也不能从节点进入配置。
- Prompt Preview 只显示 Natives Prompt Block；Tool 与 Skill 关系完全缺席。
- 历史可视化骨架仍在 CSS 中，可复用但不足以单独满足需求。

### 价值验证度量

- `[假设 7/10]` 核心指标：首次使用者在 60 秒内能从页面指出“某个 Skill 影响了哪段 Prompt、暴露了哪个 Tool、是否声明 Hook”；测试成功率目标 ≥ 80%。
- `[假设 7/10]` 辅助指标：从指定 Hook Point 创建一个无害 Command Hook 草稿，中位耗时 ≤ 3 分钟。
- `[假设 6/10]` 辅助指标：用户能在 90 秒内解释某次 Run 的 Prompt 来源、模型可见 Tool 和 Hook 执行结果，成功率 ≥ 80%。

## 用户场景

### 事实

- 用户先查看 Native 执行链，再配置未来 Run。
- 用户可能选择项目、全局模板或具体 Run 作为观察上下文。
- Claude/Codex 只作为外部只读参照，不由 Natives 接管。

### 规则

- 默认进入“运行原理/当前有效计划”，而不是空 Draft 表单。
- 基础视图使用业务中文；内部枚举、digest、schema、revision 进入专家详情。
- 每个对象必须回答：是什么、何时发生、来源、当前生效内容、能否编辑、最近运行证据。

### 验收

- 无 Harness Profile 时，仍能看见 Native 固定流程，并明确“当前使用默认配置”。
- 选择项目后，同一地图更新为该项目的有效 Prompt/Skill/Tool/Hook 计划。
- 选择 Run 后，同一地图覆盖真实 Snapshot/Trace，不重建第二套生命周期。

## 执行地图

### 事实

- 固定主链为：用户输入 → Context/Prompt 装配 → Provider/模型 → Tool Gate → Permission → Tool Execute → 结果回模型 → Subagent/Compact/Stop/Terminal。
- Provider 与 Tool Execute 之间存在循环，而不是一次性的线性流水线。

### 规则

- 主链上方展示阶段职责，下方至少三条同步轨道：
  - Prompt：按实际装配顺序显示 layer、owner、scope、digest、token estimate、脱敏预览。
  - Hook：显示每个 Hook Point 及其 attached Hook、来源、类型、状态。
  - Capability：显示 Expert/Team、Skill、MCP 与有效 Tool Surface。
- 点击对象打开右侧检查器；无选中对象时显示当前计划摘要与风险。
- Hook Point 提供“添加 Hook”；自动预填 Event，handler 类型再由用户选择。
- Tool 卡片必须标明“模型可自主选择调用”或“仅 Hook 内部调用/系统调用”。

### 验收

- 用户无需切 Tab 即可看见 Prompt、Hook、Skill 和 Tool 在同一 Run 中的相对位置。
- Stage 无 Hook 时显示“此阶段无可配置 Hook 点”，不展示空白专业术语。
- Tool 调用在 Run 模式中沿 Tool Gate → Permission → Execute 回路高亮。

## Skill、Prompt、Tool 与 Hook

### 事实

- Skill 不是 Hook。Native 当前 Skill 可贡献目录 Prompt，并通过 `skill` Tool 按需加载；外部 Runtime 的 Skill/Agent frontmatter 还可能声明 Hook。
- 当前 `harness.prompt.preview` 未覆盖完整 PromptPlanBuilder 来源。
- 当前全局 `tool.list` 不能代表某项目/Run 的模型可见 Tool Surface。

### 规则

- Skill 详情分别列出：
  - Prompt 贡献装载到 Context 的位置。
  - `skill`/MCP Tool 暴露到模型的位置。
  - 若真实存在，Skill 声明的 Hook 挂载点；不存在则明确“未声明 Hook”。
- Prompt 层按 owner 分为：
  - Natives-owned：可版本化编辑或替换。
  - Capability/项目文件/父 Run：只读并跳转到拥有者。
  - 敏感来源：只显示脱敏预览与 digest。
- `[冲突]` 用户要求替换 Natives 内置 Agent Prompt，而现有设计把 Built-in Surface 锁定。推荐增加显式、版本化的 Built-in Prompt Replacement，保留“恢复默认”、校验、Diff、发布和 Run 快照，不允许直接改源码常量。

### 验收

- Prompt Preview 与新 Run Snapshot 的 layer 顺序、digest、token estimate 和 effective hash 一致。
- “替换内置 Prompt”只影响未来 Native Run；活动 Run 不变。
- imported Prompt/Hook 没有编辑按钮，只有来源与打开拥有者入口。

## 配置工作台

### 事实

- 现有 Profile/Draft/Version/Binding 与保存、校验、发布、回滚链可复用。

### 规则

- 顶栏：Runtime、项目/全局、当前 Profile/版本、运行健康。
- 中央：执行地图。
- 右栏：选中对象检查器。
- 底栏：未保存状态、Draft revision、Validate、Diff、Publish、冲突。
- 引导模式隐藏 schema/revision/digest 等实现细节；专家模式展开，但两者编辑同一 Draft。

### 验收

- 从 Hook Point 新建、保存、校验、Diff、发布并绑定形成闭环。
- 发布确认展示新增命令、HTTP host、MCP、Prompt/Agent 内容 digest 与信任提升。
- 版本回滚以新版本发布，历史 Run Snapshot 不变。

## 外部 Runtime

### 规则

- Claude/Codex 读取真实用户、项目、本地、插件/Skill/Agent/managed 来源后按生命周期展示。
- 每项显示来源文件/层级、Runtime 支持状态和 Native 映射状态。
- 解析失败显示错误；未发现显示未发现；不可观察显示不可见。
- 禁止根据目录存在推测 Hook。

### 验收

- Codex 当前只把官方真正执行的 command handler 标为可执行；parsed-but-skipped 类型明确标注。
- 外部视图无 Natives 内编辑/发布按钮，只提供 Host 执行的打开/显示位置。

## 全局约束

- Renderer → Tauri Host → UDS → Daemon。
- 不新增第二套 Run、Capability、Provider、Credential 或 Trace 权威。
- Raw effective system prompt、凭证、secret 和敏感 Hook 输入输出不持久化、不进 Renderer。
- 大列表分页；隐藏 Run/Trace 详情停止订阅和昂贵投影。
- M1 Mac 8GB：复用 React/CSS 固定画布，不引入通用图编辑器。

## 优先级

- Must：执行地图、完整 Prompt Plan、有效 Tool Surface、Skill 三类贡献、Hook Point 直达编辑、Run 覆盖层、真实外部只读边界。
- Must：先修订 Built-in Prompt 的可替换语义与安全边界，再开放编辑。
- Should：引导/专家模式、模板、键盘排序、节点级文档说明。
- Could：跨版本地图动画、运行耗时热力图。
- Won't：自由 DAG、活动 Run 热修改、接管 Claude/Codex 配置。

## 决策日志

| 决策点 | 选项 A | 选项 B | 最终选择 | 理由 | 来源 | 依据源数 | 工具名摘要 |
|---|---|---|---|---|---|---:|---|
| 主界面 | 配置 Tab | 执行地图 | 执行地图 | 用户先理解后配置 | M1/M2/M3/M4/M9/M11 | 6 | pm-collect/refine |
| Tool 数据 | 全局 tool.list | 有效 Tool Surface | 有效 Tool Surface | 与模型实际可见内容一致 | M7 | 1 | 调用链扫描 |
| Skill 表达 | 当作 Hook | 三类贡献拆分 | 三类贡献拆分 | 符合真实运行语义 | M7/M9 | 2 | 领域建模 |
| 画布技术 | 通用图框架 | 现有 React/CSS | React/CSS | 固定拓扑、成本低 | M3/M5 | 2 | repo 扫描 |
| 外部 Runtime | 推测映射 | 证据驱动只读 | 证据驱动只读 | 防止假能力 | M8/M9/M10 | 3 | 源码+官方文档 |

## 假设清单与验证计划

| 假设 | 置信度 | 风险类型 | 验证方式 | 成功阈值 | 时机 |
|---|---:|---|---|---|---|
| 执行地图比 Tab 更易理解 | 8 | 可用性 | 5 人首次任务测试 | ≥4 人 60 秒内定位 Skill/Tool/Hook | 原型阶段 |
| 右侧检查器足够承载编辑 | 7 | 交互 | 高保真原型走查 | 无需跳页完成 Hook 草稿 | 原型阶段 |
| Built-in Prompt replacement 符合用户“直接替换” | 7 | 产品/安全 | 审阅 replacement、override、继承三种语义 | 明确一种并更新设计 | 实施前 |

## 风险项

- `[冲突]` Built-in Prompt 当前锁定只读，用户要求直接替换；未修订设计/schema 前不能只加前端按钮。
- `[待确认]` Native 是否要支持 Skill frontmatter Hook。当前能力审计显示 Skill 解析并不完整，不能先画出“已装载 Hook”。
- `[假设 7/10]` 首版以桌面宽布局为主，小窗口降级为阶段列表 + 抽屉。

## 信息缺口

- Built-in Prompt 的“替换”是完全替换、前置/后置 override，还是按 Surface 单独替换；推荐完全替换 + 一键恢复默认。
- 是否要求 Native 兼容解析 Claude/Codex Skill frontmatter Hook；这会扩展执行能力，而非纯 UI。

