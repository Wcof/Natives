# 空间 AI 效能完整整改实施方案

修订日期：2026-09-12  
状态：**E0–E8 代码与门禁已实施；完成声明仍受 §11–§13 验收门槛约束（逐工具表含阻断项，见文末实施记录）**  
适用范围：Chrome/Chromium 扩展 `extension/`、单用途 Go `model-host`、Workspace 持久化 `crates/native-file-host`。

本文是 [AI 效能组件总方案](./ai-efficiency-components-plan.md) 的整改执行清单。总方案已经冻结的产品身份、空间视觉规范、计量口径和安全边界继续有效；本文只处理当前代码审计发现的未完成项，避免建立第二套架构或重复实现已有能力。

## 1. 整改结论与完成定义

当前实现已经完成七个独立组件、空间外观桥接、基础用量账本、真实 session 查询、预算/提醒/降本基础视图及主要工程门禁，但仍不能宣布完整交付。

本次整改完成必须同时满足以下条件：

1. 用户明确要求的 AtomCode、Claude Code、Codex、ZCode、Pi、Cursor、DeepSeek Harness、Hermes Agent，以及 `agentclients` 注册表中的全部工具，都进入真实、可审计的逐能力矩阵。
2. 所有存在真实结构化来源的工具均能采集 Token、可证明成本和真实会话；不存在某项来源时显示有证据的产品限制，不能显示为 0 或“已支持”。
3. 会话按 `(toolId, sourceInstanceId, nativeSessionId)` 去重，跨账号、profile 和安装实例不会错误合并。
4. Skill、MCP、插件和子代理有真实关系时可以归因；没有稳定 ID 或官方 trace relation 时保持未归属，绝不推断。
5. 账单 CSV、手动确认支出、订阅和 credits 日期通过现有“数据与用量”入口完成预览、确认、导入、对账和提醒。
6. 任一采集文件、数据库或 parser 失败都形成明确的 source 状态；不得吞错后写入“成功”。
7. 七个组件在真实 Space 中使用空间局部外观，所有支持的图形、状态、缩放和独立实例行为有浏览器证据。
8. 本文 §10 的逐工具验收、§11 的数据正确性验收和 §12 的工程门禁全部通过。

“矩阵中列出了工具”“测试中写了 implemented”“存在后端函数但没有可用入口”“fixture 单测通过但没有安装版本联调”均不算完成。

## 2. 必须保持的架构边界

- Model Host 继续作为 AI 用量归一、存储与查询的唯一权威；Local Proxy 只是采集来源之一。
- 不恢复 Tauri、Daemon、Jobs、Agent Runtime、Harness Runtime 或通用 Plugin Runtime。
- 扩展页面不直接读取工具目录、SQLite、账单文件或 Secret；这些操作由 Model Host 在用户授权后执行。
- 查询不得隐式扫描工具目录。采集只由显式导入、显式刷新或已审阅的一次性工具事件触发。
- 不读取 Prompt、代码正文、浏览器 Cookie 或 Secret；只读取冻结 schema 中的用量、时间、模型、会话和稳定关系字段。
- 不添加另一套图表系统、缓存层或账本。复用 `extension/ai-performance/`、`model-host/internal/usage/` 和现有 Native Messaging API。
- 不向 Space 注入 Files/Host 页面颜色。AI 组件只消费当前卡片的 `--space-widget-*` 局部角色。

## 3. 当前基线与整改范围

| 能力 | 当前状态 | 本轮要求 |
|---|---|---|
| 七个独立组件 | 已实现 | 保持独立 key；修正文案、设置和跨实例回归 |
| 空间视觉 | 基本实现 | 补齐六形态、状态、缩放、图片背景和独立颜色证据 |
| 历史采集 | 8/13 来源存在 parser | 补缺失来源或冻结有证据的产品限制；完成安装版本联调 |
| 会话统计 | 已按 `(source, sessionId)` 去重 | 升级为三元身份并按用户时区统计活跃日 |
| 账单 | 数据模型和 CSV parser 已存在 | 接入现有导入事务与 UI；支持预览、确认、回滚 |
| Skill/MCP/插件/子代理 | 只有 subjects/edges 表 | 建立事件关联、查询和详情展示；只接受可信关系 |
| 预算/续费提醒 | 核心函数存在 | 接入真实账单日期、设置入口和通知状态 |
| 采集错误 | 部分 adapter 吞错 | 统一 partial/error 状态、游标与恢复语义 |
| 工具矩阵测试 | 工具列表仍被手写 | 从生产注册表生成/对照，防止新增工具漏检 |

整改只修改与上述缺口直接相关的文件。当前工作区存在大量其他未提交改动，开发引擎不得格式化、重写或回滚无关内容。

## 4. 统一数据契约整改

### 4.1 来源实例身份

在 `usage_events`、采集游标和必要的 session 状态中增加：

```text
tool_id                 规范工具 ID，例如 claude-code、codex、pi
source_instance_id      同一工具内稳定的账号/profile/安装实例 ID
native_session_id       工具原生 session/thread ID
```

规则：

- `source_instance_id` 优先使用工具公开、非 Secret 的 account/profile ID。
- 没有公开 ID 时，对“工具 ID + 经授权 profile 根目录规范路径”做本机稳定摘要；数据库不得保存任意原始路径。
- 单 profile 工具也必须写明确的实例 ID，不能以空字符串作为长期产品语义。
- 旧记录迁移为可识别的 `legacy-default`，并标记 identity evidence；不得把旧记录伪装成已识别账号。
- 所有 adapter 输出统一携带三项身份；导入 fixture 也必须包含来源实例。

真实会话唯一键：

```text
(tool_id, source_instance_id, native_session_id)
```

区间会话数、每日会话数、会话详情、工具排行使用同一键。无 `native_session_id` 的记录进入 `unattributedRequests`，保留 Token 和成本，不制造会话。

### 4.2 用户时区

- 所有事件继续以 UTC 保存。
- 查询 Filter 增加 IANA `timezone`，由 Space/设置传入用户当前时区。
- 活跃天数和日热力图在 Model Host 内按该时区分桶。
- 不识别的时区返回参数错误，不静默退回 UTC。
- 同一 filter 的数字、趋势、热力图、表格与详情必须使用同一时区。

### 4.3 事件与收费原子

每次可计量调用必须拥有稳定 `billingAtom`。同一调用由 Proxy、原生日志、OTel 或账单重复观察时只保留一个收费原子；来源证据可以追加，金额不得重复。

新增或补齐事件与归因的关联表：

```text
usage_event_subjects
  event_id
  subject_id
  role          direct_owner | association
  evidence      stable_id | trace_relation | provider_receipt | user_confirmed
```

数据库约束必须保证同一事件、subject、role 不重复。`direct_owner` 只有一个；Skill、MCP、插件和子代理默认是 association。父任务成本按唯一后代 billing atom 求和，不同时累加父事件和子事件。

### 4.4 来源运行状态

每个来源至少返回：

```text
availability       ready | partial | unavailable | unsupported | error
lastAttemptAt
lastSuccessAt
lastErrorCode
lastErrorMessage
recordsImported
bytesRead
schemaVersion
sourceVersion
```

状态规则：

- 没有安装：`unavailable`。
- 经安装版本审计确认没有结构化来源：该能力为 `unsupported`，审计记录工具版本和检查位置。
- 部分文件成功、部分失败：`partial`，保留成功提交的数据和失败列表。
- 全部失败：`error`，不得更新 `lastSuccessAt`。
- 空目录但来源可用：`ready` 且记录数为 0；它与未授权、未安装不同。

## 5. 采集器整改

### 5.1 消除吞错

修改 `model-host/internal/usage/collector.go`：

1. Claude Code、Codex、Pi、Kimi Code、AtomCode 的循环不得再使用 `imported, bytes, _ := ...`。
2. 按来源汇总文件级错误；单文件失败不撤销已经提交的其他文件。
3. 来源只有在全部已处理文件成功后更新 `lastSuccessAt`。
4. partial/error 返回给 `model_usage_collect`，并在来源管理界面可见。
5. `findFilesWithExt` 返回文件和错误，不吞 `filepath.Walk` 错误。
6. 明确拒绝越过授权根目录的 symlink；路径数量、单文件大小和单次读取总量有上限。
7. 半行、损坏行、轮转和截断不得错误推进游标；恢复后可以继续导入。

### 5.2 每个 adapter 的交付物

每个 HistoricalUsage=implemented 的来源必须同时具备：

- 安装工具版本和 source schema 版本；
- 脱敏 fixture，来自安装版本真实结构；
- parser；
- collector 实际遍历路径；
- Token 桶映射；
- billing atom 规则；
- 三元 session 身份规则；
- 增量、重放、归档/轮转测试；
- 真实本机只读联调记录；
- 隐私字段白名单。

不得只通过静态 `wantImplemented` map 证明采集存在。能力矩阵测试必须读取生产 `agentclients` 注册表或其共享导出，并验证每个 implemented 来源注册了真实 adapter。

## 6. 逐工具整改要求

| 工具 | 必须完成 | 允许的诚实限制 |
|---|---|---|
| Claude Code | 项目 JSONL、Notification/Stop、可选 OTel；冻结 profile 身份；验证 Token、session、估算成本、重复去重 | Provider 账单和额度没有凭据时 unavailable；OTel 未开启时归因 partial |
| Codex | sessions + archived_sessions；累计快照跨批求差；notify 已验证事件；profile/account 身份 | Admin usage/rate limits 没权限时 unavailable；未验证 App Server 事件不得声称实时 |
| ZCode | 安装版本 rollout source；requestId 原子；session/profile 身份；真实三指标联调 | 无账单/实时事件时分别显示 unavailable/unsupported |
| AtomCode | sessions schema；同 turn 流式终态；撤销行处理；实例身份；真实三指标联调 | 缺 model 时成本为 unpriced，不能猜模型 |
| Pi | session v3；分支/response ID；usage 桶；实例身份；真实三指标联调 | 本地 cost 只作 estimate；无通用额度显示 unknown |
| Hermes Agent | state.db 只读白名单；WAL；schema_version；profile 身份；本地 cost 与 Provider 账单分离 | 压缩续接不当作 subagent；无实时事件不发即时提醒 |
| OpenCode | DB 只读投影；实例身份；cache 桶；真实联调 | SSE 未实现时 live/notification unsupported |
| Kimi Code | 安装版本 session fixture；四桶 Token；真实 collector 联调 | 本机未安装时保持未完成，不能仅靠公开文档标完整 |
| Cursor | 将个人账单 CSV/手动确认接入 UI 和对账；记录订阅/credits 日期；若有组织权限再接 API | 个人端没有逐请求 source 时 Token/session unavailable；不得用 IDE 活跃代替 |
| DeepSeek Harness | 获取安装版本或用户提供的脱敏 fixture；完成 source 审计、parser、collector 和真实联调 | 没有安装环境/fixture 时是交付阻塞项，不能改写为产品限制 |
| Claude Desktop | 独立于 Claude Code 审计；接账单回退 | 经指定版本证明没有用量 source 后可以将 Token/session 标为产品限制 |
| Grok Build | 冻结安装版本审计结果；接账单回退 | 日志没有 usage 字段时 Token/cost 不伪造；版本变化需重新审计 |
| OpenClaw | 完成安装版本 source 审计和账单回退 | “配置注入已支持”不构成用量支持 |

用户强制核心六项 Claude Code、Codex、ZCode、AtomCode、Pi、Hermes Agent 必须都有真实 Token、session 和可计价/未计价成本结果。Cursor 与 Harness 是用户明确名单，也必须有可执行来源结论和产品入口；缺少安装环境时最终报告必须列为阻断，不得宣布整版完成。

## 7. 账单、成本、额度与提醒整改

### 7.1 复用现有导入事务

不要新建第二套上传协议。扩展现有 `model_usage_import_begin/chunk/preview/commit/cancel`：

- `importKind=usage_events` 保持现状；
- 新增 `importKind=billing_csv`，commit 复用现有 `ImportBillingCSV`；
- begin 固定 provider、account、currency/evidence 输入；
- preview 返回合法记录、重复、币种、账期、金额三口径和逐行错误；
- commit 原子写入；取消或失败不产生半份账单；
- 重复导入同一文件按内容指纹幂等。

在现有“设置 → 数据与用量”中加入账单入口。Widget 只展示摘要并跳转到带筛选条件的详情页，不在每张卡片复制上传控件。

### 7.2 手动确认账务

支持用户手动录入经过确认的：订阅、API 扣款、充值、credits 变化、退款、折扣、税费、预付余额到期。必须记录 evidence=`user_confirmed`、币种、时间、Provider/account 和可选账期。

以下三类金额分开显示：

1. 已确认服务消耗；
2. 现金流出；
3. credits/余额变化。

充值不能再次算作服务消耗；订阅折算参考不能进入额外费用；多币种不自动换算。

### 7.3 额度和续费

- 额度复用现有 quota client，仅展示来源真实提供的窗口、remaining 和 resetAt。
- 没有来源显示 unknown，不显示 100%。
- 将 `EvaluateRenewals` 接入账单 commit、用户打开额度卡和受支持的一次性事件入口。
- 默认 7/3/1/0 天提醒，每个 entry/window 只产生一次。
- OS 通知区分 queued、submitted、failed 和 inbox_only；submitted 不表示用户已看到。

## 8. Skill、MCP、插件和子代理归因整改

### 8.1 接入顺序

1. Claude Code 可选 OTel：只在用户显式开启后读取官方稳定字段。
2. Codex：只使用 session/agent 的稳定 ID 或 App Server 官方 relation。
3. 其他工具：只有安装版本 fixture 中存在明确 ID/relation 才接入。
4. 没有证据的来源保持 session 级归属，不按时间、模型或 Token 相似度建树。

### 8.2 展示与查询

数据与用量详情支持按以下 subject 过滤：

```text
project / session / agent / subagent / skill / plugin / mcp_server / mcp_tool
```

每项显示 Token、唯一成本、请求数、会话数、覆盖率和未归属数量。关联项展示“参与成本”，只有 direct owner 进入总计。MCP 第三方费用必须有 receipt/账单；没有 receipt 时显示 unknown，与模型 Token 成本分列。

### 8.3 验收不变量

- 同一请求关联 Skill、MCP、插件和子代理后，总成本不增加。
- 父任务成本等于唯一后代 billing atom 之和。
- 重放相同 trace 不新增 subject、edge 或费用。
- 缺 relation 时保持独立记录。
- 不持久化 prompt、tool input 或代码正文。

## 9. 七组件与 Space UI 整改

### 9.1 独立产品身份

保持以下 key：

```text
widget/aiCost
widget/aiTokens
widget/aiSessions
widget/aiRequests
widget/aiLimits
widget/aiAttention
widget/aiSavings
```

旧 `widget/aiPerformance` 只用于兼容历史配置，从目录隐藏。修正 AI Sessions 描述，删除“当前以调用近似”，因为新卡已经使用真实 session 协议。

每个组件可独立添加、多实例、设置、删除、刷新、导出和导入。改变一张卡片的范围、图形或外观不得影响相邻实例。

### 9.2 来源与详情交互

- 成本、Token、会话、调用卡均提供工具多选和 `all_connected`。
- 过滤条件传到 Model Host；不得只过滤已经返回的前几条数据。
- 卡片显示覆盖范围、最近成功采集时间以及 partial/unpriced/unattributed 状态。
- 点击卡片进入现有数据与用量详情，并携带 range、timezone、toolIds、account、model、billingMode 和 currency。
- 额度卡提供预算设置入口；提醒卡支持已查看、静音和安静时段；降本卡支持采用/忽略及后续验证状态。
- 普通卡片不显示 billingAtom、adapter 状态码或内部 ID。

### 9.3 空间视觉

数字、趋势、排行、热力图、时间线和表格都必须：

- 从当前卡片根读取 `--space-widget-*`；
- 响应卡片文字色、字号、字重和空间背景；
- 不使用公共 shadow root 读取颜色；
- 不回退到 Files/Model Settings 的全局色；
- loading、empty、error、partial、unpriced 状态有清晰文字；
- 关键正文和图形达到 WCAG AA 对比度；
- 125% 和 200% 缩放无页面横向滚动、金额和 Token 不被截断。

外观改变只重绘当前组件，不触发采集或新的数据查询。

## 10. 实施阶段与改动落点

### E0：冻结基线

改动：文档和测试清单，不改业务逻辑。

- 保存当前门禁结果和 13 工具矩阵。
- 为每个工具登记安装版本、授权根、fixture、source schema、三指标状态。
- 将生产注册表导出为测试可用的共享工具 ID 列表，删除测试中的重复手写列表。

完成门槛：新增一个注册工具时，能力矩阵测试必然失败，直到补齐该工具审计。

### E1：来源状态与采集错误

主要文件：

- `model-host/internal/usage/collector.go`
- `model-host/internal/usage/sources.go`
- `model-host/internal/usage/store_schema_v2.go`
- 对应 collector/source tests

完成门槛：权限错误、损坏文件、半行、轮转和 symlink 越界均得到显式、可恢复结果。

### E2：三元会话身份与时区

主要文件：

- `model-host/internal/usage/types.go`
- `model-host/internal/usage/store.go`
- `model-host/internal/usage/store_schema*.go`
- 所有 native adapter
- `extension/ai-performance/metrics.js`
- `extension/model-settings-api.js`

完成门槛：两个 profile 使用同一个 native session ID 时统计为两个会话；同一 session 跨两天区间总数为 1、每日各 1、活跃天数为 2。

### E3：核心工具真实联调

顺序：Claude Code → Codex → ZCode → AtomCode → Pi → Hermes → OpenCode → Kimi。

完成门槛：每项都有安装版本、真实 source、Token、session、成本状态、重放去重和隐私证据。

### E4：Cursor、Harness 与其余注册工具

- Cursor 完成账单导入与手动确认入口。
- Harness 获取安装版本/fixture 后实现 adapter；缺环境时保持阻断。
- Claude Desktop、Grok Build、OpenClaw 完成版本化 source 审计与账单回退。

完成门槛：每个工具都有可复现证据；unsupported 只用于已经证明的产品限制。

### E5：账单、额度和续费闭环

主要文件：

- `model-host/internal/usage/billing_import.go`
- `model-host/internal/usage/billing.go`
- `model-host/internal/usage/renewal.go`
- `model-host/internal/host/engine.go`
- `model-host/internal/host/usage_*handlers.go`
- `extension/model-usage-*`
- `extension/model-settings-api.js`

完成门槛：真实 CSV 从 UI 预览并提交，对账数字正确，重复导入不增加金额，续费提醒可见。

### E6：归因闭环

主要文件：usage schema、adapter、聚合查询和现有数据与用量详情页。

完成门槛：带真实 relation 的 Skill/MCP/plugin/subagent 可查询，唯一收费原子不重复；无 relation 的数据不推断。

### E7：七组件交互与视觉收口

主要文件：

- `extension/ai-performance/widget.js`
- `extension/ai-performance/metrics.js`
- `extension/ai-performance/charts.js`
- `extension/ai-performance/views.js`
- Space dashboard/settings 现有桥接文件

完成门槛：§9 所有行为和视觉要求在真实 Chrome/Shadow DOM 中通过。

### E8：全量验收与文档回写

- 执行 §11–§12。
- 生成逐工具、逐能力、逐证据报告。
- 将真实完成状态回写本方案和既有领域文档。
- 删除过期或互相矛盾的 evidence 描述，不保留多份“最终结果”。

完成门槛：没有“代码已写但入口未接”“fixture 有但真机未测”“unsupported 代替未完成”的项目。

## 11. 硬验收场景

### 11.1 数据与会话

- [ ] 同一 session 两天各 10/20 次请求：区间会话 1，每日各 1，活跃日 2，请求 30。
- [ ] 两个 profile 的相同 session ID：会话为 2。
- [ ] 无 session ID：Token/成本保留，会话不增加，未归属增加。
- [ ] Proxy 与本地日志重复观察同一 request ID：费用、Token、请求均只计一次。
- [ ] Codex 累计快照 1000 → 1000 → 1600：增量为 1000、0、600。
- [ ] sessions 文件移入 archived：统计不增加。
- [ ] 半行、损坏、权限撤销、轮转：游标不越过未完成记录，source 状态明确。
- [ ] 用户时区跨午夜和夏令时：日桶与活跃天数正确。

### 11.2 成本与账单

- [ ] 缺价格显示 unpriced；合法零价显示 0；部分有价显示 partial。
- [ ] 订阅、充值、credits、退款、折扣、税费分别影响正确口径。
- [ ] 账期总额显示为未归因支出，不平均分配到项目/Skill。
- [ ] 同一 CSV 重复导入不增加金额。
- [ ] 预览失败或取消不写入账本。
- [ ] 空数据库无示例 `$20/$50` 账单。
- [ ] 订阅和 credits 到期在 7/3/1/0 天窗口各提醒一次。

### 11.3 归因与提醒

- [ ] 一次请求同时关联 Skill、MCP、插件和子代理，总成本不增加。
- [ ] 父任务成本等于唯一后代 atom 之和。
- [ ] 没有稳定 relation 时不建父子树。
- [ ] 两个工具中只有一个等待许可时，只出现对应提醒。
- [ ] Stop/notify 只描述本轮结束，不声称任务完成或测试通过。
- [ ] 同一事件重放、两个 Space 同时打开：收件箱和通知决策各一份。
- [ ] 通知失败时收件箱保留，状态为 failed；submitted 不展示为“已看到”。

### 11.4 Space UI

- [ ] AI 效能分类直接展示七个独立目录项。
- [ ] 成本、Token、会话三张卡同时存在并独立设置。
- [ ] 两张成本卡选择不同工具后互不影响。
- [ ] 数字、趋势、排行、热力图、时间线、表格单位正确。
- [ ] 深色、浅色、反向全局主题、图片背景、两卡不同文字色全部通过。
- [ ] 960×600 的 100%/125%/200% 缩放无横向滚动和关键数字截断。
- [ ] 外观切换不增加 Native 数据请求。
- [ ] loading、empty、error、partial、unpriced、unattributed 均有独立状态。

### 11.5 逐工具交付表

最终报告每个工具必须填写，空项即未完成：

| 工具 | 工具版本 | Source/schema | 实例 ID | Token | 成本 | Session | 归因 | 提醒 | 真机证据 | 限制/阻断 |
|---|---|---|---|---|---|---|---|---|---|---|
| AtomCode |  |  |  |  |  |  |  |  |  |  |
| Claude Code |  |  |  |  |  |  |  |  |  |  |
| Codex |  |  |  |  |  |  |  |  |  |  |
| ZCode |  |  |  |  |  |  |  |  |  |  |
| Pi |  |  |  |  |  |  |  |  |  |  |
| Cursor |  |  |  |  |  |  |  |  |  |  |
| DeepSeek Harness |  |  |  |  |  |  |  |  |  |  |
| Hermes Agent |  |  |  |  |  |  |  |  |  |  |
| OpenCode |  |  |  |  |  |  |  |  |  |  |
| Kimi Code |  |  |  |  |  |  |  |  |  |  |
| Claude Desktop |  |  |  |  |  |  |  |  |  |  |
| Grok Build |  |  |  |  |  |  |  |  |  |  |
| OpenClaw |  |  |  |  |  |  |  |  |  |  |

## 12. 工程验证门禁

开发阶段只运行覆盖当前改动的最小检查；最终集成阶段运行一次完整门禁：

```sh
rtk npm run extension:check
rtk npm run model:host:check
rtk npm run perf:check
rtk env -u CARGO_TARGET_DIR cargo fmt --check
rtk env -u CARGO_TARGET_DIR cargo test --workspace
```

此外必须运行：

```sh
rtk node extension/tests/space-seven-widgets-verify.test.mjs
rtk node extension/tests/space-ai-layout-verify.test.mjs
```

新增专项检查至少覆盖：

- 13 工具矩阵与生产注册表自动一致；
- 每个 implemented adapter 有真实 collector 注册；
- 三元 session 身份和用户时区；
- 采集 partial/error 与游标恢复；
- billing CSV UI 事务；
- subject/edge/event 归因不重复收费；
- 20 张相同筛选卡每个刷新周期共享相同查询；
- 1 万/10 万/50 万事件的查询与增量扫描性能；
- 页面隐藏、删除最后一张卡、pagehide、Host EOF 后无遗留 Port、timer 或 collector。

性能结果必须使用同一设备、Release 构建和相同数据集记录前后值。Extension 发布产物当前接近预算，新增前端代码优先复用现有视图和控件；不得通过提高预算掩盖增长。

## 13. 完成报告格式

开发引擎最终回复必须包含：

1. 实际修改文件及每项解决的问题；
2. 数据迁移版本和回滚方式；
3. 13 工具逐能力表；
4. 真实联调环境、工具版本和脱敏 evidence 路径；
5. 所有验收命令、结果和关键性能数字；
6. 明确区分 implemented、partial、unavailable、unsupported 和 blocked；
7. 未完成项及其所缺的安装环境、fixture、凭据或外部资料。

只有 §11.5 没有空项、所有用户强制工具完成要求满足、§12 门禁通过时，才可以使用“AI 效能完整实施完成”。否则只能报告对应阶段完成和剩余阻断。

## 14. 可直接交给开发引擎的执行指令

> 请执行 `docs/development/ai-efficiency-remediation-implementation-plan.md` 的 E0–E8。先阅读 `docs/README.md`、Standards、ADR-0020、ADR-0028、ADR-0030 和 AI 效能总方案，检查当前未提交改动并复用已有实现。不要恢复已删除的 Agent/Daemon/Jobs/Plugin Runtime，不创建第二账本或第二采集服务。优先修复采集吞错、三元 session 身份、账单导入产品入口和真实归因链，然后完成逐工具真实 source 联调。每个阶段运行最小相关测试，最终执行 §12 全部门禁并提交 §13 的逐工具证据报告。没有安装环境、fixture 或可靠 source 的工具必须明确列为阻断，不得用 unsupported、mock 或静态矩阵冒充完成。保留用户无关的工作区改动，不提交、不发布。

## 15. 实施记录（2026-09-12，开发引擎回写；唯一事实来源）

本节是 E0–E8 的真实完成状态回写。只记录已验证的事实；未列出的验收项按原门槛继续约束。

### 15.1 已完成的代码与迁移

| 阶段 | 状态 | 落点与证据 |
|---|---|---|
| E0 冻结基线 | 完成 | `sources_matrix_test.go` 直接读 `agentclients.Definitions()`（注册表新增工具必失败）；采集器注册表 `nativeSources` 为 implemented 声明唯一真源（`NativeCollectorToolIDs()` 对照） |
| E1 来源状态 | 完成 | `usage_events`/`usage_sources` v3 迁移（`store_schema_v3.go`）；`CollectSummary.Sources` 返回 §4.4 结构化状态；5 处 `imported, bytes, _` 吞错清除；`findFilesWithExt` 返回错误、拒绝 symlink/非普通文件、文件数与读取量上限；轮转/截断按 billingAtom 幂等从 0 重读；partial/error 不更新 `lastSuccessAt` |
| E2 三元身份与时区 | 完成 | `usage_events` 新增 `tool_id`/`source_instance_id`；实例 ID 由授权根目录摘要派生（`inst-<16hex>`，不落原始路径）；旧记录迁移为 `legacy-default`；会话键/明细/排行/日桶全部升级为 `(toolId, sourceInstanceId, nativeSessionId)`；Filter 新增 IANA `timezone`，handler 白名单核验（不识别即参数错误），日桶/活跃天数按真实 ZoneBounds 夏令时分段分桶，`today` 按本地零点换算 |
| E5 账单闭环 | 完成 | `importKind=billing_csv` 复用既有 begin/chunk/preview/commit 事务；preview 返回合法记录、重复、币种三口径与逐行错误；commit 幂等（内容指纹）；手动确认支出 `model_usage_billing_entry_upsert`（evidence 强制 `user_confirmed`）；`EvaluateRenewals` 接入账单 commit、手动录入、额度卡（`model_usage_budgets`）；设置 → 数据与用量新增「账单与支出」tab（三口径汇总、条目表、CSV 导入、手动录入） |
| E6 归因闭环 | 完成 | `usage_event_subjects`（event, subject, role 唯一；evidence 词表校验；direct_owner 全事件唯一；重放幂等）；subject 注册显式（无证据不注册）；`model_usage_subjects` 查询：direct_owner 计唯一成本、association 只计参与、未归属=无 direct_owner 的收费原子成本；tool-event 可选 subjects 关联会话事件（stable_id） |
| E7 七组件 | 完成 | AI Sessions 描述改为真实三元去重口径（删除「当前以调用近似」）；`fetchDataset` 全部查询透传用户时区；API 白名单新增 `model_usage_subjects`/`model_usage_billing_entry_upsert`；33 条账单文案 zh_CN/en 同步；space-seven-widgets-verify / space-ai-layout-verify 全部通过。**周期选择（2026-09-12 增量）**：统计范围扩展为 近24小时/近7天/近30天/**自定义**/全部——自定义在卡片设置内提供起止时间（datetime-local，本地时区输入、ISO/UTC 持久化与查询透传，Host Filter.custom 校验 start<end），无效起止回退近 7 天缺省；钻取到数据与用量详情携带同一时间窗并回显 |

### 15.2 真机只读联调证据（2026-09-12，本机 macOS，临时 usage.db，工具目录只读）

| 来源 | 结果 | 记录数 | 三元会话 | 备注 |
|---|---|---|---|---|
| claude-code | ready | 6359 | 52 | 二次采集 0 重复 |
| codex | partial | 2470 | 18 | 新到旧遍历；2026-07/08 旧版本 rollout 无 usage 记录（如实跳过）；>512 MiB 预算分块续读收敛 |
| zcode | ready | 269 | 3 | 二次采集 0 重复 |
| atomcode | ready | 946 | 166 | 真机修正 wire schema（v/turn_id 为数字）后导入 |
| pi | ready | 80 | 3 | 二次采集 0 重复 |
| opencode | ready | 360 | 15 | 二次采集 0 重复 |
| hermes | unavailable | — | — | 本机 `~/.hermes/state.db` 不存在（adapter 已实现并以 fixture 验证） |
| kimi-code | 阻断 | — | — | 本机未安装；真实 collector 联调缺环境 |
| deepseek-harness | 阻断 | — | — | 本机未安装（`~/.dsh`/`~/.deepseek` 缺失）；缺安装环境与 fixture |
| cursor | 账单回退可用 | — | — | 个人端无逐请求来源（产品限制）；billing CSV + 手动录入入口已交付 |
| claude-desktop / grok-build / openclaw | unsupported | — | — | 版本化 source 审计结论见矩阵 audit 字段（无结构化用量来源/无 usage 字段/仅配置注入） |

联调过程中发现并修复的真实缺陷：AtomCode `turn_id`/`v` 实际为数字（原 fixture 按字符串猜测冻结，整行反序列化失败导致 0 导入）；Codex 超大 rollout 文件曾因按整文件大小预判预算而永远无法启动导入（改为按实际读取字节计预算 + 单文件内 10 MiB 分块续读 + 新到旧遍历）。

### 15.3 遗留阻断与未完成项

1. **DeepSeek Harness / Kimi Code**：本机无安装环境，无法完成 §6 要求的安装版本 source 审计与真实联调；矩阵中保持阻断声明，不得标 implemented。
2. **Hermes 真机联调**：adapter 已实现并 fixture 验证，但本机无 `state.db`，缺真实只读联调记录。
3. **Provider 账单/额度**：Claude/OpenAI 等 Provider 侧账单与额度端点无本机凭据，维持 unavailable。
4. **Codex 首轮导入收敛**：历史 rollout 总量 > 预算，需多次显式采集收敛（checkpoint+partial 为设计语义，非遗漏）。

以上阻断项之外的 §12 门禁已通过：`cargo fmt --check`（exit 0）、`cargo test --workspace`（181 passed）、`npm run model:host:check`（143 passed，含 10 万事件规模基准）、`npm run extension:check`（exit 0）、`npm run perf:check`（exit 0，架构规模审计无违规）、`space-seven-widgets-verify` / `space-ai-layout-verify`（PASSED）。规模基准（10 万事件临时库）：GetOverview(30d) 203ms（UTC 与用户时区同值）、GetSessions(30d) 475ms（UTC 与 DST 时区同值）、GetEvents(50) 2ms；Host snapshot 基准 180ms / 预算 1500ms。在阻断项解除前，整体交付状态为「对应阶段完成 + 明确阻断」，不使用「AI 效能完整实施完成」。
