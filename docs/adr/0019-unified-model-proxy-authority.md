# ADR-0019: 统一模型代理（Unified Model Proxy）Authority 边界与实施选型

- **状态**: 部分被 [ADR-0020](./0020-ai-native-personal-workspace-rearchitecture.md) 取代（保留 P0 源码审计；Daemon 完成态 authority 与 SQLite Secret 设计已失效）
- **日期**: 2026-08-18
- **决策者**: 技术方（工程归属拍板；产品需求由用户提出）
- **关联**: [ADR-0011](./0011-native-engine-production-gaps.md)、[ADR-0015](./0015-job-module-ownership-and-dispatch.md)、[ADR-0016](./0016-capability-hub.md)、`docs/architecture/provider-proxy-architecture.md`、`docs/architecture/provider-routing-sub2api.md`、`docs/standards/technical/01-layering.md`、`docs/standards/technical/02-security.md`
- **Plan**: `Natives-Proxy-Minimal-Dependency-Plan-v4`（`00-EXECUTION-OVERRIDES.md` 为最高优先级修订；本 ADR 不预先冻结 Go / CLIProxy SDK / CLIProxy binary，除非 P0.5 证据充分）

---

## 上下文

> **2026-08-19 修订**：目标实现改为 Tauri Host-owned `ProxyEngine` 与 OS Keychain Secret ownership。本文的源码审计仍可引用，但任何 Daemon-only production 结论只描述迁移前现状。

产品需要 Provider OAuth、多 Credential 管理、统一模型访问（internal）+ 可选本机/LAN 共享（external）。历史尝试曾以「Natives 必须引入 Go proxy-core / CLIProxy SDK」为前提，用户已回滚并要求重审：**实现语言不预选、外部依赖必须过 Reference Necessity Gate、P0.5 后只保留一条 production path**（Plan v4）。

P0 审计（2026-08-18，基线 `deploy` @ `e075c1e`，见 `provider-proxy-architecture.md` §2）确认 Natives 已拥有：

- provider-adapters：6+1 适配器（openai/anthropic/gemini/deepseek/openai_compatible/ollama + Codex OAuth）、SSE 解析器、canonical `ProviderRequest/ProviderEvent/ProviderUsage/ProviderError`、`ProtocolResolver` 自动协议路由；
- Credential Broker：单 key 租约、Sub2API 池租约、routing settings/plan 租约、UDS listener、KEK/DEK 信封加密、AES-256-GCM v2 持久化；
- Daemon 路由执行器：`RoutedProvider`/`Sub2ApiPoolProvider`、熔断、failover、OAuth 刷新（Codex）、governor 限流冷却、loopback 本地入站；
- 进程监督：`sidecar_supervisor`（watchdog/readiness/重启/进程树清理）；
- 参考实现：`commands/mcp_oauth.rs` 浏览器流（loopback + PKCE）。

缺口（G1–G8，见架构文档 §3）：通用 Provider OAuth 生命周期、`has_usable_credential`（API-key-first 预检）、统一 Gateway 契约未接线（协议 DTO 已定义未广告）、旧路由语义（2026-08-11 冻结）与代码不一致、Proxy 政策服务层、usage 来源标注、quota 可靠来源、前端旧路由域收敛。

---

## 决策

### 1. Authority 边界（冻结，不随选型变化）

| Authority | 归属 | 职责 |
|-----------|------|------|
| **Provider** | Provider 域（Host 持久化配置 + provider-adapters） | API Key / OAuth 账户 / identity / quota / health / model catalog / credential usability |
| **Host** | `src-tauri/` | `natives.db`、加密凭证 SoT（AES-256-GCM）、政策/设置、外部入站权威、子进程权威（若选 B/C） |
| **Agent Daemon** | `src-agent-daemon/` + `crates/*` | Run / Agent Engine / Provider 执行语义 / Tools / 事件；**唯一 Agent runtime** |
| **Renderer** | `src/` | UI 投影；经 `tauri-adapter`；禁止直连 SQLite / 子进程 / 上游密钥 |
| **Proxy / Model Gateway** | 能力边界 `ModelDataPlane`（语言中立） | 统一模型访问、credential 路由、协议兼容、本地/LAN 共享、usage 来源标注、健康/失败态 |

不变量：

- Credential persistent SoT 唯一（Host）；**禁止 plaintext credential JSON/YAML、secret argv/env/log**。
- Routing executor 唯一；Proxy 不得成为第二个 Agent runtime；不新增第三个一级业务 Authority（不新造「Routing」Authority）。
- Usage Authority 唯一（Natives canonical usage；外部实现返回的 usage 一律 normalize 到 canonical，不建第二套统计 DB）。
- `ModelDataPlane` 是能力契约（start/stop/health/apply credential/remove credential/apply routing settings/request transport/usage event/OAuth runtime event），上层只依赖契约，不知道底层语言/项目。

### 2. OAuth-only Provider 必须工作

`provider_api_keys = 0` 且 `provider_accounts` 含可用 OAuth 账户时，Agent run 必须成功。修正一切 API-key-first 逻辑（`has_active_key`、API-key-only 预检、key-first Broker 逻辑）为统一的 `has_usable_credential`。此决策独立于数据面选型（P1）。

### 3. 优先复用现有 Credential Broker 与协议

不默认创建第二套 Broker / ProxyEndpointLease / 第二套 Credential RPC。现有 `assistant-protocol` v2 `credential.rs` DTO（CredentialRef / CredentialLeaseMeta / CredentialPoolLease / RoutingPlanLease）先验证能否表达 Proxy-backed Credential；只有证明无法表达时才扩展协议，且 `assistant-protocol` 是唯一协议定义源（R-B4，改动必须 `npm run protocol:check`）。

### 4. 实施选型（**Pending P0.5**）

P0.5 在三个候选间做窄 Spike 并给证据，不预先实现任何候选的完整架构：

- **A. Natives-native Rust Model Gateway**：复用 provider-adapters / Broker / Daemon 执行权威；无新语言、无新进程。优先评估。
- **B. stock CLIProxyAPI binary sidecar**：仅当其公开/runtime 接口满足 Natives 安全与生命周期（无 plaintext 持久化、刷新可回写、管理面可隔离、loopback-only 可运行）。不引入 Go 源码/工具链。**安全 gate（凭证持久化、刷新回写）未证实前默认不合格（fail-closed）。**
- **C. thin isolated CLIProxy SDK adapter**：仅当 A 成本明显过高且 B 不满足安全/控制要求。若 SDK 为 Go，Go 只是 C 的后果，不是项目要求；adapter 必须可替换、不拥有 Natives DB / Provider SoT / Proxy Settings SoT / Usage DB / UI 政策。

选择标准：Security MUST 通过（gate，非评分）→ 比较架构契合、新代码量、构建/运行依赖、协议重复度、上游耦合、测试负担、性能、故障恢复、升级/回滚复杂度 → **选长期总复杂度最低者**。不只为「少写今天几行」或「零新语言」或「最大复用」优化。

选型后：更新本 ADR（Selected / Evidence / Rejected）；删除被拒 spike 代码；只保留一条 production path；不保留「native + CLIProxy fallback」。

### 4.1 P0.5 Spike 结论（2026-08-18）

> 本轮 P0.5 未创建任何候选 spike 代码——选型证据来自代码审计（架构文档 §2/§3）与 CLIProxyAPI 上游公开契约的定向核查（Plan 22 Level 1，仅回答 B/C 安全与生命周期问题）。

**Selected: A — Natives-native Rust Model Gateway**

**Evidence（为什么选 A）**：

1. 现有 provider-adapters 已可执行所需请求形状（streaming / tool calls / reasoning / usage 解析 / error 映射 / cancellation），无需新协议实现（`crates/provider-adapters`，6+1 适配器）。
2. Agent Daemon 可承载通用 model-gateway 契约：`loopback.rs` 已提供 OpenAI Chat/Responses + Anthropic Messages 本地入站并复用 `RoutedProvider`——证明「Daemon 内网关 + 复用 provider-adapters」路径可行且已部分存在。
3. Credential Broker 已支持 OAuth 账户解析：`broker_pool_acquire` 解密 Sub2API OAuth 账户；`routing.rs refresh_codex_account` 已实现 OAuth 刷新（含 JWT 过期解析）——OAuth-only Provider 只需泛化账户模型（P1），无需新 Broker。
4. 路由/冷却/故障转移已存在：`RoutedProvider`（failover、首 delta 不重放、熔断）、`governor`（RPM 限流 + 429 冷却）、`Sub2ApiPoolProvider`（池内轮转）——新 Proxy 政策层只需包装而非重写。
5. 无需新协议翻译：`ProtocolResolver` 已是唯一协议解析器，回退边界（401/403/429 不换协议、首 delta 后不重放）已冻结。
6. 零新增语言/进程/构建依赖；`sidecar_supervisor` 无需改动。

**Rejected（为什么不选其他方案）**：

- **B 被拒（安全 gate 失败）**：CLIProxyAPI stock 配置（`config.example.yaml`）证实 `auth-dir: "~/.cli-proxy-api"` 下 OAuth/token 以 **JSON 文件明文落盘**（`weight`/`request_retry` 等字段写在 auth JSON 中），`api-keys` 亦明文写于 YAML——违反 Natives `R-S12`（AES-256-GCM、无明文凭证持久化）。Plan P0.5-B 明确「凭证持久化不满足 → reject B，不再深入调查」。B 无 plaintext 隔离路径可证明，且增加 binary 版本钉定/升级耦合/管理面安全边界，总复杂度上升。
- **C 被拒（必要性不成立）**：CLIProxyAPI Go SDK（`sdk/cliproxy`）虽支持 `WithTokenClientProvider` 内存凭证（可避开明文落盘），但引入 Go build target（新语言）+ 新 sidecar 进程 + 第二 security boundary + 上游版本耦合；而 Natives 已拥有其核心能力（路由/熔断/冷却/刷新/协议翻译）。Reference Necessity Gate 问题 1「已有等价能力？」= 是；问题 10「总复杂度下降还是上升？」= 上升。Benefit <= Long-term Complexity → 不引用。

**新增依赖/语言/进程/security boundary**：0 / 0 / 0 / 0（A 方案）。

**Reference Necessity Evidence**：CLIProxyAPI 仓库 `config.example.yaml`（auth-dir/plaintext api-keys，B 拒因）与 `docs/sdk-usage.md`（内存 token provider 存在但需 Go，C 拒因）；EasyCLI 未参与选型（无未解决 UX 问题，Level 2 不触发）。

### 5. 旧路由语义收敛（P10，已按选型 A 解决）

2026-08-11 冻结（`provider-routing-sub2api.md` §决策冻结）曾计划退役 failover 路由绑定 / loopback / rectifier / global proxy。选型 A（Natives-native Rust）**复用**这些组件作为唯一生产路径，故该退役计划被取代：不删除、不迁移。唯一 routing executor 仍是 `RoutedProvider`；gateway（`model.gateway.stream`）是附加的外部入站，不构成第二套调度。无新旧双路径共存。

### 6. 外部引用纪律

- EasyCLI 仅为 UX / Product / Integration 参考（OAuth 交互 UX、quota 展示），**永不当架构权威**，不复制其 Rust/TS 布局、management API、settings/quota/usage DB、auth-file 架构。
- CLIProxyAPI 是候选 direct upstream，不是默认 production dependency；任何引用必须回答具体未解决问题（凭证注入/刷新回写/协议路由/生命周期/打包）。
- 版本：不硬编码上游 SDK/binary 版本；选型后从实际源码解析并记录 commit/checksum。

---

## 后果

### 正面

- **单一权威不变**：凭证、路由执行、usage 各保持唯一 Authority；Proxy 只是能力边界，不引入第二个业务后端。
- **可替换性**：`ModelDataPlane` 契约隔离实现细节，未来实现可替换而不动 Provider/UI/领域类型。
- **安全红线保持**：OAuth token 持久化沿用 AES-256-GCM；Renderer 只拿到 safe session 状态；无明文降级。
- **诚实能力面**：未接线/未实现的能力不广告（与 R-T5、`IMPLEMENTED_METHODS` 纪律一致）。

### 负面 / 成本

- A 方案可能需要显著的协议翻译 / 路由 / OAuth 工作（P0.5 需实测量化）；B/C 引入二进制/SDK 依赖与第二 security boundary，升级耦合上升。
- 旧路由语义（loopback/rectifier/global proxy）与代码的收敛需要 P10 专项清理，期间文档需持续对齐。
- OAuth-only provider 的预检改造（`has_usable_credential`）会触碰现有 key-first Broker 逻辑，需回归测试保护 API Key 路径。

### 迁移 / 回滚

1. P1 先落 Provider Credential 域（OAuth 账户模型、`has_usable_credential`、加密持久化），不依赖选型；
2. P2 按选型建 `ModelDataPlane` 基础；
3. P4 cutover 内部流量走所选数据面，验证 stream/tool/reasoning/cancel/error/usage 对齐后删除迁移期 direct fallback；
4. 破坏性 schema 清理前确认无引用、可回滚；失败 spike 一律删除。

### 后续跟踪

- P0.5 Spike 证据完成后更新本 ADR 第 4 节（Selected / Evidence / Rejected / 新增依赖清单 / 安全影响）。
- `provider-proxy-architecture.md` 只记录被选 production path；被拒候选仅留在本 ADR 历史。
- 若最终选 C（Go adapter）：新增 `docs/development/model-gateway-build-and-release.md`；若选 A：不新增语言/构建文档。
