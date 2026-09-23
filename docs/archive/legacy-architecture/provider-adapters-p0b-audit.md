# provider-adapters 逐文件 P0-B Salvage Audit

> **状态**: 审计完成（2026-08-20，baseline `deploy` 工作树）
> **目标**: ADR-0020 将 provider-adapters 从 Agent 语义 shell 中救出协议/传输/模型元数据资产
> **权威**: `docs/standards/` > ADR-0020 > `architecture/provider-proxy-architecture.md` §9.2
> **处置类别**: Keep / Extract / Rewrite / Delete（删除需在替代路径落地后执行，禁止"先 rm 再修编译"）

## 1. 总体结论

provider-adapters **不能整体删除**：canonical request/event/usage/error 类型、Chat/Responses/Messages/Gemini body builders、四协议 SSE parsers、HTTP transport、协议 resolver、模型画像都是 Host `ProxyEngine` 的直接资产。但公开 API 仍被两处 legacy Agent 语义污染，需在 cutover 前剥离：

1. `ProviderAdapter` trait（`capabilities.rs:783`）暴露 `chat/chat_stream/stream` + 静态模型 + 发现 + 测试 4 组语义，是浅 interface —— 完成态由 Host 深 `ProxyEngine` 隐藏。
2. `ProviderStreamEvent`（`capabilities.rs:308`）只被 legacy `src-agent-daemon`（`creative_ai.rs:200`、`rpc/handlers/provider.rs:135`）消费 —— **Delete after cutover**。
3. 全部 `providers/*.rs` 依赖 `assistant_protocol::v1::provider::{ModelCapabilities, ProviderType}` 表达厂商/模型能力 —— 需 Extract 到本 crate 自有类型。
4. `stream/openai_responses.rs:237`、`http_stream/transport.rs:139` 依赖 `assistant_protocol::v2::redact_secrets` —— 需 Extract 本地等价实现（宿主日志脱敏已有 `src-tauri/src/log_sanitizer.rs`，可下沉复用）。

## 2. 逐文件处置矩阵

### 2.1 核心类型与协议（Keep / Extract）

| 文件 | 行数 | 处置 | 依据与目标 consumer |
|---|---:|---|---|
| `src/capabilities.rs` | 921 | **Extract**（>700 原则拆分） | canonical `ProviderMessage/ContentBlock/Request/Tool/Usage/Error`（308-728 区间）Keep 并迁入 ProxyEngine 域；`ProviderAdapter` trait（783）与 `ProviderStreamEvent`（308）删除后由深 Engine 取代。当前已受 `contract.rs` 保护 |
| `src/model_profile.rs` | 582 | **Keep + Extract** | `ModelFamily/PromptCacheMode/ReasoningControl/ModelProfile` 是模型元数据 SoT 候选；`resolve/resolve_max_output` 保持。`unknown()` 与静态 output limits 不一致问题（§9.2 SoT 冲突）需在 Rewrite 阶段收敛到单一来源 |
| `src/protocol_resolver.rs` | 234 | **Rewrite** | `Protocol/ProtocolContext/candidate_protocols/should_retry_next_candidate` 概念保留；实现改为 typed resolver，不再依赖旧 ProviderType 枚举 |
| `src/capabilities_history.rs` | 211 | **Keep** | History 归一资产，独立无耦合 |
| `src/lib.rs` | 22 | **Keep** | 装配面；删除 `ProviderStreamEvent`/trait 导出后瘦身 |

### 2.2 HTTP / Transport（Keep / Extract）

| 文件 | 行数 | 处置 | 依据与目标 consumer |
|---|---:|---|---|
| `src/http_client.rs` | 24 | **Keep** | proxy-aware `client()` 构造，直接复用 |
| `src/http_stream.rs` | 23 | **Keep** | 装配面；`build_*_body` / `stream_*` / `map_http_status` / `retry_after_ms` 公开面已是正确边界 |
| `src/http_stream/body.rs` | 370 | **Keep** | Chat/Responses body builders + `message_to_json`；受 `request_body_golden.rs` 字节级保护 |
| `src/http_stream/transport.rs` | 438 | **Extract** | SSE streaming/non-streaming + status/retry 映射 Keep；`redact_secrets`（139）改为本地实现；`append_sse_bytes` 有界缓冲（1 MiB）保留 |
| `src/http_stream_tests.rs` | 394 | **Keep** | body/transport 单测资产 |

### 2.3 SSE Parsers（Keep / Extract）

| 文件 | 行数 | 处置 | 依据与目标 consumer |
|---|---:|---|---|
| `src/stream/mod.rs` | 11 | **Keep** | 装配面；`ProviderEvent` re-export 保留（新域命名） |
| `src/stream/openai_sse.rs` | 483 | **Keep** | `OpenAiSseParser` + `ProviderEvent/ProviderStopReason` 核心流事件类型；受 `protocol_stream_fixtures.rs` 保护 |
| `src/stream/openai_responses.rs` | 335 | **Rewrite** | stateful parser（保留 call_id/name/arguments delta）；`redact_secrets`（237）改本地实现；补 failed/incomplete/cancelled fixture |
| `src/stream/anthropic_sse.rs` | 319 | **Keep** | `AnthropicSseParser`；受 fixture 保护 |
| `src/stream/gemini_sse.rs` | 198 | **Keep** | `parse_gemini_chunk`；受 fixture 保护 |

### 2.4 Provider Adapters（Extract / Delete）

| 文件 | 行数 | 处置 | 依据与目标 consumer |
|---|---:|---|---|
| `src/providers/mod.rs` | 65 | **Rewrite** | `register_all`/`ProviderNotFoundError` 概念保留；注册表语义迁入 Connection registry；`ProviderType` 依赖移除 |
| `src/providers/anthropic.rs` | 593 | **Extract** | body builders + 行为 + SSE 接入 Keep（`anthropic_tests.rs` 443 行资产保留）；`ModelCapabilities/ProviderType` import（6）改为本地类型 |
| `src/providers/gemini.rs` | 728 | **Extract**（>700 拆） | body/行为 Keep；拆 >700；同上解耦 v1::provider |
| `src/providers/antigravity.rs` | 552 | **Keep + Rewrite** | 专用 adapter + `x-goog-cloud-target-resource` project header（已测）；Connection 语义保留，Agent shell 删除 |
| `src/providers/openai.rs` | 333 | **Extract** | body/行为 Keep；解耦 v1::provider |
| `src/providers/openai_codex.rs` | 107 | **Rewrite** | 内存态 `OpenAiCodexCredential` + Responses transport 迁入 Host 私有；OAuth 语义由新 Connection/Credential 取代 |
| `src/providers/deepseek.rs` | 191 | **Extract** | 保留协议能力；解耦；命名不再作为"厂商"（§9.2 SoT 冲突） |
| `src/providers/ollama.rs` | 161 | **Extract** | 同上 |
| `src/providers/openai_compatible.rs` | 160 | **Extract** | pass-through wrapper 资产保留为 Connection 类型；§9.2 标注可能 Delete 的"wrappers"仅指无独立语义的浅透传，此处保留协议转换能力 |

### 2.5 测试资产（Keep）

| 文件 | 处置 | 依据 |
|---|---:|---|
| `tests/contract.rs` | Keep | 每个注册 adapter 的契约套件；随 Connection registry 演化 |
| `tests/request_body_golden.rs` | Keep | 四 body builder 字节级回归 + controls 到 wire 证据 |
| `tests/protocol_stream_fixtures.rs` | Keep | 三协议 fixture 端到端（tool/reasoning/usage/stop reason） |
| `tests/stream_transport_lifecycle.rs` | Keep | EOF 结构化 Error / cancel 释放 socket / 有界缓冲 |
| `tests/fixtures/*.sse`（4 文件） | Keep | 真实 wire fixture |
| `tests/prompt_cache_kill_switch.rs` | Keep | 环境开关隔离测试 |
| `tests/live_anthropic_e2e.rs` / `live_openai_compatible_e2e.rs` | Keep | opt-in live 验证 |

## 3. 依赖收敛清单（按序执行）

| # | 动作 | 影响文件 |
|---|---|---|
| 1 | 本地化 `redact_secrets`（Extract 自 `assistant_protocol::v2`，语义对齐 `src-tauri/src/log_sanitizer.rs`） | `transport.rs:139`、`openai_responses.rs:237` |
| 2 | 建立本 crate `ModelCapabilities`/`ProviderType` 本地类型并迁移 7 个 `providers/*.rs` | 全部 providers |
| 3 | 删除 `ProviderStreamEvent` 与 `ProviderAdapter` trait 前，先落地 Host ProxyEngine 替代执行路径（P0-A 已验证 codec 可复用） | `capabilities.rs`、`lib.rs` |
| 4 | cutover 后删除 Daemon caller（`src-agent-daemon/src/creative_ai.rs:200`、`rpc/handlers/provider.rs:135`） | legacy 路径 |
| 5 | 删除后同步 `Cargo.toml` 的 `assistant-protocol` 依赖（若已无其它引用） | `crates/provider-adapters/Cargo.toml` |

## 4. 验收证据

- `rtk cargo test -p provider-adapters`：122 passed / 4 ignored（9 suites）。
- 现有 fixture/transport/golden 测试覆盖第 2 节 Keep/Extract 资产。
- 任何 Extract/Rewrite 不得破坏 `tests/contract.rs`、`request_body_golden.rs`、`protocol_stream_fixtures.rs`、`stream_transport_lifecycle.rs` 的通过态。
