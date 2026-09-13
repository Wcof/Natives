# ADR-0030（草案）：AI 使用组件——三层账本、统一计量边界与受限采集/提醒入口

- 状态：**accepted**（2026-09-11 随 T0–T10 实施指令接受；T0 契约修复在此前已先行完成，不依赖本 ADR）。
- 关联：补充 [ADR-0028](0028-ai-performance-usage-widget-regression.md)（其"手动导入、自动采集另立 ADR"的约定由本 ADR 承接）；遵循 [ADR-0020](0020-ai-native-personal-workspace-rearchitecture.md) 的 Model Host 用量权威与 R-S12；不修改 [ADR-0029](0029-unified-suite-preinstalled-apps.md) 的应用中心模型。
- 实施输入：[空间 AI 效能组件实施方案](../development/ai-efficiency-components-plan.md)（本轮修订版，2026-09-11；§4 独立组件、§5 空间视觉、§7 数据契约、§8 R0–R8 执行清单）。

## 背景与选择

当前 AI 效能组件只能回答"用了多少"，不能回答个人开发者的四个核心问题：额外花了多少、额度还能用多久、哪个任务在等待、如何降本。核验（见方案 §3.2）还发现契约缺陷：成功率二次乘 100、明细时间字段错配、缓存成本重复计入、缺价计为零——其中大部分已在 T0 修复（成功率/时间/缓存/缺价 costStatus），本 ADR 负责其余结构性边界。

选择：在 Model Host 单一用量权威内扩展三层账本（账单/用量/活动归因），以唯一计费原子（billingAtom）消除重复计费，以静态适配器注册表逐工具核验数据源能力，以受限一次性事件入口承接工具 hook，以有界 Spike 验证 macOS 原生通知。不选择：新建第二用量库、通用任务系统、常驻采集进程、通用 OTel 平台、自动拦截请求。

## 决策

1. **三层完整性分离**：账单完整性（实际花费）、用量完整性（消耗了什么）、活动归因完整性（由什么造成）分别声明覆盖，禁止用一个百分比混合。每条金额/用量带 `evidenceLevel`：`actual_charge` / `provider_reported_usage` / `local_estimate` / `activity_only`；不同等级不相加。
2. **唯一 billingAtom**：同一收费原子只能进入金额总和一次；session/agent/subagent/Skill/plugin/MCP 只能通过 `direct_owner` 汇总、`association` 证明参与；`allocation` 仅在有可验证分量或用户明确分摊规则时建立。父子树求唯一原子之和，禁止父会话与子代理重复计费。
3. **静态 adapter registry**：13 个基线工具逐个声明 `historicalUsage/liveEvent/billing/quota/attribution/notification/privacy` 能力，状态只能是 `implemented/partial/unsupported/unavailable`；"配置已支持"不等于"用量支持"。没有结构化来源的工具完成审计后显式返回 `unsupported`，不从统计范围删除，也不猜测文件名冒充支持。
4. **受限一次性事件入口**：已安装 `model-host` 二进制新增 `tool-event` 类受限命令模式（参数名实现前冻结）：读取有上限的 hook/notify/SSE 输入 → 白名单元数据 → 复用既有 singleinstance relay/worker 幂等写入 → 评估提醒 → 退出。总时限约 3 秒、无无限重试；不开模型代理、不接任意 RPC、不创建第二个 DB 写进程。失败不阻塞原 AI 工作，错误显示在集成诊断。
5. **Claude OTel 边界（默认关闭）**：仅当用户显式开启精细归因并允许 Model Host 常驻时，启用 loopback、Claude-only 受限接收器（白名单 `service.name=claude-code`；拒绝远程地址/未知信号/过大 body；默认不落盘 prompt、代码、路径、凭据）。已有外部 OTLP 配置不得静默覆盖；配置变更走 inspect → plan → backup → apply → verify/rollback。未开启常驻时不承诺页面关闭后的 OTel 实时完整性。
6. **本地采集为有界增量**：首次用户触发导入；增量在页面可见恢复/显式点击/受支持的工具结束事件上进行。每来源同一时刻最多一次扫描；前台增量建议上限 2 秒/10 MiB，超限返回 checkpoint+partial；游标与事件同事务提交；拒绝 symlink 越界与非普通文件。只保留用量元数据，不持久化 Prompt/代码/工具参数/凭据。
7. **提醒不接管执行**：提醒顺序 等待许可 > 等待输入 > 错误 > 本轮结束；默认只提醒前三类。确定动作只有"标记已查看""复制恢复指令"（由 Host 按工具 ID/参数模板生成，不拼接任意 shell）；不自动批准、不发送消息、不续跑任务。同事件不重复入箱；离线历史导入不发过去的通知。
8. **macOS 通知（V1 唯一验收平台）**：复用/新建系统通知的**固定程序 + 固定脚本 + 独立 argv** 调用路径（先验证 `/usr/bin/osascript` 固定 `display notification`），禁止把外部文本拼成 AppleScript/shell；不创建独立 `.app`/托盘/LaunchAgent。通知记录区分 queued/submitted/failed；若真机无法可靠投递，本项 Spike 判失败，保留收件箱但不宣布"后台通知已完成"。
9. **数据落在现有用量库**：`usage_events` 增量补语义字段；新增 `usage_sources/usage_import_cursors/usage_sessions/usage_budgets/usage_alerts/usage_subjects/usage_subject_edges/usage_charge_components/usage_billing_entries/usage_cost_attributions` 等表，全部归同一 Model Host 管理；沿用 365 天/50 万事件上限，提醒 30 天/1000 条、会话 90 天/1 万条、游标 5000 项；预算周期在历史裁剪时显式标注不完整。金额用有界整数微单位 + 显式币种，多币种分列，不用浮点累计。

## 生命周期与所有权（不变）

- `model-host/internal/usage`：用量、计费、预算、提醒状态唯一写入权威；`internal/quota` 只做额度适配；`internal/agentclients` 只做检测/授权根定位/配置事务。
- Extension：Widget 投影与用户操作；不扫描文件、不解析私有日志、不取 Secret、不直连提供方。Files Host 不写用量副本。
- Service Worker 保持无 Native Port/轮询/保活；查询只在既有 owner 页面消费。

## 不做的事

不建通用任务系统/Plugin Runtime/通用 OTel 平台；不做自动切号、自动重试、自动省钱路由；不做请求硬拦截（如需 Proxy 硬预算另立 ADR）；不读取浏览器 Cookie 填充额度；不在本轮把 AI 视图打包为 managed app。

## 接受后实施顺序

T2（能力矩阵与 source contract fixture）→ T3（schema/账本/去重）→ T4（适配器）→ T5（Provider 对账）→ T6（事件/OTel/归因）→ T7（六视图）→ T8（额度/预算/提醒）→ T9（降本建议）→ T10（真机闭环与门禁）。T0 已完成部分不需重复实施。

## Spike 记录（实施前核验）

- **通知 Spike**：macOS + `/usr/bin/osascript` 固定 `display notification` 路径以独立 argv 调用，退出码 0、通知出现即为路径可用；命令模板与失败分支记录于实施证据，不再重复本 ADR。
- 结果（2026-09-11 回填）：本机 macOS 执行 `/usr/bin/osascript 'display notification ... with title ...'`（固定文案、无外部输入拼接）退出码 **0**，系统接受通知请求。固定程序 + 固定脚本的调用路径初步可用；用户侧实际可见性（通知权限/专注模式/通知身份归属 osascript）与 queued/submitted/failed 三态记录仍按 T8 真机验收，不因本次退出码 0 提前宣布"后台通知已完成"。
