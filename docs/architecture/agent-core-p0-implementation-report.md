# Agent Core P0 实施报告

## 1. 开发基线

- 分支：fix/agent-core-p0
- 开始 Commit：1b4b1792932e2e24160091c7700a2c092d01e5f2
- 代码结束 Commit：a601636 + bd3f88c（文档提交随后生成；最终 Worktree 提交以 git log 为准）
- Worktree：/Users/ldh/Downloads/project/AiNative/Natives-agent-core-p0
- Pi 参考 Commit：583f153d502aa8e958eefdb9af0fbd3344e68f95
- 原工作区：保持原有 Settings/UI 冲突与研究文档未修改

## 2. 审计问题复核

| 问题 | 当前是否存在 | 修复状态 | 代码证据 |
|---|---|---|---|
| Credential 固定 provider-stream | 原生产主链不存在 | 已修复 | agent-core EngineProviderContext；daemon RealProvider::stream_with_context_controls |
| Provider API Mode 进程全局 env | 不存在于请求路径 | 已修复 | OpenAiApiMode；resolve_adapter 不再 set_var |
| 非法 JSON / Schema 未校验 | 不存在于执行边界 | 已修复 | engine strict parse；CapabilityGateway::validate_schema |
| Stop Reason 丢失 | 不存在于生产 Adapter 事件 | 已修复 | ProviderStopReason；EngineProviderEvent::CompletedWithReason |
| Hook Deny Tool Result 丢失 | 不存在 | 已修复 | PreparedToolCall.rejected；统一 pairing |
| Gateway 通用 Cancel | 不存在 | 已修复 | child CancellationToken + select Handler/Timeout/Cancel |
| 完整 Tool Progress Sink | 仍存在 | 本轮不做 | EngineToolRuntime 仍无 progress sink |
| Active Context 无损恢复 | 仍存在 | 阶段 1 | 未修改 Conversation 数据库模型 |
| Event persist failure 全链路 fail closed | 仍存在 | 后续 P1 | 未修改 Event/RunManager 权威边界 |

## 3. 实施内容

### 3.1 Run Identity

- 修改：新增 EngineProviderContext，包含 run_id 与 attempt；Engine 每次 Provider Attempt 和模型摘要请求均通过 stream_with_context；RoutedProvider 将上下文传入 RealProvider；Credential Broker 使用真实 Run ID。
- 不变量：Credential 明文仍只在 Daemon；Parent/Child 不共享 Lease ID；Engine 不拥有 Credential。
- 测试：production_credentials::run_identity_tests::broker_receives_distinct_run_ids；agent-core 全量测试通过。

### 3.2 Provider API Mode

- 修改：OpenAiAdapter 增加 OpenAiApiMode::ChatCompletions / Responses；Responses route 显式构造 Adapter；删除请求期间 NATIVES_OPENAI_API set_var/read。
- 不变量：Mode 是 Adapter 实例不可变状态；其他 Provider 不受影响。
- 测试：api_mode_tests::explicit_api_mode_is_request_independent；provider-adapters 全量测试通过。真实 HTTP endpoint/body 并发 fixture 留作后续。

### 3.3 Stop Reason

- 修改：ProviderEvent::Completed 携带 ProviderStopReason；OpenAI SSE、Responses、Anthropic、Gemini 与兼容路径映射原始原因；Engine 增加 CompletedWithReason，并对 Length、Unknown、无 Final Event 的 Tool Call fail closed。
- 不变量：截断或不可靠终态不得进入 Handler；每个未执行调用仍有同 ID 错误 Result。
- 测试：OpenAI length parser；agent-core length_stop_never_executes_collected_tool_call；provider-adapters 96 通过。

### 3.4 Schema Validation

- 修改：Gateway 在 Path/Permission/Handler 前验证注册 Schema 的常用 Draft 子集；提供 validate_tool_schema；Core 不再构造 raw fallback。
- 不变量：Schema 是执行信任边界；校验失败 Handler invocation 为零。
- 测试：malformed/type mismatch；全部内置 Schema 清点；capability-gateway 109 通过。

### 3.5 Tool Result Pairing

- 修改：PreparedToolCall.rejected 统一承载 Hook Deny、Invalid JSON、截断/未知 Stop；执行器只跳过 Handler，后续保留 Assistant Tool Call 与同 ID Tool Result。
- 不变量：成功、拒绝、失败、取消、超时和截断调用均保持一对一 pairing；RunManager 仍是唯一终态权威。
- 测试：hook_deny_keeps_tool_call_and_emits_one_error_result；length test；agent-core 164 通过。

### 3.6 Cancellation

- 修改：CapabilityGateway::execute 为每次调用创建 child CancellationToken，同时 select Handler、Timeout、父 Cancel；timeout/cancel 错误码区分并先 cancel child token。
- 不变量：取消不伪装为超时；资源型 Handler 可观察 child cancellation；未实现完整 Progress Sink。
- 测试：cancellation_wins_over_blocking_handler；Daemon 串行全库 327 通过；Shell/MCP 真实 cleanup fixture 留作专项集成。

## 4. 修改文件

| 文件 | 修改原因 |
|---|---|
| crates/agent-core/src/engine.rs | Provider context、stop reason、strict JSON、rejected pairing |
| crates/capability-gateway/src/lib.rs | Schema boundary、child cancel、P0 tests |
| crates/provider-adapters/src/stream/*.rs | Provider stop reason mapping |
| crates/provider-adapters/src/providers/*.rs | Explicit OpenAI mode、invalid JSON fail closed |
| crates/provider-adapters/src/http_stream.rs | Missing-final stop reason |
| src-agent-daemon/src/production.rs | Real Run ID、explicit Responses adapter、event mapping |
| src-agent-daemon/src/routing.rs | Context propagation、stop reason mapping |
| src-agent-daemon/src/production_credentials.rs | Run ID broker test |
| src-agent-daemon/src/production_hooks.rs | Hook provider request Run ID |
| src-agent-daemon/src/loopback.rs / rpc.rs | New completion variant compatibility |
| docs/architecture/agent-core-p0-revalidation.md | Current HEAD revalidation record |
| docs/architecture/natives-agent-core-audit-findings.md | Original findings plus repair status/evidence |

## 5. 测试结果

| 命令 | 结果 | 说明 |
|---|---|---|
| cargo test -p provider-adapters | 通过 | 96 passed, 4 ignored |
| cargo test -p agent-core | 通过 | 164 passed |
| cargo test -p capability-gateway | 通过 | 109 passed |
| cargo test -p natives-agent-daemon --test rpc_dispatch_contract | 通过 | 10 passed |
| cargo test -p natives-agent-daemon --lib -- --test-threads=1 | 通过 | 327 passed, 1 ignored |
| cargo check -p provider-adapters -p agent-core -p capability-gateway -p natives-agent-daemon | 通过 | 4 crates compiled |
| cargo fmt --check | 通过 | 无格式差异 |
| git diff --check | 通过 | 无空白错误 |
| cargo test --workspace | 未纳入通过统计 | 运行期间没有返回可审计退出结果；目标 crate 与 Daemon 串行全库已分别通过 |
| npm / UI 全量验证 | 未运行 | 本轮 Rust P0，原工作区存在 UI 冲突且不在范围 |

## 6. 未完成或偏差

- 本轮没有引入完整 Turn、AgentMessage、Tool Scheduler、Progress Sink、Steering/Follow-up、Context Snapshot 或 Checkpoint Resume。
- Schema 校验使用现有依赖之外的最小 Draft 子集；若未来引入更复杂 Draft/oneOf/Normalizer，应先做 manifest 兼容性清点并统一验证器。
- Provider Stop Reason 的 live 网络终态和 Shell/MCP 资源 quiet 仍需专项集成测试。
- EngineProvider 保留无上下文兼容方法；生产 Engine 主链使用上下文方法，后续可在阶段 1 删除 legacy seam。

## 7. 阶段 1 前置条件

- 保持本轮 Run ID、Stop Reason、Schema、Tool Result pairing 和 Cancel 测试为回归门。
- 先定义 additive Turn/typed message 内部转换，不修改外部 RPC 和 Conversation schema。
- 为 Active Context 建立可重放 fixture，明确 Full History 与 Active Snapshot 的边界。
- 将 Tool Result pairing 属性测试扩展到多 Tool Call、Permission、Timeout、Cancel 和 Provider no-final。

## 8. 风险与回滚

- 代码回滚点：a601636；文档随后单独提交。
- 主要兼容风险是旧 Provider Fixture 使用无参数 Completed；当前保留该变体并按 Stop 处理，生产 Adapter 使用 CompletedWithReason。
- 若某 Provider 的原始终态无法可靠映射，应保持 Unknown 并拒绝 Tool Call，不得恢复 raw 执行回退。
- 不推送远程；原始脏工作区不受本 Worktree 影响。
