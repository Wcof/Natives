# 代码模块划分与文件规模规范

> **版本**: 1.0.0 · **日期**: 2026-07-23  
> **权威关系**：本文件为**架构级编码与模块边界指南**（含可操作阈值）。与产品红线冲突时，以 [`docs/standards/`](../standards/README.md) 为准；分层依赖的进程边界以 [`technical/01-layering.md`](../standards/technical/01-layering.md) 为准；前端组件单文件阈值以 [`frontend/01-structure.md`](../standards/frontend/01-structure.md) R-E6 为准（组件侧更严）。  
> **适用范围**：本项目全部手写代码，包括前端、Tauri Host、Agent Daemon、Rust crates、执行引擎、Provider、工具系统、MCP、数据存储及相关测试代码。  
> **不适用范围**：自动生成代码、第三方代码、数据库迁移快照、协议生成文件和大规模静态数据。  
> **核心目标**：通过明确的模块边界、单一状态归属和稳定内部接口，控制代码复杂度，避免超大文件、超级管理器、循环依赖和跨层调用。

---

## 1. 核心原则

### 1.1 按职责拆分，不按行数机械拆分

代码拆分的主要依据是：

1. 模块承担了多个不同职责；
2. 模块存在多个独立变化原因；
3. 模块持有多个生命周期不同的状态；
4. 模块依赖大量无关组件；
5. 模块难以独立测试；
6. 模块内部已经形成明显的能力分区；
7. 修改一个功能经常影响无关功能。

代码行数仅作为复杂度预警信号，不得采用以下无意义拆分方式：

```text
production_part1.rs
production_part2.rs
production_part3.rs
```

也不得通过创建无明确语义的文件掩盖复杂度：

```text
utils.rs
helpers.rs
common.rs
misc.rs
manager.rs
```

新增模块必须具有明确的业务或技术职责。

---

### 1.2 模块必须能够用一句话描述职责

每个模块应当能够用一句话说明：

> 该模块管理什么能力、拥有什么状态、通过什么接口对外提供服务。

例如：

```text
InteractionBroker：负责创建、等待、恢复和结束用户交互请求。
```

不合格的描述：

```text
ProductionRuntime：负责 Agent 运行相关的各种功能。
```

无法用一句话说清职责，通常意味着模块边界过大。

---

### 1.3 状态必须有唯一归属

同一业务状态只能有一个权威写入者。

例如 Run 状态应明确由一个模块负责提交：

```text
AgentEngine 产生状态变更请求
→ RunLifecycleService 校验状态转移
→ Repository 持久化
→ 更新内存投影
→ 发布事件
```

禁止多个模块分别修改同一状态：

```text
UI 修改任务状态
AgentEngine 修改任务状态
RunManager 修改任务状态
数据库代码再次修正任务状态
```

前端 Store、缓存和数据库中的状态只能作为权威状态的投影，不得成为并列真相源。

---

### 1.4 模块通过接口协作，不直接访问内部状态

禁止跨模块直接访问内部字段：

```rust
runtime.engines.write().await.remove(&run_id);
```

应通过公开接口：

```rust
engine_registry.remove(run_id).await?;
```

禁止一个模块直接操作另一个模块的数据表或内部集合。

正确依赖方式：

* 明确输入输出：方法或 Trait；
* 状态变化通知：领域事件；
* 跨进程通信：RPC、IPC、stdio 或其他明确协议；
* 持久化访问：Repository；
* 长任务终止：统一 CancellationToken。

---

## 2. 文件规模规范

### 2.1 单个源代码文件

|        文件规模 | 处理要求           |
| ----------: | -------------- |
|     0～300 行 | 正常范围           |
|   300～500 行 | 检查是否开始混合职责     |
|   500～700 行 | 必须进行模块边界审查     |
| 700～1,000 行 | 原则上应拆分；保留需说明原因 |
|  超过 1,000 行 | 必须拆分或形成书面例外说明  |
|  超过 2,000 行 | 视为严重架构预警       |

项目默认要求：

```text
推荐：单文件不超过 500 行
警戒：超过 700 行
整改：超过 1,000 行
```

> **前端组件**：UI 组件文件遵循更严的 SHOULD 阈值（约 300 行 / 3+ 不相关状态即应拆分），见 `docs/standards/frontend/01-structure.md` R-E6。

这里的行数以主要有效代码为参考，可排除：

* 版权头；
* 空行；
* 大段注释；
* 测试夹具；
* 生成代码；
* 静态映射数据。

---

### 2.2 允许较长的文件

以下文件在职责单一的情况下可以超过一般限制：

* 协议类型定义；
* 请求和响应结构；
* 错误码映射；
* 状态枚举；
* RPC 方法常量表；
* 数据库迁移；
* 自动生成代码；
* 静态配置表；
* 纯声明式路由表。

即使属于例外，当文件超过 1,500 行时，也应评估能否按协议版本、能力域或数据类别拆分。

---

## 3. 函数规模规范

|     函数规模 | 处理要求         |
| -------: | ------------ |
|   0～30 行 | 推荐范围         |
|  30～60 行 | 正常可接受        |
|  60～80 行 | 检查是否包含多个执行阶段 |
| 80～120 行 | 应优先拆分        |
| 超过 120 行 | 原则上必须重构      |
| 超过 200 行 | 视为严重复杂度预警    |

函数出现以下情况时，不论行数多少都应拆分：

* 同时负责校验、查询、决策、执行和持久化；
* 存在超过三层的嵌套条件；
* 包含多个相互独立的错误处理分支；
* 需要大量布尔参数控制行为；
* 修改一处容易破坏无关逻辑；
* 测试必须覆盖大量组合才能确认正确性。

推荐将长流程拆成明确步骤：

```text
validate_request
resolve_configuration
prepare_context
execute_action
verify_result
persist_result
publish_events
```

Agent Loop 主函数可以保留循环骨架，但不得把以下逻辑全部内嵌在循环中：

* 上下文组装；
* Provider 调用；
* Tool Call 解析；
* 权限审批；
* 工具执行；
* 重试；
* 状态持久化；
* 事件广播；
* 子 Agent 调度。

---

## 4. 结构体、类和实现块规范

### 4.1 字段数量

|    字段数量 | 判断               |
| ------: | ---------------- |
|  1～10 个 | 正常               |
| 11～15 个 | 检查状态职责           |
| 16～20 个 | 高概率需要拆分          |
| 超过 20 个 | 原则上视为 God Object |

以下字段应重点关注：

```rust
Arc<Mutex<HashMap<...>>>
Arc<RwLock<HashMap<...>>>
DashMap<...>
Sender<...>
Receiver<...>
CancellationToken
```

如果一个结构体持有多个独立共享状态集合，通常应将它们拆为独立状态组件。

例如：

```text
permission_waiters
assignment_waiters
task_outputs
engines
cancel_flags
tool_grants
allowlists
```

不应长期集中在一个 Runtime 对象中。

---

### 4.2 公开方法数量

|  公开方法数量 | 判断                 |
| ------: | ------------------ |
|  1～10 个 | 推荐范围               |
| 11～20 个 | 检查是否存在多个能力域        |
| 21～30 个 | 应考虑拆分 Facade 和内部服务 |
| 超过 30 个 | 原则上必须拆分            |

一个模块可提供统一 Facade，但 Facade 应只负责转发和编排，不应实现所有业务细节。

---

### 4.3 `impl` 规模

一个 Rust `impl` 块原则上应围绕同一个能力组织。

可以按语义拆分多个 `impl`：

```rust
impl RunManager {
    // 生命周期入口
}

impl RunManager {
    // 查询能力
}

impl RunManager {
    // 恢复能力
}
```

但如果这些能力拥有不同依赖和状态，应进一步拆为独立组件，而不是只拆 `impl` 块。

---

## 5. 模块拆分判断标准

一个文件或对象满足以下任意三项时，应进入拆分流程：

1. 无法用一句话说明职责；
2. 存在两个以上主要变化原因；
3. 持有多个生命周期不同的状态；
4. 包含两个以上独立业务域；
5. 公开接口超过 20 个；
6. 单元测试需要初始化大量无关依赖；
7. 出现大量跨层调用；
8. 出现多个共享可变集合；
9. 修改一个功能经常影响其他功能；
10. 文件内部依赖关系无法画成清晰单向图；
11. 需要大量章节注释区分内部职责；
12. 已经出现“所有功能都往这个文件加”的趋势。

---

## 6. 合理的拆分维度

模块应优先按照以下维度拆分。

### 6.1 按能力域拆分

```text
runtime/
permission/
interaction/
provider/
tools/
process/
subagent/
events/
storage/
```

### 6.2 按状态所有权拆分

例如：

```text
EngineRegistry
CancellationRegistry
InteractionBroker
ToolGrantStore
TaskOutputStore
```

每个组件只拥有一类核心可变状态。

### 6.3 按生命周期拆分

例如：

* Run 生命周期；
* Session 生命周期；
* Tool Call 生命周期；
* Process 生命周期；
* Permission 生命周期；
* Child Run 生命周期。

生命周期不同的状态不得为了方便全部放入同一个 Manager。

### 6.4 按接口边界拆分

例如：

```text
ModelProvider
ToolRuntime
RunRepository
EventPublisher
ProcessExecutor
CredentialResolver
```

核心业务代码只依赖接口，不依赖具体实现。

### 6.5 按协议或适配器拆分

例如：

```text
providers/
  openai.rs
  anthropic.rs
  compatible.rs

transport/
  uds.rs
  embedded.rs
  stdio.rs
```

供应商差异、传输差异和存储差异应位于适配层，不得渗透到 Agent Loop。

---

## 7. 项目分层规范

推荐采用以下依赖方向（与 `docs/standards/technical/01-layering.md` 的进程边界互补；此处强调**库/域内**分层）：

```text
UI / Host
    ↓
Application / RPC Handler
    ↓
Domain / Runtime Core
    ↓
Ports / Traits
    ↑
Infrastructure Adapters
```

允许的依赖方向：

```text
rpc → application
application → runtime
runtime → traits
adapters → traits
```

禁止的依赖方向：

```text
runtime → rpc
runtime → UI
agent-core → Tauri
domain → SQLite 具体实现
tool policy → 前端 Store
```

核心执行引擎不得依赖：

* Tauri Command；
* WebView；
* React/Vue Store；
* 具体数据库驱动；
* 具体模型供应商 SDK；
* 具体 RPC 协议。

---

## 8. 内部通信规范

### 8.1 直接接口调用

用于需要明确返回结果的操作：

```rust
let profile = project_analyzer.analyze(path).await?;
let decision = permission_policy.evaluate(request).await?;
```

### 8.2 领域事件

用于状态发生变化、需要多个订阅方获知的场景：

```text
RunStarted
RunStateChanged
ToolCallStarted
ToolCallCompleted
PermissionRequested
ProcessExited
RunCompleted
```

不得通过事件总线代替所有函数调用。

### 8.3 Repository

持久化必须通过 Repository 接口：

```rust
trait RunRepository {
    async fn save_transition(&self, transition: RunTransition) -> Result<()>;
}
```

核心模块不得直接拼接 SQL。

### 8.4 跨进程协议

以下场景才使用 RPC、IPC 或 stdio：

* Tauri Host 与 Daemon；
* Daemon 与外部 CLI；
* Daemon 与 MCP Server；
* 本地客户端与云端 Executor。

进程内部模块不得为了“解耦”而使用 HTTP 或 JSON-RPC 相互调用。

---

## 9. Agent 执行引擎专项规范

### 9.1 Agent 只负责决策

模型只能生成：

* 文本回复；
* 结构化 Tool Call；
* 任务计划；
* 状态建议；
* 子任务请求。

模型不得直接：

* 执行 Shell；
* 修改文件；
* 访问数据库；
* 更新 Run 状态；
* 写入权限授权；
* 创建绕过 Runtime 的子进程。

正确链路：

```text
模型提出 Action
→ Action Schema 校验
→ 权限策略判断
→ 用户审批或策略授权
→ Tool Runtime 执行
→ 结果结构化
→ 写入事件与会话
→ 返回模型
```

---

### 9.2 Agent Loop 必须保持轻量

Agent Loop 只负责：

1. 获取当前上下文；
2. 调用 Provider；
3. 解析模型响应；
4. 调度 Tool Runtime；
5. 接收 Observation；
6. 判断继续或结束；
7. 响应取消；
8. 产生领域事件。

以下能力必须独立：

```text
ContextAssembler
ProviderGateway
ToolRuntime
PermissionService
InteractionBroker
CancellationService
RunLifecycleService
EventPublisher
CompactionService
ChildRunCoordinator
```

---

### 9.3 Run 状态必须统一提交

禁止 AgentEngine、RunManager、数据库和 UI 分别修改状态。

推荐：

```text
AgentEngine
→ RunTransitionRequest
→ RunLifecycleService
→ 状态合法性校验
→ 持久化
→ 内存投影
→ 事件广播
```

状态提交失败时，不得先向 UI 广播成功状态。

---

### 9.4 取消机制必须统一

每个 Run 必须拥有统一的取消根节点：

```text
RunCancellationToken
├── Provider Stream
├── Tool Call
├── Shell Process
├── MCP Request
├── Child Run
└── External CLI
```

禁止不同模块各自维护互不关联的取消标志。

Run 被标记为 `Cancelled` 前，必须明确：

* 当前执行是否已停止；
* 子进程是否已清理；
* 子 Agent 是否已取消；
* Provider 流是否已关闭；
* 外部 CLI 是否已终止。

---

### 9.5 工具执行必须收敛

所有原生工具执行必须经过统一链路：

```text
Tool Registry
→ Schema Validation
→ Path Scope
→ Permission Policy
→ Approval
→ Execution
→ Audit
```

以下入口不得绕过统一链路：

* Scheduler；
* 子 Agent；
* UI；
* MCP；
* Skill；
* Extension；
* 自动修复；
* Replay；
* CLI Bridge。

外部 CLI 自带 Harness 时，必须明确标记为独立执行后端，不得宣称其工具调用自动经过原生 Capability Gateway。

---

### 9.6 重试必须有幂等控制

模型请求、Tool Call、Run Retry 和 Scheduler Trigger 必须有明确幂等标识。

禁止在无法确认工具是否已成功执行时直接重复执行具有副作用的操作。

高风险操作包括：

* 删除或覆盖文件；
* 安装依赖；
* Git push；
* 网络请求；
* 数据库迁移；
* 发送消息；
* MCP 写操作。

---

## 10. 超大模块专项整改规范

### 10.1 `ProductionRuntime` 类型模块

如果 Runtime 同时负责：

* Engine 注册；
* Provider 创建；
* 权限等待；
* Tool Grant；
* 子 Agent；
* Hook；
* 取消；
* 任务输出；
* 事件发布；

则应拆为：

```text
runtime/
  mod.rs
  production_runtime.rs
  engine_registry.rs
  cancellation_registry.rs
  interaction_broker.rs
  child_run_coordinator.rs
  tool_grant_service.rs
  hook_runtime.rs
  runtime_event_publisher.rs
```

`ProductionRuntime` 只保留：

* 组件组装；
* 生命周期初始化；
* 高层编排；
* 对外统一入口。

---

### 10.2 `RunManager` 类型模块

如果 RunManager 同时负责：

* Run 创建；
* 状态转移；
* SQLite；
* 恢复；
* 重试；
* 订阅；
* 事件回放；
* Engine 启动；

则应拆为：

```text
run/
  manager.rs
  lifecycle.rs
  registry.rs
  repository.rs
  recovery.rs
  retry.rs
  subscription.rs
  model.rs
```

RunManager 可作为 Facade，但不得继续持有全部实现逻辑。

---

### 10.3 `rpc.rs` 类型模块

RPC 层仅允许负责：

```text
鉴权
→ 参数解析
→ 调用 Application Service
→ 错误映射
→ 返回响应
```

不得在 RPC Handler 中：

* 创建 AgentEngine；
* 执行 Shell；
* 修改 Run 内部状态；
* 直接操作 SQLite；
* 直接调用 MCP 工具；
* 拼装复杂业务流程。

推荐：

```text
rpc/
  server.rs
  router.rs
  auth.rs
  error.rs
  handlers/
    run.rs
    conversation.rs
    permission.rs
    tool.rs
    mcp.rs
    scheduler.rs
```

---

## 11. 测试要求

模块完成拆分后必须能够独立测试。

### 11.1 必须可替换的依赖

以下依赖应通过 Trait 或测试替身注入：

* Provider；
* Tool Runtime；
* Repository；
* File System；
* Process Executor；
* Event Publisher；
* Clock；
* Credential Resolver；
* Permission Interaction；
* MCP Client。

### 11.2 关键测试覆盖

Agent 执行引擎至少需要：

* Run 状态合法转移测试；
* Agent Loop 结束条件测试；
* 最大轮次测试；
* Provider 超时测试；
* Tool Call 参数校验测试；
* 路径逃逸测试；
* 权限拒绝测试；
* 权限等待取消测试；
* 子进程树停止测试；
* Run 取消级联测试；
* 子 Agent 深度和并发限制测试；
* 事件 persist-first 测试；
* Daemon 重启恢复测试；
* Tool Call 幂等测试；
* Scheduler 重复触发测试；
* MCP 信任边界测试；
* 外部 CLI 终止测试。

拆分模块时，不得只移动文件而不补充对应测试。

---

## 12. 例外管理

代码超过规范限制但暂不拆分时，必须在代码或审计文档中记录：

```text
文件：
当前行数：
保留原因：
职责是否单一：
主要风险：
计划整改版本：
负责人：
```

允许暂缓拆分的理由包括：

* 当前文件为自动生成代码；
* 当前文件为纯协议声明；
* 正处于迁移阶段；
* 拆分会影响正在进行的关键版本；
* 已有明确的后续重构计划。

以下理由不能作为例外：

* 目前还能运行；
* 拆分比较麻烦；
* 所有逻辑都相关；
* 以后再说；
* 这是核心文件所以应该放在一起。

---

## 13. Code Review 检查清单

新增或修改模块时必须检查：

### 文件和函数

* [ ] 文件是否超过 500 行；
* [ ] 函数是否超过 80 行；
* [ ] 是否存在超过三层嵌套；
* [ ] 是否出现大量布尔参数；
* [ ] 是否存在无语义的 `utils/helpers/common`。

### 模块职责

* [ ] 能否用一句话说明模块职责；
* [ ] 是否只有一个主要变化原因；
* [ ] 是否只拥有一类核心状态；
* [ ] 是否直接访问其他模块内部字段；
* [ ] 是否绕过公开接口。

### 依赖关系

* [ ] 是否出现反向依赖；
* [ ] 核心层是否依赖具体基础设施；
* [ ] Provider 差异是否侵入 Agent Loop；
* [ ] UI 是否直接调用底层执行器；
* [ ] RPC 是否包含业务逻辑。

### Agent 安全

* [ ] 模型输出是否经过结构校验；
* [ ] Tool Call 是否经过权限判断；
* [ ] 路径是否经过 Scope 校验；
* [ ] Shell 是否使用结构化 executable + args；
* [ ] 是否支持超时和取消；
* [ ] 是否记录审计事件；
* [ ] 是否存在执行旁路。

### 状态和恢复

* [ ] 状态是否有唯一权威；
* [ ] 状态是否先持久化再广播；
* [ ] 取消是否真正终止底层任务；
* [ ] 应用重启后的语义是否明确；
* [ ] Retry 是否可能重复产生副作用。

---

## 14. Codex / AI 协作者执行要求

Codex、Claude Code 及其他 AI 协作者在新增、修改或重构代码时，必须遵守：

1. 修改前先识别目标模块的职责和状态所有权；
2. 不因减少文件行数而创建无意义文件；
3. 优先按能力域、状态域和生命周期拆分；
4. 保持现有公开协议兼容，除非任务明确要求修改；
5. 不得让 UI、RPC、Scheduler 或扩展绕过执行权威；
6. 拆分前确认调用关系和测试覆盖；
7. 拆分后更新模块导出、引用、测试和架构文档；
8. 不得同时进行无关格式化或大范围重命名；
9. 每次重构应保持可编译、可测试、可回滚；
10. 无法确认模块边界时，先输出分析，不得盲目拆分。

发现以下情况时，应主动提示需要重构：

```text
单文件超过 700 行
单函数超过 120 行
结构体字段超过 15 个
公开方法超过 20 个
同一对象持有多个共享可变状态
同一文件出现三个以上独立能力域
核心模块无法独立测试
多个模块可以修改同一状态
```

---

## 15. 最终原则

本项目统一遵循：

> **500 行开始审查，700 行进入警戒，1,000 行原则上必须拆分；真正的拆分依据是职责、状态、生命周期和变化原因，而不是行数本身。**

同时遵循：

> **模块代码相互独立，通过稳定内部接口协作；状态具有唯一归属，核心依赖指向抽象，扩展能力不得绕过执行主链。**
