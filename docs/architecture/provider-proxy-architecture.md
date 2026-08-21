# Provider OAuth + Unified Proxy / Model Gateway 架构

> **状态**: 迁移前源码审计 + P0 Gate 证据；目标 authority 见 ADR-0020
> **日期**: 2026-08-19
> **审计基线**: `deploy` @ `e075c1e`（与 Plan v4 baseline 一致，工作树 clean）
> **权威顺序**: `docs/standards/` > ADR-0020 > 其它 ADR > 本文件
> **关联**: [ADR-0019](../adr/0019-unified-model-proxy-authority.md)（Authority 边界 + 选型）；`provider-routing-sub2api.md`（旧路由语义，2026-08-11 冻结，本文件第 4 节收敛）
> **Plan**: `Natives-Proxy-Minimal-Dependency-Plan-v4`（`00-EXECUTION-OVERRIDES.md` 为最高优先级修订）

> **ADR-0020 delta**：完成态为 Host-native `ProxyEngine`，Provider/Connection/Credential 分离，Secret 进入 OS Keychain；Daemon 路由仅是待迁移 current production。P0 fixture parity、stream lifecycle、OAuth/Key Pool 和 Secret migration Gate 通过前不得切换或宣称完成。

---

## 1. 目标与权威边界

产品能力目标（Plan v4 README §Core principle）：

```text
Provider            Unified Proxy / Model Gateway
├─ API Keys         ├─ Natives 内部模型访问
├─ OAuth Accounts   ├─ 账号路由 / failover
├─ Quota / Health   ├─ OpenAI / Anthropic / Gemini 兼容入站
└─ Model Catalog    ├─ 本地共享 → 可选 LAN 共享
                    └─ usage / observability
```

权威边界（冻结，不随选型变化）：

| 权威 | 归属 | 职责 |
|------|------|------|
| Provider | Provider 域（Host 持久化配置 + provider-adapters） | API Key / OAuth 账户 / identity / quota / health / model catalog / credential usability |
| Host | `src-tauri/` | `natives.db`、加密凭证 SoT、政策/设置、外部入站权威、子进程权威（如选 B/C） |
| Agent Daemon | `src-agent-daemon/` + `crates/*` | Run / Agent Engine / Provider 执行语义 / Tools / 事件（唯一 Agent runtime，Proxy 不得成为第二个 runtime） |
| Renderer | `src/` | UI 投影；经 `tauri-adapter`；禁止直连 SQLite / 子进程 / 上游密钥 |
| Model Gateway / Proxy | 能力边界 `ModelDataPlane`（语言中立） | 统一模型访问、credential 选择、协议兼容、本地/LAN 共享、usage 来源标注、健康/失败态 |

不变量：

```text
Credential persistent SoT 唯一（Host，AES-256-GCM）
Routing executor 唯一（P0.5 后只有一条 production path）
Usage Authority 唯一（Natives canonical usage，不建第二套统计 DB）
ProxyDataPlane 实现只有一条 production path
```

---

## 2. 现状能力审计（P0，2026-08-18 @ e075c1e）

### 2.1 Provider 协议适配（`crates/provider-adapters`）

| 能力 | 现状 | 证据 |
|------|------|------|
| ProviderAdapter trait | `chat` / `chat_stream` / `stream`（带 Credential 的 SSE）/ `list_models` / `discover_models` / `test_connection(_with_credential)` | `capabilities.rs:743-833` |
| 注册适配器 | openai / anthropic / gemini / deepseek / openai_compatible / ollama + `openai_codex`（Codex OAuth Responses transport，内存态 `OpenAiCodexCredential`） | `providers/mod.rs:14-23`、`providers/openai_codex.rs` |
| SSE 解析器 | anthropic_sse / gemini_sse / openai_responses / openai_sse | `stream/mod.rs` |
| canonical 类型 | `ProviderRequest` / `ProviderEvent` / `ProviderUsage`（input/output/reasoning/cache tokens + cost_usd）/ `ProviderError` + 10 类 `ProviderErrorCategory`（Auth/RateLimit/QuotaExceeded/ModelNotFound/ContextLengthExceeded/BadRequest/ServerError/Timeout/Network/Unknown） | `capabilities.rs:308-382`、`717-728` |
| 协议自动路由 | `ProtocolResolver`：Chat Completions / Responses / Anthropic Messages / Gemini / Ollama 候选顺序；仅 404/405/形状不兼容可回退，401/403/429/配额/网络不换协议；首 delta 后不重放；成功缓存不含密钥 | `protocol_resolver.rs` |
| 模型画像 | `ModelProfile` / `ModelFamily` / `ReasoningControl` / `PromptCacheMode` | `model_profile.rs` |

### 2.2 Credential Broker（Host 权威）

| 能力 | 现状 | 证据 |
|------|------|------|
| 单 key 租约 | `broker_acquire_lease`：查 `provider_api_keys`（信封解密）→ `lease_registry` 签发（TTL 120s、run 绑定）→ `key_lease` 落 natives.db 租约元数据（非密钥） | `credential_broker.rs:179-276` |
| Sub2API 账号池租约 | `broker_pool_acquire`：查 `provider_accounts`（active + 未过期）→ 逐账号信封解密 credentials + proxy → 返回池 | `credential_broker.rs:297-394` |
| 路由设置/计划租约 | `broker_routing_settings` / `broker_routing_plan` | `credential_broker.rs:398-509` |
| 其它租约 | `broker_secret_acquire`（能力库 secret）/ `broker_setting_get` / `broker_host_subagents` | `credential_broker.rs:514-619` |
| UDS broker listener | `credential_broker_uds.rs`：`spawn_broker_uds_listener` / `handle_broker_connection`（constant-time 认证、`MAX_FRAME_BYTES`） | `credential_broker_uds.rs:184-269` |
| 凭证加密 | `provider_key_manager.rs` KEK/DEK 信封（`envelope_encrypt/decrypt`）；`env_manager.rs` AES-256-GCM `v2:<nonce>:<ct>:<tag>` | 两者均有单测 |
| 子账号数据 | `provider_accounts.rs`：Sub2API 池 CRUD、导入/删除、`provider_routing_*` 命令（已注册 lib.rs:862-871） | `provider_accounts.rs` |
| OAuth 参考实现 | `commands/mcp_oauth.rs`：浏览器流（loopback + PKCE + callback + token 交换，`FLOW_TIMEOUT`），Host 侧 | `commands/mcp_oauth.rs:61-212` |

### 2.3 协议层（`crates/assistant-protocol` v2）

- `credential.rs`（512 行）：`CredentialRef` / `CredentialMaterial`（Drop 零化）/ `CredentialBrokerRequest·Response` / `CredentialLeaseMeta`（TTL 120s、`binds_run`）/ `CredentialLeaseEnvelope`（Debug 脱敏）/ `CredentialPoolLease*` / `LoopbackSettingsLease` / `RoutingPlanLease*` / `HostSubagentsLease` / `CredentialSecretLease` / `CredentialSettingLease`；`validate_broker_request` / `redact_credential_error`。
- `methods.rs`：`ALL_METHODS` 已收录 `credential.lease.acquire/revoke/status`、`credential.pool.acquire`、`credential.routing.settings`、`credential.routing.plan`、`credential.secret.acquire`、`credential.setting.get`、`host.subagents.export`——**但注释明确未进 `IMPLEMENTED_METHODS`**：Host UDS broker listener 尚未在 `lib.rs` 接线、`is_host_owned_method` 未拦截（与 `mcp.auth.oauthStart` 同等待办）。`NATIVE_ENGINE_FULL_REMEDIATION.md` §19 blocker #3 亦记录「Broker socket 无 Host listener/lifecycle/authentication；Provider lease 不可用」。
- 约束：`R-B4` 协议唯一来源 = `assistant-protocol`，改动必须 `npm run protocol:check`。

### 2.4 Daemon 执行与路由

| 能力 | 现状 | 证据 |
|------|------|------|
| 路由执行器（唯一） | `routing.rs`：`RoutedProvider`（`EngineProvider` impl，跨目标 failover、首 delta 不重放、60s 首字节超时）+ `Sub2ApiPoolProvider`（池内账号轮转 + OAuth 刷新 `refresh_codex_account`/`jwt_expiry_epoch`）+ 熔断（`FAILURE_THRESHOLD=3`、`COOLDOWN`、半开）+ inflight 计数 + `route_health_connection` | `routing.rs` 全文件（64 symbols） |
| 生产接入 | `production.rs:588` 与 `production_hooks_native.rs:136` 均 `RoutedProvider::new(load_plan(...))` —— 仍在生产使用 | grep 确认 |
| 限流/冷却 | `governor.rs`：`ProviderRequestGovernor`（RPM 限流、429 `retry-after` 冷却、FIFO 队列、CancellationToken） | `governor.rs:81-208` |
| 本地入站 | `loopback.rs`：`127.0.0.1:15721` Bearer 服务（/health /v1/models /v1/chat/completions /v1/responses /v1/messages），`main.rs:204` 启动 supervisor | `loopback.rs`、`main.rs:204` |
| 整流器 | `request_rectifier.rs`：thinking/budget 整流，`provider.rs:242` 仍调用 | `provider.rs:242` |
| 配置读取 | `natives_db_broker.rs`：Daemon 经 broker 租约读 Host 配置（`routing_plan` / `loopback_settings`），Daemon 不直开 natives.db | `natives_db_broker.rs` |

### 2.5 Usage / Observability

- Daemon：`metrics.rs` `DaemonMetrics`（run/turn/provider/tool/storage 计数器，持久 sink 未接）。
- Host：`src-tauri/src/usage/`（gemini.rs / opencode.rs 等源解析）+ `usage_dashboard_snapshots` 表（按 time_zone 主键）+ `commands/usage.rs`（`usage_get_cached` / `usage_sync`）。
- Canonical usage 结构：`ProviderUsage`（input/output/reasoning/cache/cost）已存在，`ProviderEvent::Usage` 携带。

### 2.6 进程监督（Host）

- `sidecar_supervisor.rs`（932 行）：`SupervisorState` / `spawn_child` / `readiness_probe` / `poll_child_health` / `ensure_healthy_or_restart` / `restart_unresponsive_child` / `shutdown_with_grace` / `force_kill_child_tree` / watchdog（`R-S1` / `R-B7` 落点）。已管理 Daemon sidecar；Proxy 若需要子进程必须复用/泛化此基础，禁止第二套框架。

### 2.7 前端（`src/`）

- 供应商域：`src/components/settings/provider-routing/`（旧路由 UI）、ProviderSettingsTabs 等。
- 取数路径：`src/lib/tauri-adapter.ts` → Host command（`R-E9`）；能力门控 `src/lib/assistant-workspace/capability-gate.ts`。
- i18n：`src/i18n/zh.ts` / `en.ts`（`R-I3` 键必须一致，`npm run i18n:check` 强制执行）。

---

## 3. 缺口分析（Gap）

| # | 缺口 | 说明 | 对应 Plan 阶段 |
|---|------|------|----------------|
| G1 | 通用 Provider OAuth 生命周期缺失 | 仅有 Codex OAuth（内存态、Sub2API 导入）与 MCP OAuth（PKCE 浏览器流）。无通用 Provider OAuth 账户模型（start/callback/session/persist/refresh/reauth 状态机、identity 去重、auth_revision） | P1 / P3 |
| G2 | `has_usable_credential` 缺失 | Broker 与预检均为 API-key-first：`broker_acquire_lease` 无 key 即报 `No active key for provider`；OAuth-only provider（`provider_api_keys=0`、`provider_accounts` 可用）无法通过 run 预检 | P1 / P4 |
| G3 | 统一 Model Gateway 契约缺失 | Daemon 无通用 model-gateway 请求/流式 RPC（现仅 `provider.*` 静态方法 + `run.*`）；协议目录有 `credential.routing.*` 但未广告、Host broker UDS 未接线 | P2 / P4 |
| G4 | 旧路由语义与冻结不一致 | `provider-routing-sub2api.md` 2026-08-11 冻结称 failover 绑定/loopback/rectifier/global proxy 退役，但代码仍生产使用（`production.rs:588`、`loopback.rs`、`request_rectifier.rs`、`global_proxy_for_daemon`）。**文档与代码必须收敛**（第 4 节） | P0 / P10 |
| G5 | Proxy 政策服务边界缺失 | 路由配置仍以旧 `provider_routing_*` + Sub2API 池形态存在，无独立的 Proxy/ModelGateway 政策服务层 | P2 |
| G6 | usage 来源标注缺失 | canonical `ProviderUsage` 无 internal/external、provider/account 维度；无代理流量专属观测 | P7 |
| G7 | quota 无可靠来源 | quota 仅为 usage 快照派生；OAuth 账号 quota 无独立可靠来源（按 05 §5.6 无来源则展示 Unknown，不虚构） | P1 / P3 |
| G8 | 前端旧路由域需收敛 | `src/components/settings/provider-routing/` 为旧语义 UI，按选型迁移到 `provider/` 与 `proxy/` 域 | P6 |

---

## 4. 路由现状与单一执行器（selection A 确认）

> 2026-08-11 冻结语义（`provider-routing-sub2api.md` §决策冻结）曾计划退役 failover 路由绑定 / loopback / rectifier / global proxy。**该计划已被 v4 选型 A 取代**：选型 A（Natives-native Rust Model Gateway）**复用**这些组件作为唯一生产路径，不退役、不删除。

| 组件 | 现状 | 选型 A 下的角色 |
|------|------|-----------------|
| `RoutedProvider` | `production.rs` 与 `model.gateway.stream` 共用 | **唯一路由执行器**（failover / 首 delta 不重放 / 熔断）；内部 run 与外部 gateway 都经它 |
| `loopback.rs` | `main.rs:204` 启动 | OpenAI/Anthropic/Gemini 兼容 HTTP 入站（本地工具用）；与 UDS RPC gateway 并存、职责不同，不退役 |
| `request_rectifier.rs` | `provider.rs:242` 调用 | thinking / budget 整流，继续保留 |
| `global_proxy_for_daemon` | `credential_broker.rs` | 出站代理回退，继续保留 |
| `provider_routing_*` 命令 | `provider_accounts.rs` + `lib.rs` | 路由设置 UI 的 Host 命令，继续保留 |
| Sub2API 账号池 | `provider_accounts` 表 + `broker_pool_acquire` | Provider 账户域（OAuth / 上游账号池），继续保留 |

原则：**单一 routing executor（`RoutedProvider`）、单一 credential SoT（Host）、单一 usage authority**。gateway（UDS RPC）是附加的外部统一模型访问入口，不构成第二套路由/调度；无新旧双路径共存。

---

## 5. Standards Compliance Matrix（P0）

| Natives MUST | 适用点 | 当前状态 | 缺口动作 |
|--------------|--------|----------|----------|
| R-T1 Host-default authority | 旧 Proxy/Gateway 仍是 Host 政策 + Daemon 执行 | current production only | P0 parity 后一次切到 Host `ProxyEngine`，同批删除 Daemon fallback |
| R-S12 OS Keychain Secret ownership | OAuth access/refresh token 与 API Key | 当前 KEK/DEK + SQLite 不满足完成态 | 按 ADR-0020 迁 Keychain；迁移前仍禁止任何明文落盘 |
| R-S13 Renderer 无凭证明文 | OAuth 状态仅回 safe session 摘要 | 现有掩码模式 ✓ | 保持：`session_id/provider/state/authorization_url?/safe_error?` |
| R-B1 命令/RPC 结构化 Result、无 unwrap | 新 OAuth/gateway 命令与 RPC | 现有 error.rs 模式 | 全部新路径走 `Result<T>` + sanitize |
| R-B2 日志脱敏 | OAuth callback payload / token / raw response | `log_sanitizer.rs` ✓ | 新 pattern 补进 sanitizer |
| R-B4 协议唯一来源 + protocol:check | Host↔Daemon 新契约 | `assistant-protocol` 已有 credential/routing DTO | 修改必须过 `npm run protocol:check` |
| R-B7 子进程纳入 sidecar_supervisor | 若选 B/C（sidecar） | supervisor 已存在 | 复用/泛化，禁第二套 |
| R-B3 共享逻辑下沉 crates | OAuth/路由共享逻辑 | provider-adapters / assistant-protocol ✓ | 不复制双份实现 |
| R-T2 / R-D1-R-D5 数据权威与迁移 | natives.db Host-only、增量迁移 | ✓ | 新表登记附录 + 迁移测试 |
| R-P5 / R-P9 不轮询、有界 | gateway 状态刷新、队列 | governor 有界 ✓ | 事件推送优先；缓存/并发设上限 |
| R-E1 / R-E9 前端域结构与 adapter | `src/components/provider/`、`src/components/proxy/` | 旧 provider-routing 待收敛 | 新组件按域落位，薄 page |
| R-E10 三态 / R-F2 无假数据 | OAuth/Proxy 状态展示 | 现有三态模式 | 不虚构未实现状态 |
| R-I1/R-I3 文案走 t() 且双语同步 | 所有新增可见文本 | `zh.ts`/`en.ts` 同步机制 ✓ | 新键双语同步 + `npm run i18n:check` |
| CODE_MODULE 500/700/1000 行阈值 | 新 gateway/OAuth 模块 | — | 按职责拆分，禁 `utils.rs` 式掩盖 |

---

## 6. 参考必要性日志（Reference Log）

Plan v4 §22 要求：每个外部引用必须回答「具体未解决问题」。当前（P0）无任何外部生产依赖。

| 参考 | 为什么需要 | 生产依赖？ |
|------|-----------|-----------|
| Natives 自身源码/standards | 现状与权威（本文件第 2、5 节） | No（native） |
| CLIProxyAPI 源码 | 仅当 P0.5 评估 B/C 时回答具体问题：stock 凭证注入/刷新回写/协议路由/生命周期（Goal §7、P0.5-B） | Pending P0.5 |
| EasyCLI | 仅具体 UX 问题（OAuth 交互 UX / quota 展示），**永不当架构权威** | No |
| MCP OAuth 浏览器流（`commands/mcp_oauth.rs`） | Natives 内部已有 OAuth 参考实现，P1/P3 复用 | No（native） |

### 6.1 Reference Necessity Gate — 10 问评估（P0 结论）

对两个外部候选（CLIProxyAPI / EasyCLI）执行 Goal §5 的 10 问评估：

| # | 问题 | 评估结论（基于第 2 节审计） |
|---|------|------------------------------|
| 1 | Natives 当前是否已有等价能力？ | **大部分有**：provider-adapters（6+1 适配器、SSE、usage、error 映射）、Credential Broker（单 key + Sub2API 池 + routing plan 租约 + UDS listener）、路由执行器（RoutedProvider/Sub2ApiPoolProvider/熔断/failover/OAuth 刷新）、governor（限流/冷却）、本地入站（loopback 3 协议）、ProtocolResolver、MCP OAuth PKCE 参考实现 |
| 2 | 缺口具体是什么？ | G1–G8（见第 3 节）：通用 Provider OAuth 生命周期、`has_usable_credential`、统一 Gateway 契约接线（协议 DTO 已定义未广告）、旧路由语义收敛、Proxy 政策服务层、usage 来源标注、quota 来源、前端域迁移 |
| 3 | 自己基于现有架构实现成本？ | 可估算：多为「现有能力泛化/接线」而非从零造轮子；主要增量在 OAuth 状态机、Gateway RPC 契约、路由政策层 |
| 4 | 引用外部能力能减少多少代码？ | 可能减少 OAuth/路由细节实现，但需以 P0.5 实测证明；减少量尚不能抵消依赖成本 |
| 5 | 会增加哪些长期维护成本？ | 二进制/SDK 版本钉定、升级耦合、上游配置/auth-store 语义冲突、第二 security boundary |
| 6 | 是否增加新语言？ | B：否（stock binary）；C：若 SDK 为 Go 则引入 Go build target（条件性） |
| 7 | 是否增加新的 runtime / sidecar？ | B/C 均增加一个 sidecar 进程（需纳入 sidecar_supervisor） |
| 8 | 是否增加新的 security boundary？ | B/C 均增加进程边界与凭证传递边界 |
| 9 | 是否形成第二个 Authority？ | 设计上必须禁止（C 不得拥有 DB/Provider SoT/Usage；B 不得强制 plaintext 持久化）——违反即 Stop-the-line（Goal §40） |
| 10 | 最终总复杂度下降还是上升？ | 由 P0.5 决策矩阵裁定；**当前无任何外部生产依赖被证明必要** |

**P0 结论**：`Benefit <= Long-term Complexity` 尚不能否定，但**必要性未被证明** → 按 Plan v4，不引入任何外部生产依赖；CLIProxyAPI 仅作为 P0.5 候选评估对象（Level 1，具体问题驱动），EasyCLI 仅作 UX 参考（Level 2）。B 的安全 gate（plaintext 持久化、刷新回写）未证实前默认不合格（fail-closed）。

---

## 7. 候选方案（P0.5 决策，本文件不预先选型）

```text
A. Natives-native Rust Model Gateway
   现有 provider-adapters / Broker / Daemon 执行权威复用，Host↔Daemon 协议最小扩展
   —— 无新语言、无新进程（若能力可满足）

B. stock CLIProxyAPI binary sidecar
   仅当其公开/runtime 接口满足 Natives 安全与生命周期（无 plaintext 持久化、刷新可回写）
   不引入 Go 源码/工具链

C. thin isolated CLIProxy SDK adapter
   仅当 A 成本明显过高且 B 不满足安全/控制；Go 只是 C 的后果，不是项目要求
   必须可替换、不拥有 Natives DB / Provider / Credential SoT / Usage
```

决策矩阵维度（P0.5）：Security MUST 通过与否（gate）、架构契合、新代码量、构建/运行依赖、协议重复度、上游耦合、测试负担、性能、故障恢复、升级/回滚复杂度 → **选择长期总复杂度最低且全 MUST 通过者**；失败 spike 删除。

---

## 8. 迁移与回滚原则

- 迁移：新 Provider OAuth 域与旧 Key 路径并存期仅在过渡分支；cutover 后删除旧生产 fallback（Goal §37）。
- 回滚：破坏性 schema 清理前先确认无引用、可回滚（Plan P10）；旧表 inert 保留一个版本周期。
- 文档收敛：选型后本文件只记录被选 production path；被拒候选仅留在 ADR 历史（Plan §21）。
- Docs DoD：ADR current、docs/README current、ARCHITECTURE current、旧 routing 文档不再与代码冲突、无已废弃候选被描述为 active。

---

## 9. 2026-08-19 P0 Gate 复核（baseline `1497cf7`）

本节按 ADR-0020 重新评估现有资产；它取代本文中“Daemon 是完成态执行 authority”的结论。

### 9.1 能力矩阵

| 能力 | 当前状态 | 关键证据 / 缺口 |
|---|---|---|
| canonical request/event/usage/error | implemented，可提取 | `provider-adapters/capabilities.rs`、`stream/*.rs` |
| Anthropic Messages outbound | partial | body/SSE 与完整 stream fixture 已覆盖 text/thinking/tool/usage/terminal；仍缺 non-stream/error fixture 与真实 ingress parity |
| OpenAI Chat Completions outbound | partial | body/SSE、strict terminal、异常 EOF 与完整 stream fixture 已覆盖；仍缺 non-stream/error fixture 集 |
| OpenAI Responses outbound | partial | 已改为 stateful parser，保留 `output_item.added` 到 arguments delta 的 `call_id`/name；仍缺完整 failed/incomplete/cancelled fixture 集 |
| Local `/v1/messages` / chat / responses | partial | `loopback.rs` 丢 tools，history/system/usage/stop reason/framing 不完整 |
| `model.gateway.stream` | partial/unproven | 协议与 handler 已有，无真实 Host caller；`temperature`/`max_tokens` 未生效，无 reasoning control |
| Tool / reasoning / usage | partial | outbound normalization 有资产；ingress 与最终 framing 仍丢字段 |
| cancellation / EOF / backpressure | partial | 四 transport 的无 terminal EOF 返回结构化 Error；drop stream 会关闭 socket；SSE remainder 上限 1 MiB。Host Proxy 下游断连、慢 consumer 与 task 全链回收仍未证明 |
| OAuth account pool | partial | priority/concurrency/affinity 有基础；refresh/expiry/client secret 与 Antigravity adapter 路由未闭环 |
| API Key Pool | implemented（P0-A Spike） | `src-tauri/src/key_pool.rs`：priority + round-robin 公平轮转、cooldown（连续失败阈值进入/到期恢复）、disabled、failover 切换、可观测计数（selection/failover/rejection）；10 个纯逻辑测试覆盖公平/并发/冷却/失败切换/可观测 |
| OS Keychain | implemented（P0-A Spike） | `src-tauri/src/secrets/`：`SecretStore` seam（R-S12）+ `KeychainSecretStore`（macOS security-framework，generic password，service=ai.natives.secrets）+ `MemorySecretStore`（locked/unavailable 模拟）+ `migrate_secret` 状态机（写→回读验证→切换引用→延后清理，幂等/回滚/locked/不可用 8 场景）；`scripts/secret-scan.sh` 全仓明文 Secret 扫描 PASS（122 provider-adapters + 10 key_pool + 17 secrets 测试通过） |

### 9.2 `provider-adapters` 处置矩阵

`ProviderAdapter` 暴露 legacy `chat/chat_stream/stream`、静态模型、发现和测试等多组语义，是浅 module。完成态应由 Host 的深 `ProxyEngine` interface 隐藏 Connection 解析、Credential 选择、协议策略、HTTP、stream lifecycle 与 usage normalization。

| 处置 | 资产 |
|---|---|
| **Keep / Extract** | canonical message/content/tool/reasoning/usage/error types；Chat/Responses/Messages/Gemini request builders；OpenAI/Anthropic/Gemini/Responses SSE parsers；retry/status mapping；request golden tests |
| **Rewrite** | Responses stateful parser、strict EOF/cancellation transport、typed protocol resolver、model profile Source of Truth、Antigravity/Codex/Ollama 专用 Connection adapters |
| **Move to Host private implementation** | reqwest client、auth header、Credential material、Connection health/model discovery、routing/failover/Key Pool policy |
| **Delete after cutover** | Agent history bridge、legacy `ProviderStreamEvent`、static registry、DeepSeek/OpenAI-compatible pass-through wrappers、mock trait contract、Daemon provider callers/fallback |

关键 Source of Truth 冲突：旧 `ProviderType` 把 `OpenaiCompatible` 当厂商；Antigravity 自报 Gemini；Claude/Gemini model profile 与 adapter 静态列表的 output limits 不一致；`structured_output` 字段被 body builder 静默忽略。

### 9.3 已完成的 P0 安全修复

- `Credential` 与 `OpenAiCodexCredential` 的 `Debug` 明确脱敏。
- transport error 不再直接回传含 URL 的 `reqwest::Error` 文本。
- Gemini API key 从 query string 移到 `x-goog-api-key` header，并有本地 TCP request-target 测试。
- OAuth 无 identity 时使用 secret-safe fingerprint，账户显示名不再回退为 token/identity。
- in-memory credential lease 在 issue/revoke/status 前清理过期项。
- Proxy 请求使用唯一 request/run identity，避免共享固定 lease identity。
- Chat Completions、Responses、Messages、Gemini/Antigravity 的异常 EOF 统一返回 `incomplete_stream`，parser Error 后不再伪造 Completed。
- Responses parser 改为 stateful，跨事件保留 function `call_id` / name / argument delta。
- 四协议真实 wire fixture 固化 tool、reasoning、usage 与 stop reason；Gemini terminal 保证位于同 chunk payload 末尾。
- SSE 未完成 remainder 限制为 1 MiB；本地 TCP 测试证明取消/drop 会关闭 upstream socket。

以上是安全基线修复，不代表 Proxy P0 Gate 已完成。

### 9.4 最小实施顺序

1. 固化 Messages / Chat Completions / Responses 的真实 request、stream、non-stream、error fixtures。
2. 将异常 EOF 改为结构化 Error，补取消、断连、慢 consumer 与 socket/task 回收测试。
3. 用 canonical request/event 贯通一个 ingress vertical slice，保留 tools/history/reasoning/usage/stop reason。
4. 修复 Antigravity 专用 adapter/project header 与 OAuth refresh/expiry/client secret。
5. 在 Host 建私有 `ProxyEngine`，先复用 pure codecs/parsers，再迁 HTTP、Connection 与 Key Pool policy。
6. 通过 fixture parity 后一次切换 production path，同批删除 Daemon provider caller/fallback。
7. 按 ADR-0020 / R-S12 引入 Host-private `SecretStore` seam，完成 Keychain read/write/verify/rollback/locked migration；不得形成第二 Credential authority。

### 9.5 Gate 验证基线

- `provider-adapters`: 初始 106 passed / 4 ignored；安全与 strict-stream 修复后 121 passed / 4 ignored。
- `assistant-protocol`: 初始因新增 `project_id` fixture 缺字段无法编译；补齐 fixture 后 63 passed。
- `protocol:check`: 通过，165 个 Rust methods 已登记。
- 全局方案路径 `/Users/ldh/Downloads/AiNative-Rearchitecture-Plan-2026-08-19` 在当前文件系统不存在；本节以真实源码、Standards、ADR-0020、用户冻结目标与 Home Patch 为依据。
