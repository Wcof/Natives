# Step 1：问题重构与材料审计

## 结构化问题

目标用户不是来维护 Blueprint JSON 的引擎开发者，而是想回答三个问题的人：

1. Native 收到一句用户输入后，到底经过哪些阶段？
2. 每个阶段装载了哪些 Prompt、Skill、Tool 和 Hook，它们由谁拥有？
3. 我如何在不破坏运行权威的前提下，从对应位置修改未来 Run？

## 四源扫描

- 对话：2 条（当前反馈、既有实施方案）。
- 项目：6 条（当前 UI、历史 UI、固定拓扑、Prompt Plan、Capability/Tool、外部 Inspector）。
- URL：3 条（Claude Hooks、Codex Hooks、Codex agent loop）。
- 知识库：0 条；未配置独立知识库。

## 审计三元组

`<依据集: [用户原话, 既有实施方案, 当前 NativeHarnessPanel, topology.rs, prompt_plan.rs, 官方 Claude/Codex 文档]> → [工具: /pm-collect, 技术: 对话提取 + 调用链扫描 + git 历史对比 + 官方文档核对] → [转换: 将“看不懂”按信息出现顺序映射为 UI 主对象错误，并把每项可视化诉求关联到现有或缺失的权威数据源] → <产出: 11 条关联材料 + 结构化问题>`

