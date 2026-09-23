# Step 2：领域模型

## 实体

- 用户：查看者、Harness 配置者。
- Runtime：Native（可编辑）、Claude/Codex（只读）。
- Execution Stage：固定运行阶段。
- Hook Point：阶段中的确定性触发点。
- Hook Definition：来源拥有的 handler 定义。
- Hook Overlay：对 imported Hook 的稀疏覆盖。
- Prompt Layer：一次有效 Prompt Plan 中的有序贡献。
- Skill：能力对象；可贡献目录提示、按需 `skill` Tool，部分外部格式还能声明 Hook。
- Tool Surface：该项目/Run 实际向模型暴露的 Tool Schema。
- Harness Profile / Draft / Version / Binding：未来 Run 的版本化配置。
- Run Snapshot / Trace：一次已启动 Run 的冻结执行证据。

## 关系

- Runtime 拥有固定 Execution Stage。
- Stage 包含 Hook Point；Hook Definition 挂载到 Hook Point。
- Prompt Layer 在 Context 阶段装配并传给 Provider。
- Tool Surface 在 Provider 调用前暴露给模型；模型选择 Tool 后进入 Tool Gate → Permission → Tool Execute 回环。
- Skill 可影响 Prompt Layer 和 Tool Surface；只有实际声明且被 Runtime 装载的 Skill Hook 才关联 Hook Point。
- Profile 经 Binding 选择，Draft 发布为 Version，Run 冻结 Version 解析结果为 Snapshot。

## 不变量

- 固定拓扑不可新增、删除、改线或重排。
- Native 可配置未来 Run，活动 Run 永远使用启动时快照。
- imported/外部来源只读；Natives-owned Hook/Prompt 才可版本化编辑。
- Renderer 不读取文件、SQLite 或 socket；所有视图来自 Daemon 投影。
- Tool 展示必须是有效项目/Run Tool Surface，不得用全局 Tool 清单冒充。
- Skill、Prompt、Tool、Hook 是四种不同概念，UI 不得互相冒充。
- 外部 Runtime 无证据就显示“不可见/未发现”，不得推测。

## 审计三元组

`<依据集: [Step 1 问题重构, topology.rs, capability_resolution.rs, Harness 设计 §12/§15]> → [工具: /pm-refine Step 2, 技术: 领域实体映射 + 权威边界隔离] → [转换: 把用户口语中的“装载到哪里”拆为 Prompt 注入、Tool 暴露、Hook 挂载三种不同边，并保留 Run 快照作为已执行证据] → <产出: 12 个实体、7 类关系、7 条不变量>`

