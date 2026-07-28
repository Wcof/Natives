# Native Harness 可视化工程台原型

问题：哪种信息架构能让用户先理解 Native Agent 的真实执行过程，再进入 Harness 配置？

结论：选择方案 A 的节点画布，并将其细化为统一的「执行引擎」工作区。它同时表达固定执行阶段、模型自主决策、Tool 回环，以及 Prompt / Skill / Hook 在真实链路中的挂载位置。

## 已确定的产品结构

- Settings 只保留一个「执行引擎」入口。
- Native Engine 提供完整可视化与配置；Claude CLI / Codex CLI 只显示外部所有权和只读指引。
- 同一张固定拓扑支持三种上下文：理解当前执行、配置未来 Run、回放实际 Run。
- Overview、Blueprint、Live Runs 复用拓扑和节点上下文；Hooks、Prompts、Audit 保持专用工作区。
- 「引擎能力」不再是独立卡片页面。Provider、模型可见 Tool、MCP、Skills、Extensions、限流和权限以只读能力投影挂到对应节点。
- Scheduler / Job 不属于 Native Engine，迁回独立任务模块。
- 创建 Harness、绑定项目、版本与回滚属于二级管理，不阻挡普通用户理解当前执行。

## 权威边界

- Capability Hub 继续拥有 MCP、Skills、Extensions、Agent Profile 和 conversation/run capability selection。
- Provider Authority 继续拥有路由、模型、凭证与 lease。
- Harness 只拥有 Blueprint、Hook/Prompt 配置、发布版本、Run 快照和 Harness 审计。
- 画布只组合 typed projection/reference，不复制 capability 对象，也不展示凭证或 raw effective system prompt。

## 正式实现需要的投影

画布至少需要按同一 scope 返回：

- 固定 stage / edge / Hook Point；
- 当前有效 Hook attachment 与来源；
- Prompt Plan 摘要、顺序、所有者、digest 和编辑权限；
- capability snapshot 引用及模型可见 Tool 分类计数；
- Provider 只读解析摘要；
- 运行保护摘要；
- selected Run 的冻结 snapshot 与 node event state。

如果现有方法分开返回这些数据，Host façade 必须用一致的 project/session scope 组合，禁止 Renderer 猜测归属或生成合成拓扑。

## 当前代码接缝（实施时必须处理）

- `src-agent-daemon/src/harness/control_plane.rs::workspace_get` 仍用
  `serde_json::Value` 拼接 overview/topology/catalog/draft；替换为
  `assistant-protocol` 中的 typed request/response 后才能作为新 UI 接口。
- `crates/assistant-protocol/src/v2/harness.rs` 当前只定义 subscribe 类型；
  需要增加 workspace、attachment、authority、Tool Plan wire 类型并生成前端投影。
- `src/components/settings/NativeHarnessPanel.tsx` 在组件内手写 Workspace、
  Prompt、Run、Audit 类型；新模块必须删除这些本地 wire 类型。
- `ResolvedCapabilitySnapshot::to_audit_json` 只冻结 capability IDs，
  `ResolvedHarnessSnapshot` 也没有 Tool Plan；Run 回放不能从当前发现结果
  反推，必须在 Provider 调用前冻结 model-visible Tool 名称、来源和
  schema digest。
- 当前 `ResolvedHarnessSnapshot::PromptPlanSummary` 只有 source digest 和
  token estimate，尚未冻结 `CompiledPromptPlan` 的 effective hash 与有序
  layer summary；正式回放不能依赖当前 Prompt preview 补算。
- `Sidebar.tsx` 当前同时展示 runtime / engineering / engine；迁移后只展示
  runtime，并暂时保留旧 target alias。
- `EngineCapabilitiesPanel` 当前混合 MCP、Skills、Extensions、Rate Limit 和
  Scheduler；按迁移表拆分，禁止把 Scheduler 带入 Native workspace。

权威实施细节、typed schema、迁移矩阵、状态模型和验收场景统一维护在
Harness 设计第 14.2、14.3、15.1–15.3、20、22 节，本原型不再复制。

打开方式：

```sh
open docs/prototypes/native-harness-visual-workbench/index.html
```

历史方案：

- `?variant=A`：节点画布优先，用阶段泳道、挂载节点和回环连线解释真实执行。
- `?variant=B`：新手分镜讲解，用教学叙事解释 Prompt、Skill、Tool、Hook。
- `?variant=C`：专业 Harness IDE，资源树、固定拓扑、检查器和发布栏。

该目录是一次性原型。正式实施时只重写方案 A，不直接复制原型代码；B/C 仅保留为历史对照，实施前删除。
