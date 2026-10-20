# Natives 文档入口

> 版本：v4.1 · 维护日期：2026-10-18

Natives 是单一用户产品：共享页面供 Chrome 扩展与本机 Web 使用，Rust native-file-host 提供 Files 与产品配置，Go Model Host 提供 AI Provider/OAuth/Keychain/loopback，Rust natives-app-runtime 提供内置应用通用运行时，Fund 等功能作为 Monorepo 内置应用模块随同一个安装包交付。

## 当前架构

- 业务界面：共享 `extension/` 页面供 Chrome 扩展与 ADR-0042 本机 Web 使用；Service Worker 无状态。双入口迁移和验收的当前状态见 ROADMAP。
- 文件与产品 Host：`crates/native-file-host`；文件领域逻辑：`crates/file-manager-core`。
- AI Host：`model-host`；投资模块推理经双入口共享的通用会话代理进入 Model Host（ADR-0037），密钥不进入模块 iframe；不得扩展为通用 Agent、Jobs、Plugin Runtime 或第二数据库权威。
- 内置模块与统一运行时：所有官方内置应用归入 `modules/` 并编译进统一 `natives-app-runtime`，系统 Native Messaging 注册单一 `com.natives.app_runtime`。每个活动 appId 由 Core 持有一个 Runtime，多 Surface 共享；最后一个 owner 释放且无受保护任务时保留 60 秒宽限，之后退出回收在 2 秒内完成；没有模块下载、独立可执行文件、独立 Native Host 注册、独立安装或独立 Release。Runtime loopback 统一绑定动态端口 `127.0.0.1:0`（实际端口经 `app:start` 上报），见 ADR-0035 与[端口注册表](standards/technical/05-port-registry.md)。外部 AI 查询统一收口至 `model-host:8766`（ADR-0033）。
- 性能、内存回收、安全、数据和可访问性规则继续有效，分别见 `standards/technical/04-performance.md`、`02-security.md`、`03-data.md`、`06-built-in-modules.md`、`07-built-in-app-development.md` 和 `ui-ux/`。

## 阅读顺序

验收执行频率见[性能规范：长测频率与结果复用](standards/technical/04-performance.md#长测频率与结果复用)：完整 30 分钟长测只需一次有效验收，后续先评估差异并复用，不作为每轮整改／交付的默认检查。

非技术说明：[架构白话图解](architecture/architecture-explained.html) -- 解释当前分工、内部耦合及建议调整；仅说明与提案，不替代现行规范，不代表整改已实施。

1. [Standards](standards/README.md)
2. 适用 [ADR](adr/)
3. [Contracts](contracts/)
4. 当前架构文档和领域实现
5. [Legacy archive](archive/README.md) 只用于追溯

## 任务速查

| 任务 | 先读 |
|---|---|
| Chrome 扩展＋本机独立 Web 双入口迁移（进行中，未整体验收） | [完整施工包](/Users/ldh/Downloads/project/plan/memory/dual-entry-migration/README.md)、[当前收尾核验](/Users/ldh/Downloads/project/plan/memory/dual-entry-migration/evidence/20261018-closeout/acceptance.md)、[当前逐页对照](/Users/ldh/Downloads/project/plan/memory/dual-entry-migration/evidence/20261018-closeout/page-comparison.html)；实际进度只在 ROADMAP 汇总。旧截图、局部脚本与诊断长测不自动继承完整验收。 |
| 2026-10-11 投资产品流程与已确认原型实施 | [当前实施方案](/Users/ldh/Downloads/project/plan/memory/2026-10-11-investment-implementation-plan.md)、[执行提示词](/Users/ldh/Downloads/project/plan/memory/2026-10-11-investment-agent-execution-prompt.md)、[设计与事实索引](/Users/ldh/Downloads/project/plan/memory/README.md)；覆盖资产分布／组合设置／目标水位、首页原地 AI、私人持仓与基金四部分详情及全页面回归。原型仅作设计参考，真实数据契约与工程规范优先；生产实施状态见 ROADMAP |
| 投资私人详情与资讯决定 | [数据与生命周期补充契约](contracts/fund-holding-detail-contract.md)：历史账户份额、价格覆盖、报告引用与 v28 增量决定记录；当前验证状态见 ROADMAP |
| 可维护双主题与投资全页面整改 | [DES V2.1](/Users/ldh/Downloads/project/plan/des/README.md)、[五个宿主／辅助页与投资索引](/Users/ldh/Downloads/project/plan/des/03-page-matrix.md)、[任务](/Users/ldh/Downloads/project/plan/des/05-implementation-tasks.md)、[44 项验收](/Users/ldh/Downloads/project/plan/des/06-acceptance.md)、[执行提示词](/Users/ldh/Downloads/project/plan/des/07-agent-execution-prompt.md)；先单源静态主题／严格构建再投资全页；含图形／控件／富内容和调色维护演练，仅 Archive／Volt，保留用户配置和金融契约；实施与验证状态见 ROADMAP |
| 模块边界、内部 API、状态与 SDK 解耦整改 | [完整 Plan](/Users/ldh/Downloads/project/plan/mode/README.md)、[施工任务](/Users/ldh/Downloads/project/plan/mode/06-implementation-tasks.md)、[执行提示词](/Users/ldh/Downloads/project/plan/mode/08-agent-execution-prompt.md)、[ADR-0039](adr/0039-module-boundaries-and-internal-contracts.md)、[技术 08](standards/technical/08-module-boundaries-and-contracts.md)；`mode/P/T/A` 与父投资方案编号分开，源码状态以 ROADMAP 为准 |
| 投资产品升级与 PanWatch R2 页面／功能迁移 | [最新方案目录](/Users/ldh/Downloads/project/plan/README.md)、[实施任务](/Users/ldh/Downloads/project/plan/06-implementation-tasks.md)、[验收](/Users/ldh/Downloads/project/plan/07-acceptance.md)、[执行提示词](/Users/ldh/Downloads/project/plan/08-agent-execution-prompt.md)；范围为持仓替换、首页／机会合并、单金额目标与财务设置；当前开发状态见根 ROADMAP.md |
| 单一产品、内置 Fund 和 App Center | [托管应用契约](contracts/managed-app-contract.md)、[内置模块标准](standards/technical/06-built-in-modules.md)、[应用中心实施方案](development/app-center-fund-implementation-plan.md)。注：fund = 投资终端（含基金记账），displayName "投资/Invest" |
| 安装、引导、打包和发布 | [应用中心实施方案](development/app-center-fund-implementation-plan.md)、[当前架构](architecture/ARCHITECTURE.md) |
| Files | [Files 审计](architecture/FILE_MANAGER_AUDIT.md) |
| Files PDF 安全例外 | [ADR-0043](adr/0043-native-chrome-pdf-preview.md)：仅 Host 校验的 PDF Blob 使用 Chrome 内置阅读器，其余 iframe 沙箱不变 |
| AI 用量、成本、会话和提醒 | [AI 效能总方案](development/ai-efficiency-components-plan.md)、[整改方案](development/ai-efficiency-remediation-implementation-plan.md)、ADR-0030 |
| AI 用量／额度／会话查询边界 | [ADR-0033](adr/0033-model-host-readonly-query-port.md)、[ADR-0035](adr/0035-retire-fixed-app-runtime-port.md)、[端口注册表](standards/technical/05-port-registry.md)；[旧收口方案](development/tokenusage-consolidation-plan.md) 仅供历史追溯 |
| 性能、进程生命周期和内存回收 | [性能整改方案](architecture/application-performance-remediation.md)（含 T0～T6 施工、L01～L16 验收与 Agent 交接提示词）、[性能标准](standards/technical/04-performance.md) |
| Provider、OAuth、Secret、路由内核与内置模块推理 | ADR-0020、[ADR-0037](adr/0037-built-in-module-inference-broker.md)、[ADR-0040](adr/0040-bounded-streaming-module-inference.md)、[ADR-0041](adr/0041-cliproxyapi-v8-kernel-migration.md)、[Security](standards/technical/02-security.md)、[Data](standards/technical/03-data.md) |
| 内置模块 Secret Keychain 能力与深链视图白名单 | [ADR-0038](adr/0038-module-secret-capability-and-view-whitelist.md)、[托管应用契约](contracts/managed-app-contract.md)、[Security](standards/technical/02-security.md) |
| 本地 loopback 端口申请与登记 | [端口注册表](standards/technical/05-port-registry.md)、ADR-0032、ADR-0033、ADR-0035 |
| 旧方案处置 | [Legacy 删除证明](architecture/legacy-death-list.md)、[历史归档](archive/README.md) |

## 变更纪律

活动目录中的文档必须描述当前代码和当前决策。废弃方案移入 `docs/archive/`；ADR 保留历史正文但必须有 supersedes 关系。放宽 MUST 或改变产品边界先更新 ADR，再同步 Standards、Contracts 和门禁。完成声明必须区分本地验证、发布验证和未验证项。
