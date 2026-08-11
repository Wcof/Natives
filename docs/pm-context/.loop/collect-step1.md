# Step 1：问题重构与材料审计

## 结构化问题

目标用户是 Natives 产品与工程团队。当前诉求不是启动应用或修改执行代码，而是在规范整改期间研究八个本地 Agent 项目，回答：

1. 各项目的 Agent Harness 如何划分执行权威、Session/Run、Tool、Permission、Hook 与扩展边界？
2. 哪些机制已由源码调用链证实，哪些只存在于文档、CHANGELOG 或目标设计？
3. 哪些优点可以在不引入第二套 Run、Capability、Provider、Credential 或 Trace 权威的前提下吸收到 Natives？

动机是为后续 Natives Harness、Tool Runtime、上下文持久化、权限与多 Agent 设计提供可追溯的候选机制、拒绝清单和实施优先级。

约束：用户明确说明当前应用因规范整改无法启动；本轮只做源码、文档与 Git 研究，不以应用运行结果替代源码证据，不修改生产执行代码。

## 四源扫描

- 对话：1 组用户目标 + 既有研究摘要。
- 项目：8 个固定 commit 的对标仓库、Natives 权威文档、既有 Goose 调研。
- URL：0；用户未提供 URL，本轮不使用网络材料补全本地证据。
- 知识库：0；未配置或未发现可用于本任务的独立知识库。

## 审计三元组

`<依据集: [用户原话, 8 个固定 commit, 各仓 Harness/Tool/Permission/Session/Hook 源码与文档, Natives standards/architecture/既有 Goose 报告] → [工具: pm-collect, rg/sed/git 调用链扫描, codebase-design 深模块评估] → [审计: 源码调用链追踪 + 同主题实体映射 + 权威边界对照；逐项标记源码证实/文档可见/迁移中/不可证实] → <产出: docs/pm-context/collect/agent-engineering-benchmark-2026-08-10.md + docs/harness/agent-engineering-comparative-research-2026-08-10.md + docs/harness/projects/ 单仓复核>`

## 逐仓复核进度

| 顺序 | 项目 | 状态 | 独立报告 |
| ---: | --- | --- | --- |
| 1 | AtomCode | 已完成 | `docs/harness/projects/atomcode-agent-engineering-review-2026-08-10.md` |
| 2 | Claude Code | 已完成；核心源码不可见边界已单列 | `docs/harness/projects/claude-code-agent-engineering-review-2026-08-10.md` |
| 3 | DeepChat | 已完成；已校正当前 `deepchat_subagents` 实现 | `docs/harness/projects/deepchat-agent-engineering-review-2026-08-10.md` |
| 4 | Goose | 已完成；已重新抽查固定提交并固化独立报告 | `docs/harness/projects/goose-agent-engineering-review-2026-08-10.md` |
| 5 | Grok Build | 已完成；已核验并发入史、loop/fuse、双事件轨、compaction 提交与 child 恢复边界 | `docs/harness/projects/grok-build-agent-engineering-review-2026-08-10.md` |
| 6 | Kimi Code | 已完成；已核验 Wire/kap 持久边界、Tool 调度、Goal、Hook、child/Swarm 与 Task 恢复 | `docs/harness/projects/kimi-code-agent-engineering-review-2026-08-10.md` |
| 7 | Kun | 下一项目 | 待生成 |
| 8 | OpenCode | 待复核 | 待生成 |

## 材料缺口

- Claude Code 核心源码未在本地仓库提供，核心 loop、权限排序、持久化一致性只能列为黑盒行为或不可证实。
- 用户未提供八仓 URL；本轮不把外部网页版本作为固定证据。
- Natives 应用当前无法启动，未执行跨项目 UI/端到端行为实验。
