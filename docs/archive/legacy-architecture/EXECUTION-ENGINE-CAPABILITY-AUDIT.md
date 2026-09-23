# Native 执行引擎能力审计

> **版本**: 1.0.0
> **审计日期**: 2026-07-26
> **审计基线**: worktree `feat/engine-hardening`，commit `682453e3`
> **范围**: Native 执行引擎全链 —— Hook 运行时、Agent Loop、Provider 适配层、Context/压缩、Skills、Subagent、执行图、GUI 接口契约
> **性质**: 现状描述与缺口定级（**非约束**）。约束以 [`docs/standards/`](../standards/README.md) 为准
> **进度权威**: [`NATIVE_ENGINE_FULL_REMEDIATION.md`](./NATIVE_ENGINE_FULL_REMEDIATION.md)。本文是该文件第 2、3 节标签的证据来源；两者冲突时以进度表为准，并回头修正本文

---

## 0. 阅读须知（诚实边界）

本文出现的百分比是**工程判断，不是测量数据**。请按以下前提阅读：

1. **Claude Code 与 Codex 都是黑盒。** 我们没有它们的源码、没有它们的内部契约文档。对照结论是从三类可观测材料**推断**出来的：公开文档与 changelog、它们落在磁盘上的产物（`~/.claude/` 配置、hook JSON 协议、stream-json 输出格式、usage JSONL），以及本仓库既有的兼容层实现（`cli_runtime_bridge.rs` 解析 Claude CLI 的 stream-json；`hook_handlers.rs` 解析 Claude Code 的 hook stdout 契约）。
2. **百分比的分母是「我们推断的对标能力集」，不是「对方的真实能力集」。** 对方若有未公开能力，我们的分母偏小、百分比偏高。
3. **百分比不可跨环节比较。** 「Hook 80%」与「Skills 30%」的分母是两套不同的能力集，两个数字之间做算术没有意义。
4. **代码结论可复核。** 本文每条能力结论都标注 `文件:行号`（相对仓库根），全部在基线 `682453e3` 上人工复核过。行号会随并行改动漂移，复核时请以符号名为主、行号为辅。
5. **本文描述基线 `682453e3`，不是描述 HEAD。** 审计进行期间有并行改动正在同一 worktree 落地。截至写稿，以下被本文引用的文件已在工作区被修改：
   `crates/agent-core/src/{doom_loop,profile,subagents}.rs`、`crates/assistant-protocol/src/v2/methods.rs`、`crates/capability-gateway/src/tools/extra.rs`、`crates/provider-adapters/src/{capabilities,lib}.rs`、`crates/provider-adapters/src/providers/anthropic.rs`、`crates/provider-adapters/src/stream/{anthropic_sse,openai_sse}.rs`、`src-agent-daemon/src/{production,production_tools,rpc}.rs`、`src/lib/assistant-workspace/capability-gate.ts`。
   即第 3（Agent Loop）、4（Provider）、7（Subagent）、9（GUI 契约）节涉及的部分缺口**可能已被修复或部分修复**。**复核时请以符号名定位，不要依赖行号**；判断某条缺口是否仍然存在，须重新读码，不可直接引用本文。本文的价值是**缺口清单与定级**，不是实时状态看板 —— 实时状态看 [`NATIVE_ENGINE_FULL_REMEDIATION.md`](./NATIVE_ENGINE_FULL_REMEDIATION.md)。

---

## 1. 能力矩阵（结论先行）

| 环节 | 追平度（推断） | 已经站住的地基 | 一句话缺口 |
|------|------|------|------|
| **Hook 系统** | **~80%** | 16 个事件全部有触发点；`parse_hook_stdout` 真兼容 Claude Code 的 hook stdout 契约 | hook 不能自动**批准**权限；`Rewake` 语义为空转；Hook 对 GUI 完全不可见 |
| **Agent Loop** | **~70%** | 重试/退避、空响应重试、已产出内容不重试、max_steps、并行安全批处理、安全点插话 | 无 Plan Mode；doom-loop 只看尾部连续段；`retry_after_ms` 解析出来后被忽略 |
| **Provider 层** | **~50%** | 7 个适配器 + 4 个 SSE 解析器（3778 行），真流式、真工具、真 usage、真取消 | 无 prompt caching、无 `tool_choice`、无请求侧 `thinking`；`image_input: true` 是**谎报** |
| **Context / 压缩** | **~30%** | 两条压缩路径都已接线；PreCompact/PostCompact 钩子位置正确 | 两条路径都是**纯机械截断**，无模型摘要；PostCompact 无回写通道，接缝形状本身堵死实现 |
| **Skills** | **~30%** | 8 个扫描根、`skill.list` RPC、注入父/子 Run | 不解析 YAML frontmatter；无渐进披露（拼全文）；`trusted: true` 硬编码 → 提示词注入面 |
| **Subagent** | **~60%** | 预算账本（10 项，单 mutex 原子预留）、`cap_child_permission` 子≤父、独立凭证路由 —— 这三项**超出** Claude Code 已知能力 | `AgentProfile` 字段齐全但从未接到子 Agent；`task` schema 无 `subagent_type`/`system_prompt`/`tools`/`model` |
| **执行图** | **~15%** | 父子取消令牌树（632 行）、扁平血缘事件 | **没有 DAG**。现有结构的用途是取消传播，不是调度 |
| **GUI 接口契约** | **~75%** | 78 条广告方法中 73 条有真实分发；`method_status` 三态 fail-closed 设计正确 | **6 条广告方法无 handler**，落兜底返回 `internal_error`（而非诚实的 `unsupported`）；前端能力门被广告骗开 |

**跨环节的三个系统性问题**（比任何单项缺口都值得优先处理）：

1. **「声明了但从不消费」的字段成群出现。** `image_input`（无任何代码读取）、`HookFailurePolicy::{Skip,Default}`（无任何代码读取）、`SubAgentConfig.failure_policy`（无任何代码分支）、`ExtensionManifest.permissions`（只读入不判断）、`AgentProfile` 的 11 个字段（解析后从不读取）、`EngineError::Provider.retry_after_ms`（解析后重试路径不用）。这不是六个独立 bug，是同一种失效模式：**接口先落地、实现留待日后，但没有任何机制标记「这个字段是空头承诺」**。
2. **接缝形状堵死实现路径。** PostCompact 的返回值被 `let _ =` 丢弃、`SafePoint` 的类型参数被 `let _ = point;` 丢弃。这两处不是「实现没写」，而是「接口不允许实现」——补实现前必须先改接口。
3. **广告面与分发面靠人工维护，没有编译期或 CI 约束。** 第 9 节的 6 条违规全部源于「往名单里加了一行、忘了往 match 里加一行」。

---

## 2. Hook 系统（推断 ~80%）

### 2.1 事件模型：16 个事件全部有触发点（证实）

`HookEvent` 定义在 `crates/harness-core/src/hooks/definition.rs:18`，常量 `ALL: [HookEvent; 16]` 在 `:40`。`crates/agent-core/src/hooks.rs:13-16` 只做 re-export。

> 修正一处：审计初稿称该枚举在 `agent-core`。实际权威定义在 `harness-core`，`agent-core` 是转发层。

| 事件 | 定义 | 触发点 |
|------|------|--------|
| SessionStart | definition.rs:19 | `crates/agent-core/src/engine.rs:358` |
| SessionEnd | :20 | engine.rs:333（收尾，无条件） |
| UserPromptSubmit | :21 | engine.rs:368 |
| PreToolUse | :22 | engine.rs:700（逐条消费 decision，`:702+`） |
| PostToolUse | :23 | engine.rs:876 / 957 / 997（task batch / 并行 / 串行 三条路径） |
| PostToolUseFailure | :24 | engine.rs:878 / 959 / 999（按 `result.is_error` 二选一） |
| PermissionRequest | :26 | `src-agent-daemon/src/production_tools.rs:663`（返回值经 `aggregate_allow`，`:669`，真阻断） |
| PermissionDenied | :27 | production_tools.rs:783（仅 `!approved` 分支；返回值 `let _ =` 丢弃） |
| Notification | :28 | production_tools.rs:404 —— **但见下方 2.2** |
| SubagentStart | :29 | `src-agent-daemon/src/production.rs:644`（`aggregate_allow` 可阻止启动，`:654-655`） |
| SubagentStop | :30 | production.rs:863（返回值 `let _ =` 丢弃） |
| PreCompact | :31 | engine.rs:1056（deny 则跳过压缩，`:1062`） |
| PostCompact | :32 | engine.rs:1091 |
| Stop | :33 | engine.rs:649（`aggregate_allow` 校验，`:655`） |
| StopFailure | :34 | engine.rs:659（仅 Stop 被 deny 时） |
| Error | :35 | engine.rs:320（run 返回 Err 时） |

engine.rs 去重后覆盖 11 个事件，与初稿判断一致。

### 2.2 缺口：Notification 的触发点形同虚设

production_tools.rs:404 的 dispatch 被 `:395` 的 `if name == "notification"` 包裹 —— 只有当**被执行的工具名字面等于 `"notification"`** 时才触发。这不是通用通知事件的入口，实际路径上永不执行。

> 修正一处：初稿把它列为「真实触发点」。它是**条件锁死的触发点**，等级应低于其余 15 个。

### 2.3 Claude Code hook 契约兼容性（证实）

`crates/agent-core/src/hook_handlers.rs:196` 的 `parse_hook_stdout` 确实按 Claude Code 的 hook stdout 约定解析：

| 字段 | 行号 | 产出 |
|------|------|------|
| `continue: false` | :206 | `Deny`（reason 依次取 `stopReason`(:210) → `systemMessage`(:211) → 默认串） |
| `hookSpecificOutput` 入口 | :218 | — |
| `hookSpecificOutput.permissionDecision` | :219-222 | 仅 `== "deny"` 时 `Deny`；reason 取 `permissionDecisionReason`(:227) |
| `hookSpecificOutput.updatedInput` | :234 | `Modify { payload }` |
| `hookSpecificOutput.additionalContext` | :241 | `Inject { messages }` |

另有 Natives 自有的顶层 `decision` 字段（:249-252，默认 `"allow"`），支持 `deny|block`(:254)、`modify`(:263)、`inject`(:268)、`rewake`(:281)。空 stdout → Allow（:199-203）。`CommandHook`(:100) 与 `HttpHook`(:171) 共用此函数。

### 2.4 缺口：不认 `"allow"`，`"ask"` 被静默降级

hook_handlers.rs:219-222 只精确匹配 `Some("deny")`：

```rust
if output
    .get("permissionDecision")
    .and_then(Value::as_str)
    == Some("deny")
```

后果分三级：

- `permissionDecision: "allow"` → 落到顶层 `decision` 默认 `"allow"` → 最终 `Allow`。**行为看似正确，但纯属兜底巧合**，不是显式支持。协议层面 hook 无法「主动批准以跳过权限弹窗」。
- `permissionDecision: "ask"` → **被静默降级为 Allow**。这是真实安全缺口：hook 想要求人工确认，实际得到自动放行。
- `HookDecision` 无 `Ask` 变体（`agent-core/src/hooks.rs:20-25`），所以「ask」在类型层就无法表达。

### 2.5 缺口：`Rewake` 是被 match 到的空操作，不是死变体

> 修正一处：初稿称 `HookDecision::Rewake` 是「从不响应的死变体」。**这是错的。**

全仓库 `Rewake` 命中 4 处：
- `crates/agent-core/src/hooks.rs:25` —— 定义
- `crates/agent-core/src/hook_handlers.rs:282` —— **被构造**：hook 输出 `{"decision":"rewake"}` 即产生
- `crates/agent-core/src/engine.rs:1130` —— **被显式 match**：`HookDecision::Allow | HookDecision::Rewake => {}`，与 Allow 合并成空操作
- `docs/superpowers/specs/2026-07-26-native-harness-control-plane-design.md:552` —— 设计文档称「`Rewake` is aggregated with logical OR」

准确表述是：**可构造、被 match、语义为 no-op**。`aggregate_allow`（hooks.rs:177-185）只看 `Deny`，完全忽略 Rewake。这是文档-实现偏差（设计承诺了 OR 聚合与唤醒行为，实现没有），比「死变体」更值得关注 —— 因为一个死变体只是冗余，一个被文档承诺过的空转变体会让使用者写出静默失效的 hook。

### 2.6 缺口：Hook 对 GUI 完全不可见（证实）

`grep -ci "hook" src-agent-daemon/src/rpc.rs` = **0**。98KB 的 RPC 层没有任何 hook 查询/管理/检视方法。

讽刺之处：`HookRegistry::describe()`（hooks.rs:122）和 `describe_event()`（:131）的注释自称是「the inspection surface the control plane renders」（:119-121），但控制面拿不到它 —— 没有 RPC 把它送出去。

### 2.7 配置来源与策略（证实，且比初稿判断更宽）

配置发现在 `src-agent-daemon/src/production_hooks.rs`：

- **项目级 7 个候选**（`PROJECT_CANDIDATES`，:32-40）：`.claude/hooks.json`、`.claude/settings.json`、`.claude/settings.local.json`、`.agents/hooks.json`、`.agents/settings.json`、`.grok/hooks.json`、`.natives/hooks.json`
- **用户级 5 个候选**（`USER_CANDIDATES`，:44-50）：`~/.natives/hooks.json`、`~/.agents/hooks.json`、`~/.agents/settings.json`、`~/.claude/settings.json`、`~/.claude/settings.local.json`
- 解析 `collect_file_hooks`（:206）接受 `{"hooks": {...}}` 包裹或裸对象（:222-227）；事件名经 `HookEvent::parse`（:233）支持 snake_case 与 `CompactStart`/`SubagentEnd` 别名（definition.rs:91-110）。**读取失败/JSON 破损静默 skip**（:216-221），不阻断 Run。
- **command 型**（:312-321）默认 `/bin/sh -lc`（:307-310），Windows 走 `cmd.exe /C`；**http 型**（:322-325）；其余 type 丢弃（`_ => return`，:331）
- **matcher 支持**：解析 :240-243，写入 :335，dispatch 时两道门取严（`definition.applies_to()` hooks.rs:147-150 → definition.rs:337-339；`handler.matches_tool()` hooks.rs:153）。匹配语义 definition.rs:356-370 支持 `|` 多候选、`*` 全通配、`prefix*` 前缀。
- **timeout**：:305-309，默认 10s，clamp 1–600s

**fail-closed 分三层实现**：

1. 注册表级：`fail_closed_security` 字段（hooks.rs:66），默认关（:73-76，注释说明是为了裸 `AgentEngine::new()` 的测试），生产开启于 production_hooks.rs:197。敏感事件白名单仅 3 个（definition.rs:59-65：`PreToolUse | PermissionRequest | PermissionDenied`）。拒绝逻辑 hooks.rs:158-171。
2. 聚合级：`aggregate_allow`（hooks.rs:177-185）any-Deny-wins。
3. 单 handler 级：hook_handlers.rs 所有失败路径都产出 `Deny`，无一 fail-open —— 不可信插件(:34)、输入超限(:53)、spawn 失败(:80)、输出超限(:95)、非零退出(:103-116)、wait 失败(:118)、超时(:123)、HTTP URL 校验失败(:147)/非 2xx(:177)/请求失败(:182)。

### 2.8 两个初稿未发现的缺口

- **`HookFailurePolicy` 完全未接线。** 定义在 definition.rs:249-257（`Fail`/`Skip`/`Default`，后者注释为「Treat the failure as an Allow」），字段声明 :327。所有构造点硬编码 `Fail`（production_hooks.rs:84/122/146/338、hooks.rs:99），**无任何一处读取该字段来改变行为**，`hooks.json` 也没有解析入口。即 `Skip` 与 `Default` 两个 fail-open 策略是声明未接线的死枚举。
- **HTTP hook 的 `allow_hosts` 恒为空数组**（production_hooks.rs:324）。SSRF 主机白名单机制形同虚设，只剩 `validate_http_hook_url` 的默认规则（hook_handlers.rs:145）兜底。

---

## 3. Agent Loop（推断 ~70%）

`crates/agent-core/src/engine.rs` 共 2031 行。

### 3.1 已站住的地基

| 能力 | 证据 | 复核结论 |
|------|------|----------|
| Provider 重试上限 3 次 | engine.rs:416 `const MAX_PROVIDER_ATTEMPTS: u32 = 3;`（函数体内局部常量） | 证实 |
| 退避 | engine.rs:1136-1143 `sleep_provider_backoff`：`1 => 500ms, 2 => 1000ms, _ => 2000ms` | **部分正确**，见 3.2 |
| 空响应重试 | engine.rs:597-609（`attempt < 2` 时发 `EMPTY_RESPONSE` + `retrying: true`）；:611-627 第二次仍空则 `retryable: false` 直接失败 | **部分正确**，见 3.2 |
| 已产出内容的中途错误不重试 | 标志位 `saw_generation_delta`（:476，在 :485/:491/:501 三处置真）；判定 `:547` `if !saw_generation_delta && retryable && attempt < MAX_PROVIDER_ATTEMPTS`；已产出时发 `GenerationAttemptDiscarded`(:563-571) + `GenerationAttemptFailed { retrying: false }`(:572-581) 并 return Err(:582-588) | **证实**。这是一个正确且不常见的设计 —— 避免重放已经流给用户的内容 |
| max_steps | 字段 :202；判定 :411-414 `step += 1; if step > config.max_steps { return Err(EngineError::MaxSteps) }`；错误码映射 :179 | 证实 |
| 并行安全工具批处理 | 判定 `crates/harness-core/src/session_actor.rs:571-583`；并发上限 :585 `PARALLEL_SAFE_MAX_CONCURRENCY = 4`；标记 engine.rs:759；执行 engine.rs:826-950（批聚合 :910-921，`buffer_unordered` :938，按原序回填 :942-955） | **证实**，但见 3.3 |
| 安全点插话 | `SafePoint` 枚举 session_actor.rs:27-36（4 变体）；`CoordinatorAction` :117-126；`on_safe_point` :509-541；引擎侧注入 engine.rs:284-305（以 `role: "user"` + `[interjection]\n` 前缀 push） | **部分正确**，见 3.4 |

> 注：`crates/agent-core/src/session_coordinator.rs` 只有 8 行，是 re-export shim，真身在 `harness-core::session_actor`。

### 3.2 需修正的两处措辞

- **「指数退避」不准确。** 实际是硬编码三级查表 500/1000/2000ms，**封顶 2s、无抖动**。更严重的是：`EngineError::Provider.retry_after_ms` 字段存在（engine.rs:149、:163）但**在重试路径里完全未被消费** —— 服务端告知的等待时间被解析出来后丢弃。而且 **RateLimit 类错误反而跳过退避直接重试**（:454-456 `if !e.is_rate_limited()`；:556-558 `if category != "RateLimit"`）。注释暗示意图是「由上层按 retry_after 处理」，但上层没接 —— 这大概率是 bug 而非设计。
- **「空响应重试」上限是 2 次尝试，不是 3 次**（`attempt < 2`，:597），与 provider 错误的 3 次不一致。

### 3.3 并行判定是硬编码白名单，不是能力推导

`is_parallel_safe_tool`（session_actor.rs:571-583）是一个 `matches!` 白名单，共 7 个：`read_file`、`list_dir`、`grep`、`search_files`、`task_output`、`memory_search`、`memory_get`。**不看工具 schema、不看 read-only 元数据、MCP 工具永远不并行。** 批聚合只合并**连续（contiguous）**的 parallel_safe 调用，遇到一个写工具就断批（engine.rs:910-921）。

新增只读工具时必须手工改这个白名单，否则默默降级为串行 —— 一个典型的浅接口（新增能力要改两处，且漏改无提示）。

### 3.4 需修正：`SafePoint` 的类型被丢弃

`on_safe_point` 首行是 `let _ = point;`（session_actor.rs:510）—— **四种 SafePoint 行为完全一致**。且引擎只调用 3 处：engine.rs:676-680（BeforeTool）、:774-778（AfterTool）、:818-822（ProviderBatchBoundary）；`AfterPermissionResolved` **从未被引擎调用**。

准确表述：所谓「安全点」目前退化为「三个固定检查时机」，无法表达「只在 BeforeTool 插话」这类策略。

### 3.5 缺口：无 Plan Mode（证实）

全仓库 grep `plan_mode` / `PlanMode` / `planMode` / `AgentMode` / `agent_mode`（排除 node_modules、target、lock）：**0 命中**。权限模式全部命中在 `crates/agent-core/src/profile.rs:17,82` 与 `src-agent-daemon/src/cli_runtime_bridge.rs:82-90`，取值只有 `readonly|read_only` → `dontAsk`、`full_access|autonomous|full` → `acceptEdits`、其余 → `None`（:88-90）。**没有任何 `"plan"` 值，TS 前端也无痕迹。**

### 3.6 缺口：doom-loop 只检测尾部连续段（证实）

`crates/agent-core/src/doom_loop.rs` 共 102 行。参数 :21-24（`tool_window: 8`, `text_window: 6`, `tool_threshold: 3`, `text_threshold: 3`）。判定 `is_doom_loop()` :49-75，核心是 `take_while(|k| **k == last)`（:56 工具、:68 文本）—— **从最新一条往前数严格相等的连续 streak**，`streak >= 3` 才触发。窗口 8/6 只用于内存裁剪，判定实际只看尾部连续段。文本额外要求 `last.len() > 20`（:70）。

漏检的真实循环形态：
- `A B A B A B` 交替循环 → 检测不到
- `read_file(/a)` → `read_file(/b)` → `read_file(/a)` → 检测不到

另有假阳性风险：工具指纹是 `name:args_fingerprint`，args 只取前 80 字符（engine.rs:691），长参数的尾部差异被忽略 —— 两次不同的长路径读取可能被判为重复。

接入点：engine.rs:404（构造）、:638（observe_text）、:640-642（回合级检查）、:691-694（observe_tool + 逐工具检查）。

---

## 4. Provider 层（推断 ~50%）

`crates/provider-adapters/src/` 共 3778 行：7 个适配器（anthropic 391 / openai 299 / gemini 419 / deepseek 183 / ollama 159 / openai_compatible 154 / openai_codex 83）+ 4 个 SSE 解析器（anthropic_sse 235 / openai_sse 309 / gemini_sse 151 / openai_responses 141）+ `capabilities.rs` 482 + `http_stream.rs` 687。

### 4.1 Anthropic 请求体缺三项（证实，且范围比初稿更大）

`build_messages_body` 在 `crates/provider-adapters/src/providers/anthropic.rs:36-139`，请求体构造终点 :117-138，只有 6 个键：`model` / `messages` / `max_tokens` / `stream` / `system` / `tools`。

| 缺失项 | 复核结论 |
|--------|----------|
| `cache_control` | **全仓库零命中**（Rust / TS / 配置 / 文档）。不是「anthropic.rs 忘了加」，而是 **prompt caching 在整个产品中不存在** |
| `tool_choice` | **全仓库零命中**（含 `toolChoice`）。所有适配器都无法强制或禁用工具调用 |
| 请求侧 `thinking` | 引擎自建请求体从不发送。`thinking` 的全部命中是：响应侧解析（`stream/anthropic_sse.rs:24`、:114-115）、能力声明注释（`capabilities.rs:24`、`assistant-protocol/src/v1/content_block.rs:5,12,36`）、以及**旁路 JSON 代理**（`src-agent-daemon/src/request_rectifier.rs:53-63,71-72`，作用于原始 JSON 透传，且 `normalized_thinking()` 只在**已存在** `thinking` 键时调用，对引擎请求体无作用）。**extended thinking 无法从原生引擎路径开启** |

### 4.2 `max_tokens` 4096：需修正为「有一个 DB 开关」

anthropic.rs:120 是 `request.max_tokens.unwrap_or(4096)` —— **兜底默认值，不是硬编码常量**。真正的固定值在上游：`src-agent-daemon/src/production.rs:1166` 与 `src-agent-daemon/src/routing.rs:376` 都传 `max_tokens: Some(4096)`，所以 `unwrap_or` 分支在生产中永不触发。

> 修正一处：初稿称「硬编码 4096」。存在一个初稿漏掉的覆盖点 —— `src-agent-daemon/src/request_rectifier.rs:12-16`：
> ```rust
> pub fn rectify_provider_request(request: &mut ProviderRequest, enabled: bool) {
>     if enabled && request.max_tokens.unwrap_or(0) < 64_000 {
>         request.max_tokens = Some(64_000);
>     }
> }
> ```
> 在 production.rs:1171 与 routing.rs:380 被调用。开关来自 DB 配置，**默认关闭**（routing.rs:440-452，读表失败即 `false`）。
>
> 准确表述：**默认路径确实是 4096，但「不可变」不成立** —— 有一个默认关闭的 DB 开关能抬到 64000。

### 4.3 cache token 列：需修正为「写入点存在但恒写 0」

> 修正一处：初稿称「有列但无人写入」。

schema 定义：`src-agent-daemon/src/conversation_store.rs:985-986`、`src-agent-daemon/src/event_log.rs:153-154`（Host 侧另有 `src-tauri/src/db.rs:477-478`）。

两个 daemon 写入点**存在**，但值写死为字面量 `0, 0`：

- event_log.rs:161-168：`VALUES (?1, 'natives', 'daemon:run_event', ?2, ?3, ?4, 0, 0, 1, 0.0)`，且 `ON CONFLICT`(:165-168) 只累加 `input_tokens`/`output_tokens`/`request_count`，**cache 两列既不写入也不累加**
- conversation_store.rs:1026-1034：同构，`DO UPDATE SET` 同样排除 cache 列

准确表述：**有写入点，但语义为空**。因果上是自洽的 —— 既然请求侧从不发 `cache_control`（4.1），服务端也就永远不会返回非零 cache usage。修 cache token 统计的前置条件是先做 prompt caching。

（其余约 90 处命中在 `src-tauri/src/usage/*.rs`、`src/lib/usage-*.ts`，属于**外部 CLI 日志导入路径**（claude/codex/opencode/gemini/grok/ccusage 的 JSONL 解析），其中确有真实赋值如 `src-tauri/src/usage/claude.rs:315-316`，但数据来源是第三方 CLI 日志，与本引擎无关。）

### 4.4 多模态在类型层被堵死 —— 且 `image_input: true` 是谎报（比初稿更严重）

`EngineMessage` 定义在 `crates/agent-core/src/engine.rs:111-119`，`content: String`（:114）。全仓库仅此一处定义，47 处引用。

`image_input` 字段定义在 `crates/provider-adapters/src/capabilities.rs:21`。声明点 14 处：`anthropic.rs:159,322`、`gemini.rs:143,321,337`、`openai.rs:88,204,220` 声明 **true**；`deepseek.rs:53,149,165`、`ollama.rs:50,129`、`openai_compatible.rs:56,133`、`rpc.rs:2584` 声明 false。协议镜像 `assistant-protocol/src/v1/provider.rs:121`，前端类型 `src/components/assistant/ModelSelectorDropdown.tsx:14`。

**关键：无任何代码读取 `image_input` 做决策。** 它纯粹是上报给 UI 的展示位。

「装不下图片」的证据链三段全断：

1. **类型层**：`EngineMessage.content: String`（engine.rs:114），无 image variant。
2. **桥接层**：`capabilities.rs:89-133` 的 `history_message_to_provider()` 是 Engine→Provider 的唯一转换入口（由 `production.rs:1289` 的 `engine_message_to_history` 喂入）。三个返回分支（:94 ToolCall、:117 ToolResult、:131 纯文本）**只产出 `Text`/`ToolCall`/`ToolResult`，从不产出 `Image`**。
3. **适配层**：`ProviderContentBlock::Image { image_url: ImageSource }` 类型确实存在（capabilities.rs:46-48，`ImageSource` 在 :232-235），但**四个适配器全部静默丢弃**：`anthropic.rs:85-87`（匹配后只设 `is_tool_result_only = false`，不 push 任何 block）、`gemini.rs:85`（`Image { .. } => {}`）、`http_stream.rs:83` 与 `:429`（同为空块）。

**准确表述应加重：不是「装不下」，而是「声称装得下且悄悄丢弃」。** 即使有人手工构造带 `Image` 的 `ProviderRequest`（目前只有测试代码这么做），四个适配器也会把它吞掉 —— 不报错、不降级、不提示。

旁证：daemon 侧附件在写入会话时被降级为文本占位串 `[attachment: {name} at {path}]`（`src-agent-daemon/src/conversation_store.rs:506`），进一步确认全链无图片通道。

---

## 5. Context / 压缩（推断 ~30%）

### 5.1 两个同名函数，两条独立路径（初稿判断正确，此处补齐差异）

| | `context.rs:254-306` | `compaction.rs:70-121` |
|---|---|---|
| 签名 | `(&[(String, String)], token_budget: u64) -> (Vec<(String,String)>, Option<String>)` | `(&[Value], max_tool_chars: usize) -> CompactResult` |
| 策略 | **删整条消息**（固定保留末 4 条） | **截断单条 tool 输出**，不删消息 |
| 触发 | token 预算超限 | 单条 tool output > `max_tool_chars` |
| 时机 | 会话启动装配历史 | 每个工具批次边界 |
| 调用链 | `crates/agent-core/src/lib.rs:35` 导出 ← 唯一生产调用者 `src-agent-daemon/src/production.rs:455` | `engine.rs:7` 别名导入为 `compact_tool_history` ← `engine.rs:1067` ← `maybe_compact_history`(engine.rs:1033-1101) ← 唯一调用者 `engine.rs:817` |

`lib.rs:38-41` 显式重命名导出以避免冲突，注释直言「Prefer explicit imports for compaction to avoid clashing with context helpers」—— 代码作者自己也知道这俩同名函数容易混淆。

### 5.2 两条路径都是纯机械截断（证实）

**`context.rs:254`**：
- :259-263 用 `chars/4` 估算（`ContextBudget::estimate_tokens`，:244-246），未超预算原样返回
- :268 `let mut keep_from = messages.len().saturating_sub(4);` —— **固定保留最后 4 条**
- :271-288 循环只做一件事：切点会劈开 tool_call/tool_result 对时把切点左移
- :290-292 保底最后 2 条
- :299-301 所谓 "summary" 是一句模板串，**只报告丢了几条**，不含任何被丢弃内容的信息

**`compaction.rs:70`**：:79-105 tool 消息超长时保留前 `max_tool_chars/2` 字符，追加 `…[truncated N chars for compaction]`；`summary`(:115-119) 同样只是机械拼接 `"tool_call_id={id} truncated {}→{} chars"`。

**两个函数都是同步的** —— 类型系统层面就排除了发起模型调用的可能。

**结论：长会话触发压缩后，被丢弃的消息内容 100% 永久丢失、不可恢复。**

全仓库 grep `summarize|summari[sz]ation|summary` 逐条归类后**零个模型摘要实现**：模板字符串 2 处、纯数据字段/事件透传 5 处、DB 存取（读的正是那些模板串）2 处、同名无关 3 处、测试固件 1 处、e2e 测试的 system prompt 2 处、provider 响应解析 1 处（`stream/openai_responses.rs:40` 的 `reasoning_summary_text.delta`）。无 summarization prompt 常量、无摘要专用 model 调用、无 async 压缩函数。

### 5.3 附带发现：`context.rs` 压缩后的元组回填是脆弱点

`production.rs:456-470` 在拿到压缩结果后，把 `(role, content)` 元组**按 role + content 字符串相等**匹配回 `EngineMessage` 以恢复 tool 字段。内容重复的消息会匹配到错误的原始项；未匹配上的则 `tool_call_id` 被置 `None`。

### 5.4 缺口升级：接缝的形状本身堵死了实现

初稿说「PreCompact/PostCompact 钩子位置正确，接缝已就位，缺的是接缝背后的实现」。**复核后这个判断需要加重**：接缝形状不允许实现。

- PreCompact dispatch：engine.rs:1053-1061，payload 仅 `{ "before_chars": ... }`；:1062-1064 检查 `aggregate_allow`，被否决则**原样返回未压缩消息** —— 即 hook 只能「取消压缩」，不能「替换压缩结果」
- 实际压缩：:1067 调用 `compact_tool_history`（机械截断）
- PostCompact dispatch：:1088-1099，payload `{after_chars, dropped_tool_outputs, repaired_dangling}`，**返回值被 `let _ =` 丢弃** —— 无法影响结果
- 触发门槛：:1039-1051，`before_chars < HISTORY_COMPACT_CHARS`（engine.rs:20 = 48_000）时直接跳过，连 hook 都不发

**所以：即便用户注册了一个「用模型做摘要」的外部 hook，也没有任何通道把摘要结果写回 message 列表。** PreCompact 只有 allow/deny 二值语义，PostCompact 是纯通知。补实现前必须先改接口。

`hook_handlers.rs` 中 0 处 Compact 相关代码；`production_hooks.rs:365-366,529-532` 的两处引用全在 `#[cfg(test)]` 的 handler 计数探针里。

---

## 6. Skills（推断 ~30%）

`src-agent-daemon/src/skill_store.rs` 共 276 行。

### 6.1 不解析 YAML frontmatter（证实）

解析逻辑内联在 `scan_dir`（:68-136），**全文件无 `---` 分隔符处理**。

- `name` = `path.file_stem()`（:98-102），其中 `path` 是**目录项**（:73）—— 对 `skills/<dir>/SKILL.md` 布局就是目录名
- `description` = 第一条非空且非 `#` 开头的行，截断 160 字符（:111-118）：
  ```rust
  let description = body.lines()
      .find(|l| !l.trim().is_empty() && !l.starts_with('#'))
  ```
  注意 `starts_with('#')` 检查的是**未 trim 的行**，缩进的 `#` 标题会被当作 description。`body_preview` 是盲取前 400 字符（:119）。

**讽刺之处**：真正的 frontmatter 解析在本仓库**已经存在** —— `crates/agent-core/src/profile.rs:37-113` 为 agent profile 实现了它。能力在仓库里，只是从没用到 skill 上。

### 6.2 无渐进披露：全文拼进系统提示词（证实）

`inject_prompt()` 在 skill_store.rs:158。:178-184 **重新从磁盘读取每个文件并 push 完整正文**：

```rust
parts.push(format!("### Skill: {}\n{}", skill.name, body))
```

不存在「只注入 name + description」的层级 —— 这是渐进披露的反面。doc 注释（:157）自己承认：「Concatenate enabled trusted skill bodies for system prompt injection.」

调用点（都经 `prompt_for_project` 包装，:191-195），**都在 production.rs**：
- 父 Run：`src-agent-daemon/src/production.rs:442`，喂入 `assemble_context`（:443-447）
- 子 Agent：`src-agent-daemon/src/production.rs:788`，追加到子系统提示词（:790-791）

> 修正一处：初稿把 `run_manager.rs` 列为可能调用点。`run_manager.rs` **无**任何 skill 注入调用。

### 6.3 同时又有按需加载工具 —— 重复加载（证实）

- 工具注册：`crates/capability-gateway/src/tools/extra.rs:647-658`（name `"skill"`，schema `{"name": string}`，`PermissionClass::AlwaysAllowed`）
- handler：在 gateway **之前**被拦截，`src-agent-daemon/src/production_tools.rs:202-226`，委托 `load_skill_for_project`（:211），返回完整 `content`（skill_store.rs:216-222）

**后果**：一个启用的 skill 全文在系统提示词里，**同时**还能被工具调用再取一遍。token 成本随启用 skill 数量线性增长，且模型可能重复读到同一份内容。

### 6.4 `trusted: true` 硬编码 —— 提示词注入面（证实）

skill_store.rs:125-127 原文：

```rust
// Project skills under project tree are trusted for injection; still
// can be disabled. Untrusted external packs would set trusted=false later.
trusted: true,
```

「would set ... later」是对信任模型未完成的明确承认。`set_enabled`（:150-152）确实以 `trusted` 为门，但**由于没有任何代码把它设为 `false`，这个守卫不可达**。用户域 skill（来自 `~/.claude/skills` 等，:53）同样得到 `trusted: true`。

**攻击面**：任何能往 8 个扫描根之一投放 `.md` 的进程，都能让内容自动进入系统提示词。

### 6.5 无 per-skill `allowed-tools`、无随包脚本资源（证实）

- 全仓库 grep `allowed-tools|allowed_tools|allowedTools`（`*.rs`）只命中：`profile.rs:16,97-98`（`disallowed_tools`，仅 agent profile）、`production.rs:414`、`cli_runtime_bridge.rs:11,498`（外部 CLI 参数）。**skill 代码零命中**；`SkillRecord`(:21-32) 无 tools 字段。
- `scan_dir`(:75-94) 只解析目录内的 `SKILL.md`/`skill.md` 或裸 `*.md`，其余全部 `continue`。`SkillRecord.path` 是单个文件路径。**无目录拷贝、无脚本执行、无资源解析。**

### 6.6 扫描根：8 个（4 相对路径 × 2 域）

`.natives/skills`、`.grok/skills`、`.agents/skills`、`.claude/skills`；用户域 :47-54（`$HOME`/`$USERPROFILE`），项目域 :57-64。

---

## 7. Subagent（推断 ~60%）

### 7.1 超出 Claude Code 已知能力的三项（证实）

**（一）预算账本**（`crates/agent-core/src/subagents.rs`，871 行）。`SubAgentConfig` 在 :69-83：

| 项 | 字段:行 | 默认:行 | 是否真执行 |
|---|---|---|---|
| 全局并发 | `max_concurrent_global`:72 | 3 :89 | 是，`reserve_batch`:205-209 |
| 父级并发 | `max_concurrent_per_parent`:73 | 3 :90 | 是，:215-220 |
| 父级任务总数 | `max_tasks_per_parent_total`:74 | 32 :91 | 是，:226-231 |
| 深度 | `max_depth`:75 | 5 :92 | 是，`register`:444-449 |
| per-child token | `max_tokens_per_child`:77（+legacy `max_tokens_per_sub`:76） | 100_000 :93-94 | 是，`settle_tokens`:287-292 |
| per-tree token | `max_tokens_per_tree`:78 | 500_000 :95 | 是，:298-303 |
| per-child 工具调用 | `max_tool_calls_per_child`:79 | 200 :96 | 是，`consume_tool_call`:319-324 |
| per-tree 工具调用 | `max_tool_calls_per_tree`:80 | 1_000 :97 | 是，:330-335 |
| 超时 | `child_timeout_ms`:81 | 600_000 :98 | 是，`production_tools.rs:1249,1253` |
| 失败传播 | `failure_policy`:82 | `Isolate` :99 | **否 —— 死配置** |

「单 mutex 原子预留」证实：唯一 `ledger: Arc<Mutex<ReservationLedger>>`（:156，结构体 :105-117），所有 reserve/settle/consume 都取这把锁（:203、:250、:262、:276、:313）。预留入口 `reserve_batch`(:199) 由 `register` 以 `reserve_batch(parent, 1)`(:453) 调用，在深度检查之后。

**两个限定**：
- `failure_policy` 有 3 个变体（`Isolate`/`FailFast`/`RequireAll`，:47-55），**作为类型**证实；但 grep 显示 `subagents.rs` 外无引用（`FailurePolicy` 的其他命中是无关的 `HookFailurePolicy`），文件内的读取只有字段声明、默认值、和一个解析单测（:866-869）。**没有任何代码分支于它** —— 声明未执行。
- `reserve_batch` / `release_batch_reservation` **无外部调用者**，批 API 只以「批量为 1」的形式被内部使用，所以「多子 all-or-nothing」保证在生产中未被走过。`register_root_depth`(:193) 与 `assert_no_parent_cycle`(:341) 同样无外部调用者（后者内部在 :436 调用）。

**（二）`cap_child_permission` 保证子 ≤ 父**（证实）。subagents.rs:26-43，排序 `readonly`=0 < `ask`=1 < `full_access`=2（:27-34），取 `label(rank(parent).min(rank(requested)))`（:42）—— 严格 min，构造上保证子 ≤ 父。未知/空值落到 `ask`（:32），相对 `full_access` 是 fail-closed。调用点 `production.rs:638`、`production_tools.rs:1028`。

**（三）独立凭证路由**（证实）。`RouteBinding` 在 `src-agent-daemon/src/subagent_store.rs:53-57`，恰好三个 `String` 字段：`provider_id`、`key_id`、`model_id` —— **只存 ID，无密钥材料**。`SubAgentManager::register` 拒绝空三元组（subagents.rs:428-432）；`task` 工具描述显式拒绝模型自带凭据（`extra.rs:604`）。

### 7.2 硬伤 —— 全部证实，但有一个重大限定：那段代码是死代码

> **需修正**：初稿把硬编码系统提示词列为 subagent 的首要硬伤。它真实存在，但**影响为零**。

初稿列举的六项全部在 `SubAgentRuntime::spawn_child_task`（`src-agent-daemon/src/production.rs:627`）中证实：

- 硬编码系统提示词，:785-786，原文 `"You are a subagent with independent credentials. Complete the task."`（细微差别：:787-793 会追加项目 skills，所以最终提示词是该串 + skill 正文，但仍无任何 task/role/profile 相关内容）
- `agent_profile_id` 传 `None` —— :669（`subagents.spawn` 第 9 参），导致 `SubagentCreated` 事件也带 `None`（:681）
- `assemble_context(None, ...)` —— :794-795
- `messages: Vec::new()` —— :801（子 Agent 零对话历史）
- `max_steps: 20` 写死 —— :803
- 工具白名单写死 —— :639 调用 `default_subagent_tool_allowlist()`，定义在 `crates/agent-core/src/subagents.rs:11-17`，完整列表 **`read_file`、`list_dir`、`grep`**（恰是初稿所称三项）

**限定**：全仓库 grep `spawn_child_task` 只返回它自己的定义（production.rs:627）—— **零调用点、零测试**。这是不可达的 `pub` 死代码。

**真正的活路径**是 `PermissionGatedTools::execute_task`（`src-agent-daemon/src/production_tools.rs:229-230` → 约 `:990-1250`），走 `RunManager::create_run` + `start_detached_global`，**没有**硬编码系统提示词。但初稿的核心论点在活路径上依然成立：

- `agent_profile_id: None` —— production_tools.rs:1120（`SubagentCreated` 在 :1194）
- `max_steps: Some(15)` 写死 —— :1124
- 工具白名单继承父级、否则回落同一套只读三件套 —— :1030-1043

**分级建议**：「硬编码提示词」应降级为「死代码待清理」；「profile 从未接到子 Agent」应保持为**首要硬伤**（它在活路径上成立）。

### 7.3 `task` 工具 schema 只收 4 个字段（证实）

`crates/capability-gateway/src/tools/extra.rs:602-622`，schema 在 :605-614。完整属性列表恰好 4 个：

- `prompt`（string，**唯一 required**，:613）
- `task`（string，"Alias for prompt"）
- `name`（string，可选标签）
- `permission_profile`（string，"ask | full_access (capped by parent)"）

**确认缺失**：`subagent_type`、`system_prompt`、`tools`、`model`。凭据字段被刻意丢弃（production_tools.rs:1012-1015）。

**附带发现**：一个未声明的字段 `tool_allowlist` **确实被 handler 读取**（production_tools.rs:1030），但不在 schema 里 —— 模型永远无法发现它。schema/handler 不一致。

### 7.4 `AgentProfile` 字段齐全，从未接到 subagent（证实）—— 而且对父 Run 也大半是空头

`crates/agent-core/src/profile.rs:9-34` 共 21 个字段：`id`:10、`name`:11、`description`:12、`prompt_mode`:13、`system_prompt`:14、`tools`:15、`disallowed_tools`:16、`permission_mode`:17、`skills`:18、`provider_id`:19、`key_id`:20、`model_id`:21、`base_url_override`:22、`context_mode`:23、`isolation_mode`:24、`max_steps`:25、`max_duration`:26、`token_budget`:27、`completion_requirement`:28、`body`:31、`source_path`:33。初稿列举的 9 个字段全部存在。

消费点（排除 `profile.rs` 自身与测试噪音）：

| 位置 | 用途 | 到得了子 Agent？ |
|---|---|---|
| `production.rs:355-357` | `load_agent_profile` —— **全仓库唯一 load 调用** | 仅父 Run |
| `production.rs:360` | `token_budget` → `ContextBudget::resolve` | 仅父 Run |
| `production.rs:409` | `tools` → 白名单回落 | 仅父 Run |
| `production.rs:414-416` | `disallowed_tools` → 从白名单相减 | 仅父 Run |
| `production.rs:444` | `profile.as_ref()` → `assemble_context` | 仅父 Run |
| `crates/agent-core/src/context.rs:16,30-37` | 只消费 `system_prompt` | — |
| `run_manager.rs:1679` | 把 `run.agent_profile_id` 传入 `start_run` | 管道通，但子 Run 行里存的是 `None` |

**决定性证据**：`load_agent_profile` 全仓库只被调用一次（production.rs:357），且以 `agent_profile_id` 为门；而**两个子 Run 创建点都把它硬编码为 `None`**（production.rs:669 死路径、production_tools.rs:1120 活路径）。DB 列存在（`storage/migrations.rs:330`、`storage/mod.rs:305`），`run_manager.rs:1679` 也会转发 —— 值只是从来没被填过。**「接线盒装好了、线没接」的判断证实。**

**初稿未发现的扩展结论**：`profile` 的 `max_steps`、`prompt_mode`、`permission_mode`、`skills`、`provider_id`/`key_id`/`model_id`、`base_url_override`、`context_mode`、`isolation_mode`、`max_duration`、`completion_requirement` —— **11 个字段被解析但在任何地方都不被读取**。只有 `token_budget`、`tools`、`disallowed_tools`、`system_prompt` 4 个真被消费。所以 `AgentProfile` **即使对父 Run 也大半是空头承诺**，不只是「没接到子 Agent」。

---

## 8. 执行图（推断 ~15%）

### 8.1 没有 DAG / 依赖图调度（证实）

在 `crates/`、`src-tauri/src/`、`src-agent-daemon/src/` 全量 grep `\bdag\b`、`depends_on`、`dependency_graph`、`dependencies`：**零个执行调度相关命中**。全部命中是 `Cargo.toml` 的 `[dependencies]` 段，以及 `src-tauri/src/creative_app/local/deps.rs`（npm 依赖安装）、`scan.rs:531`（读 package.json）—— 都是包管理。

唯一的「批」概念是 `EngineToolSeam::execute_task_batch`（engine.rs:60-75）：把同一轮内的多个 `task` 调用打成一个 subagent_assignment，默认实现是**纯串行 for 循环**（:70-76）。**无依赖边、无拓扑排序、无就绪队列。**

### 8.2 `ExecutionRegistry` 是取消令牌树，不是调度器（证实）

`src-agent-daemon/src/runtime/execution_registry.rs` 共 632 行。结构体 :88-97（字段 `runs: Mutex<HashMap<String, RunExecution>>`、`grace: Duration`、`process_cancel` hook、`external_cancel` hook）。节点 `RunExecution` :56-64（含 `parent_run_id`、`token: CancellationToken`、`join: Option<JoinHandle<()>>`、`resources: HashSet<ManagedResource>`、`cancel_phase`）。

文件头文档 :1-11 自我定位为「Per-run CancellationToken tree + join handles + managed resource ids」，不变量是「Each Run has exactly one root CancellationToken / Child runs use `parent.child_token()`」。

主要方法全部围绕取消传播与资源清理，**无任何调度/排序/就绪判定 API**：`register_root`:143、`register_child`:149、`register_with_token`:167、`token`:214、`is_registered`:218、`attach_join`:222、`track_resource`:234、`untrack_resource`:247、`list_tree`:254（仅按 `parent_run_id` 树遍历收集，供取消广播）、`signal_tree`:277、`cancel_tree`:298、`tree_quiet`:434、`mark_finished`:441、`cancel_all_execution_roots`:453、`active_count`:476、`cancel_phase`:480。`CancelPhase` 状态机 :36-43（`Idle → Signalled → Force → Clean/CleanupFailed`）是纯取消语义。装配点 `production.rs:74`（字段）、`:117`（构造）。

### 8.3 血缘事件是扁平的（证实）

`crates/assistant-protocol/src/v2/run_event.rs:176-188`：
- `SubagentCreated { sub_run_id, agent_profile_id, task }` :176-180
- `SubagentCompleted { sub_run_id, result }` :181-184
- `SubagentFailed { sub_run_id, error }` :185-188

三个 payload **没有任何依赖字段**（无 `depends_on`、无 `blocked_by`、无 `after`、无 sibling 引用）。父子关系是**隐式的** —— 靠事件被 append 到哪个 run 的流上（`production.rs:677-684` append 到 `parent_run_id`；`:839-846` append 到 `parent_owned`；同模式在 `production_tools.rs:1192`、`:1312`）。

**因此只能重建一棵 parent→children 的血缘树，无法表达「child A 必须在 child B 之后」。**

### 8.4 澄清：`codegraph.rs` 与执行图无关（证实）

`src-tauri/src/commands/codegraph.rs` 共 390 行。主要函数：`rtk_gain`:29（tauri command）、`parse_rtk_gain_output`:52、`parse_token_amount`:139、`read_codegraph`:178（tauri command）、`find_project_root`:210、`generate_codegraph_via_cli`:228、`read_codegraph_dir_tree`:249、`read_simple_tree`:295、`try_parse_json_index`:340。

一句话：给 UI 用的**只读代码符号索引读取器** —— 优先读磁盘上的 `.codegraph/`（:181-185），否则 shell out 调 `codegraph explore --format json`（:229-232），再否则退化为扫 `src/` 目录树（:194-204）；外加解析 `rtk gain` 文本统计 token 节省。**名字里的 "graph" 指代码符号图，不是执行图。**

---

## 9. GUI 接口契约（推断 ~75%）

> **本节描述审计时点（`682453e3`）状态。此处的广告面缺口正在被并行修复 —— `methods.rs`、`rpc.rs`、`capability-gate.ts` 三个文件在写稿时均已被改动，且新增了 `src-agent-daemon/tests/rpc_dispatch_contract.rs`（看名字是给第 9.3 节的违规加 CI 门禁）。本节内容应按「缺口曾经存在过、根因是什么」阅读，当前是否已闭环须重新读码确认。**

### 9.1 清单规模：需修正初稿的「各 84 条且一致」

> **初稿判断错误。** 实测（`crates/assistant-protocol/src/v2/methods.rs`）：

| 常量 | 条数 | 位置 |
|------|------|------|
| `ALL_METHODS` | **84** | :5-84 |
| `IMPLEMENTED_METHODS` | **74** | :94-176 |
| `HOST_IMPLEMENTED_METHODS` | **10**（其中 4 条不在 `IMPLEMENTED_METHODS`） | :179-198 |
| 实际广告面 = 并集 | **78** | `capabilities.rs:72-76`（`host_mediated`） |
| `rpc.rs` 真实 match 覆盖 | **73** | `src-agent-daemon/src/rpc.rs:396-2263` |

`ALL_METHODS - IMPLEMENTED_METHODS` 的 10 条差集是：`agent.list`、`artifact.reveal`、`conversation.update`、`mcp.auth.oauthCallback`、`mcp.auth.oauthStart`、`mcp.call`、`permission.listPending`、`run.finish`、`run.getActivity`、`run.listChildren`。

### 9.2 兜底机制的设计是正确的

`rpc.rs:2264-2280` 的 `_ =>` 分支调用 `method_status()`（methods.rs:295-305），三态映射：`Unsupported` → 码 `"unsupported"`，`InvalidRequest` → `"invalid_request"`，`Implemented` → `"internal_error"`。

**这个设计是对的** —— 如果一个方法被广告为 implemented 却落到兜底，那确实是内部错误。问题不在兜底，在于**有 6 个方法真的落进来了**。

### 9.3 广告 ⊆ 可调 的 6 条违规（初稿说 9 条，需修正）

以下方法进入 `daemon.getCapabilities().methods` 但 daemon `rpc.rs` 无 match 分支：

| 方法 | 广告来源 | 实际结果 | 根因 |
|------|----------|----------|------|
| `conversation.listPage` | `IMPLEMENTED_METHODS` | `internal_error` | **初稿未发现。** `conversation_store.rs:13` 已实现 handler，但 `rpc.rs:615-625` 的分发名单漏列（`names` 模块也没有对应常量）。前端在用：`src/lib/assistant-workspace/controller.ts:42`（有 catch 降级到 `conversation.list`） |
| `conversation.getMessagesPage` | `IMPLEMENTED_METHODS` | `internal_error` | **初稿未发现。** 同上，handler 在 `conversation_store.rs:17`。前端在用：`src/lib/assistant-gateway/daemon-adapter.ts:307`（有 catch 降级到 `conversation.getMessages`） |
| `permission.listPending` | `HOST_IMPLEMENTED_METHODS` | `internal_error` | Host 的 `is_host_owned_method`（`src-tauri/src/assistant_service.rs:135-145`）**不含**它，`permission.` 前缀被转发给 daemon（`:104`） |
| `run.finish` | `HOST_IMPLEMENTED_METHODS` | `internal_error` | 同上，`run.` 前缀转发给 daemon |
| `run.listChildren` | `HOST_IMPLEMENTED_METHODS` | `internal_error` | 同上 |
| `artifact.reveal` | `HOST_IMPLEMENTED_METHODS` | Tauri 路径 OK；直连 daemon 路径 `internal_error` | Host **有**实现（`assistant_service.rs:99`）且被 `is_host_owned_method` 拦在本地，故 GUI 实际可用。但对任何直连 daemon 的客户端仍是空头广告 |

**这两条分页方法是本次审计的新发现**，且它们是最典型的失效模式：handler 写好了、名单加好了，**只差 `rpc.rs` 的 match 里一行**。前端两处调用都套了 `.catch()` 降级，所以线上表现是「分页静默失效、退回全量拉取」——不报错，但性能预算被悄悄破坏。

### 9.4 初稿列入 9 条中的 5 条实际上是合规的（需修正）

`agent.list`、`conversation.update`、`run.getActivity`、`mcp.auth.oauthStart`、`mcp.auth.oauthCallback` —— 这 5 条**只在 `ALL_METHODS`，不在任何广告面**。落兜底后 `method_status()` 判定 `Unsupported`，返回码 `unsupported`。

**这符合契约**（目录可列目标、广告只列已实现、未实现返回明确 unsupported），不属于违规。methods.rs:334-345 甚至有专门的单测 `oauth_browser_redirect_is_explicitly_unsupported_until_handler_exists` 和 `known_but_unimplemented_methods_are_not_advertised` 守着这个不变量。

**准确表述**：违规是 6 条（其中 1 条仅影响非 Tauri 客户端），不是 9 条。把这 5 条合规项混进违规清单，会让真正的违规被稀释。

另有 `mcp.call`：有 match 分支（`rpc.rs:1430`）但故意返回 `invalid_input: direct_mcp_call_disabled`，且**未**进广告面（methods.rs:126 有显式注释「intentionally NOT implemented (task-06): direct RPC transport bypass disabled」）—— 诚实关闭，合规。

### 9.5 前端能力门被广告骗开（证实）

`src/lib/assistant-workspace/capability-gate.ts` 以 `caps.methods` 为唯一渲染门禁（`hasMethod`，:33-39）。

- `:51` `canListTasks` 接受 `run.listChildren` 作为 `task.list` 的降级路径 —— **门开在一个返回 `internal_error` 的方法上**
- `HOST_METHODS_UI`（:11-29）同时列了 `permission.listPending` 与 `run.listChildren` 两个破损方法

### 9.6 文档注释与现实相反（证实）

capability-gate.ts:2-7 的文件头注释：

```
 * Rule: never render / call an RPC that is not advertised. Known-but-unimplemented
 * methods (run.rewind, conversation.getContextUsage, task.list, …) must stay hidden
 * until IMPLEMENTED ∪ HOST advertises them.
```

它举的三个「known-but-unimplemented」例子 —— `run.rewind`、`conversation.getContextUsage`、`task.list` —— **全部已在 `IMPLEMENTED_METHODS` 中且有真实 handler**（`rpc.rs:2214`、`:2240`、`:1642`）。methods.rs:355-365 的单测正是断言它们已实现。

初稿提到了前两个；`task.list` 是第三个，同样过期。注释描述的是一个至少两个 phase 之前的世界。

---

## 10. 其它缺口

### 10.1 无 `web_search`（证实）

`web_search` / `websearch` **没有任何工具实现**。全部命中是噪音：
- `src-tauri/src/usage/claude.rs:929,1013,1014` —— 测试 fixture 里 Anthropic usage JSON 的 `server_tool_use.web_search_requests`（账单解析）
- `src/lib/agent-narration.ts:21` `WebSearch: 'Searching'` 与 `:40` 一个正则 —— 前端把**外部 Claude Code CLI** 输出的 `WebSearch` 字样映射成中文叙述，本仓库自己不提供该工具

`web_fetch` 存在：`crates/capability-gateway/src/tools/mod.rs:639` `pub struct WebFetchTool;`，`impl ToolHandler` 在 `:641`，注册项 `:841`（name）+ `:850`（handler）。配套 SSRF 防护 `crates/capability-gateway/src/tools/ssrf.rs`（scheme / DNS / private / link-local / metadata 拦截），安全测试 `crates/capability-gateway/tests/security.rs:122`。策略分类 `src-agent-daemon/src/runtime/tool_policy.rs:258,629`（`external_write`），副作用账本 `src-agent-daemon/src/side_effect_ledger.rs:84`（`network`）。

### 10.2 MCP 只铺了 tools 面（证实）

`src-agent-daemon/src/mcp_runtime.rs` 共 1014 行：

| MCP 方法 | 结论 | 行号 |
|---|---|---|
| `initialize` | 已实现 | :213（stdio 握手），测试 stub :963 |
| `notifications/initialized` | 已实现（best-effort，:224 注释说明服务端可忽略） | :228，stub :966 |
| `tools/list` | 已实现 | :235，stub :968 |
| `tools/call` | 已实现（stdio + HTTP/SSE 两路） | stdio :533（错误处理 :543）；HTTP :571（:599/:607/:619 错误分支，:625 兜底）；stub :971 |
| `resources/list`、`resources/read` | **未实现** | 全仓库 0 命中 |
| `prompts/list`、`prompts/get` | **未实现** | 全仓库 0 命中 |
| `roots/list` | **未实现** | 全仓库 0 命中 |
| `sampling/createMessage` | **未实现** | 全仓库 0 命中 |
| `elicitation/*` | **未实现** | 全仓库 0 命中 |

grep 范围覆盖 `crates/` + `src-agent-daemon/src/` + `src-tauri/src/` + `src/` 全量 —— 说明不是「实现在别处」，而是**协议面根本没铺**。文件头 :1 自述范围就是「registry + stdio session + HTTP/SSE discovery + tools/call」。

### 10.3 Extension 无沙箱模型（证实），且 `extension-host/` 是悬空原型

`src-agent-daemon/src/extension_store.rs` 共 233 行。文件头 :1-6 自我承认：「Phase 6 minimum real surface … **Full host isolation and install/update land later**; this module provides real state + RPC so capabilities can honestly say extensions=true for list/enable/disable.」

内容只有：`ExtensionScope`:16、`ExtensionManifest`:23-33（含 `trusted: bool`、`enabled: bool`、`permissions: Vec<String>`）、`ExtensionStore`:36、`new`:41、`discover_defaults`:45、`scan_dir`:62、`list`:147、`register`:154、`set_enabled`:167、`global_extensions`:182。

唯一的「安全」逻辑是**一个布尔开关**，不是沙箱：:137（`enabled: enabled && trusted`）、:158-160（`register` 拒绝 untrusted+enabled）、:172-174（`set_enabled` 拒绝启用 untrusted）。

**`permissions: Vec<String>` 只从 manifest JSON 读入存着（:119-127），全文件无任何一处消费它做准入判断。** 无进程隔离、无 seccomp/沙箱、无资源上限、无文件系统限制、无 capability 校验。信任来源是 manifest **自己声明**的 `trusted` 字段（:111-113）—— 即扩展自称可信即可信。

`extension-host/`（TypeScript，共 503 行含测试）：`src/index.ts`(94)、`rpc.ts`(55)、`limits.ts`(51)、`types.d.ts`(9)、`host.test.ts`(146)、`extensions/index.ts`(148)、`extensions/__tests__/integration.test.ts`。`package.json` 自述「Isolated TypeScript extension host for MCP, Skills, Hooks, and Plugins」，`index.ts:1-4` 注释称在 "sandboxed environment" 中运行扩展。

**实际没有沙箱**：
- `limits.ts` 只有三个数字 + 一个 `Promise.race` 超时（:44-51）和一个 `checkOutputSize` 纯比较（:37-39）。`maxMemoryMb: 128`（:19）只是个存着的 getter，**从未被强制执行**（JS 里也无从强制）。
- `rpc.ts:19-40` 的 `ExtensionRPC.call` 是**假的**：注释写 "In production, this would route through the Rust capability gateway"、"Simulate RPC call"，实现是 `setTimeout(..., 10)` 后 `resolve({ status: 'ok', method, params })`。无任何真实 IPC，无 gateway 策略校验。
- **完全未接线**：全仓库 grep `extension-host` / `extension_host` / `ExtensionHost`（排除 node_modules/target/.git），唯一命中是 `extension-host/package.json:2` 它自己的 name。根 `package.json` 无 `workspaces` 字段，Rust 侧零引用。**这是一个不在任何构建或运行路径上的悬空原型目录。**

---

## 11. 缺口排序

排序依据：**（安全或诚实性影响）> （用户可感知的功能损失）> （成本）> （能力上限）**。「代码位置」列给出下手点。

### P0 —— 诚实性与安全（违反 `docs/standards/` 的 MUST 级约束）

| # | 缺口 | 为什么是 P0 | 代码位置 |
|---|------|------------|----------|
| 1 | **6 条广告方法无 handler**，返回 `internal_error` | 直接违反「广告 ⊆ 可调」（`standards` 自检清单最后一条）。前端能力门以广告为准，门被骗开 | `methods.rs:94-198` ↔ `rpc.rs:396-2280`（**已有 agent 并行修复中**） |
| 2 | **`image_input: true` 是谎报，且图片被静默丢弃** | 违反「无假数据」：UI 会据此显示模型支持图片。且丢弃是静默的，不报错不降级 | `capabilities.rs:21`（声明）、`anthropic.rs:85-87` / `gemini.rs:85` / `http_stream.rs:83,429`（丢弃） |
| 3 | **Skill `trusted: true` 硬编码 → 提示词注入面** | 任何能往 8 个扫描根之一写 `.md` 的进程都能注入系统提示词。注释自己承认信任模型未完成 | `skill_store.rs:125-127` |
| 4 | **hook `permissionDecision: "ask"` 被静默降级为 Allow** | hook 想要求人工确认，实际得到自动放行。安全方向的静默降级 | `hook_handlers.rs:219-222`；需先给 `HookDecision` 加 `Ask` 变体（`hooks.rs:20-25`） |
| 5 | **HTTP hook `allow_hosts` 恒为空** | SSRF 主机白名单机制形同虚设 | `production_hooks.rs:324` |
| 6 | **Extension `permissions` 只读入不判断 + 自称可信即可信** | 无隔离模型下的「已启用扩展」是无边界代码执行 | `extension_store.rs:111-127` |

### P1 —— 用户可感知的功能损失

| # | 缺口 | 影响 | 代码位置 |
|---|------|------|----------|
| 7 | **压缩是纯机械截断，长会话信息不可恢复丢失** | 最严重的用户可感知质量损失。且**接缝形状堵死实现**（PostCompact 无回写通道），必须先改接口 | `context.rs:254-306`、`compaction.rs:70-121`、回写通道需改 `engine.rs:1088` |
| 8 | **`AgentProfile` 从未接到子 Agent** | 子 Agent 无法定制角色、提示词、工具、模型 —— 「接线盒装好了、线没接」。修它只需把 `agent_profile_id` 从 `None` 换成真值 | `production_tools.rs:1120`（活路径）；`production.rs:669`（死路径） |
| 9 | **`task` schema 无 `subagent_type`/`system_prompt`/`tools`/`model`** | 与 8 互为因果：即使 profile 接上了，模型也没有字段来指定用哪个 | `extra.rs:602-622` |
| 10 | **分页方法静默失效** | 前端 catch 降级为全量拉取，破坏性能预算而不报错 | 属于 P0-1 的子集，但值得独立验收 |
| 11 | **无 Plan Mode** | 缺少「先出计划再执行」的交互档位 | 全仓库无痕迹，需新建 |
| 12 | **无 `web_search`** | 只能 `web_fetch` 已知 URL，无法发现 URL | `tools/mod.rs` 需新增 |

### P2 —— 成本与效率

| # | 缺口 | 影响 | 代码位置 |
|---|------|------|----------|
| 13 | **Skill 无渐进披露 + 与 `skill` 工具重复加载** | token 成本随启用 skill 数线性增长；模型可能读到两遍同一内容 | `skill_store.rs:178-184` + `production_tools.rs:202-226` |
| 14 | **无 prompt caching（`cache_control`）** | Anthropic 路径长系统提示词每轮重复计费。修它同时解锁 cache token 统计（4.3） | 全仓库零命中，需从 `anthropic.rs:117-138` 起 |
| 15 | **`max_tokens` 默认 4096** | 长输出被截断；抬高的开关默认关闭 | `production.rs:1166`、`routing.rs:376`、开关 `routing.rs:440-452` |
| 16 | **`retry_after_ms` 解析后被忽略 + RateLimit 跳过退避** | 限流时立刻重试，可能加剧限流。疑似 bug 而非设计 | `engine.rs:149,163`（定义）、`:454-456`、`:556-558` |

### P3 —— 能力上限与清理

| # | 缺口 | 代码位置 |
|---|------|----------|
| 17 | 无请求侧 `thinking` / 无 `tool_choice` | 全仓库零命中 |
| 18 | doom-loop 漏检交替循环（`A B A B`） | `doom_loop.rs:49-75` |
| 19 | `SafePoint` 类型被丢弃，四种点位行为一致 | `session_actor.rs:510` |
| 20 | 并行判定是 7 项硬编码白名单，MCP 工具永不并行 | `session_actor.rs:571-583` |
| 21 | MCP 缺 resources / prompts / roots / sampling / elicitation | `mcp_runtime.rs` 需扩协议面 |
| 22 | 无 DAG 调度 | 需新建；现有 `ExecutionRegistry` 不可复用（语义是取消） |
| 23 | Hook 对 GUI 不可见（`describe()` 无 RPC 出口） | `rpc.rs` 需新增方法；`hooks.rs:122,131` 已有数据源 |
| 24 | **死代码/死配置清理**：`spawn_child_task`（零调用）、`HookFailurePolicy::{Skip,Default}`、`SubAgentConfig.failure_policy`、`AgentProfile` 的 11 个未消费字段、`reserve_batch` 的多子路径无调用者、`extension-host/`（悬空原型） | 见各节 |

---

## 12. 与 Claude Code / Codex 的对照及推断依据

### 12.1 推断依据（这是本节的可信度基础）

我们**没有**这两个产品的源码。对照结论来自四类可观测材料：

| 材料 | 例子 | 能支持什么推断 |
|------|------|----------------|
| **本仓库既有的兼容层实现** | `hook_handlers.rs:196-247` 解析 Claude Code 的 hook stdout（`continue` / `hookSpecificOutput.permissionDecision` / `updatedInput` / `additionalContext`）；`cli_runtime_bridge.rs` 解析 Claude CLI 的 stream-json | **最强证据。** 这些字段名不可能凭空写出，说明写它们的人见过对方的真实契约 |
| **对方落在磁盘上的产物** | `~/.claude/settings.json` / `hooks.json` 的目录结构与 schema（被 `production_hooks.rs:32-50` 直接支持）；`~/.claude/skills/` 的 `SKILL.md` 布局；usage JSONL 的 `server_tool_use.web_search_requests`（被 `src-tauri/src/usage/claude.rs:929` 解析） | 强证据，但只覆盖「有磁盘痕迹」的能力 |
| **前端对外部 CLI 输出的映射表** | `src/lib/agent-narration.ts:21` 把 `WebSearch` 映射为中文叙述 | 证明对方**有** `WebSearch` 工具（否则不会写这个映射） |
| **公开文档与 changelog** | Plan Mode、progressive disclosure of skills、prompt caching | 最弱证据，且可能滞后于实际版本 |

**因此以下三类结论的可信度依次递减**：
1. 「对方有 X，我们没有」——`X` 有磁盘痕迹或兼容层痕迹时，可信度高（如 skill frontmatter、hook allow 决策、WebSearch）
2. 「对方有 X，我们没有」——`X` 只有文档来源时，可信度中（如 Plan Mode 的具体交互形态）
3. 「我们有 Y，对方没有」——**可信度最低**。对方可能有而未公开。第 12.3 节的三项要按这个折扣阅读

### 12.2 逐项对照

| 能力 | Claude Code（推断） | Natives 现状 | 推断依据强度 |
|------|--------------------|-------------|-------------|
| Hook 事件覆盖 | 一组固定生命周期事件 + 工具 matcher | **16 个事件全部有触发点**；matcher 支持 `\|` / `*` / `prefix*` | 高（`production_hooks.rs` 直接支持对方的目录与 schema） |
| Hook 决策语义 | 至少 `deny` / `allow`；`ask` 大概率存在 | 只认 `deny`；`allow` 靠兜底巧合；**无 `Ask` 变体** | 高（`hookSpecificOutput.permissionDecision` 字段名来自对方契约） |
| Hook 控制面可见性 | 有（`/hooks` 之类的检视入口） | **零 RPC 出口** | 中 |
| Plan Mode | 有 | **无痕迹** | 中（仅文档来源，具体交互形态不确定） |
| Skill frontmatter | YAML frontmatter（`name` / `description` / `allowed-tools`） | 目录名 + 第一行文本；**无 `allowed-tools`** | 高（`SKILL.md` 布局有磁盘痕迹；本仓库 `profile.rs:37-113` 已实现 frontmatter 解析，说明作者知道这个格式） |
| Skill 渐进披露 | 先给 name+description，按需展开正文 | **拼全文** + 另有重复的 `skill` 工具 | 高 |
| Skill 随包脚本/资源 | 有 | **无**（只读 `.md`） | 中 |
| 上下文压缩 | 模型摘要 | **纯机械截断** | 高（对方长会话后仍能引用早期内容，这是可观测行为差） |
| Prompt caching | 有 | **全仓库零命中** | 高（Anthropic API 公开能力，且 usage 字段有 cache 列） |
| `tool_choice` | 有 | **全仓库零命中** | 高（Anthropic/OpenAI API 公开能力） |
| Extended thinking（请求侧） | 有 | 只解析响应侧 delta，**请求侧不发** | 高 |
| 多模态图片输入 | 有 | **类型层堵死 + 谎报 + 静默丢弃** | 高 |
| `web_search` | 有 | **无** | 高（`agent-narration.ts:21` 的映射表证明对方有） |
| MCP 协议面 | tools + resources + prompts + roots + sampling + elicitation | **只有 tools** | 高（MCP 是公开规范） |
| Subagent 定制（type / prompt / tools / model） | 有 | **schema 只收 4 字段；profile 从不接** | 高 |
| Subagent 资源预算 | 未见公开的 token/工具调用配额账本 | **10 项预算，单 mutex 原子预留** | **低**（见 12.3） |
| 子权限不可提升 | 有（子不继承更高权限） | `cap_child_permission` 严格 min | 中 |
| 子 Agent 独立凭证 | 未见公开支持 | 独立 provider/key/model 路由 | **低**（见 12.3） |
| 执行图 DAG | 未见公开的 DAG 调度 | **无** | 低（双方都可能没有，这一格不构成差距） |

### 12.3 我们**可能**领先的三项（按最低可信度阅读）

以下三项在公开材料中未见 Claude Code 支持，但**「未公开」不等于「没有」**：

1. **Subagent 预算账本**（`subagents.rs:69-83`）—— 10 项配额，单 mutex 原子预留。**但**：`failure_policy` 是死配置，`reserve_batch` 的多子 all-or-nothing 路径无外部调用者（只以「批量为 1」被使用），所以「原子预留多个子 Agent」这个卖点在生产中未被走过。**领先幅度应打折。**
2. **`cap_child_permission` 的构造性保证**（`subagents.rs:26-43`）—— 用 `min(rank)` 而非运行时检查，子 ≤ 父在类型/构造层成立。这是干净的设计，可信度相对高。
3. **子 Agent 独立凭证路由**（`subagent_store.rs:53-57`）—— 只存三个 ID、不存密钥材料。设计正确。

**一个必须一起说的反例**：这三项「领先」都建立在一个基础缺陷之上 —— 子 Agent **既没有可定制的系统提示词、也没有对话历史、也没有 profile**（第 7.2、7.4 节）。即：我们把子 Agent 的**资源治理**做得比对方细，却把子 Agent 的**能力表达**做得比对方浅。治理一个只能 read_file/list_dir/grep、提示词写死、`max_steps` 写死的子 Agent，治理得再精细也价值有限。

**优先级含义**：修 P1-8（把 `agent_profile_id` 接上）与 P1-9（扩 `task` schema）的收益远高于继续深化预算账本。

### 12.4 关于 Codex 的对照

本仓库对 Codex 的态度是**明确 fail-closed**：`src-agent-daemon/src/codex_runtime_bridge.rs` 仅 2.4KB，`NATIVE-DAEMON-CAPABILITY-MAP.md` 第 3.4 节将其标为「**红线**：未就绪则 fail-closed，不得因二进制存在而广告可执行」。

**因此本次审计不对 Codex 做能力对照** —— 我们既没有接通它，也没有声称接通。这是一个诚实的空缺，不是缺口。

---

## 13. 相关文档

| 文档 | 角色 |
|------|------|
| [`NATIVE_ENGINE_FULL_REMEDIATION.md`](./NATIVE_ENGINE_FULL_REMEDIATION.md) | **契约与进度唯一权威**。本文是其第 2、3 节标签的证据来源 |
| [`NATIVE-DAEMON-CAPABILITY-MAP.md`](./NATIVE-DAEMON-CAPABILITY-MAP.md) | Daemon 内部模块边界（v1.1 已较准确，本次未发现需修正处） |
| [`NATIVE_ENGINE_ENV.md`](./NATIVE_ENGINE_ENV.md) | 环境变量与运维契约 |
| [`../superpowers/specs/2026-07-26-native-harness-control-plane-design.md`](../superpowers/specs/2026-07-26-native-harness-control-plane-design.md) | Harness 控制面设计稿。其第 552 行承诺的 `Rewake` OR 聚合与本文第 2.5 节的实现不符 |
| [`../standards/technical/01-layering.md`](../standards/technical/01-layering.md) | 分层约束 |
| [`../standards/technical/02-security.md`](../standards/technical/02-security.md) | 第 11 节 P0 项的判定依据 |
| ADR-0011 | 生产闭环缺口决策 |
