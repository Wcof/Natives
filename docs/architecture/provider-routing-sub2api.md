# 供应商路由与 Sub2API 账号池

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
