# 执行引擎技术设计方案

> **版本**: 1.0.0
> **日期**: 2026-06-23
> **状态**: Draft（待评审）
> **范围**: 输入框发送 Bug 修复 + E4 执行引擎全量落地（Runtime 抽象层 + CLI Runtime ×2 + Native Runtime + Context Assembler + Agent Loop + Tools Registry + Task Scheduler）
> **参考**: `/Users/ldh/Downloads/project/AiNative/References/CodePilot`
> **决策依据**: `CONTEXT.md`（已固化的 Q1-Q29 决策）+ `docs/architecture/module-workshop-kernel-runtime.md`（KI-1~KI-5）

---

## 0. 文档定位与阅读顺序

本文件是**实现导向**的技术方案，承接两份上游文档：

| 上游 | 角色 | 本文件关系 |
|------|------|------------|
| `CONTEXT.md` | 领域词汇表（Q1-Q29 决策沉淀） | 术语以 CONTEXT.md 为准，本文不复述定义 |
| `docs/architecture/module-workshop-kernel-runtime.md` | KI-1~KI-5 内核不变量 | 所有写盘路径必须过 KI 门禁 |
| `docs/architecture/TECHNICAL_DESIGN_V2.md` M15 | Agent 观测层（会话扫描/Skills/用量） | 范围不重叠——M15 是观测，本文是执行 |

**非目标**：不在本文讨论 M15 观测层、buddy 人格、onboarding 多步流程、迁移向导（已 V2 否决）。

---

## 1. 总体架构

### 1.1 现状基线

当前已有：

| 文件 | 行数 | 现状 |
|------|------|------|
| `src-tauri/src/assistant_stream_proxy.rs` | 539 | `stream_chat` 单轨——直连 OpenAI 兼容协议，含 agentic loop 雏形 + 自愈熔断 |
| `src-tauri/src/assistant_executor.rs` | 457 | 6 个工具（read_file/list_dir/write_file/write_module/run_terminal/lint_module）+ circuit breaker |
| `src-tauri/src/context_window.rs` | ~80 | 启发式 token 估算 + 滑动窗口截断（无压缩） |
| `src-tauri/src/commands/assistant.rs` | 341 | 会话/消息 CRUD |
| `src-tauri/src/commands/executor_settings.rs` | 84 | executor 设置持久化 |
| `src/lib/prompt-context-injector.ts` | 267 | 视觉主权 + Bridge 规约注入（**存在但 stream_chat 未用**） |
| `src/lib/tauri-adapter.ts` | 745 | `assistant.*` + `executorSettings.*` IPC 封装 |
| `src-tauri/src/wechat/driver.rs` | — | 已有 `detectAgents`（claude/codex CLI 的 `which()` 检测） |

**关键缺口**：
1. 无 runtime 抽象层——`stream_chat` 硬编码走 Native 路径
2. 无 CLI runtime（Claude CLI / Codex CLI）
3. Context Assembler 仅 token 截断，无 AST 索引、无摘要压缩、无 MCP 按需深查
4. Agent Loop 步数上限/doom-loop/进度心跳未实现（仅有自愈熔断）
5. Task Scheduler 完全缺失
6. `prompt-context-injector.ts` 未接入 `stream_chat`

### 1.2 目标架构

```
┌──────────────────────────── 前端 (Next.js / React) ────────────────────────────┐
│  AssistantWorkbench · MessageInput · MessageList · useAssistantStream           │
│         ↓ Tauri IPC                                                             │
│         ↓ stream_chat / cancel_stream / scheduled_tasks CRUD                    │
└─────────────────────────────────────────────────────────────────────────────────┘
                                  ↓
┌──────────────────────────── Rust 后端 (src-tauri/) ─────────────────────────────┐
│                                                                                  │
│  ┌───────────────────── Runtime 抽象层 (trait AgentRuntime) ─────────────────┐  │
│  │  resolve_runtime() → 按可用性分流                                            │  │
│  └────────────────────────────────────────────────────────────────────────────┘  │
│         ↓                          ↓                          ↓                  │
│  ┌──────────────┐         ┌──────────────────┐      ┌──────────────────────┐    │
│  │ Claude CLI   │         │ Codex CLI        │      │ Native Runtime       │    │
│  │ Runtime      │         │ Runtime          │      │ ┌──────────────────┐ │    │
│  │ spawn 二进制  │         │ 常驻 app-server  │      │ │ Protocol Adapter│ │    │
│  │ stdin/stdout │         │ JSON-RPC         │      │ │ (trait LlmProto)│ │    │
│  │ stream-json  │         │ notification     │      │ └──────────────────┘ │    │
│  └──────────────┘         └──────────────────┘      │ ┌──────────────────┐ │    │
│         ↓                          ↓                │ │ Context Assembler│ │    │
│  ┌────────────────────────────────────────┐         │ │  静态摘要+MCP    │ │    │
│  │      模块生成工具注入（三 runtime 共用） │         │ └──────────────────┘ │    │
│  │  write_module / lint_module (KI 门禁)   │         │ ┌──────────────────┐ │    │
│  └────────────────────────────────────────┘         │ │ Agent Loop       │ │    │
│         ↓                          ↓                │ │  步限/doom/心跳   │ │    │
│  ┌────────────────────────────────────────────┐     │ └──────────────────┘ │    │
│  │      CLI 写盘权限分流（路径白名单 W1）        │     │ ┌──────────────────┐ │    │
│  │  ~/.natives/modules/ → write_module 强制     │     │ │ Tools Registry   │ │    │
│  │  其他路径 → CLI 原生工具放行                  │     │ │  6 现有+补齐      │ │    │
│  └────────────────────────────────────────────┘     └──────────────────────┘    │
│                                                                                  │
│  ┌───────────────────── Task Scheduler (常驻 tokio task) ───────────────────┐  │
│  │  10s 轮询 scheduled_tasks → resolve_runtime().stream() → 落库 + 通知       │  │
│  └────────────────────────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────────────────────────┘
```

### 1.3 实施分期

| 期 | 内容 | 依赖 |
|----|------|------|
| **P0** | Q1-Q4 输入框发送 Bug 修复 | 无 |
| **P1** | Runtime 抽象层 + Native Runtime 重构（把现有 stream_chat 拆出 Native） | P0 |
| **P2** | Context Assembler（AST 索引 + 摘要压缩 + MCP 桥） | P1 |
| **P3** | Agent Loop 保护机制（步限 + doom + 心跳） | P1 |
| **P4** | Claude CLI Runtime | P1 |
| **P5** | Codex CLI Runtime | P1 |
| **P6** | CLI 写盘权限分流 + 模块生成工具注入 | P4, P5 |
| **P7** | Task Scheduler | P1 |
| **P8** | 前端适配（Settings 管理面板 + 三态气泡补齐） | P2-P7 |

P0 可独立先行；P1 是关键路径阻塞 P2-P7；P4/P5/P2/P3/P7 互相独立可并行。

---

## 2. P0 — 输入框发送 Bug 修复

### 2.1 根因

`AssistantWorkbench.tsx` 的 `activeSessionId` 初始为 `null`，传入 `MessageInput` 后触发 `disabled={!activeSessionId}`，导致首次进入助理界面输入框被禁用、无法发送。

### 2.2 修复方案

**Q2-A + Q3-B + Q3-B-1 + Q4-A** 组合：

```typescript
// AssistantWorkbench.tsx —— mount 时自动建会话
useEffect(() => {
  let cancelled = false;
  (async () => {
    // 1. 等 providers 就绪（Q3-B）
    const providers = await nativesAPI.providers.list();
    if (cancelled) return;

    if (providers.length === 0) {
      // Q3-B-1: 无 provider → 禁用输入 + 提示配置
      setInputDisabledReason('no_provider');
      return;
    }

    // 2. Q2-A: 自动建草稿会话
    const session = await nativesAPI.assistant.createSession({
      projectId: currentProjectId ?? null,  // null = 全局草稿
      model: providers[0].defaultModel,
      providerId: providers[0].id,
    });
    if (!cancelled) {
      setActiveSessionId(session.id);
      setInputDisabledReason(null);
    }
  })();
  return () => { cancelled = true; };
}, []);  // Q4-A: 仅 mount 触发，不依赖 currentProjectId
```

`MessageInput` 改造：

```typescript
// 新增 inputDisabledReason 优先于 activeSessionId 判断
disabled={!activeSessionId || inputDisabledReason === 'no_provider'}
helperText={
  inputDisabledReason === 'no_provider'
    ? t('assistant.noProviderHint')  // "请先在设置中配置 AI 供应商"
    : !activeSessionId
      ? t('assistant.creatingSession')
      : null
}
```

**不处理项目切换 bug**（Q4-A 决策）——项目切换时的会话切换逻辑不在本次范围。

### 2.3 i18n 键

`en.ts` / `zh.ts` 同步新增：

| key | en | zh |
|-----|----|----|
| `assistant.noProviderHint` | "Configure an AI provider first" | "请先在设置中配置 AI 供应商" |
| `assistant.creatingSession` | "Starting session..." | "正在创建会话..." |

---

## 3. P1 — Runtime 抽象层

### 3.1 trait 设计

参考 CodePilot `runtime/types.ts` 的 `AgentRuntime` 接口，落地为 Rust trait：

```rust
// src-tauri/src/runtime/mod.rs (新建)
use async_trait::async_trait;
use futures_util::Stream;
use std::pin::Pin;

/// 单次 stream 调用的入参（对应 CodePilot RuntimeStreamOptions）
pub struct RuntimeStreamOptions {
    pub session_id: String,
    pub prompt: String,
    pub model: String,
    pub provider_id: String,
    pub system_prompt: Option<String>,
    pub working_directory: Option<PathBuf>,
    pub abort_handle: tokio::sync::oneshot::Sender<()>,
    /// runtime 专属透传字段（CLI 的 sdk_session_id、Native 的 files 等）
    pub runtime_options: serde_json::Value,
}

/// SSE 下行事件载荷（对应 CodePilot RuntimeRunEvent 8+1 union）
/// 前端按 type 分发渲染，未知 type 走 unknown_item fallback
#[derive(serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuntimeEvent {
    AssistantDelta { text: String },
    ToolStarted { tool_name: String, tool_call_id: String, args: serde_json::Value },
    ToolCompleted { tool_call_id: String, status: String, output: serde_json::Value },
    FileChanged { path: String, change_type: String },
    UsageUpdated { input_tokens: u64, output_tokens: u64 },
    RunCompleted { reason: String },
    RunFailed { error: String },
    UnknownItem { raw: serde_json::Value },
}

pub type EventStream = Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>;

#[async_trait]
pub trait AgentRuntime: Send + Sync {
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn is_available(&self) -> bool;
    async fn stream(&self, options: RuntimeStreamOptions) -> Result<EventStream>;
    fn interrupt(&self, session_id: &str);
    fn dispose(&self);
}
```

**与 CodePilot 差异**：
- CodePilot 用 ReadableStream<string>（SSE 文本行），我们直接用 `Stream<RuntimeEvent>` 强类型——Tauri Event 序列化时再转 JSON，省一层 SSE 文本解析
- 不抽 `interrupt(sessionId)` 之外的细粒度方法——CLI/Codex 的会话管理细节藏各自实现里

### 3.2 Registry 与分流

```rust
// src-tauri/src/runtime/registry.rs (新建)
use std::sync::Arc;
use tokio::sync::RwLock;

lazy_static! {
    static ref REGISTRY: RwLock<Vec<Arc<dyn AgentRuntime>>> = RwLock::new(vec![]);
}

pub async fn register(runtime: Arc<dyn AgentRuntime>) { /* ... */ }

/// Q20 P3 分流：Claude CLI > Codex CLI > Native
/// 失败时降级并返回降级原因（供前端提示）
pub async fn resolve_runtime(override_id: Option<&str>) -> (Arc<dyn AgentRuntime>, Option<String>) {
    let all = REGISTRY.read().await;
    let order = ["claude_cli", "codex_cli", "native"];

    // 1. 显式 override 优先（session pin）
    if let Some(id) = override_id {
        if let Some(rt) = all.iter().find(|r| r.id() == id && r.is_available()) {
            return (rt.clone(), None);
        }
        // Q20: 显式指定但不可用 → 抛错而非静默降级
        return Err(...);
    }

    // 2. 自动按优先级
    for id in order {
        if let Some(rt) = all.iter().find(|r| r.id() == id && r.is_available()) {
            let downgrade_hint = if id == "native" {
                Some("未检测到 Claude/Codex CLI，已降级为内置引擎，建议安装以获得更好体验".into())
            } else { None };
            return (rt.clone(), downgrade_hint);
        }
    }
    unreachable!("native runtime 永远 available")
}
```

**注册时机**：`lib.rs` 的 `setup` 钩子里依次 register 三个 runtime。Native 永远注册（兜底）；CLI runtime 在 `is_available()` 里跑 `which()` 检测，false 时不进候选。

### 3.3 stream_chat 入口改造

现有 `assistant_stream_proxy.rs::stream_chat` 改为薄入口：

```rust
#[tauri::command]
pub async fn stream_chat(app: AppHandle, input: StreamChatInput) -> Result<()> {
    let (runtime, hint) = runtime::resolve_runtime(input.runtime_override.as_deref()).await?;

    // emit 降级提示（若有）
    if let Some(h) = hint { app.emit("assistant://stream/hint", h)?; }

    let abort_tx = /* oneshot */;
    let stream = runtime.stream(RuntimeStreamOptions { /* ... */ }).await?;

    // 转发事件到 Tauri Event channel
    tokio::spawn(async move {
        tokio::pin!(stream);
        while let Some(ev) = stream.next().await {
            app.emit(&format!("assistant://stream/{}", input.session_id), ev)?;
            if matches!(ev, RuntimeEvent::RunCompleted | RuntimeEvent::RunFailed) { break; }
        }
        Ok::<_, Error>(())
    });

    // 注册 abort handle
    STREAM_REGISTRY.lock().insert(input.session_id, StreamHandle { abort_handle: abort_tx });
    Ok(())
}
```

**原 `assistant_stream_proxy.rs` 里的 Native agentic loop 代码移到 `runtime/native_runtime.rs`**，文件保留作为 IPC 入口薄层。

---

## 4. P2 — Native Runtime 子系统

### 4.1 协议适配层（trait LlmProtocol）

```rust
// src-tauri/src/runtime/native/protocol.rs (新建)
#[async_trait]
pub trait LlmProtocol: Send + Sync {
    /// 流式发送 messages + tools，返回事件流
    async fn stream(
        &self,
        endpoint: &str,
        api_key: &str,
        model: &str,
        messages: &[serde_json::Value],
        tools: &[serde_json::Value],
    ) -> Result<Pin<Box<dyn Stream<Item = LlmChunk> + Send>>>;
}

pub enum LlmChunk {
    Delta(String),
    ToolCall(serde_json::Value),  // 累积后的完整 tool_call
    Usage { input: u64, output: u64 },
    Done,
    Error(String),
}

// 现有实现
pub struct OpenAiCompatibleProtocol { /* reqwest Client */ }
// 未来扩展：AnthropicNativeProtocol / OllamaProtocol
```

现有 `assistant_stream_proxy.rs` 里那段 reqwest SSE 解析代码平移进 `OpenAiCompatibleProtocol`。

### 4.2 Context Assembler

参考 CodePilot `context-assembler.ts` 的分层装配，落地为 Rust：

```rust
// src-tauri/src/runtime/native/context_assembler.rs (新建)
pub struct AssembledContext {
    pub system_prompt: String,
    pub messages: Vec<serde_json::Value>,  // 已截断+摘要替代的历史
}

pub async fn assemble(
    session_id: &str,
    user_prompt: &str,
    working_dir: Option<&Path>,
    budget: TokenBudget,
    db: &Db,
) -> Result<AssembledContext> {
    // ── 静态摘要打底（Q8 C5 + Q12 A4 + Q13 P3+S3）──
    let mut sections = vec![];

    // L1: 项目文件树
    if let Some(wd) = working_dir {
        sections.push(format_file_tree(wd, MAX_TREE_ENTRIES));
    }

    // L2: 身份文件全文（claude.md/soul.md/user.md，单文件 PER_FILE_LIMIT）
    sections.push(format_identity_files(working_dir).await);

    // L3: 代码签名摘要（Q14 T2：syntect + regex fallback）
    sections.push(format_code_signatures(working_dir, &budget).await);

    // L4: 模块生成规约（复用 prompt-context-injector.ts 的 Rust 等价物）
    sections.push(inject_module_specs());

    // ── 预算分配（Q16 B1）──
    // 系统规约 15% + 历史 40% + 动态注入 30% + 输出 15%
    let tokens = estimate_tokens(&sections.join("\n"));
    let history_budget = budget.total * 40 / 100 - tokens;  // 简化
    let history = assemble_history(session_id, history_budget, db).await?;

    // ── MCP 按需深查（Q12 动态感官层）──
    // 不预载，仅声明 MCP 工具让 LLM 自己查

    Ok(AssembledContext { system_prompt: sections.join("\n\n"), messages: history })
}
```

**文件树 + 身份文件**：

```rust
fn format_file_tree(wd: &Path, max_entries: usize) -> String {
    // 深度优先遍历，跳过 node_modules/.git/dist 等
    // 输出形如：
    // src/
    //   assistant/
    //     AssistantWorkbench.tsx
    //     MessageInput.tsx
    // ...
}

async fn format_identity_files(wd: Option<&Path>) -> String {
    // 查找 claude.md / soul.md / user.md / AGENTS.md
    // 单文件 PER_FILE_LIMIT (默认 8KB)，超限截断 + [truncated] 标记
}
```

**代码签名摘要（Q14）**：

```rust
async fn format_code_signatures(wd: Option<&Path>, budget: &TokenBudget) -> String {
    // 1. syntect 提取 Rust/TS/JS/Python/Go 的函数/类型签名
    // 2. 不支持的语言降级为正则提取（fn/func/function/class/interface 等）
    // 3. 单文件签名上限 4KB，整体上限为 budget.dynamic * 30%
}
```

**依赖**：`syntect` crate（Cargo.toml 新增）。

**AST 索引（Q15 F2+L2）**：

```rust
// src-tauri/src/runtime/native/ast_index.rs (新建)
pub struct ProjectIndex {
    pub project_hash: String,  // sha256(absolute_path)
    pub entries: Vec<IndexEntry>,  // 文件签名 + mtime
}

pub fn index_path(project_hash: &str) -> PathBuf {
    // ~/.natives/assistant-index/{project_hash}/index.json
    dirs::home_dir().unwrap().join(".natives/assistant-index").join(project_hash)
}

pub async fn ensure_indexed(project_dir: &Path) -> Result<ProjectIndex> {
    let hash = sha256(project_dir);
    let idx_path = index_path(&hash);
    // 存在且最新 → 直接读；否则增量重建
    // fsWatch 监听变更触发后台增量更新
}
```

fsWatch 复用现有 `src-tauri/src/fs_watch.rs`，新增对项目目录的监听注册（与 modules 目录监听并行）。

**摘要压缩（Q16 E3+M3）**：

```rust
// src-tauri/src/runtime/native/compressor.rs (新建)
const COMPRESSION_THRESHOLD: f32 = 0.8;  // 80% 触发

pub async fn maybe_compress(
    session_id: &str,
    messages: &[serde_json::Value],
    model_context_limit: usize,
    compress_model: &str,  // Q16: 用户可选，默认用会话主模型
    protocol: &dyn LlmProtocol,
) -> Result<CompressResult> {
    let usage = estimate_total_tokens(messages);
    if (usage as f32 / model_context_limit as f32) < COMPRESSION_THRESHOLD {
        return Ok(CompressResult::Unchanged);
    }
    // 取旧消息（保留最近 N 轮完整）→ 调压缩模型生成摘要 → 存 session 元数据
    let summary = call_compress_model(compress_model, messages, protocol).await?;
    Ok(CompressResult::Compressed { summary, messages_dropped: n, tokens_saved })
}
```

**Tokenizer（Q16 插件化）**：

```rust
// src-tauri/src/tokenizer/mod.rs (新建)
pub trait Tokenizer: Send + Sync {
    fn count(&self, text: &str) -> usize;
}

pub struct HeuristicTokenizer;  // chars/3.5 启发式，默认兜底
pub struct TiktokenTokenizer { encoding: Encoding }  // tiktoken-rs，可选 provider 插件
```

tokenizer 插件路径：`~/.natives/tokenizers/{provider_id}.so`（dylib 动态加载）——**MVP 阶段只做 HeuristicTokenizer + tiktoken-rs 静态链接**，动态插件化留 V2。设置页提供「tokenizer 选择」开关，关闭则用启发式。

### 4.3 Agent Loop 保护机制（Q17）

现有 `assistant_stream_proxy.rs` 的 agentic loop 补三件套：

```rust
// src-tauri/src/runtime/native/agent_loop.rs (从 stream_proxy 拆出)
const DEFAULT_MAX_STEPS: u32 = 50;
const DOOM_LOOP_THRESHOLD: u32 = 3;  // 固定阈值，不可配置

pub async fn run_loop(opts: LoopOptions) -> Result<()> {
    let mut step = 0;
    let mut last_tool_signature: Option<String> = None;
    let mut doom_count = 0;
    let mut fail_count = 0;

    while step < opts.max_steps {
        step += 1;

        // 流式调模型 + 累积 tool_calls（现有逻辑）
        let (delta, tool_calls) = stream_one_round(&opts).await?;

        if tool_calls.is_empty() { return Ok(()); }  // 模型说完了

        // ── Doom Loop 检测（Q17 D1）──
        let sig = tool_signature(&tool_calls);
        if Some(&sig) == last_tool_signature.as_ref() {
            doom_count += 1;
            if doom_count >= DOOM_LOOP_THRESHOLD {
                emit(RuntimeEvent::RunFailed { error: "doom loop detected".into() });
                return Err(...);
            }
        } else {
            doom_count = 0;
            last_tool_signature = Some(sig);
        }

        // ── 执行工具（现有逻辑）+ 进度心跳（Q17 K3）──
        for tc in &tool_calls {
            emit(RuntimeEvent::ToolStarted { /* ... */ });
            // 长任务工具（run_terminal）透传 stdout 行作心跳
            // 无进度工具降级为 15s 间隔空心跳（防前端超时）
            let result = execute_tool(tc).await;
            match result.status.as_str() {
                "success" => fail_count = 0,
                "error" => {
                    fail_count += 1;
                    if fail_count > MAX_SELF_HEAL {  // 现有 3
                        emit(RuntimeEvent::RunFailed { error: "circuit broken".into() });
                        return Err(...);
                    }
                    // 把错误塞回 messages 让模型自纠（现有逻辑）
                }
            }
            emit(RuntimeEvent::ToolCompleted { /* ... */ });
        }
    }

    // 步数超限
    emit(RuntimeEvent::RunFailed { error: format!("max steps ({}) exceeded", opts.max_steps) });
    Ok(())
}
```

**步数上限配置**：`executor_settings` 新增字段 `max_steps: u32`（默认 50），设置页可改。

### 4.4 Tools Registry 扩展

现有 6 个工具保留，补齐 CONTEXT.md「补齐后含记忆检索/会话搜索/反问用户/视觉规约/通知调度/媒体导入等组」：

| 工具名 | 类型 | 用途 | 阶段 |
|--------|------|------|------|
| `read_file` | 现有 | 读文件 | — |
| `list_dir` | 现有 | 列目录 | — |
| `write_file` | 现有 | 写非模块路径 | — |
| `write_module` | 现有 | 写模块（KI 门禁） | — |
| `run_terminal` | 现有 | 执行命令 | — |
| `lint_module` | 现有 | 契约 lint | — |
| `search_memory` | 新增 | 检索助理记忆 | P2 |
| `search_sessions` | 新增 | 检索历史会话 | P2 |
| `ask_user` | 新增 | 反问用户（阻塞等回答） | P3 |
| `inject_visual_specs` | 新增 | 注入 Liquid Glass 视觉令牌 | P2 |
| `schedule_task` | 新增 | 创建定时任务 | P7 |
| `import_media` | 新增 | 导入截图/图片到上下文 | P8 |

**命名空间**：保持裸名（Q18 决策），与现有 6 个风格一致。

**CLI 轨不走此注册表**——CLI 用自带工具，模块生成工具由 Rust 注入（见 P6）。

---

## 5. P4 — Claude CLI Runtime

### 5.1 调用模型

参考 CodePilot `claude-client.ts::streamClaudeSdk`，但**不用 Node SDK**（避免引入 Node 运行时），改为直接 spawn 二进制（Q24 C1）：

```rust
// src-tauri/src/runtime/claude_cli.rs (新建)
use tokio::process::Command;
use tokio::io::{AsyncBufReadExt, BufReader};

pub struct ClaudeCliRuntime {
    binary_path: Option<PathBuf>,  // which() 缓存
}

#[async_trait]
impl AgentRuntime for ClaudeCliRuntime {
    fn id(&self) -> &'static str { "claude_cli" }
    fn is_available(&self) -> bool { self.binary_path.is_some() }

    async fn stream(&self, opts: RuntimeStreamOptions) -> Result<EventStream> {
        let bin = self.binary_path.as_ref().ok_or(...)?;
        let mut child = Command::new(bin)
            .arg("--print")
            .arg("--output-format").arg("stream-json")
            .arg("--input-format").arg("stream-json")
            .arg("--model").arg(&opts.model)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        // stdin 喂 prompt（JSON 行）
        let stdin = child.stdin.take().unwrap();
        stdin.write_all(json!({"type":"user","message":opts.prompt}).to_string().as_bytes()).await?;

        // stdout 读 stream-json，每行一个事件
        let stdout = child.stdout.take().unwrap();
        let reader = BufReader::new(stdout);
        let stream = reader.lines()
            .filter_map(|line| async move {
                line.ok().and_then(|l| translate_claude_cli_event(&l))
            });
        Ok(Box::pin(stream))
    }
}
```

**事件翻译**：Claude CLI 的 stream-json 格式 → `RuntimeEvent` union（`assistant_delta` / `tool_started` / `tool_completed` 等）。

**会话续接**：CLI 自管会话，Rust 侧存 `sdk_session_id` 到 session metadata，下次传 `--resume {sdk_session_id}`。

**is_available**：复用 `wechat/driver.rs::detectAgents` 的 `which("claude")` 检测。

### 5.2 二进制发现

```rust
fn find_claude_binary() -> Option<PathBuf> {
    // 1. 设置页用户自定义路径
    // 2. PATH 上的 `claude` / `claude.exe`
    // 3. ~/.claude/local/claude
    // 缓存结果，invalidate 接口供设置页调用
}
```

---

## 6. P5 — Codex CLI Runtime

### 6.1 调用模型

参考 CodePilot `codex/app-server-manager.ts`，常驻 app-server + JSON-RPC（Q25 X2）：

```rust
// src-tauri/src/runtime/codex_cli.rs (新建)
pub struct CodexCliRuntime {
    server: Mutex<Option<CodexAppServer>>,  // 单例常驻
}

struct CodexAppServer {
    proc: Child,
    client: JsonRpcClient,  // over stdio
}

#[async_trait]
impl AgentRuntime for CodexCliRuntime {
    fn id(&self) -> &'static str { "codex_cli" }
    fn is_available(&self) -> bool { find_codex_binary().is_some() }

    async fn stream(&self, opts: RuntimeStreamOptions) -> Result<EventStream> {
        let server = self.ensure_server().await?;  // 懒启动单例

        // 1. thread/resume 或 thread/start
        let thread_id = resume_or_start_thread(&server.client, &opts).await?;

        // 2. 订阅 notification
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        server.client.subscribe(move |notif| {
            if let Some(ev) = translate_codex_notification(&notif) {
                tx.blocking_send(ev).ok();
            }
        }).await?;

        // 3. turn/start
        server.client.call("turn/start", json!({
            "thread_id": thread_id,
            "prompt": opts.prompt,
        })).await?;

        // 4. 转 Stream
        Ok(Box::pin(ReceiverStream::new(rx)))
    }
}

impl CodexCliRuntime {
    async fn ensure_server(&self) -> Result<&CodexAppServer> {
        // 单例：已启动则复用；未启动则 spawn `codex app-server` + 建 JSON-RPC client
        // 崩溃检测 + 重启
    }
}
```

**生命周期**：app-server 全局单例，崩溃自动重启；`dispose()` 在 app 退出时 graceful kill。

**事件翻译**：Codex notification（`agentMessage/delta` / `item/started` / `item/completed` / `turn/completed`）→ `RuntimeEvent`。

**JSON-RPC client**：自建轻量实现（tokio stdio + serde_json），不引第三方 crate。

---

## 7. P6 — CLI 写盘权限分流 + 模块生成工具注入

### 7.1 写盘权限分流（Q28 W1）

```rust
// src-tauri/src/runtime/cli_permission.rs (新建)
const MODULES_DIR: &str = ".natives/modules/";

pub fn classify_write(path: &Path) -> WriteKind {
    let modules_root = dirs::home_dir().unwrap().join(MODULES_DIR);
    if path.starts_with(&modules_root) {
        WriteKind::ModulePath  // 必须走 write_module
    } else {
        WriteKind::GeneralPath  // CLI 原生工具放行
    }
}

pub enum WriteKind { ModulePath, GeneralPath }
```

**三层防护**：

1. **system prompt 引导**（事前）：CLI 启动时注入指令「涉及 `~/.natives/modules/` 写入必须用 write_module 工具」
2. **Rust fsWatch 兜底**（事中）：监听 modules 目录，检测到非 write_module 的写入即告警 + 回滚
3. **Codex approval bridge**（事中）：Codex 的 exec 命令经 approval 拦截 modules 路径写入

### 7.2 模块生成工具注入（Q27 I1）

CLI runtime 启动时，通过 `--append-system-prompt`（Claude）或 MCP server（Codex）注入模块生成规约 + `write_module` / `lint_module` 工具定义。

**复用 `prompt-context-injector.ts`**：现有该文件输出的是 TypeScript 字符串，Rust 侧需建等价物（`src-tauri/src/module_specs.rs`），导出规约文本 + 工具 JSON schema。CLI 启动时读取并注入。

**Claude CLI 注入方式**：

```rust
.arg("--append-system-prompt").arg(module_specs::full_specs())
```

**Codex 注入方式**：起一个内置 MCP server（Rust 实现，stdio），暴露 `write_module` / `lint_module` 两个工具，启动 Codex 时通过 `--mcp-config` 挂载。

---

## 8. P7 — Task Scheduler

### 8.1 数据模型

```sql
-- assistant DB 新增表（与 KI-2 WAL 隔离）
CREATE TABLE IF NOT EXISTS scheduled_tasks (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    prompt TEXT NOT NULL,
    runtime_override TEXT,          -- null = auto
    schedule_type TEXT NOT NULL,    -- once / interval / cron
    schedule_value TEXT NOT NULL,   -- ISO8601 / 秒数 / cron 表达式
    next_run TEXT NOT NULL,         -- ISO8601
    last_status TEXT,               -- success / failed / running
    consecutive_errors INTEGER DEFAULT 0,
    last_error TEXT,
    enabled INTEGER DEFAULT 1,
    created_at TEXT NOT NULL,
    expires_at TEXT                 -- 定时任务过期时间（recurring 7 天）
);

CREATE TABLE IF NOT EXISTS task_runs (
    id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES scheduled_tasks(id) ON DELETE CASCADE,
    started_at TEXT NOT NULL,
    finished_at TEXT,
    status TEXT,                    -- success / failed
    result_summary TEXT,
    error TEXT
);
```

### 8.2 调度器实现

```rust
// src-tauri/src/scheduler.rs (新建)
const POLL_INTERVAL: Duration = Duration::from_secs(10);
const BACKOFF_DELAYS: &[Duration] = &[30s, 1m, 5m, 15m];
const MAX_CONSECUTIVE_ERRORS: u32 = 10;

pub fn start_scheduler(app: AppHandle) {
    tokio::spawn(async move {
        // 启动时恢复：处理 missed tasks + stale running
        recover_missed_tasks().await;
        recover_stale_running().await;

        let mut ticker = tokio::time::interval(POLL_INTERVAL);
        loop {
            ticker.tick().await;
            let due = get_due_tasks().await;
            for task in due {
                tokio::spawn(execute_task(task, app.clone()));  // fire-and-forget
            }
        }
    });
}

async fn execute_task(task: ScheduledTask, app: AppHandle) {
    // 1. 标记 running + 创建 task_run
    // 2. resolve_runtime().stream() —— 复用 P1 的 runtime 抽象
    // 3. 收完事件 → 提取 assistant 文本
    // 4. 落库到 assistant_messages（Q29 T1）
    // 5. 系统通知（Q29 T1）
    // 6. 更新 next_run + consecutive_errors
    let (runtime, _) = resolve_runtime(task.runtime_override.as_deref()).await?;
    let stream = runtime.stream(/* ... */).await?;
    // ... 收完 ...
    notification::send("定时任务完成", &task.name, &summary).await?;
}
```

**cron 解析**：用 `cron` crate。

**通知**：复用现有 `src-tauri/src/commands/notification.rs`。

### 8.3 IPC 命令

```rust
// src-tauri/src/commands/scheduler.rs (新建)
#[tauri::command] pub async fn scheduler_list_tasks() -> Vec<ScheduledTask>
#[tauri::command] pub async fn scheduler_create_task(task: CreateTaskInput) -> ScheduledTask
#[tauri::command] pub async fn scheduler_update_task(id: String, patch: TaskPatch) -> ()
#[tauri::command] pub async fn scheduler_delete_task(id: String) -> ()
#[tauri::command] pub async fn scheduler_run_task_now(id: String) -> TaskRunResult
#[tauri::command] pub async fn scheduler_list_runs(task_id: String) -> Vec<TaskRun>
```

`tauri-adapter.ts` 对应新增 `scheduler.*` 命名空间。

---

## 9. P8 — 前端适配

### 9.1 设置页扩展

新增两个管理面板：

**执行引擎设置**（扩展现有 `executorSettings`）：
- runtime 选择（auto / claude_cli / codex_cli / native）
- CLI 二进制路径配置 + 检测按钮
- max_steps 滑块（默认 50）
- 压缩模型选择
- tokenizer 选择（启发式 / tiktoken / 关闭）

**定时任务管理**（新增 `scheduler.*` 面板）：
- 任务列表（CRUD）
- 每个任务：name + prompt + schedule_type + schedule_value + runtime_override
- 运行历史查看

### 9.2 三态气泡补齐

现有 `useAssistantStream` 已有 `toolStatus` / `toolResult` / `selfHealCount` 字段。需补齐 `MessageList` 对新增 `RuntimeEvent` 类型的渲染：

| RuntimeEvent | UI 行为 |
|--------------|---------|
| `tool_started` | 显示 pending 气泡（工具名 + 参数摘要 + loading） |
| `tool_completed` (success) | 切 result 气泡（可折叠 JSON） |
| `tool_completed` (error) | 切 error 气泡（红色 + 重试按钮） |
| `file_changed` | 文件卡片闪烁（heat glow） |
| `usage_updated` | 更新 UsagePanel |
| `run_failed` | 终止流 + 错误提示 |

### 9.3 降级提示

`resolve_runtime` 返回 hint 时，工作台顶部显示横幅：「未检测到 Claude/Codex CLI，已降级为内置引擎，[去设置安装]」。

---

## 10. 数据流与 IPC 清单

### 10.1 新增 IPC 命令

| 命令 | 文件 | 期 |
|------|------|-----|
| `stream_chat` 改造 | `assistant_stream_proxy.rs` | P1 |
| `cancel_stream` 保持 | 同上 | — |
| `runtime_list_available` | `commands/runtime.rs` | P1 |
| `runtime_detect_cli` | 同上 | P4/P5 |
| `scheduler_*` (6 个) | `commands/scheduler.rs` | P7 |

### 10.2 新增 Tauri Event

| 事件 | 触发 | 消费 |
|------|------|------|
| `assistant://stream/{session_id}` | runtime 事件下行 | `useAssistantStream` |
| `assistant://stream/hint` | 降级提示 | 工作台横幅 |
| `assistant://task/completed` | 定时任务完成 | 通知中心 |

### 10.3 DB 变更

| 表 | 变更 | 期 |
|----|------|-----|
| `scheduled_tasks` | 新建 | P7 |
| `task_runs` | 新建 | P7 |
| `assistant_sessions` | 加 `runtime_override` / `sdk_session_id` / `summary` 字段 | P1 |
| `executor_settings` 表 | 加 `max_steps` / `compress_model` / `tokenizer` 字段 | P3/P2 |

---

## 11. Cargo.toml 新增依赖

```toml
[dependencies]
syntect = "5"          # P2 AST 签名提取
cron = "0.12"          # P7 cron 解析
tiktoken-rs = "0.5"    # P2 tokenizer（可选，默认启 feature）
async-trait = "0.1"    # P1 trait AgentRuntime
```

---

## 12. 风险与未决项

| 风险 | 缓解 |
|------|------|
| Claude CLI stream-json 格式版本变更 | 锁版本 + `unknown_item` fallback 兜底 |
| Codex app-server JSON-RPC 协议未公开稳定 | 抽象在 `codex_cli.rs` 内，协议变更只改翻译层 |
| syntect 对超大项目索引慢 | 增量索引 + 后台 tokio task + 超时降级正则 |
| tiktoken-rs 编译体积 | 设为 feature flag，默认关，用户在设置页开启时动态加载（MVP 阶段静态链接） |
| **KI-2 WAL 未实现**（既有缺口） | Task Scheduler 的 `scheduled_tasks` 表与核心 WAL 隔离，不阻塞；但模块写入仍依赖 KI-2，需先补 |

**未决项**：
1. **KI-2 WAL Journal 实现优先级**——模块写入链路依赖它，是否在 P6 前先补？建议单独立项。
2. **tokenizer 动态插件化**——MVP 用静态链接 tiktoken-rs，真正的 dylib 插件留 V2。
3. **Codex approval bridge 完整实现**——MVP 阶段先做 system prompt 引导 + fsWatch 兜底，approval bridge 留 V2。

---

## 13. 验证策略

| 期 | 验证命令 |
|----|----------|
| 全期 | `rtk tsc --noEmit` + `cd src-tauri && rtk cargo check` |
| P1 | `cd src-tauri && rtk cargo test --lib` runtime 抽象单测 |
| P2 | AST 索引 + 压缩单测（固定样本输入） |
| P3 | doom-loop / 步限 / 熔断单测 |
| P4/P5 | 集成测试：mock CLI 二进制，验证 spawn + 事件翻译 |
| P7 | 调度器单测：mock 时间，验证 cron 解析 + 退避 |
| 全期 | `rtk npm run lint` |

---

## 14. 实施顺序建议

```
P0 (输入框修复) ──→ 可独立合并，最快见效
  ↓
P1 (Runtime 抽象) ──→ 关键路径，阻塞后续
  ↓
  ├─ P2 (Context Assembler)     ┐
  ├─ P3 (Agent Loop 保护)       ├── 可并行
  ├─ P4 (Claude CLI)            │
  ├─ P5 (Codex CLI)             │
  └─ P7 (Task Scheduler)        ┘
       ↓
P6 (CLI 写盘分流 + 模块工具注入) ──→ 依赖 P4/P5
       ↓
P8 (前端适配) ──→ 依赖 P2-P7
```

预估工作量（粗略）：P0 0.5d / P1 2d / P2 3d / P3 1d / P4 2d / P5 3d / P6 2d / P7 2d / P8 2d ≈ **17.5 人日**。
