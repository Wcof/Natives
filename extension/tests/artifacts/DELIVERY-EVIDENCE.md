# AI 效能组件本轮交付证据报告

生成时间：2026-09-12（第二轮补证后更新）。本报告是聊天总结被截断后的持久证据底稿；所有命令可复现，
路径均相对仓库根。开发/本地验证不构成发布授权；所有改动未提交。

## 〇、第二轮补证（本轮新增，全部现场复现）

### O.1 §7.2 计费/去重正确性（Go verbose，全部通过）

| 证据项 | 复现命令 | 结果 |
|---|---|---|
| 缓存计费 $0.28（非重叠桶） | `cd model-host && rtk go test ./internal/usage/ -v -run TestCalculateCostCacheBuckets` | 2 passed |
| Codex 累计快照求差 / 事件去重 / billingAtom 幂等 | `… -run "TestParseCodexCumulativeDiff\|TestNativeEventInsertDedup\|TestBillingAtomDedup"` | 3 passed |
| 缺价 ≠ 免费 | `… -run TestCalculateCostMissingPriceIsNotFree` | 1 passed |

### O.2 R4 来源审计（本机探测，目录/版本级，未读日志内容）

| 工具 | 本机实测 | 审计结论（已冻结进 sources.go Audit 字段，矩阵测试锁定） |
|---|---|---|
| Claude Code | 已装 2.1.268，~/.claude 199M | collector implemented（native_log + fixture） |
| Codex | ~/.codex/sessions 存在 | collector implemented；累计求差语义已测试 |
| OpenCode | 已装 1.18.3 | **opencode-store/1 parser 本轮落地**（见 O.3） |
| Grok Build | 已装 grok 0.2.118（~/.grok） | usage/quota/event source 待核验 |
| ZCode | ~/.zcode 存在（v2 会话结构），CLI 不在 PATH | 显式 unsupported，待核验 |
| Pi | ~/.pi 存在 | collector implemented（pi-session/1） |
| Hermes | ~/.hermes 存在 | collector implemented（state.db 白名单只读） |
| Claude Desktop | ~/Library/Application Support/Claude 存在 | 不得套用 Claude Code 日志，独立审计未完成 |
| Cursor / AtomCode / DeepSeek Harness / Kimi Code / OpenClaw | 未安装（目录缺失 / CLI 不在 PATH） | 待安装版本 fixture；显式 unsupported |

### O.3 OpenCode 采集器（R4 实质推进，新增）

- `model-host/internal/usage/opencode_store.go`：本机安装版本（1.18.3）实测 schema 的只读白名单投影
  （message 表 → role/modelID/providerID/cost/tokens/time）；**OpenAI 口径 cache 是 input 子集，
  adapter 转换一次为非重叠桶** input_uncached = input − cache_read − cache_write（§7.2）；
  计量原子 `opencode:<message.id>` 幂等；不读 part 正文/工具参数。
- `opencode_store_test.go`：fixture 冻结契约（2 passed）——含 cache 子集不重复计费断言
  （input=10639, cache read 800/write 120 → input_uncached=9719, total=11059）、坏行跳过、
  无 session 归属保留空 ID（未归属由 GetSessions 单独计数）、外来 schema 显式拒绝。
- collector.go 接入 `~/.local/share/opencode/opencode.db` 采集路径；矩阵 opencode
  HistoricalUsage → implemented、Attribution → partial，双向一致性测试同步（42 passed）。

### O.4 R6/R7 证据（Go + 前端 verbose，全部通过）

| 证据项 | 复现命令 | 结果 |
|---|---|---|
| 提醒回归/确认/优先级 + insights + 会话 distinct 语义 | `… -run "TestSessionStateNoRegression\|TestAcknowledgeAttention\|TestAttentionOrderPriority\|TestInsightsEngine\|TestGetSessionsDistinctSemantics"` | 5 passed |
| 前端 AI 组件 + Space | `node --test extension/ai-performance.test.mjs extension/space.test.mjs` | pass 2 / fail 0 |
| 七组件 Space 浏览器验证 | `node extension/tests/space-seven-widgets-verify.test.mjs` | exit=0，四场景全绿 |

### O.5 13 工具逐能力矩阵全表（`go test -v` dump，落盘于 go-matrix-billing-evidence.log）

复现：`cd model-host && go test ./internal/usage/ -run TestDumpCapabilityMatrixForEvidence -v`
落盘日志：`extension/tests/artifacts/go-matrix-billing-evidence.log`（矩阵全表 + §7.2 计费 7 项 PASS）。

| 工具 | HIST | LIVE | BILL | QUOTA | ATTR | NOTIF | PRIVACY |
|---|---|---|---|---|---|---|---|
| claude-code | IMPL | IMPL | UNAVL | UNAVL | PART | IMPL | metadata_only |
| codex | IMPL | PART | UNAVL | UNAVL | PART | PART | metadata_only |
| opencode | IMPL | UNSUP | UNAVL | UNAVL | PART | UNSUP | metadata_only |
| pi | IMPL | UNSUP | UNAVL | UNAVL | PART | UNSUP | metadata_only |
| hermes | IMPL | UNSUP | UNAVL | UNAVL | PART | UNSUP | metadata_only |
| kimi-code | UNSUP | UNSUP | UNAVL | UNAVL | UNSUP | UNSUP | metadata_only |
| zcode | UNSUP | UNSUP | UNAVL | UNAVL | UNSUP | UNSUP | metadata_only |
| deepseek-harness | UNSUP | UNSUP | UNAVL | UNAVL | UNSUP | UNSUP | metadata_only |
| claude-desktop | UNSUP | UNSUP | UNAVL | UNAVL | UNSUP | UNSUP | metadata_only |
| grok-build | UNSUP | UNSUP | UNAVL | UNAVL | UNSUP | UNSUP | metadata_only |
| openclaw | UNSUP | UNSUP | UNAVL | UNAVL | UNSUP | UNSUP | metadata_only |
| cursor | UNAVL | UNAVL | UNAVL | UNAVL | UNAVL | UNAVL | metadata_only |
| atomcode | UNAVL | UNAVL | UNAVL | UNAVL | UNAVL | UNAVL | metadata_only |

逐条 AUDIT 审计结论（含来源版本、缺口原因、下一步）见落盘日志 AUDIT[...] 行（13 条齐全）。
UNSUP = 已实现范围内无该能力（有审计结论）；UNAVL = 本机未安装/未授权（cursor/atomcode 为
目录探测证实的环境缺口）。矩阵 = 注册表 ∪ 用户名单，由 sources_matrix_test.go 三项测试锁定。

### O.6 第三轮现场复核（2026-09-12，全部当轮运行，非引用旧结论）

R1 七组件注册链路（当轮命令输出）：
- sanitizer 白名单 ai* key = 8（七个新 key + 旧 aiPerformance 兼容）
- space-catalog.js ai* key = 8；Rust WIDGET_KEYS ai* = 8；两端一致
- widgets/index.js 由 definition 拼接 key（字面量 grep=0 为预期）
- `node --test extension/ai-performance.test.mjs extension/space.test.mjs` → pass 2 / fail 0

R2 空间外观桥接（当轮重跑）：
- `node extension/tests/space-seven-widgets-verify.test.mjs` → exit=0，四场景全绿
- space-dark / space-light / global-light-space-dark-reverse / explicit-colours-dark
- 每场景 7 widgets mounted；派生默认对比度 ≥4.5:1（#101210 / #f4f1e8 / #20242c）；显式逐卡颜色保留

R4–R6 当轮测试：
- `cargo test -p native-file-host widget` → 5 passed / 0 failed（WIDGET_KEYS 白名单）
- Go：TestSessionStateNoRegression / TestAcknowledgeAttention / TestAttentionOrderPriority /
  TestInsightsEngine / TestGetSessionsDistinctSemantics → 全部 PASS
- `rtk npm run model:host:check` → Go 99 测试 / 12 包通过（含矩阵 dump 测试）

R4 采集闭环（累计）：claude-code、codex、opencode、pi、hermes 共 5 来源有 collector+fixture；
其余 8 工具为带 AUDIT 审计结论的未完成状态（cursor/atomcode 本机未安装属环境缺口），
矩阵三锁定测试保证不夸大不低估。

### O.7 四项门禁现场复核（2026-09-12，exit code 均为 0，日志落盘本目录）

- gate-extension-check.log：extension:check exit=0（"apps install failed: sample APP_INVALID_STATE" 为 apps.test.mjs 预期负路径输出，非失败）
- gate-model-host-check.log：model:host:check exit=0，Go test 99 passed / 12 packages
- gate-perf-check.log：perf:check exit=0，串行单独运行；idleCPUPercent=0（预算 5），node 测试 fail 0
- gate-cargo-fmt.log：cargo fmt --check exit=0
- gate-cargo-test.log：cargo test --workspace exit=0，181 passed / 0 failed（主仓库根、无独立 CARGO_TARGET_DIR）

### O.8 R4 新增 ZCode 采集器与 R7 交互复现（2026-09-12，当轮运行）

R4（来源 5→6）：
- 新增 `zcode_rollout.go`（parser zcode-rollout/1）+ collector 接线（collectZCodeRollout），
  计量原子 `zcode:<requestId>`，白名单投影不读 body/text；fixture 测试 2 项 PASS
  （TestParseZCodeRolloutJSONL / TestParseZCodeRolloutTotalOverride）。
- 本机探测（键名级，不读正文）：ZCode rollout `type=model_io` 行含
  requestId/sessionId/startedAt/model{modelId,providerId}/usage{input,cacheRead,cacheWrite,output,total}。
- Grok Build 审计冻结：sessions/<proj>/<uuid>/ 的 chat_history/events.jsonl 与 summary.json
  均无 usage/token/cost 字段；memtrace 仅 start/sample 心跳。显式 unsupported。
- Claude Desktop 审计冻结：local-agent-mode-sessions/<uuid>/<uuid>/ 仅含
  remote_cowork_plugins/manifest.json，无结构化用量文件。显式 unsupported。
- 矩阵同步：zcode historicalUsage→implemented；wantImplemented 与 sources_test 断言同步；
  usage 包测试全绿（ok 11.566s）。矩阵状态：HIST=IMPL 6（claude-code/codex/opencode/pi/hermes/zcode）。

R7 交互复现（当轮输出）：
- `space-seven-widgets-verify.test.mjs` PASSED exit=0（四场景各 7 卡、对比度 ≥4.5:1）
- node --test ai-performance + space → pass 2 / fail 0

### O.9 R1/R5/R6/R8 复核（2026-09-12，当轮运行）

R1（Rust 侧七 key 端到端）：
- `cargo test -p native-file-host widget` 通过，含 widget_upsert_reorder_and_background_save、
  duplicate_copies_widgets_in_one_transaction、migration_purges_retired_widgets_and_cleans_personal_templates
  （增/存/排序/复制/迁移清理往返均覆盖）。

R5/R6（verbose 落盘 go-r5-r6-evidence.log，6 PASS）：
- TestBillingEntryKindValidation / TestBillingEntryUpsertByIdempotent（账单层级+幂等）
- TestBillingAtomDedup（唯一计费原子去重）
- TestInsightsEngine（R7 降本建议）
- TestAcknowledgeAttention / TestAttentionOrderPriority（R6 提醒收件箱+优先级）

R8（ZCode 改动后门禁重跑，exit 全 0，日志覆盖本目录）：
- gate-model-host-check.log：Go 101 passed / 12 packages（+2 为 ZCode fixture）
- gate-extension-check.log：exit=0
- gate-perf-check.log：exit=0，snapshotMs=1076.15<1500，idleCPUPercent=0<5

### O.10 Codex 归档/跨批基线 + 外观动态切换缺陷修复（2026-09-12，当轮运行）

R4（Codex 来源补齐，方案 §3.2/§7.3-3）：
- `archived_sessions` 接入采集路径（与 sessions 共用 parser，稳定 atom 去重）；
- 新缺陷修复：累计快照基线跨 checkpoint 持久化——分批读取时 delta 对上一批
  求差（usage_import_cursors 新增 baseline_in/cache/out/epoch 列，与游标同表）；
  atomSalt（文件指纹）消除跨批 atom 冲突（原实现第二批 atom 与首批相同会被
  唯一索引静默丢弃）；
- fixture 测试 4 项 PASS：TestParseCodexCumulativeDiff / SequenceReset /
  CrossBatchBaseline（基线续差 1600-1200=400 而非重算 1600）/ 
  TestCollectCodexArchivedFileNoDoubleCount（归档重导入=0）；usage 包全量
  ok 12.210s。

R2/R8（§5.5 动态切换，appearance-toggle 新场景 PASS）：
- 真实缺陷修复：`deriveReadableText` 读 `getComputedStyle`，而
  .background-layer 有 0.3s background transition——读到过渡起始旧背景
  （旧色快照），切背景后派生文字色不更新。改为优先读插件刚写入的内联
  目标值，回退计算样式（space-dashboard.js）。
- 新断言：同一 dashboard 实例深→浅→深往返，native calls 0→0（改外观
  零数据请求）；派生色随背景更新、往返恢复、7 卡挂载一致。
- harness 修复：snapshot 显式 brightness:1，消除 nightDim 时钟非确定性。
- 复跑：node --test ai-performance+space → pass 2/fail 0；
  extension:check exit=0（gate-extension-check.log 已覆盖更新）。

### O.11 §5.5 字体设置维度 + 字重传导缺陷修复（2026-09-12，当轮运行）

R8 门禁（Codex 跨批基线改动后复跑，exit 全 0）：
- gate-model-host-check.log：Go 103 passed / 12 packages（+2 为 Codex 基线/归档 fixture）。
- gate-perf-check.log：exit=0（串行运行）。

R2（§5.5 字体设置维度，font-settings 新场景 PASS）：
- 新增真实缺陷修复：`.ai-perf-number-value` 消费 `var(--space-widget-weight,700)`
  但 applyWidgetDisplayStyles 从未设置该变量——用户 fontWeight 永远到不了主指标。
  现已传导：设置时 setProperty，未设置时移除（mock 环境无 removeProperty 的
  回退分支保留，space.test.mjs 修复）。
- font-settings 浏览器场景断言（真实 Chrome + Shadow DOM + Native Messaging
  mock 按真实 Host fixture 回包）：默认态主指标 22px/700（不被宿主 32px 传染）；
  覆盖态 fontSize:36/fontWeight:400 全量传导；3 张 number 视图卡采样
  （会话卡默认热力图无主指标属 allowedCharts 正确行为）。
- 全场景 SEVEN-WIDGET SPACE VERIFY PASSED exit=0：4 外观场景 + appearance-toggle
  （往返 0 数据请求）+ font-settings（22px/700 → 36px/400）。
- harness 协议根因记录：widget 数据层走 client.js → chrome.runtime.connectNative，
  不经 dashboard nativeCall；mock 须在 page.goto 前注入 boot HTML，回包
  {id, ok, result} 与 native-client pending 解析一致。
- 复跑：node --test ai-performance+space → pass 2/fail 0（含回归修复）；
  extension:check exit=0（gate-extension-check.log 更新）。

### O.12 §9.0.1 图形切换/双成本卡多实例 + 13 工具矩阵 verbose 复现（2026-09-12，当轮运行）

R7/§9.0.1 浏览器场景（chart-switch 新场景 PASS，真实 Chrome + Shadow DOM + Native Messaging mock 按真实 Host fixture 回包，mock 按 params.range 分发 30d/7d 两份 overview 证明两卡不共用同一查询结果）：
- 四轮往返：两卡 number → A table/B number → A line/B number → A number。
- 断言全过：成本 A（30d）=$0.00、成本 B（7d）=$1.23 独立实例互不影响（A 切表格/趋势时 B 数字不变）；成本表格表头含"费用"列（按 metricId 定义，不硬编码 requests/tokens）；成本趋势 max 标注为美元格式（同口径，不用 Token formatter）；往返回数字无残留。
- 7 场景汇总（space-ai-seven-widgets-verify.json 更新）：4 外观场景（对比度 ≥4.5:1）+ appearance-toggle（往返 0 数据请求）+ font-settings（22px/700 → 36px/400）+ chart-switch（4 rounds）。
- 如实缺口：configJson.toolIds 未流入查询 filter、Host 端无 tool_ids 过滤——"两张不同工具范围成本卡"以不同 range 证明独立实例/查询；工具范围过滤待 R3 补接线。

13 工具矩阵 verbose 复现（go-matrix-billing-evidence.log 更新，5 项测试全 PASS）：
- TestDumpCapabilityMatrixForEvidence / CoversRegistryAndUserList / FieldsCompleteAndValid / TestUsageSourcesBaseline13 / HonestStatuses。
- 落盘 dump 含新状态行：codex HIST=IMPL LIVE=PART、zcode HIST=IMPL（AUDIT 记录 2026-09-12 实测 ~/.zcode rollout model_io 字段与 parser zcode-rollout/1 边界）。
- 矩阵现状不变式：HIST=IMPL 6（claude-code/codex/opencode/pi/hermes/zcode）；grok-build/claude-desktop 有键名级审计的无来源限制；kimi-code/deepseek-harness/openclaw 未安装；cursor/atomcode UNAVL。

### O.13 R1 注册链路 + §5.5 布局维度 + 门禁复跑（2026-09-12，当轮运行）

R1 注册链路证据（r1-registration-evidence.log，node 断言 exit=0）：
- WIDGET_CATEGORIES ai 分类恰为七个独立 key（aiCost/aiTokens/aiSessions/aiRequests/aiLimits/aiAttention/aiSavings），旧 widget/aiPerformance 从目录隐藏。
- sanitizer WIDGET_KEYS=32 含七个新 key 且保留旧 key（兼容读取）。

§5.5 布局维度（space-ai-layout-verify.test.mjs 新增，真实 Chrome + Shadow DOM + fixture mock，exit=0）：
- 4 组合 PASS：1440×900@100%、960×600@100%/125%/200%（缩放以"缩小视口"模拟浏览器缩放本质；documentElement.zoom 会叠加 100vw 制造假横滚，已弃用）。
- 断言：documentElement scrollWidth<=clientWidth（无页级横滚）；3 张 number 卡金额元素 scrollWidth<=clientWidth（金额不截断）。
- 产物：space-ai-layout-verify.json（960×600@200% 即 CSS 视口 480×300）。

门禁复跑（改动后，exit 全 0）：
- gate-model-host-check.log：Go 103 passed / 12 packages。
- gate-extension-check.log：exit=0。
- gate-perf-check.log：exit=0（snapshotMs=678.16<1500、idleCPUPercent=0）。中途真实失败一次：architecture-check 对 space-seven-widgets-verify.test.mjs（749 行）报 UNREGISTERED>=700——按门禁要求在 scripts/perf/architecture-check.mjs ALLOWED_EXCEED_REASONS 登记 rationale 后通过。

### O.14 R5/R6/R7 verbose 复现 + §5.5 形态矩阵截图 + R8 门禁复跑（2026-09-12，当轮运行）

R5/R6/R7 Go verbose 新鲜复现（go-r5-r6-evidence.log 更新，9 项全 PASS，ok 1.633s）：
- §7.2 黄金计费示例：TestCalculateCostCacheBucketsNonOverlapping 源码断言 200k uncached + 800k cacheRead（输入 $1/1M、缓存读 $0.1/1M）= 280000 微单位（$0.28），并含"total-as-input 重复计费"回归护栏。
- 唯一原子去重：TestBillingAtomDedup。真实会话 distinct：TestGetSessionsDistinctSemantics。
- 对账三影响分离：TestReconcileSeparatesThreeImpacts、TestBillingEntryKindValidation、TestBillingEntryUpsertByIdempotent。
- 提醒链路：TestAttentionOrderPriority（许可>输入>错误>轮次结束）、TestAcknowledgeAttention。降本建议：TestInsightsEngine。

§5.5 图形与状态矩阵截图（space-ai-form-matrix-screenshots.mjs 新增，真实 Chrome + Shadow DOM，exit=0）：
- 30 张截图：核心三卡（aiCost/aiTokens/aiSessions）× 各自 allowedCharts 5 形态 × 深/浅空间背景（tests/artifacts/ai-form-matrix/）。
- ready 态经真实 Host 导出 fixture + chrome.runtime Native Messaging mock（§9.4 契约 fixture，不手写；sessions 走 model_usage_sessions 真实聚合协议）。
- manifest.json 标注"测试数据 fixture，非生产数据"，不进生产数据。

R8 门禁复跑（改动后，exit 全 0）：
- gate-model-host-check.log：Go 103 passed / 12 packages。
- gate-extension-check.log：exit=0。
- gate-perf-check.log：exit=0（snapshotMs=675.71<1500、idleCPUPercent=0）。

### O.15 R4 补强：AtomCode/Hermes/Pi 三列证据与审计冻结（2026-09-12，当轮运行）

本机数据根元数据审计（目录/文件名/键名级，不读正文，符合 §6.2 探测边界）：
- AtomCode：~/.atomcode/history-v2/<hash>/entries.jsonl 存在（5 会话文件），首行顶层键仅 [text]，无 usage/token/session/cost 结构化字段 → 键名级审计冻结为无计量来源（产品限制），sources.go 审计文案已按证据更新（含重审条件：新版本引入结构化字段后冻结脱敏 fixture）。
- Hermes Agent：~/.hermes 仅 config.yaml + skills，无 state.db/profiles/session 文件 → 安装不完整，本机 UNAVL（数据可用状态）与官方来源已核验（HIST=IMPL，hermes-state/1 parser）分开记录。
- Pi：~/.pi/agent/sessions/ 存在 3 个真实 session JSONL（时间戳+UUID 命名，项目目录映射），与已实现 pi_session parser 对应。

R4 fixture verbose 验收（go-pi-hermes-evidence.log，4 项全 PASS，ok 0.504s）：
- TestParsePiSessionJSONLFixture / TestParsePiSessionJSONLCacheWrite1h（缓存写桶 1h TTL 计价分支）。
- TestParseHermesStateDBProjection / TestParseHermesStateDBRejectsNonProfileDB（白名单投影 + 非 profile DB 拒绝边界）。

矩阵 dump 复现（go-matrix-billing-evidence.log 更新，5 项 PASS）：
- pi / hermes HIST=IMPL（工具能力）与本机数据可用状态分开；atomcode UNAVL 全列 + 键名级审计文案。
- sources_test/matrix 测试在 atomcode 审计文案更新后回归 PASS。

R4 六工具三列现状（§9.0 门槛工具）：Claude Code/Codex/ZCode/OpenCode/Pi/Hermes HIST=IMPL（Token/成本/session parser 有 fixture 验收）；AtomCode 键名级审计无来源（产品限制非未做）；Cursor UNAVL（个人端账单导入待手动入口）；grok-build/claude-desktop 有键名级审计的无来源限制；kimi-code/deepseek-harness/openclaw 本机未安装。

### O.16 toolIds 接线 + 13 工具逐格证据矩阵 + 门禁复跑（2026-09-12，当轮运行）

toolIds 前后端接线（R3/§7.4 缺口修复，O.12 记录的交付缺口收口）：
- 前端：normalizeConfig 保留 toolIds（仅合法字符串）、widget load() 透传、metrics.js fetchDataset 把 toolIds 构造为 { sources } 传给 overview/analysis/events/sessions 四条查询；空/未配置 = 不携带 sources（全部已接入来源，scopeMode 语义）。
- Host：sources JSON → usage.Filter.Sources 天然映射，overview/analysis/events/sessions 共用 buildFilterWhere 的 source IN (...) 子句。
- Host 测试 TestSourcesFilterScopedToAllAggregates PASS：四聚合按来源收窄 + 空 Sources=全部 + 跨工具同 session ID 不合并（claude-code 与 codex 的 sess-1 记 distinct 2）。
- 前端测试 ai-performance.test.mjs 新增断言 PASS："toolIds forwarded as sources across overview/analysis/events/sessions"（非法元素丢弃、空数组省略 sources、sessions 分支同样透传）。

13 工具逐格证据矩阵（sources_matrix_dump_test.go 重写，go-matrix-cell-evidence.log，4 项 PASS）：
- 新增逐格原因冻结登记表 cellReasons（每工具 HIST/LIVE/BILL/QUOTA/ATTR/NOTIF 六格），测试强制一致性：非 IMPL 格必须登记原因（§9.0：状态必须有原因，unsupported ≠ 未做），IMPL 格不得遗留原因（升格时同步清理）。
- dump 输出含逐格状态表 + 71 处逐格原因（环境缺口/产品限制/交付缺口分类标注）+ 工具级 AUDIT。
- 现状：HIST=IMPL 6（claude-code/codex/opencode/pi/hermes/zcode）；claude-code LIVE/NOTIF=IMPL；grok-build/claude-desktop/atomcode 为有键名级审计证据的产品限制；kimi-code/deepseek-harness/openclaw 本机未安装（环境缺口）；cursor 个人端无实时来源。

R8 门禁复跑（toolIds 接线与矩阵改动后，exit 全 0）：
- gate-model-host-check.log：Go 104 passed / 12 packages（+1 Sources 过滤测试）。
- gate-extension-check.log：exit=0。
- gate-perf-check.log：exit=0（snapshotMs=1082.35<1500、idleCPUPercent=0）。
- 本轮未修改 Rust（仅 Go/JS），cargo fmt/test 门禁不触发。

### O.17 R5/R6 缺口验收 + §5.5 状态矩阵 + R7 闭环 + 门禁复跑（2026-09-12，当轮运行）

R5/R6 缺口验收（budgets_test.go 新增，go-budgets-evidence.log，3 项 PASS）：
- TestBudgetThresholdDedupAcrossPeriods：预算跨周期去重（UNIQUE(budget_id, period_key, threshold, kind)）——day1 触发 80 一次、同周期重评不重复、周期按用户时区（Asia/Shanghai）翻转到 day2 后 80/100 各再触发一次，收件箱恰 3 条；事件严格早于评估时刻（半开区间 [start, now)）。
- TestBudgetSuppressAlertsForHistoryImport：历史导入只算不弹（SuppressAlerts 报告越过阈值、零入箱，§4.3 不批量弹窗）。
- TestEvidenceLevelsNeverMergedInReconcile：Reconcile 只汇总账务事实（actual_charge/provider_reported_usage），用量估算永不并入已确认支出；证据等级读取往返保留；空账户返回 0 条（§7.5 无假账单，seedBaselineBillingIfEmpty 已确认不在生产代码）。
- 真实缺陷修复：EvaluateBudgets 的 Triggered 原本把"越过阈值"直接追加，未按 UNIQUE 去重过滤，与"本次新触发的阈值"契约不符——已改为按 INSERT OR IGNORE RowsAffected 过滤，重复评估/已触发不再重复报告。

提醒去重/状态 verbose（go-attention-states-evidence.log，6 项 PASS）：同事件重发不重复入箱、ended 不进收件箱、状态不倒退、已查看、通知 queued/submitted/failed 区分、许可>输入>错误优先级。

§5.5 图形与状态矩阵截图（space-ai-state-matrix-screenshots.mjs 新增，exit=0，真实 Chrome + Shadow DOM）：
- 21 张截图落盘 ai-state-matrix/：ready 3 / empty 5 / error 5 / loading 5 / unpriced 3（三卡 number × 5 状态 + 成本/Token line × empty/error/loading）。
- 缺价状态断言：estimatedCostUsd=null 时金额不伪装 0（§9.1 缺价≠免费）；error 为 Host 调用失败显式文案；loading 为应答延迟期状态。
- manifest.json 标注"测试数据 fixture，非生产数据"。

R7 交互闭环 verbose（go-r7-interactive-evidence.log，4 项 PASS）：提醒已查看（AcknowledgeAttention）、建议引擎与忽略规则（InsightsEngine，按 ruleKey 持久化、7 天到期恢复）、同事件重发去重、优先级排序。

R8 门禁复跑（budgets.go 修复 + 新测试 + 状态矩阵脚本后，exit 全 0）：
- gate-model-host-check.log：Go 107 passed / 12 packages（+3 预算/证据等级测试）。
- gate-extension-check.log：exit=0。
- gate-perf-check.log：exit=0（snapshotMs=1084.46<1500、idleCPUPercent=0）。
- 本轮未修改 Rust，cargo fmt/test 门禁不触发。

### O.18 §5.5 对比度证据 + R7 钻取核验 + 门禁复跑（2026-09-12，当轮运行）

§5.5 对比度证据（space-ai-contrast-verify.mjs 新增，真实 Chrome + Shadow DOM，exit=0）：
- 8 采样全 PASS（space-ai-contrast.json 落盘）：主指标（metric-large，≥20px/600 大号文字档）6 采样 ≥3:1——深背景 17.28、浅背景 14.65；表格正文（table-text，4.5:1 档）2 采样——深背景 17.28、浅背景 14.65。
- WCAG 2.1 相对亮度合成计算：半透明前景按实际合成色评估（§5.5"合成后的实际空间背景"），逐层向上找非透明底色，最终落到空间背景 hex。
- note 标注"测试数据 fixture，非生产数据"。

R7 钻取闭环核验（交互清单全量 grep 结论）：
- 已闭环：提醒"标记已查看"（views.js:101）、建议"忽略（7 天）"（views.js:197）按钮事件真实存在且有 Go 测试（TestAcknowledgeAttention、TestInsightsEngine）。
- 如实缺口：图表 rank/table 点击钻取到"带原筛选详情"的入口不存在（charts.js/metrics.js/widget.js 均无 click 处理器、无 onOpenDetail 回调），§5.1 第 5 条"点击金额/图形进入带原筛选的真实账本/会话/账单详情"未实现——记为交付缺口，待 R7 补线（Host 侧 model_usage_events/analysis 查询协议已就绪，缺前端跳转与筛选透传）。

R8 门禁复跑（对比度脚本与 budgets 测试后，exit 全 0）：
- gate-model-host-check.log：Go 107 passed / 12 packages。
- gate-extension-check.log：exit=0。
- gate-perf-check.log：exit=0（snapshotMs=687.4<1500、idleCPUPercent=0）。
- 本轮未修改 Rust，cargo fmt/test 门禁不触发。

### O.19 R7 钻取闭环实现 + 测试 + 门禁复跑（2026-09-12，当轮运行）

R7 钻取实现（§5.1 第 5 条"点击金额/图形进入带原筛选的真实账本/会话或账单详情"，O.18 记录的交付缺口本轮补齐）：
- extension/model-settings.js：openModelSettings 新增 initialUsageFilter 通道 + setInitialUsageFilter（白名单字段 range/startTime/endTime/model/provider/source/accessKeyId/result，page 重置第一页；loadUsageData 已有的 query 构造天然消费）。
- extension/space-dashboard.js：widget render env 注入 openDrilldown 回调（动态 import model-settings.js，initialPage='usage' + initialUsageFilter）。
- extension/ai-performance/widget.js：ui 透传 range/dimension/openDrilldown（旧环境 openDrilldown=null 不绑定）。
- extension/ai-performance/metrics.js：toRank/toTableRows 保留 Host 维度 key。
- extension/ai-performance/charts.js：drillFilter 组装筛选（dimension=model/provider/source → model/provider/source 槽位）；number 卡整体点击（role=button + Enter/Space 键盘支持）、bar 行、table 行 [data-drill-key] 点击 → ui.openDrilldown(filter)。

钻取测试（extension/tests/ai-drilldown.test.mjs 新增，6 项 PASS）：
- number 点击/Enter → {range:'30d'}；bar 第 2 行 → {range, model:'key2'}；table source 维度 → {range, source:'claude-code'}；table provider 维度 → {range, provider:'openai'}；无回调环境三形态均不绑定 click、不加 drillable 类（旧 env 兼容）。
- 已纳入 package.json extension:check 门禁链（ai-performance.test.mjs 之后）。

R8 门禁复跑（钻取改动后，exit 全 0）：
- gate-extension-check.log：exit=0（含 "AI drilldown tests: 6 passed"）。
- gate-perf-check.log：exit=0（snapshotMs=611.32<1500、idleCPUPercent=0）。
- gate-model-host-check.log：Go 107 passed / 12 packages（本轮未改 Go，回归确认）。
- 本轮未修改 Rust，cargo fmt/test 门禁不触发。

### O.20 R5 对账证据 + R6 额度诚实状态 + §5.5 动态切换 + 门禁复跑（2026-09-12，当轮运行）

R5 对账完整性证据（billing 三项 verbose PASS，go-billing-evidence.log）：
- TestReconcileSeparatesThreeImpacts：多币种 USD/EUR 分列不合并、充值 5000 不进服务消耗（1900=2000+200−300）、credits 抵扣 −1200 不进现金流（7200=2000+5000+200）、余额变化 3800（§7.4 三口径分离）。
- TestBillingEntryKindValidation、TestBillingEntryUpsertByIdempotent：kind 校验与幂等 upsert。
- 盘点结论：既有测试已覆盖全部必需口径，无缺口需补；evidenceLevel 分离已在 O.17（TestEvidenceLevelsNeverMergedInReconcile）落盘。

R6 额度诚实状态证据（quota 新增 honest_state_test.go 3 项 PASS，落盘 go-quota-honest-evidence.log）：
- 不支持的 provider / HTTP 401 / 网络超时三条失败路径均 status=error + windows 空，不伪造剩余百分比（§4.3"不能因 HTTP 成功就显示 100%"）。
- 如实缺口：订阅续费/credits 到期提醒（§4.3 完整交付项）经 grep 核验未实现（renewal/续费/expiresAt 无命中），记为交付缺口，未虚报。

§5.5 动态切换证据（space-ai-appearance-switch.mjs 新增，真实 Chrome + 真实 space-dashboard.js patchWidget 路径，space-ai-appearance-switch.json 落盘 pass=true）：
- 同一 configJson + 新 displayJson 再次 render（产品真实外观更新链路），断言三项全过：数据查询 1→1 不变（外观变化不发请求）、样式实际生效（字号 22px→32px、colour/字重传导）、统计值文本稳定（valueStable）。
- note 标注"测试数据 fixture，非生产数据"。

R8 门禁复跑（quota 测试 + 外观切换脚本后，exit 全 0）：
- gate-model-host-check.log：Go 110 passed / 12 packages（+3 quota 诚实状态测试）。
- gate-extension-check.log：exit=0（含 ai-drilldown 6 passed）。
- gate-perf-check.log：exit=0（snapshotMs=677.58<1500、idleCPUPercent=0）。
- 本轮未修改 Rust，cargo fmt/test 门禁不触发。

### O.21 验收总账：目标条件 → 当轮产物映射（2026-09-12，全部当轮重跑/盘点）

三项交付条件与 §9.0.1 硬验收场景的对应产物（本轮现场重跑或盘点核对，非历史引用）：

1. 七个独立组件目录（R1）：r1-registration-evidence.log（七 key 注册链路）、seven-widgets-verify.log（7 场景）、ai-seven-widgets-space-dark/light.png（深/浅基线，MD5 f4f662…/c1c15c…）。
2. 多 Agent 逐能力闭环（R3/R4/R5）：go-capability-matrix-fresh.log（本轮重跑 TestDumpCapabilityMatrixForEvidence PASS，104 行：13 工具 × HIST/LIVE/BILL/QUOTA/ATTR/NOTIF/PRIVACY 逐格状态 + 71 处逐格原因；HIST=IMPL 6 工具：claude-code/codex/opencode/pi/zcode/hermes；claude-code LIVE/NOTIF=IMPL、codex LIVE/NOTIF=PART）；go-pi-hermes-evidence.log（4 项）；go-matrix-cell-evidence.log（71 原因冻结）；go-billing-evidence.log（多币种分列/充值不重复计/credits 余额分离）；go-budgets-evidence.log（跨周期去重/历史导入抑制/证据等级分离 + EvaluateBudgets 去重缺陷修复）；go-quota-honest-evidence.log（3 条失败路径 status=error+windows 空，不伪造 100%）。环境缺口如实：kimi-code/deepseek-harness/openclaw/cursor 未安装或无来源（逐格带原因）；订阅续费提醒未实现（O.20 记录）。
3. 空间视觉验收矩阵（§5.5，R2/R7/R8）：ai-form-matrix/ 30 张（3 卡 × allowedCharts × 深/浅）+ manifest（标注测试数据不进生产）；ai-state-matrix/ 21 张（ready 3/empty 5/error 5/loading 5/unpriced 3，缺价 null 不伪装 0）；space-ai-contrast.json（8 采样 0 失败，3:1 与 4.5:1 两档，合成底色 WCAG 计算）；space-ai-appearance-switch.json（pass=true：查询 1→1 不变、样式生效、数值稳定，走真实 patchWidget 路径）；space-ai-layout-verify.json（4 组合全 PASS：无页级横滚、3 数字卡可见）；go-r7-interactive-evidence.log + go-r6-r7-fresh-evidence.log（本轮重跑 10 项 PASS：提醒幂等/优先级/已查看/通知三态、建议引擎、预算去重/抑制、证据等级）。
4. R7 钻取闭环（O.18 缺口补齐）：ai-drilldown.test.mjs 6 项 PASS（number/bar/table 三形态 → model/provider/source 槽位带同筛选），纳入 extension:check 链。
5. R8 门禁（最近一轮全部 exit=0）：Go 110 passed/12 packages、extension:check 含钻取 6 passed、perf:check snapshotMs=677.58<1500/idleCPU=0（gate-*.log）。本轮未改 Rust，cargo 门禁不触发。

如实缺口清单（不得标绿）：5 工具未安装或无实时来源（环境缺口，逐格带原因）；grok-build/claude-desktop/atomcode 为审计冻结的产品限制；订阅续费提醒未实现（交付缺口）；OS 真机通知与关 Proxy 真机联调未验收。改动未提交；开发/本地验证不构成发布授权。

### O.22 当轮验收总账（2026-09-12，本轮全部现场重跑，非缓存引用）

本轮针对"逐能力闭环 / R6-R7 / 三门禁 / §5.5"的质疑，全部证据在本轮现场重新执行并落盘：

1. R6/R7 提醒/降本/预算交互（go-r6-r7-fresh-evidence.log，本轮 verbose 重跑）：8 项 PASS / 0 FAIL——TestIngestToolEvent（幂等入箱）、TestSessionState（不倒退）、TestAttention（许可优先级）、TestNotification（queued/submitted/failed 三态，不冒充已查看）、TestAcknowledge（标记已查看）、TestInsight（建议引擎，忽略 7 天恢复见 O.17 go-r7-interactive-evidence.log Dismissed ruleKey 用例）、TestBudget（跨周期/阈值去重 + 历史导入抑制）、TestDismiss。
2. 三门禁（本轮全部 exit=0，gate-*.log 落盘）：model:host:check → Go 110 passed / 12 packages；extension:check → 含 "AI drilldown tests: 6 passed"；perf:check → snapshotMs=690.08<1500、idleCPUPercent=0。
3. 13 工具逐能力闭环（go-capability-matrix-fresh.log，本轮重跑 TestDumpCapabilityMatrixForEvidence PASS）：13 工具 × HIST/LIVE/BILL/QUOTA/ATTR/NOTIF/PRIVACY 逐格状态 + 71 处逐格原因（go-matrix-cell-evidence.log 冻结）。HIST=IMPL 6 工具（claude-code/codex/opencode/pi/zcode/hermes）三列真实验收；claude-code LIVE/NOTIF=IMPL、codex=PART。环境缺口如实保留：kimi-code/deepseek-harness/openclaw/cursor 未安装或无来源；grok-build/claude-desktop/atomcode 审计冻结（产品限制）；订阅续费提醒未实现（O.20 交付缺口）。
4. §5.5 视觉证据（本轮现场重跑）：对比度 8 采样 0 失败（3:1 与 4.5:1 两档、合成底色 WCAG，space-ai-contrast.json 重写）；动态切换 queries 1→1 不变/styleChanged=true/valueStable=true（space-ai-appearance-switch.json 重写）。存量盘点（O.21 已核对）：form-matrix 30 张、state-matrix 21 张（ready 3/empty 5/error 5/loading 5/unpriced 3）、布局 4 组合全 PASS、七卡深/浅基线截图（MD5 f4f662…/c1c15c…），manifest 均标注"测试数据不进生产"。
5. R5 计价/对账（O.20/O.21 已落盘当轮重跑）：缓存 $0.28、多币种分列、充值不重复计、credits 余额分离、evidenceLevel 不相加（go-billing-evidence.log / go-budgets-evidence.log）；quota 诚实状态 3 条失败路径 status=error（go-quota-honest-evidence.log）。

R1/R2/R3 静态与浏览器证据（七 key 注册、空间外观桥接、共享查询契约）见 O.1–O.19 与 O.21；本轮无对应代码变更，不重复运行（核对 DELIVERY-EVIDENCE 对应章节产物文件均在盘）。

如实缺口清单（不得标绿，与前轮一致）：5 工具未安装或无实时来源（环境缺口，逐格带原因）；订阅续费/credits 到期提醒未实现（交付缺口）；OS 真机通知投递与关 Proxy 真机联调未验收。所有改动未提交；开发/本地验证不构成发布授权。

### O.23 当轮验收总账（2026-09-12，本轮全部现场重跑并即时回显）

本轮对"三门禁 / R6-R7 交互 / 13 工具逐能力 / §5.5"的全部证据在本轮会话内重新执行，输出已即时回显于对话，落盘如下：

1. R6/R7（go-r6-r7-fresh-evidence.log，verbose 重跑 8 项全 PASS）：TestIngestToolEventIdempotent、TestIngestToolEventEndedNotInInbox（幂等入箱/结束不入箱）、TestSessionStateNoRegression（状态不倒退）、TestAttentionOrderPriority（许可优先级）、TestAcknowledgeAttention（标记已查看）、TestInsightsEngine（建议引擎）、TestBudgetThresholdDedupAcrossPeriods、TestBudgetSuppressAlertsForHistoryImport（预算去重/历史导入抑制）。忽略 7 天恢复由 O.17 go-r7-interactive-evidence.log Dismissed ruleKey 用例覆盖。
2. 三门禁（本回合全部 exit=0，gate-*.log 覆写）：model:host:check → Go 110 passed / 12 packages；extension:check → "AI drilldown tests: 6 passed"；perf:check → snapshotMs=691.49<1500、idleCPUPercent=0。
3. 13 工具逐能力（go-capability-matrix-fresh.log，本回合重跑 TestDumpCapabilityMatrixForEvidence PASS）：13 工具 × HIST/LIVE/BILL/QUOTA/ATTR/NOTIF/PRIVACY 逐格 + 71 处逐格原因（go-matrix-cell-evidence.log 冻结）；HIST=IMPL 6 工具（claude-code/codex/opencode/pi/zcode/hermes）三列真实验收，claude-code LIVE/NOTIF=IMPL、codex=PART。
4. §5.5（本回合现场重跑）：对比度 8 采样 0 失败（3:1 与 4.5:1 两档、合成底色 WCAG，space-ai-contrast.json 重写）；动态切换 queries 1→1 不变 / styleChanged=true / valueStable=true（space-ai-appearance-switch.json 重写）。存量盘点（O.21 核对）：form-matrix 30 张、state-matrix 21 张、布局 4 组合 PASS、七卡深/浅基线截图，manifest 标注测试数据不进生产。
5. R5 计价/对账（O.20–O.22 已落盘）：缓存 $0.28、多币种分列、充值不重复计、credits 余额分离（go-billing-evidence.log）；quota 诚实状态 3 条失败路径 status=error（go-quota-honest-evidence.log）。

如实缺口清单（不得标绿，与前轮一致）：kimi-code/deepseek-harness/openclaw/cursor 未安装或无实时来源（环境缺口，逐格带原因）；grok-build/claude-desktop/atomcode 审计冻结（产品限制）；订阅续费/credits 到期提醒未实现（交付缺口）；OS 真机通知投递与关 Proxy 真机联调未验收。所有改动未提交；开发/本地验证不构成发布授权。

### O.24 当轮验收总账（2026-09-12，本轮 -count=1 强制重跑 + 全量门禁，输出即时回显）

本轮证据全部在本回合会话内现场执行（Go 用 -count=1 强制非缓存），PASS 输出已直接回显于对话：

1. R6/R7（go-r6-r7-fresh-evidence.log，-count=1 重跑 8 项全 PASS，1.016s）：TestIngestToolEventIdempotent、TestIngestToolEventEndedNotInInbox（幂等入箱/结束不入箱）、TestSessionStateNoRegression（状态不倒退）、TestAttentionOrderPriority（许可优先级）、TestAcknowledgeAttention（标记已查看）、TestInsightsEngine（建议引擎）、TestBudgetThresholdDedupAcrossPeriods、TestBudgetSuppressAlertsForHistoryImport（预算去重/历史导入抑制）。忽略 7 天恢复由 O.17 Dismissed ruleKey 用例覆盖。
2. 13 工具逐能力（go-capability-matrix-fresh.log，-count=1 重跑 TestDumpCapabilityMatrixForEvidence PASS）：13 工具 × HIST/LIVE/BILL/QUOTA/ATTR/NOTIF/PRIVACY 逐格 + 71 处逐格原因（go-matrix-cell-evidence.log 冻结）；HIST=IMPL 6 工具（claude-code/codex/opencode/pi/zcode/hermes）三列真实验收，claude-code LIVE/NOTIF=IMPL、codex=PART。
3. §5.5（本回合现场重跑，exit=0）：对比度 8 采样 0 失败（3:1 与 4.5:1 两档、合成底色 WCAG，space-ai-contrast.json 重写，样例 aiCost/space-light 表格 14.65≥4.5）；动态切换 queries 1→1 不变 / styleChanged=true / valueStable=true（space-ai-appearance-switch.json 重写）。存量盘点（O.21 核对）：form-matrix 30 张、state-matrix 21 张、布局 4 组合 PASS、七卡深/浅基线截图，manifest 标注测试数据不进生产。
4. 三门禁（本回合全部 exit=0，gate-*.log 覆写）：model:host:check → Go 110 passed / 12 packages；extension:check → "AI drilldown tests: 6 passed"；perf:check → snapshotMs=681.11<1500、idleCPUPercent=0。
5. R5 计价/对账（O.20–O.23 已落盘）：缓存 $0.28、多币种分列、充值不重复计、credits 余额分离（go-billing-evidence.log）；quota 诚实状态 3 条失败路径 status=error（go-quota-honest-evidence.log）。

### O.25 perf:check 完成证据回显（2026-09-12，本回合现场重跑）

针对"perf:check 完成证据未回显"的核对要求，perf:check 在本回合重新现场执行：rtk npm run perf:check → exit=0，snapshotMs=598.99（<1500 阈值）、idleCPUPercent=0，输出已直接回显于对话，gate-perf-check.log 覆写落盘。三门禁在本会话内的最新完整状态：model:host:check Go 110 passed/12 packages（gate-model-host-check.log）、extension:check exit=0 含 AI drilldown 6 passed（gate-extension-check.log）、perf:check exit=0 snapshotMs=598.99/idleCPU=0（gate-perf-check.log）。

如实缺口清单（不得标绿，与前轮一致）：kimi-code/deepseek-harness/openclaw/cursor 未安装或无实时来源（环境缺口，逐格带原因）；grok-build/claude-desktop/atomcode 审计冻结（产品限制）；订阅续费/credits 到期提醒未实现（交付缺口）；OS 真机通知投递与关 Proxy 真机联调未验收。所有改动未提交；开发/本地验证不构成发布授权。

### O.26 当轮验收总账（2026-09-12，本回合全量现场重跑并逐项回显）

本轮对"13 工具逐能力 / R5/R6 / §5.5 / 三门禁"的核对要求，全部证据在本回合会话内现场执行并逐项回显于对话：

1. R5/R6/R7 + 矩阵（go-r5-r6-r7-fresh.log，-count=1 强制重跑 14 项全 PASS，1.854s）：TestReconcileSeparatesThreeImpacts（多币种分列/充值不进服务消耗/credits 余额分离）、TestBillingEntryKindValidation、TestBillingEntryUpsertByIdempotent、TestEvidenceLevelsNeverMergedInReconcile（evidenceLevel 不相加）、TestIngestToolEventIdempotent、TestIngestToolEventEndedNotInInbox、TestSessionStateNoRegression、TestAttentionOrderPriority、TestAcknowledgeAttention、TestInsightsEngine、TestBudgetThresholdDedupAcrossPeriods、TestBudgetSuppressAlertsForHistoryImport、TestDumpCapabilityMatrixForEvidence、TestCapabilityMatrixCoversRegistryAndUserList、TestCapabilityMatrixFieldsCompleteAndValid。矩阵逐格状态与 71 处逐格原因见 go-matrix-cell-evidence.log（冻结）；HIST=IMPL 6 工具三列真实验收。
2. §5.5（本回合现场重跑，exit=0）：对比度 8 采样 0 失败（3:1 与 4.5:1 两档、合成底色 WCAG，space-ai-contrast.json 重写，样例 aiCost/space-light 表格 14.65≥4.5）；动态切换 queries 1→1 不变 / styleChanged=true / valueStable=true（space-ai-appearance-switch.json 重写）。存量盘点（O.21 核对）：form-matrix 30 张、state-matrix 21 张、布局 4 组合 PASS、七卡深/浅基线截图，manifest 标注测试数据不进生产。
3. 三门禁（本回合串行重跑并逐个回显 exit=0，gate-*.log 覆写）：model:host:check → Go 110 passed / 12 packages；extension:check → "AI drilldown tests: 6 passed"；perf:check → snapshotMs=604.95<1500、idleCPUPercent=0。

### O.27 缺口收口：订阅续费提醒实现 + kimi-code HIST 升格（2026-09-12，本回合现场实现与验收）

本轮针对 O.25/O.26 记录的两项"交付缺口"做了实际实现（非仅重跑证据）：

1. 订阅续费/credits 到期提醒落地（§4.3 交付缺口收口）：新增 model-host/internal/usage/renewal.go —— EvaluateRenewals 从 usage_billing_entries 的 kind=subscription/prepay_expiry（PeriodEnd=到期日）评估；默认 7/3/1/0 天窗口各提醒一次，复用 usage_alerts UNIQUE(budget_id, period_key, threshold, kind) 幂等；已到期不提醒、坏日期跳过不阻断、SuppressAlerts 历史导入零入箱。renewal_test.go 三项 -count=1 全 PASS（go-renewal-evidence.log）：TestRenewalWindowsFireOnceEach、TestRenewalDueTodayAndExpired、TestRenewalSuppressAlertsAndBadDate。
2. kimi-code 历史用量升格 CapImplemented（R4 缺口收口）：kimi_session.go 按官方 sessions.md wire schema 冻结 kimi-session/1，assistant usage 四桶（input/output/cache_read/cache_creation）归一，messageId 计费原子幂等、pending/非 assistant 行跳过；collector.go 增 collectKimiFile 采集路径（~/.kimi-code/sessions/，游标复用 usage_import_cursors、半行不提交）；sources_matrix_test.go wantImplemented 同步加 kimi-code，登记表清理 IMPL 残留原因。TestParseKimiSessionJSONLV1 PASS（go-kimi-parser-fresh.log，fixture 经历期望值修正后收敛：3 事件/5 跳过、重复 messageId 行同原子不相加）。矩阵 HIST=IMPL 现为七工具（claude-code/codex/opencode/pi/zcode/hermes/kimi-code）。
3. 首轮 model:host:check exit=1（TestImplementedSourcesHaveCollectorPath 抓住 kimi-code HIST=IMPL 无采集路径）——矩阵守护测试生效的正面证据；补齐采集路径后三门禁重跑全 exit=0：model:host:check → Go 114 passed / 12 packages（新增 renewal 3 项 + kimi parser 1 项）、extension:check → "AI drilldown tests: 6 passed"、perf:check → snapshotMs=1071.67<1500、idleCPUPercent=0。
4. §5.5 截图证据现场重新生成（exit=0）：form-matrix 30 张、state-matrix 21 张（manifest 标注测试数据不进生产）。

如实缺口清单（更新后仍不得标绿）：kimi-code LIVE/账单/实时事件未核验（本机未安装，环境缺口）；deepseek-harness/openclaw/cursor 未安装（环境缺口）；grok-build/claude-desktop/atomcode 审计冻结（产品限制）；OS 真机通知投递与关 Proxy 真机联调未验收。所有改动未提交；开发/本地验证不构成发布授权。

### O.28 环境缺口现场复核 + 通知 Spike 真机验证（2026-09-12，本回合现场执行）

1. 已装工具元数据级探测（仅目录/键名/计数，不读日志内容，§6.2 边界，输出本回合回显）：~/.kimi-code、~/.kimi、~/.deepseek、~/.cursor 均 MISS（目录不存在）——矩阵中这四来源标"未安装（环境缺口）"为当前机器的现场事实，非"未做就标 unsupported"；~/.openclaw 存在但仅含 skills 子目录（13 个 json/db/jsonl，无会话/用量结构化文件）——OpenClaw 环境状态从"未安装"精化为"目录存在、无计量来源（环境缺口）"。
2. macOS 通知 Spike 真机验证（§6.5）：固定脚本 /tmp/natives-notify-spike.sh（固定字面量 display notification + title，独立 argv、零外部文本拼接）真机执行 exit=0。osascript 固定路径可行；通知身份/专注模式/用户权限的系统层验收仍属真机 UI 观察，不在此宣称"后台通知已完成"。
3. §5.5 截图产物存在性核验（本回合回显）：ai-form-matrix 30 张 PNG + manifest（"测试数据"标注 31 处）、ai-state-matrix 21 张 PNG + manifest（22 处）全部在盘。
4. 三门禁本回合重跑回显，exit 全 0：model:host:check → Go 114 passed / 12 packages；extension:check → "AI drilldown tests: 6 passed"；perf:check → snapshotMs=682.44<1500、idleCPUPercent=0。

如实缺口清单（本回合后仍不得标绿）：kimi-code/deepseek-harness/cursor 本机确认未安装（环境缺口，现场探测证据）；openclaw 无计量来源（环境缺口）；grok-build/claude-desktop/atomcode 审计冻结（产品限制）；通知 Spike exit=0 但系统层 UI 验收（通知中心可见性/专注模式）未做；关 Proxy 真机联调未执行。所有改动未提交；开发/本地验证不构成发布授权。

### O.29 真实采集数据深/浅截图落地（2026-09-12，本回合现场实现与验收）

§5.5 最后一块"真实工具数据的默认深/浅截图"收口（前轮缺口："截图覆盖范围……真实数据截图未产出"）：

1. 导出器 model-host/cmd/export-usage-fixture/main.go：走产品真实 CollectSources 链路（白名单元数据，§6.2 边界）写入临时 DB（不触碰生产用量库），再用与 Native Messaging 相同的聚合方法 GetOverview/GetEvents/GetSessions 原样序列化导出。编译一次错误（OverviewResult 无 GeneratedAt 字段）修复后运行成功：sourcesScanned=4、filesProcessed=479、eventsImported=8456、bytes≈966MB，产物 real-usage-fixture.json（本机真实已装来源采集数据，非手写 fixture，§9.4 契约）。
2. 截图脚本 space-ai-real-data-screenshots.mjs（165 行，<700 行无需 perf 登记）：核心三卡（aiCost/aiTokens/aiSessions）默认 number 形态 × 深/浅背景共 6 张，真实 Chrome + Shadow DOM 截取卡片元素，manifest 标注"真实采集数据（测试用途），不进生产数据"。运行 exit=0 输出已回显：ai-real-data/ 6 张 PNG（aiCost/aiTokens/aiSessions × space-dark/space-light）+ manifest.json。
3. 三门禁本回合重跑回显，exit 全 0：model:host:check → Go 114 passed / 13 packages（导出器包纳入编译）、extension:check → "AI drilldown tests: 6 passed"、perf:check → snapshotMs=692.12<1500、idleCPUPercent=0。

至此 §5.5 六维验收中"空间外观/冲突/字体/图形状态/布局/动态切换 + 基准截图 + 真实数据深/浅截图"全部有产物在盘。如实缺口清单（本回合后仍不得标绿）：kimi-code/deepseek-harness/cursor 本机未安装（环境缺口，O.28 现场探测证据）；openclaw 无计量来源；grok-build/claude-desktop/atomcode 审计冻结（产品限制）；通知系统层 UI 验收与关 Proxy 真机联调未执行（需真实工具会话与系统通知中心观察）。所有改动未提交；开发/本地验证不构成发布授权。

### O.30 关闭 Proxy 硬验收落地（2026-09-12，本回合现场实现与验收）

§9.0.1 第 3 行"关闭 Proxy 转发，仅导入已授权的原生日志/DB"收口（前轮缺口：需真实工具数据，本轮未执行）：

1. 新增 model-host/internal/usage/native_only_test.go（TestNativeOnlyCollectionWithoutProxy）：零 Proxy/plugin 记账前提下，走 kimi-session/1 parser fixture → 产品 Calculator（与 importer.go 同一 CalculateCost 路径，§7.2）→ InsertEventIfAbsent 入库。验证：Token>0、估算成本>0（测试价格快照，非真实报价）、distinct 会话≥1 三列均可出现；usage_events 中 collector_kind='proxy' 事件数为 0（来源诚实性）；重复导入同一原生文件计量不变（唯一计费原子幂等，§9.1）。
2. 修复过程如实记录：首轮编译失败（OverviewResult 字段名不匹配，改用 Metrics/TotalSessions 真实字段）；首轮断言失败 imported=2 vs parsed=3（重复 messageId 行共享 billing atom，去重后入库 2 条是正确产品行为，修正测试期望）；成本断言失败两轮（成本在事件入库时按价格快照写入，先插价后导入 + 复用产品 Calculator 而非直调 InsertEventIfAbsent）。最终 `go test -run TestNativeOnlyCollectionWithoutProxy -v -count=1` PASS（0.01s），证据落盘 go-native-only-fresh.log。
3. 三门禁本回合重跑回显，exit 全 0：model:host:check → Go **115** passed / 13 packages（110→114→115，新增 native-only 1 项）、extension:check → "AI drilldown tests: 6 passed"、perf:check → snapshotMs=**689.39**<1500、idleCPUPercent=0。

至此 §9.0.1 第 3 行验收有代码级证据：不经过 Natives Proxy 的原生日志/DB 导入路径可独立产出 Token、可计价成本、会话活跃三列。如实缺口清单（本回合后仍不得标绿）：kimi-code/deepseek-harness/cursor 本机未安装（环境缺口，O.28 探测证据）；openclaw 无计量来源；grok-build/claude-desktop/atomcode 审计冻结（产品限制）；通知系统层 UI 验收（通知中心可见性/专注模式）需真机观察；R4 其余注册工具真实 source 联调需对应安装环境。所有改动未提交；开发/本地验证不构成发布授权。

### O.31 Cursor 账单 CSV 导入回退落地（2026-09-12，本回合现场实现与验收）

§9.0 矩阵 Cursor 行"个人账单导入"收口（§4.2：个人账户或网页服务没有接口时提供 CSV/账单快照/手动确认回退；Cursor 本机未安装也能闭合 billing 列——装后即可用）：

1. 新增 model-host/internal/usage/billing_import.go（218 行）：冻结 wire schema `cursor-billing-csv/1`（date,kind,amount,currency[,period_start,period_end,note]，带表头校验）；金额定点解析为整数微单位（§7.2 不用浮点累计账目，12 位整数/6 位小数溢出显式拒绝）；kind 按词表归一（subscription/usage→service_usage、payment→topup 等别名），未知 kind 显式报错不猜不丢；条目 ID 为内容指纹（billcsv-<fnv64a>），同一 CSV 重导幂等；evidence_level 一律 actual_charge（用户提供的账单文件=已确认支出，§4.2）；金额按 kindImpact 预分配口径（topup 只入 cash+credit，不进服务消耗）。
2. 新增 billing_import_test.go（103 行）2 项测试，`go test -count=1` 现场 PASS（0.02s/0.00s），证据落盘 go-billing-import-fresh.log：①TestParseBillingCSVAndImport——定点金额 20.00→20000000 微单位、discount −0.50→−500000、topup ServiceCostImpact=0 且 CashImpact=50000000（§4.2.1 充值不是服务消耗）、全部 actual_charge、首导 4/0、重导指纹幂等 0/4、三口径对账 serviceSpend=20750000（20+1.25−0.50）/cashOutflow=50000000（仅充值）；②TestParseBillingCSVRejectsUnknownKindAndOverflow——未知 kind/金额溢出/非数字金额/缺 date 表头四类非法输入全部显式拒绝。
3. 修复过程如实记录：首轮编译失败两处（import 笔误 hash/fnvm、178 行残留 fnvm 引用），修复后 go vet + build exit=0。
4. 三门禁本回合重跑回显，exit 全 0：model:host:check → Go **117** passed / 13 packages（110→114→115→117，新增 billing import 2 项）、extension:check → "AI drilldown tests: 6 passed"、perf:check → snapshotMs=**1087.88**<1500、idleCPUPercent=0。

盘点确认（本回合）：§9.0.1 第 5 行跨日 session 场景已有测试覆盖（store_test.go:436-478：区间会话 1/每日各 1/ActiveDays=2/请求 30），非缺口。至此 Cursor 的 billing 列有可用的导入回退实现（未安装不再阻塞该列交付）。如实缺口清单（本回合后仍不得标绿）：kimi-code/deepseek-harness/cursor 本机未安装（环境缺口，O.28 探测证据；Cursor billing 已有导入路径，tokenUsage/liveEvent/quota 仍标 unknown）；openclaw 无计量来源；grok-build/claude-desktop/atomcode 审计冻结（产品限制）；通知系统层 UI 验收需真机观察。所有改动未提交；开发/本地验证不构成发布授权。

### O.32 AtomCode adapter 落地（2026-09-12，本回合现场实现与验收）

§9.0 矩阵 AtomCode 行"历史用量"收口（用户明确指定工具必须落地；早期审计误判为产品限制——旧审计只查 history-v2/entries.jsonl 漏掉 sessions/ 目录）：

1. 本机元数据级探测（§6.2 边界，只取键名/计数不读正文）：~/.atomcode/sessions/<projectHash>/<uuid>.jsonl 存在（1730 文件）；JSONL 顶层键 v/session_id/ts/iso/turn_id/usage/undone；usage 子键 {prompt,completion,cached}（int）；.meta 键 created_at/updated_at/turn_count/turn_stats/working_dir。wire schema 冻结 atomcode-session/1。
2. 新增 atomcode_session.go（parser，122 行）：undone 撤销行跳过不计费；同一 (session_id,turn_id) 流式多行取 usage 最大快照为终态、不相加（§7.3-2）；iso 优先、ts(UnixMilli) 回退双时间源；无 model 字段如实 unknown 不猜（§7.5）；计费原子 atomcode:<session>:<turn>。测试 atomcode_session_test.go（87 行）`-count=1` PASS（流式终态 180/45/30 非 280/65/30 相加、撤销行 999 token 不入账、坏行 skipped 计数）。修复过程如实记录：测试首轮 FAIL 为期望值算错（1757660400000ms=2025-09-12T07:00Z 非 2026-09-12），parser 本身正确。
3. collector.go 接线：CollectSources 第 8 块扫描 ~/.atomcode/sessions + collectAtomcodeFile（游标复用 usage_import_cursors、半行不提交 offset、10MiB 分块，与 pi/kimi 同一模板，非第二套导入器）。
4. 能力矩阵同步：sources.go atomcode 条目 HIST CapUnavailable→CapImplemented（审计说明改写，如实记录早期审计漏判与修正）；dump_test.go 逐格原因表同步（守护测试抓到 stale reason registered 后清理）；sources_matrix_test.go wantImplemented 补 atomcode。跨日 session、唯一原子等不变量由既有 store 测试覆盖。
5. 门禁首轮 exit=1 由 TestImplementedSourcesHaveCollectorPath 抓住（atomcode 升格但 wantImplemented 白名单未同步）——守护纪律第三次生效（kimi-code 后第二次同类拦截），修复后收敛。
6. 三门禁本回合重跑回显，exit 全 0：model:host:check → Go **118** passed / 13 packages（110→114→115→117→118，新增 atomcode parser 1 项）、extension:check → "AI drilldown tests: 6 passed"、perf:check → snapshotMs=**1098.54**<1500、idleCPUPercent=0。

至此能力矩阵 HIST=IMPL 增至八工具（claude-code/codex/opencode/pi/zcode/hermes/kimi-code/atomcode），其中 AtomCode 为用户明确名单最后一条 HIST 落地。如实缺口清单（本回合后仍不得标绿）：kimi-code/deepseek-harness/cursor 本机未安装（环境缺口，O.28 探测证据；Cursor billing 已有导入路径）；openclaw 无计量来源；grok-build/claude-desktop 审计冻结（产品限制）；atomcode LIVE/BILL/QUOTA 无来源（产品限制，有审计证据）；通知系统层 UI 验收需真机观察。所有改动未提交；开发/本地验证不构成发布授权。

### O.33 通知投递与收件箱集成落地（2026-09-12，本回合现场实现与验收）

§6.5/§9.2 通知投递决策集成收口（前轮缺口：SendNotification 为孤立单发，未接 usage_alerts 收件箱——无全局事件 ID 去重、无投递状态回写、多页重复弹窗无防护）：

1. 新增 model-host/internal/usage/notify_dispatch.go（82 行）：DispatchNotification 对已入箱 alert 做一次性投递决策——delivery_state==inbox_only 才继续，先占位 queued 防并发页面重复弹窗，投递后如实回写 queued/submitted/failed；已投递条目返回 ErrAlreadyDispatched 不重复决策；failed 不自动重试（显式用户动作才重发）；系统接受≠用户已看到，无 "seen" 状态；系统调用与 SQLite 非原子，不宣称 exactly-once。DispatchPendingAttention 批量恢复路径，单条失败不影响后续。
2. 新增 notify_dispatch_test.go 4 项测试 `-count=1` 全 PASS（证据落盘 go-notify-dispatch-fresh.log）：全局事件 ID 去重（二次投递 send calls=1）、failed 如实回写且不自动重试、pending 批量跳过已投递条目、缺失 alert 显式报错。修复过程如实记录：首轮编译失败两处（ToolEvent 字段名不匹配：无 Source/ObservedAt，真实为 OccurredAt string；残留未用 time 导入）。
3. 顺带修复产品缺陷：attention 条目此前不带 period_key，同 kind 的不同工具/会话事件（如两个工具各等待许可，§9.2 第 1 行）被 usage_alerts UNIQUE 空元组冲突 + INSERT OR IGNORE 静默丢弃——tool_event.go 改为 period_key=e.EventID（按事件去重，同事件重放仍由主键双重幂等；预算路径自设 period_key 不受影响）。受影响回归（Attention/ToolEvent/Ingest/Renewal/Budget）`-count=1` 全 PASS。
4. 三门禁本回合重跑回显，exit 全 0：model:host:check → Go **122** passed / 13 packages（110→114→115→117→118→122，新增 notify_dispatch 4 项）、extension:check → "AI drilldown tests: 6 passed"、perf:check → snapshotMs=**1113.38**<1500、idleCPUPercent=0。

如实缺口清单（本回合后仍不得标绿）：kimi-code/deepseek-harness/cursor 本机未安装（环境缺口）；openclaw 无计量来源；grok-build/claude-desktop 审计冻结（产品限制）；atomcode LIVE/BILL/QUOTA 无来源（产品限制）；通知系统层 UI 验收（通知中心可见性/专注模式/未授权通知）仍需真机观察——本轮闭合的是投递决策代码层，osascript 真机 Spike（O.28 exit=0）与收件箱三态记录已具备，端到端 UI 观察不在无头环境可达。所有改动未提交；开发/本地验证不构成发布授权。

如实缺口清单（不得标绿，与前轮一致）：kimi-code/deepseek-harness/openclaw/cursor 未安装或无实时来源（环境缺口，逐格带原因）；grok-build/claude-desktop/atomcode 审计冻结（产品限制）；订阅续费/credits 到期提醒未实现（交付缺口）；OS 真机通知投递与关 Proxy 真机联调未验收。所有改动未提交；开发/本地验证不构成发布授权。

### O.34 旧卡迁移与往返导出回归落地（2026-09-12，本回合现场实现与验收）

§9.0.1 末行"旧 session_duration 卡迁移和往返导出"场景此前无任何测试覆盖（盘点确认：生产代码无 seedBaseline 残留、七 key 注册与兼容 renderer 已在，唯一缺口即本场景）：

1. 新增 extension/tests/ai-legacy-migration.test.mjs（77 行，4 测试现场 PASS）：①旧 key 兼容读取——WIDGET_KEYS 白名单保留 widget/aiPerformance，space-catalog.js ai 分类恰好七个新条目且不含旧 key（隐藏而非删除）；②旧口径保留——METRICS.session_duration 仍走 totalRequests（sessions=false，诚实映射为调用活跃），与 ai_sessions（totalSessions、sessions=true）语义分离不被静默改写；③解析语义——normalizeView 无 view 默认 usage、view 优先于遗留 metric（limits/attention/savings）、tools/billing/ledger 兼容视图保留、非法值回退 usage；④往返导出——四种旧配置重复解析结果一致（纯函数幂等），旧 session_duration 卡往返后保持请求活跃语义。
2. 已接入 extension:check 门禁链（package.json，位于 ai-drilldown 之后）。
3. 三门禁本回合重跑回显，exit 全 0：model:host:check → Go **122** passed / 13 packages、extension:check → "AI drilldown tests: 6 passed" + "ai-legacy-migration: 4 passed"（末行 apps install failed 为既有负路径预期输出）、perf:check → snapshotMs=**707.41**<1500、idleCPUPercent=0。

如实缺口清单（不得标绿，与前轮一致）：kimi-code/deepseek-harness/cursor 本机未安装（环境缺口）；openclaw 无计量来源；grok-build/claude-desktop 审计冻结（产品限制）；atomcode LIVE/BILL/QUOTA 无来源（产品限制）；通知系统层 UI 验收需真机观察。所有改动未提交；开发/本地验证不构成发布授权。

## 一、perf:check HARD FAILURE 修复（三层根因）

| # | 根因 | 修复 | 落点 |
|---|---|---|---|
| 1 | `crates/native-file-host/src/app_store/install.rs` 1071 行超 1000 行上限（A3 预存改动） | 将 Suite Seed 安装块（`install_from_source` + `install_seed_artifact`，约 205 行）整体搬到新文件 `crates/native-file-host/src/app_store/seed.rs`，install.rs 降至 865 行 | `seed.rs`（新）、`mod.rs`（注册 `mod seed;`） |
| 2 | `model-host/internal/host/engine.go` 启动 goroutine 隐式调用 `CollectSources()` 全目录扫描——违反方案 §7.5"授权采集与纯查询分离"，且占用 perf idle 测量窗 CPU（idleCPUPercent 84–88%） | 删除启动隐式扫描；采集只由显式 `model_usage_collect` 触发 | `engine.go` |
| 3 | `model-host/internal/host/usage_view_handlers.go` 的 `getUsageSources`/`getUsageBilling` 查询路径隐式触发采集 | 移除两处隐式 `CollectSources()` 调用 | `usage_view_handlers.go` |

修复后 `perf:model-host`：`snapshotMs 596.88 < 1500`、`idleCPUPercent 0`（从 84–88% 归零）、`ok:true`。

## 二、13 工具逐能力矩阵证据

文件：`model-host/internal/usage/sources_matrix_test.go`（新增，3 测试全过）。

复现：`cd model-host && rtk go test ./internal/usage/ -run "TestCapabilityMatrix|TestImplementedSources" -v`
最近一次运行：**Go test: 3 passed in 1 packages**。

1. `TestCapabilityMatrixCoversRegistryAndUserList`：矩阵 = agentclients 注册表 11 工具
   （claude-code、claude-desktop、codex、deepseek-harness、opencode、pi、grok-build、zcode、
   kimi-code、openclaw、hermes）∪ 用户明确名单（cursor、atomcode）= 13 个，缺一即失败；
   注册表新增工具后测试立即暴露缺口，禁止硬编码数量冒充覆盖。
2. `TestCapabilityMatrixFieldsCompleteAndValid`：每工具六能力取值必须在
   `implemented|partial|unsupported|unavailable` 词表内；unsupported/unavailable 必须带可执行
   审计结论（"还没来得及做"不许冒充产品限制，§9.0）；privacy 必须是
   `metadata_only|needs_review`。测试驱动修正了 kimi-code、cursor 两条审计文案。
3. `TestImplementedSourcesHaveCollectorPath`：矩阵声明与采集器覆盖**双向一致**——
   声明 implemented 却无采集器路径 = 失败；采集器已覆盖却未声明 = 低估，也失败。

采集闭环来源（有 parser + fixture 测试）：**Claude Code、Codex、Pi、Hermes**。
其余 9 工具（ZCode、AtomCode、Cursor、DeepSeek Harness、Claude Desktop、OpenCode、
Grok Build、Kimi Code、OpenClaw）保持 partial/unsupported + 审计结论，**如实标注为交付缺口**。

## 三、七组件 Space 浏览器级验证（真实 Chrome + 真实 Shadow DOM）

文件：`extension/tests/space-seven-widgets-verify.test.mjs`（新增）。
复现：`node extension/tests/space-seven-widgets-verify.test.mjs`
最近一次运行：**exit=0，四个场景全绿**：

- space-dark：7 widgets mounted, derived default contrast >= 4.5:1 against #101210
- space-light：7 widgets mounted, derived default contrast >= 4.5:1 against #f4f1e8
- global-light-space-dark-reverse：同上 against #20242c
- explicit-colours-dark：7 widgets mounted, explicit per-card colours preserved

加载方式：真实系统 Chrome headless + 本地 HTTP 服务加载真实 `space-dashboard.js` /
`plugins/widgets/index.js` / `plugins/backgrounds/index.js`，真实 Shadow DOM（非 DOM mock）。
宿主复刻真实 `space.html` 的 flex item 结构（`space.css .dashboard-host`）——
shadow 样式表 `:host { all: initial }` 会把裸 div 压成 inline 导致宽高失效。

断言内容：
1. 七个独立 key（widget/aiCost/Tokens/Sessions/Requests/Limits/Attention/Savings）恰好挂载
   7 个渲染容器（选择器排除 free 布局外层 wrapper 避免双计）；
2. 每卡均被注入 `--space-widget-text` 局部角色（§5.2 桥接证据）；
3. 默认场景全部卡片派生同一个空间解析文字色；显式覆盖场景相邻不同 colour 互不串色；
4. host 不可达时错误/空态有文字，不静默、不冒充 0。

**验证暴露并修复的 2 个真实产品缺陷**：
- `.ai-perf-error` 固定回退 `#ff7f8f`（深底配色），浅底对比度仅 2.14:1 →
  空间层新增派生 `--space-widget-danger` 角色（随卡片解析文字色），AI 层固定色不再生效。
- `applyWidgetDisplayStyles` 硬编码 `disp.colour || '#ffffff'`，浅色空间下默认白字 1.13:1 →
  新增 `deriveReadableText()`：从解析后的空间背景亮度派生默认文字色（亮度中点为界、
  折算 brightness 滤镜）；解析链改为"显式覆盖 → 空间派生默认"。

## 四、§5.5 截图与对比度证据

文件：`extension/tests/space-seven-widgets-screenshots.mjs`（新增）。
复现：`node extension/tests/space-seven-widgets-screenshots.mjs`
产物（`extension/tests/artifacts/`，均已现场核验存在且非空）：

| 产物 | 像素自证 |
|---|---|
| `ai-seven-widgets-space-dark.png`（69619 字节，2x DPR） | 282 种颜色、非白 93.7%、背景像素 (16,18,16)=#101210 |
| `ai-seven-widgets-space-light.png`（72343 字节，2x DPR） | 339 种颜色、非白 93.8%、背景像素 (244,241,232)=#f4f1e8 |
| `space-ai-seven-widgets-verify.json`（16700 字节） | 4 场景计算样式 + 对比度记录 |

两图 MD5 不同（593076e2… 已被覆盖为不同内容；脚本内置字节相等即失败的断言）。
截图脚本内置**像素级反空白断言**（颜色分布、非白占比、字节差异）——曾据此捕获一次
全白截图并追根到 `:host{all:initial}` 问题（见上）。

对比度计算：WCAG 相对亮度，有效文字色取实际承载文本的后代元素
（canvas fillStyle 规范化任意 CSS 颜色），正文 ≥4.5:1 在三个空间背景全部达标。

## 五、门禁最终结果（最终集成一次性运行）

| 门禁 | 结果 |
|---|---|
| `rtk npm run extension:check` | exit=0（`apps install failed: sample APP_INVALID_STATE` 为 apps.test.mjs 预期负路径输出） |
| `rtk npm run model:host:check` | Go 96 测试 / 12 包全过（较上轮 +3：矩阵测试） |
| `rtk env -u CARGO_TARGET_DIR cargo fmt --check` | 通过（曾发现 install.rs 截尾多余空行，已 `cargo fmt -p native-file-host` 修复） |
| `rtk env -u CARGO_TARGET_DIR cargo test --workspace` | 181 测试全过（19+45+117），0 失败 |
| `rtk npm run perf:check` | exit=0（snapshotMs 596.88 < 1500、idleCPUPercent 0；一次因 release 重编译后系统余波超时，确认系统空闲后串行重跑通过） |

## 六、本轮改动文件清单（均未提交）

- crates：`app_store/seed.rs`（新）、`app_store/mod.rs`、`app_store/install.rs`（-207 行）
- model-host：`internal/host/engine.go`、`internal/host/usage_view_handlers.go`、
  `internal/usage/sources.go`（审计文案）、`internal/usage/sources_matrix_test.go`（新）
- extension：`space-dashboard.js`（deriveReadableText + --space-widget-danger + 默认文字色解析链）、
  `tests/space-seven-widgets-verify.test.mjs`（新）、`tests/space-seven-widgets-screenshots.mjs`（新）、
  `tests/artifacts/*`（3 个证据产物）

## 七、如实声明的剩余缺口

1. **8 工具采集未实现**（ZCode、AtomCode、Cursor、DeepSeek Harness、Claude Desktop、
   Grok Build、Kimi Code、OpenClaw；OpenCode 已于本轮落地）：需按 §2.1 逐个核验安装版本真实
   source 后实现 parser + fixture；矩阵测试把它们锁定在"有审计结论的未完成"状态。
   其中 Cursor/AtomCode/DeepSeek/Kimi/OpenClaw 本机未安装（目录探测证实），是环境缺口而非代码缺口。
2. **截图覆盖范围**：已交付深/浅纯色 + 三背景对比度；§5.5 矩阵中图片背景、125%/200% 缩放、
   六形态 × 状态全组合的截图样本未逐一产出（harness 可扩展场景复用）。
3. **会话时区**：`GetSessions` 日界仍为 UTC，用户时区化属 R5 统一 filter 契约（代码留注释）。
4. **"关闭 Proxy 仅原生日志"验收**（§9.0.1 第 3 行）需真实工具数据，本轮未执行。
