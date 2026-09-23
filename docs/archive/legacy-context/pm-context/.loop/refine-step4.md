# Step 4：权衡与决策

| 权衡点 | 选择 | 原因 | 代价 |
|---|---|---|---|
| 页面主对象 | 方案 B：执行地图 | 直接回答用户“Native 怎么执行” | 需重组现有 UI |
| 画布技术 | 复用 React/CSS | 拓扑固定，无需图编辑库；适配 M1 8GB | 不支持自由连线，属于明确非目标 |
| 查看与编辑 | 同一地图 + 右侧检查器 | 避免用户在拓扑与表单间手工对应 | 需建立稳定 selection model |
| Skill 表达 | 分开显示 Prompt 贡献、`skill` Tool、Skill Hook | Skill 不是 Hook；三类装载语义不同 | Native 若要支持 Skill frontmatter Hook，需另补解析/装载链 |
| Tool 表达 | 有效 Tool Surface + Tool Call 回环 | 只有运行时实际暴露的 Schema 才是“模型可选择 Tool” | 需要复用 `PermissionGatedTools::list_tool_schemas` 的纯投影 |
| Prompt 表达 | Context 节点内完整 Prompt Stack | 当前 preview 只含 Prompt Block，不足以解释模型输入 | 需要让 preview/run start 共用完整 PromptPlanBuilder |
| Natives 内置 Prompt 替换 | `[冲突]` 新增版本化 replacement/override，而非直接改代码常量 | 用户明确要求可直接替换；现设计将 Built-in 锁定只读 | 必须先修订 Harness 设计和 schema，并保留恢复默认与 Diff |
| 外部 Runtime | 来源可追溯只读图 | 符合“能展示就展示” | 无法读取内部状态时只显示不可见边界 |
| Profile/Scope | 降为上下文选择器 | 它们是配置作用域，不是用户理解入口 | 专家信息需二级展开 |
| 发布流程 | 保持 Draft → Validate → Diff → Publish | 已有安全/兼容性不变量 | 修改不会即时生效 |

## 决策

采用方案 B。第一阶段不是继续添加 Tab，而是先定义一个 `EffectiveExecutionPlan` 只读投影，把固定 Stage、完整 Prompt Plan、有效 Tool Surface、Skill/Expert/MCP 贡献、Hook 附着和可见性边界组合为一张图。编辑操作仍写回现有 Draft schema；Run 模式读取冻结 Snapshot/Trace。

## 审计三元组

`<依据集: [方案 A/B/C, 用户先理解后配置的优先级, 固定拓扑与安全不变量, M1 8GB]> → [工具: /pm-refine Step 4, 技术: 加权决策（理解成本、真实性、闭环、安全、实现成本）] → [转换: 淘汰需要用户自行跨页映射的 A 和违反权威边界的 C，选择以同一执行地图承载查看/配置/运行证据的 B] → <产出: 10 项决策表 + 推荐方案>`

