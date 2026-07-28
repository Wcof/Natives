# Native Harness 引擎工程需求材料

## 问题重构

- 谁：希望理解并配置 Native 执行引擎的 Natives 用户。
- 什么：用户原话要求“清晰的、可视化的让用户理解当前 Native 执行引擎的执行过程”，看见 Hook、Harness、Prompt、Skill 与模型可选 Tool 的真实装载位置，并能在对应位置添加 Hook 脚本或替换 Natives 内置 Agent Prompt。
- 为什么：当前《引擎工程》直接暴露 Profile、Scope、Blueprint、matcher、timeout 等内部字段，用户“完全看不懂理解不到，不知道这个是做什么的”。
- 约束：Native 可编辑；Claude/Codex 只展示能从真实配置证明的部分；固定执行拓扑不可任意改线；运行数据、Prompt、Tool 和 Hook 都必须来自 Daemon 权威，不能造展示数据。

## 对话材料

### M1 当前用户反馈

- 来源：当前对话。
- 概要：首要目标是理解执行过程，第二步才是配置。
- 关键信息：
  - 首屏必须回答“一次请求如何执行”。
  - 必须标出 Hook 点、实际 Hook、每段 Prompt、模型可选择的 Tool。
  - 必须能追踪 Skill 在执行链中的真实贡献。
  - 必须从流程节点直接添加 Hook。
  - Natives 内置 Agent Prompt 需要可查看并可替换。
- 关联：与 M2 的“固定拓扑画布”一致；与 M4 当前配置优先 UI 冲突。

### M2 已有实施方案

- 来源：`/Users/ldh/.codex/attachments/d1e15197-c3ff-46f9-b0be-6f58e32380b5/pasted-text.txt`。
- 概要：已批准固定拓扑、三栏工程布局、引导/专家模式、Draft → Validate → Diff → Publish、五类 Native Hook 与外部 Runtime 只读视图。
- 关键信息：
  - 中栏应是固定拓扑、Hook Point、Prompt Plan 轨道和 Run 覆盖层。
  - 右栏应是所选节点的检查器。
  - Claude/Codex 配置只读，不由 Natives 改写。
- 关联：与 M1 同主题；现有实现只落了字段编辑与发布能力，没有保留此交互主线。

## 项目扫描材料

### M3 后端已有真实执行拓扑

- 来源：`crates/harness-core/src/topology.rs:152`。
- 概要：Daemon 已定义 Session、Context、Provider、Tool Gate、Permission、Tool Execute、Subagent、Compact、Stop、Terminal、Cross Stage 固定阶段，并标记 Hook Point 与 Safe Point。
- 关联：证明 M1 所需主画布有真实数据源；无需引入自由流程编辑器。

### M4 当前 Renderer 是配置优先，而非执行过程优先

- 来源：`src/components/settings/NativeHarnessPanel.tsx:358`。
- 概要：首页先展示 Profile 选择、保存/校验/发布、新建 Profile、Scope 绑定；概览只有三张统计卡，执行拓扑只是横向英文卡片。
- 关键信息：
  - Stage、Event、adapter、failure policy 多数直接显示内部英文枚举。
  - Hook 编辑与拓扑分离，只能在表单里选 Event，不能从 Hook Point 创建。
  - Prompt 编辑与执行节点分离。
  - 页面没有 Tool 可见性，也没有 Skill 到 Prompt/Tool 的关系。
- 关联：直接解释 M1 的“完全看不懂”；与 M5 历史可视化形成回退证据。

### M5 仓库曾有可视化骨架，但被功能表单替换

- 来源：提交 `519a2275` 中的 `NativeHarnessPanel.tsx`；现存 `src/app/globals.css:1026` 起的 `.engine-*` 样式。
- 概要：历史版本有执行流卡片、箭头、Hook 点、Blueprint 继承轨、Prompt Stack 和 Run/Audit 视图；当前工作区保留 CSS，却不再使用这些组件。
- 关联：说明不需要新增流程图库；可复用已有视觉语言，但历史版本仍缺少用户叙事、节点检查器和编辑闭环。

### M6 Prompt Preview 尚不是真实完整 Prompt Plan

- 来源：`src-agent-daemon/src/harness/control_plane.rs:607`、`crates/harness-core/src/prompt_plan.rs:1`。
- 概要：纯 Prompt Plan 类型已定义 Built-in、Capability Expert、Instruction Files、Skill Catalog、Natives Prompt Block、Team Roster、Child Directive；但 `harness.prompt.preview` 目前只枚举已发布的 Natives Prompt Block。
- 关联：M1 要求查看“那个环节使用了什么 prompt”，当前协议数据不足，不能只靠前端重排解决。

### M7 Tool/Skill 权威存在，但没有引擎工程投影

- 来源：`src-agent-daemon/src/capability_resolution.rs:50`、`src-agent-daemon/src/production_tools.rs:249`。
- 概要：Run capability snapshot 已有 Skill ID、MCP Server、Expert/Team；运行时会按 allowlist、Plan Mode、MCP selection 计算真正向模型暴露的 Tool Schema。
- 关联：M1 的 Tool/Skill 可视化必须复用同一解析逻辑；全局 `tool.list` 不是某项目或某 Run 的有效 Tool 列表。

### M8 外部 Runtime Inspector 目前不够可信

- 来源：`src-agent-daemon/src/harness/external_inspector.rs:39`。
- 概要：只检查项目目录下少量 Claude/Codex 文件；当目录存在但没解析出 Hook 时，会凭空补 `PreToolUse` 等事件和伪造 unsupported 项。
- 关联：违反 M1“能展示就展示，不能展示就算了”；外部视图必须移除推测值并按来源层级真实解析。

## URL 材料

### M9 Claude Code 官方 Hook 模型

- 来源：[Claude Code Hooks reference](https://code.claude.com/docs/en/hooks)。
- 概要：官方以生命周期图解释 Hook；Hook 是 Event → Matcher Group → Handler，支持 command/http/mcp_tool/prompt/agent，并能来自 User、Project、Local、Plugin、Skill/Agent frontmatter、Session、Built-in。
- 关联：印证 M1 的“先看生命周期，再看挂载项”；也说明 Skill 可能声明 Hook，但 Skill 本身不等于 Hook。

### M10 Codex 官方 Hook 模型

- 来源：[Codex Hooks](https://learn.chatgpt.com/docs/hooks)。
- 概要：Codex 当前也有生命周期 Hook；官方页面明确当前只有 `command` handler 真正执行，`prompt`/`agent` 虽能解析但会跳过。
- 关联：外部只读视图应展示真实 Codex Hook，同时明确 handler 支持状态，不能继续按旧假设只扫几个 TOML 字符串。

### M11 Codex Harness 组成

- 来源：[Unrolling the Codex agent loop](https://openai.com/index/unrolling-the-codex-agent-loop/)。
- 概要：Codex 将 Harness 核心解释为 instructions、tools、input 进入 agent loop。
- 关联：支持把 Native 页面主叙事也组织成“上下文/Prompt → 模型 → Tool → 结果回环”，而不是数据库对象列表。

## 知识库材料

- 未发现已配置的独立知识库路径；项目 `docs/`、ADR、设计文档已作为项目扫描材料纳入。

## 材料关联结论

- M1、M2、M3、M9、M11 共同指向：生命周期/执行流必须是页面主轴。
- M4 与上述目标冲突：当前 UI 把内部配置对象放在理解之前。
- M5 证明仓库已有可复用视觉骨架，但不能原样恢复，因为它仍未把 Prompt、Skill、Tool 放进同一执行叙事。
- M6、M7 证明仅改 Renderer 会产生假可视化，必须先补“有效执行计划”读模型。
- M8 与 M9/M10 冲突：外部 Inspector 的猜测数据必须删除，改为来源可追溯的只读解析。

