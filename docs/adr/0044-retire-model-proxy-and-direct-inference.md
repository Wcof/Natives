# ADR-0044 · 退役模型代理/路由，内置调用改为直连供应商

- 日期：2026-10-20
- 状态：已接受（用户明确要求取消 Natives 作为通用模型代理/路由服务）
- 决策者：产品方（用户）
- 取代：
  - ADR-0019 全文（统一模型代理 authority）；
  - ADR-0020 §6.1 中关于 CLIProxyAPI 内核、loopback gateway runtime、访问密钥与路由的规定；
  - ADR-0034（仅服务于 Antigravity OAuth 登录的公共 OAuth client 豁免；登录已退役）；
  - ADR-0041 全文（CLIProxyAPI v8 内核迁移）。
- 限定修订：
  - ADR-0030：保持 `model-host/internal/usage` 唯一写入权威；移除 CLIProxyAPI usage Record 依赖，改由内部直连调用写入。
  - ADR-0033：8766 只读查询端口与窄接口继续有效；额度查询仅保留只读/按需查询，不再依赖代理账户轮换或代理启动。
  - ADR-0037／ADR-0040：模块推理 owner／白名单／超时／流式语义继续有效；“经本地 CLIProxyAPI 受鉴权 loopback 推理端点调用”改为 Model Host 直连已配置供应商；模型身份使用 providerId+modelId，不再使用全局唯一别名。
  - ADR-0042：双入口、Native Relay、App Runtime HTTP 路由与 Shell 安全 Broker 不变；被删除的只是模型代理业务。
- 依赖：ADR-0020、ADR-0029、ADR-0030、ADR-0033、ADR-0035、ADR-0037、ADR-0038、ADR-0039、ADR-0040、ADR-0042

## 背景

Natives 曾把 Model Host 作为通用本地模型代理：对外暴露 CLIProxyAPI 兼容端点，管理代理访问密钥、模型别名路由、账号轮换/负载分配/重试、内核在线升级，并把本地代理地址注入 Claude Code、Codex 等第三方工具。该能力带来持续的内核补丁、账号池、Secret、端口与安全面，但产品内部真正需要的只是：

1. 投资等内置模块的受控 AI 推理；
2. 本地工具用量采集、导入、费用/账单/额度查看；
3. macOS 顶栏与空间组件的只读统计。

用户决定：Natives 不再作为通用模型代理/路由服务，删除代理业务，保留内部 AI 与 Tokenusage。删除模型代理不等于取消投资 AI，也不授权删除页面导航、Native Relay、App Runtime HTTP 路由或 Shell 安全 Broker。

## 决策

### 1. 删除的代理能力（不再存在生产路径）

- Gateway loopback 监听、兼容端点、启动/停止/重启、运行状态与端口配置；
- 代理访问密钥创建/更新/删除/轮换/显示；
- 模型别名（alias）作为路由身份；全局唯一别名校验与路由歧义；
- 账号轮换、负载分配、会话粘性、重试策略、上游代理 URL；
- CLIProxyAPI 内核检查/下载/构建/换装及所有 SDK、补丁、`third_party/cliproxyapi` 构建依赖；
- OAuth 登录、账户增删/启停/重新授权、账户模型排除；
- 向 Claude Code、Codex、Pi 等第三方工具生成、备份、覆盖或恢复本地代理配置；
- 设置页打开即启动代理、同步工具配置、自动注入凭据等副作用。

第三方工具已有的旧代理配置不由 Natives 静默改写、不自动恢复旧备份；设置页明确提示代理已退役，由用户自行修改外部工具配置。

### 2. Provider 连接配置

- 继续使用现有 `domain.Provider` 数据与 Handler，不重建第二套模型。
- Provider 只保留自定义（API Key）连接：名称、Base URL、协议、启用状态、模型列表、Keychain 凭据引用与掩码、局域网授权。
- 协议保持 `openai_chat`、`openai_responses`、`anthropic_messages`、`gemini`，每种协议由独立适配器构造请求、解析流式/非流式响应与 usage，禁止把所有连接按 Chat 协议发送。
- 模型删除 `alias` 字段；内部调用只接受 `providerId + modelId`。默认模型仍是 `providerId + modelId` 引用。
- Key 只进 OS Keychain；页面不获得明文、任意 URL/header、管理方法或密钥读取能力。

全局 AI 连接设置沿用上述 Provider 与默认模型引用。默认模型选择器属于整个 AI 连接页，不属于某个连接详情；所有已声明推理能力的模块经现有 Broker 使用同一默认配置。新增连接允许同时提交首个 `modelId` 与显式 `setAsDefault`，由现有 `model_provider_create` 在一次配置更新中保存连接、模型及默认引用；保留旧调用不带这两个字段的语义，不新增 schema。选为默认时连接及模型必须启用，校验／版本冲突／Keychain 失败不得留下半成品或修改原默认。已有默认不会因新增连接自动替换；用户可主动选择新的全局默认。页面只展示连接名称和模型名称，不用内部标识解释调用方式。

### 3. 内部 AI 调用链

目标链路：

```text
模块 UI → 既有 Shell/Broker（owner/能力/长度校验）→ Model Host
       → 指定 Provider 协议适配器 → 供应商 API
       → 任务结果 + 一次用量账本写入
```

- 保留 `options/start/status/cancel` 语义；`options` 返回已启用 provider+model 与默认模型可用性，不再返回 route。
- Model Host 在进程内读取可信连接配置与 Keychain，直连供应商；不再经过本地 HTTP 自代理。
- 复用既有网络安全策略：地址校验、DNS 解析、重定向同源限制、私网显式授权、连接/首响应/流空闲/总时限、响应与输出大小上限、凭据脱敏。
- 保留 owner 隔离、单 owner 单任务、并发上限、取消、断连和迟到结果处理；90 秒总时限、60 秒首响应、30 秒流空闲、8000 字符输出、1 MiB SSE、64 KiB 单行、1600 默认/512～8192 maxTokens 约束继续有效。
- 模型输出不能修改投资账本、触发交易或替代确定性规则。

### 4. 统计与生命周期独立于代理

- `model-host/internal/usage` 仍是本机用量与计费唯一写入权威；Tokenusage 只经 8766 窄接口读取，不直接访问私库。
- 删除 CLIProxyAPI usage Record 依赖；内部直连调用通过新的中性 `usage.Recorder` 写入同一 `usage_events` 表，保留既有 Event 字段、billingAtom、价格、币种、整数微单位与去重规则。
- 查看统计不要求 Provider、凭据、默认模型或代理密钥；不启动监听、不访问供应商、不改写工具配置。
- 后台驻留属于 Model Host 生命周期：`snapshot.resident` 取代 `gateway.resident`，保留用户既有常驻选择；查询与页面生命周期不再依赖 Gateway.State。
- 8766 只读查询端口继续存在；额度查询保留为显式只读能力（读取已导入 auth file 或既有 Keychain OAuth 凭据并查询供应商额度端点），不参与推理、不参与路由、不启动代理。

### 5. 用量计量规则

- 内部直连请求按来源语义归一化 input/output/cacheRead/cacheWrite/reasoning；reasoning 已含在 output 中时不重复累计。
- 成功、失败或取消只写一次事件，以任务 ID 作为幂等键；流式重复片段、结束事件与补查不重复计费。
- 未收到 usage 时记录 `usage_status=missing`，token 数字不得用于推断零消耗；失败/取消只记录实际已收到的计量证据。
- 保留实际扣费、供应商报告用量、本地估算费用的区别；缺单价显示未知/部分费用，不显示免费。
- 历史 `provider/account/model/accessKey` 归因字段继续可读，即使对应代理配置已退役。

### 6. 数据迁移

- `state.json` 升级到 schema 4：保留自定义 Provider、模型、默认模型引用、Keychain 引用与用户常驻选择；删除 Gateway、访问密钥、路由设置、模型 alias 与 OAuth Provider 配置。
- 既有 OAuth 账户元数据与 Keychain 项不自动删除；它们仅作为额度查询的只读凭据来源，不再用于推理。
- `~/.natives/auth/*.json` 不自动删除，仅继续服务显式额度查询与导入/删除管理。
- `usage.db` 不重建、不清空、不重扫；仅以事务化、幂等迁移增加必要的用量状态列。重复升级不重复写入。
- 旧代理配置不再执行，也不作为任何隐性 fallback。

### 7. 保留项

- 投资与其他内置模块 AI 调用及 `options/start/status/cancel` 契约；
- Tokenusage 模块、用量历史、费用、账单、价格、数据来源、空间用量组件、数据与用量页、macOS 顶栏统计；
- 独立于代理的本地工具日志采集、导入、游标、去重与事件失效；
- 8766 只读查询、额度缓存读取与显式额度查询；
- 双入口、Native Relay、App Runtime HTTP 路由、Shell 安全 Broker、Keychain Secret 所有权与模块隔离。

## 后果

- 正面：删除 27 MB 第三方内核、代理端口/密钥/路由/账号面与工具配置注入风险；内部 AI 调用链更短、身份更精确；统计与页面不再被代理状态绑架。
- 代价：不再支持用 Natives 为第三方工具提供模型代理；OAuth 账户不再参与推理，仅保留额度查询；旧外部工具配置会失效并需要用户手动调整。
- 不做什么：不新建第二代理、账号池、自动切换/重试或通用事件平台；不把删除代理解释为删除投资 AI、Tokenusage 或页面导航。

## 验证边界

必须覆盖：无 Provider/无密钥/无代理时 Tokenusage 空态与导入；schema 3→4 幂等迁移；设置/统计不启动监听、不访问供应商；四种协议的直连流式/非流式/取消/断连/超时/401/403/429/5xx/缺 usage/缺价格；同一请求一次计量；reasoning/cache 不重复；外部日志重复采集与重启续采不重复；扩展与本机 Web 双入口；Archive/Volt、中英、窄窗与焦点；生产构建与安装载荷不含 CLIProxyAPI 内核。

真实供应商验证只使用获授权的合成公开内容，不发送持仓或真实账本。工程测试通过不代表真实供应商已完成。

Extension 入口真实链已在隔离签名候选上运行：Fund 首页预览证据，经默认模型、Shell Broker、Native Messaging 与 Core 的 Model Session 直达可控 SSE 供应商，落库 1 条 `reported/success` 事件（10 输入／10 输出／20 总计），Tokenusage 显示 20 Token 且缺价显示 `--`，全程无 8317 监听；同一 Fund 的第二 Surface 关闭后第一页仍可交互。本机 Web 入口复用同一 App Runtime／Model Host 契约与既有双入口验收，其独立浏览器断链未在本轮重复运行。

## 实施与验收映射

2026-10-20 补充正确性约束：`usage_status=missing` 必须传递为各筛选聚合的 `missingUsageRequests`，不得因零计数而认定消耗或费用完整；已知小计与报告的真实零值保留，消费方语义见 [既有用量契约](../development/ai-efficiency-remediation-implementation-plan.md#43-事件与收费原子)。Responses 的流式终态完整正文与非流式正文同受 8000 字符限制；超限及非完成状态明确失败，保留已收到的 usage。修复与验证见 [专项记录](../development/evidence/model-proxy-closeout-2026-10-20.md)，不将工程通过扩大为完整迁移／发布验收。

| 需求 | 当前调用链／落点 | 处理方式 | Owner | 验收证据 |
|---|---|---|---|---|
| 删除模型代理／路由 | 无 Gateway、访问密钥、alias、账号池、内核更新方法 | 删除 Gateway/Kernel/OAuth 登录/Account models/Agent 注入源码、SDK、补丁与 `third_party/cliproxyapi` | Model Host | `rtk npm run model:host:check`；`TestResidentLifecycleSettingAndRetiredMethods`；生产树无 `model_gateway_*`／`model_kernel_*` 调用 |
| 内部 AI 直连 | 模块 UI → Shell Broker → Model Host `model_inference_*` → Provider adapter | `providerId`+`modelId`；四种协议独立构造与解析；Keychain 读取仅在 Host | Model Host | `TestProtocolsBuildDistinctRequests`、四种 stream 用例、`TestInferenceStreamsDirectlyAndRecordsUsageOnce`、`TestInferenceStatusMapping`、`TestStatusFailureMapping` |
| 默认模型与预览 | options 返回安全引用与 `configToken` | 默认失效／变更后拒绝；不返回 route | Model Host | `TestInferenceRejectsDefaultChangeAfterPreview`、`default_model_set` 生命周期用例 |
| 全局连接配置 | AI 连接页顶部默认选择器；新连接一次提交首个模型 | 单次 revision 保存；已有默认不自动替换；失败保留原状态与表单 | Model Host／Extension | `TestCreateGlobalAIConnectionAtomically`、`TestGlobalConnectionSetupFailuresPreserveState`、`scripts/model-connection-browser.test.mjs` |
| 用量独立 | 内部直连 → `usage.Recorder` → `usage_events`；Tokenusage 经 8766 窄接口 | 单一 writer；一次请求一次写入；缺 usage 显式 `usage_status=missing`；缺价读侧 `costStatus=unpriced`；`requested_at` 写入统一归一化为 UTC，避免本地偏移破坏按字符串比较的 today/窗口边界 | Model Host Usage Store | `TestSchemaV4PreservesLegacyUsageOnRepeatedUpgrade`、`TestBillingAtomDedup`、`TestCalculateCostMissingPriceIsNotFree`、`TestNativeOnlyCollectionWithoutProxy`、`TestInsertEventNormalizesOffsetsForTimeRanges` |
| 外部工具采集 | 本地日志 → `usage` collectors／importer | 保留游标、去重、取消与续采；不依赖代理 | Model Host Usage Store | `TestCollectCodexCursorFailureRollsBackEvents`、`TestCollectCodexResponseModeSurvivesRestart`、`TestImporterAcceptsEasyCLIProxyAPIV3Database` |
| 生命周期 | `snapshot.resident` + Native worker | 查看统计不读取 Gateway；常驻仅由用户显式设置 | Model Host／Native Relay | `TestResidentGatewaySurvivesNativeEOF`、`TestGatewayFollowsLastNonResidentPage`、`perf:model-host` resident 生命周期 |
| 设置与入口收敛 | Settings → AI 连接／使用记录／额度与凭据／应用中心 | 删除 OAuth/Gateway/Agent/Advanced 页面与死文案；保留退役说明 | Extension | `extension:check` 的 `model-settings.test.mjs`（页面、API、locale、无旧副作用） |
| 双入口真实链 | Extension → Core Relay → App Runtime → Broker → Model Host → 可控供应商 | 默认模型 `providerId`+`modelId` 直连；多 Surface 独立释放；统计与缺价显示经真实模块读取 | Extension／App Runtime／Model Host | 隔离 Chrome 链：1 次 SSE Chat 请求、1 条 `reported/success` 用量事件、Tokenusage 20 Token／`--` 价格、无 8317 监听；`.tmp/model-ai-chain-20261020/report/report.json` |
| 构建与安装载荷 | `model-host` 独立编译；产品清单只含当前 Host | 删除内核依赖与补丁；CI 只构建当前 Go 模块 | Build／Installer | `model-host/go.mod` 无 CLIProxyAPI 依赖；`perf:check`、`git diff --check` |
