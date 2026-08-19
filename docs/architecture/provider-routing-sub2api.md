# 供应商路由与 Sub2API 账号池

> **收敛状态（2026-08-18）**：本文档描述的是 2026-08-11 冻结语义（曾计划退役 failover 绑定/loopback/rectifier/global proxy）。该退役计划已被 v4 选型 A 取代：**Natives-native Rust Model Gateway 复用这些组件作为唯一生产路径，不退役、不删除**。新架构见 [provider-proxy-architecture.md](./provider-proxy-architecture.md) 与 [ADR-0019](../adr/0019-unified-model-proxy-authority.md)（已选 A）。本文件保留 Sub2API 账号导入/安全语义中仍然成立的部分，其余为历史记录。

## 目标

供应商设置新增“路由”页签。路由总开关开启后，Native Engine 与本地回环 API 共用同一套路由、故障转移、整流和出站代理逻辑。供应商模块新增 Sub2API 账号池，支持安全批量导入、批量删除，并能被执行引擎直接消费。

## 基线与范围

- 开发分支：`codex/provider-routing-sub2api`，从 `deploy` 切出。
- Sub2API 导入：兼容 `sub2api-data`、`sub2api-bundle`、缺省/0/1 版本，以及 Codex session 的 token、JSON、JSONL、数组输入。
- 执行支持：OpenAI OAuth/Codex、OpenAI API Key/upstream、Anthropic API Key/upstream、Gemini API Key/upstream；其他格式在预览阶段逐条标记为不可执行，不伪装为可用。
- 不复制 Sub2API 的计费、用户组、配额预测及高级 Top-K 调度。

## 职责与数据归属

- `agent-core` 保持只依赖 `EngineProvider`，不理解账号池、路由或 OAuth。
- Host (`natives.db`) 持有路由配置、绑定、Sub2API 账号、账号代理和加密凭据。
- Daemon (`assistant.db`) 持有 in-flight、失败次数、冷却窗口、熔断与最近选择等运行状态，绝不写入凭据。
- Provider Adapter 负责协议转换和 HTTP；OpenAI Codex 使用现有 Responses 请求/流处理，新增其 OAuth URL、认证与身份头。

## 模块

- `provider_accounts`：解析、预览、去重、导入、删除、凭据加密和账号凭据租约。
- `provider_routing`：路由配置、绑定校验和 Host 命令。
- `routing`：目标选择、账号池选择、熔断、故障转移和运行状态。
- `loopback`：本地 Bearer 鉴权与 OpenAI Chat/Responses、Anthropic Messages 协议适配。
- `request_rectifier`：Thinking signature、budget、媒体降级和纯文本预判。
- `openai_codex`：ChatGPT Codex Responses 上游适配。

## 导入与安全

标准包读取 `proxies[]`、`accounts[]`；账号支持 `name`、`platform`、`type`、`credentials`、`extra`、`proxy_key`、并发、优先级与过期策略。代理凭据和账号 credentials 均用既有 KEK/DEK 信封加密。

Codex session 支持 `access_token/accessToken`、嵌套 `tokens.*`、`refresh_token`、`id_token`、账号/用户/邮箱字段。`sessionToken` 只记录“出现过”的元信息，绝不作为 refresh token。没有 refresh token 的账号必须可确定 access token 过期时间，并启用到期停用。重复账号以账号/用户/邮箱或 token SHA-256 指纹识别；空 refresh token 不覆盖已有 refresh token。

导入以预览后确认的形式执行，返回 `created`、`updated`、`skipped`、`failed` 与警告。单次上限 10 MiB、2,000 个账号。批量删除在事务中硬删除选中账号及无引用代理；进行中的请求不被强制中止，但删除账号不会再次被选择。

## OpenAI OAuth/Codex

- `POST https://chatgpt.com/backend-api/codex/responses`
- `Authorization: Bearer <access_token>`
- `chatgpt-account-id`，FedRAMP 时 `x-openai-fedramp: true`
- `Host: chatgpt.com`、`OpenAI-Beta: responses=experimental`、成对 Codex `User-Agent`/`originator`/`version`
- refresh 使用 `POST https://auth.openai.com/oauth/token`、`grant_type=refresh_token`、Codex client id；到期前三分钟刷新并加密回写。

## 路由语义

路由绑定是“供应商 + 凭据（普通 Key 或 Sub2API 自动账号池）+ 精确模型”。当前 Run 的目标先执行；失败后按绑定顺序故障转移。Sub2API 目标先在池内切换可用账号，才进入全局队列。第一个文本、reasoning 或 tool-call delta 后禁止跨目标重放。

默认熔断：连续 3 次失败打开、冷却 60 秒、半开一次成功关闭；首字节 60 秒、流空闲 120 秒、非流式 600 秒。网络、超时、429、5xx、失效 OAuth 与额度耗尽可重试；请求格式与模型不存在不可跨目标重试。

路由总开关关闭时，Native Engine 直连当前 Run 的供应商/凭据/模型，本地路由停止，故障转移和整流不执行；全局出站代理仍独立生效。

## 本地路由与整流

本地服务默认 `127.0.0.1:15721`，必须携带本地 Bearer token，支持 `/health`、`/v1/models`、`/v1/chat/completions`、`/v1/responses`、`/v1/messages`。请求模型必须精确匹配绑定，最大请求体 32 MiB。

整流器在同一目标上最多重试一次：thinking 签名错误清理不兼容块；budget 约束把 thinking 规范为 enabled/32000，并将过小的 max_tokens 提升为 64000；文本模型或媒体不支持错误将图片替换为 `[Unsupported Image]`。全局代理支持 HTTP/HTTPS/SOCKS5；账号导入代理优先，其次才是全局代理。

## 验收

- 凭据不出现在 Renderer、日志、事件或 `assistant.db`。
- Sub2API 数据包和 Codex session 可批量预览、导入、更新、删除。
- Engine 可通过 Sub2API OpenAI OAuth 账号完成流式工具调用。
- 池内切换、全局切换、熔断及“首 delta 后不重放”均有测试。
- 本地 API Bearer 鉴权、三种入站协议、路由开关和全局代理独立性均有集成测试。

---

## 决策冻结：供应商自动协议路由（2026-08-11）

> 依据 13 项整改方案问题 8（`natives-13-issues-remediation-plan-2026-08-11.md` §3.8）。本小节**取代**上文中与旧“路由模式”相关的实施语义；旧功能退役清单见下。

### 1. “路由”开关的新语义

- “路由”只表示：**是否为当前上游自动选择 Chat Completions / Responses / Anthropic Messages 协议**。
- 开关**关闭**：沿用供应商现有显式协议，保证既有配置可运行。
- 开关**开启**：由 Daemon/provider-adapters 内的唯一 `ProtocolResolver` 生成候选协议顺序并自动选择。
- Renderer 只配置开关并显示真实决策结果，不做协议转换。

### 2. 旧“路由模式”退役清单（不再保留 UI / 公开类型 / 执行入口）

- failover 路由绑定（provider+credential+model 的绑定顺序与全局故障转移）
- loopback 本地服务（`127.0.0.1:15721` 本地 Bearer 服务与三种入站协议）
- request rectifier（thinking 签名清理、budget 规范、媒体降级）
- global outbound proxy（HTTP/HTTPS/SOCKS5 全局代理）
- 手工 route binding 管理（增删改排序）

> 旧 DB 表/列保留 inert（不 `DROP`）；Sub2API 账号池若继续使用，迁入供应商账户管理。

### 3. 协议回退边界（不可放宽）

- 只在**首个有效 text/reasoning/tool delta 之前**，对 `404/405` 或协议形状不兼容，才允许尝试下一候选协议。
- `401/403/429`、配额、权限、网络故障**一律不换协议**。
- **首增量之后绝不重放**（不重新发送已产生增量的事件）。
- 成功协议缓存**不含密钥**；provider / baseURL / model / config 任一变化即失效。

### 4. 一致性要求

- Provider“测试连接”与真实 run 必须调用**同一个** `ProtocolResolver`。
- 内部继续使用 canonical `ProviderRequest / ProviderEvent`；Renderer 不做转换。
