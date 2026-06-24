# Natives — 项目术语表

> 本文件是 Natives 项目的领域词汇表。所有术语以用户（产品方）视角定义，不含实现细节。

## 核心概念

### Natives
AI 时代的桌面应用容器（"AI Steam Base"）。定位为类似 Steam 的生态基座，用户在其中浏览、安装、运行页面级插件，内置终端和环境管理。

### 模块（Module）
用户在 Natives 中安装和运行的功能单元。每个模块是一个完整的 HTML/JS/CSS 页面，从本地目录 `~/.natives/modules/` 发现和加载。等同于"插件"。

### 插件（Plugin）
同"模块"。在技术上下文中更强调其运行时形态（iframe 中加载的页面）。

### 基座（Host / Shell）
Natives 容器本身，提供三栏布局、终端、模块管理、环境注入等基础设施。区别于用户安装的"模块"。

### 创意工坊（Workshop）
内置的插件浏览和管理界面。类似 Steam 创意工坊，用户可以在这里发现、安装、管理模块。

### 助理记忆（Assistant Memory）
助理在对话过程中隐式积累的用户偏好、习惯与项目上下文档案。当前仅后台写入，不注入 system prompt、不影响输出风格。为未来升级到“贾维斯”式主动辅助形态预留的数据基底——届时才会被读取并参与上下文装配。

**定位边界**：区别于 CodePilot 的 buddy（用户可配的陪聊人格）。我们不引入 buddy 概念——当前助理定位是工具型非陪聊型（见“助理”条目），人格化形态是未来升级方向，不在当前领域模型内。

### 应用引导（App Onboarding）
基座/应用级别的首次使用引导流程，覆盖 provider 配置、workspace 选择、助理介绍等全链路。当前仅落地欢迎页（welcome），完整的多步骤引导流程后续独立设计。

**定位边界**：区别于 CodePilot 的 onboarding（助理层首次引导，注入 system prompt 指令）。我们的引导是应用级前置流程——用户进入助理前已完成引导，Context Assembler 不处理 onboarding 指令注入。

### 定时调度（Scheduled Task）
助理的定时任务调度能力——用户可配置「定时触发 agent 执行任务」（如每天检查项目健康、定期整理文件）。剥离了 CodePilot heartbeat 的人格化主动汇报语义（因我们不引入 buddy），只保留「定时触发 + 执行 + 落库」的纯调度骨架。归入执行引擎的 Task Scheduler 子系统。

**定位边界**：区别于 CodePilot 的 heartbeat（buddy 定时主动开口陪聊）。我们的定时调度是工具型能力，不涉及人格化输出；未来若激活 buddy 人格，可在调度触发后套用人格输出层，调度骨架无需重写。

### 助理（Assistant）
基座级别的 AI 对话工作台，位于侧边栏**一级菜单**（与模块管理平级）。不是模块/插件——不经过 iframe 沙箱加载，直接运行在基座的 Next.js 渲染层。具备完整的本地系统访问权限（文件系统、AST 分析、终端），是 CodePilot 风格的重型 AI 交互界面。为了保障宿主系统的特权完整，当用户切入助理界面时，当前运行的 iframe 插件挂起进入后台“温层”；当切回插件时，无缝重新显示，防止闪烁与状态丢失。

**UI 构成与交互**：
- **侧边栏入口**：左侧菜单栏固定的"助理"条目，点击切换中间内容区为助理工作台
- **会话管理**：项目作用域隔离（Project-scoped）。会话历史列表与当前打开的项目文件夹绑定，切换项目自动切换对应的会话历史。当无活跃项目（未打开任何文件夹）时，降级为「全局草稿会话」（`project_id = null`）：此时禁用依赖项目 AST 的能力（如 `/create-app` 的代码上下文提取），输入框给出明确提示「当前无项目上下文，部分能力受限」，引导用户先打开项目文件夹。
- **内容区**：上层历史消息流（MessageList），下层多功能输入框（MessageInput）。输入框支持 `/[指令] + 自然语言`。
- **Tool Call 可视化（三态气泡）​**：消息流中的工具调用以独立气泡渲染，统一三态语义——`pending`（执行中：工具名 + 参数摘要 + loading 骨架）、`result`（成功：可折叠展开原始返回 JSON）、`error`（失败：红色标识 + 错误摘要 + 重试按钮）。`/create-app`、`/modify-app` 等写盘类工具额外展示「写入路径 + contract_id（由 Rust 回填）」，让用户清晰感知副作用。
- **斜杠指令弹出框 (Slash Command Popover)**：在输入框输入 `/` 时，自动弹出浮动框显示全套系统指令（`/create-app`、`/modify-app`、`/list-apps`、`/uninstall-app`）以及当前已发现的 Skill 与 MCP 工具列表（附带图标和简要描述），支持自动补全。
- **模型切换**：输入框左下角嵌入一个类似 CodePilot 的模型选择下拉菜单，方便快速切换不同的大模型和供应商。

**供应商管理**：
- 在底座已有的设置（Settings）面板中增设专用的“AI 供应商 (AI Providers)”配置页。
- 支持添加不同的云端 Provider（OpenAI、Anthropic、Bedrock 等）和本地 LLM（Ollama 等），支持 BaseURL 设定、API Key 加密存储以及接口连接可用性测试。
- **密钥分层（Envelope Encryption）​**：API Key 密文（AES-256-GCM）落入 SQLite，但加密主密钥（KEK / Key Encryption Key）严禁与密文同库存放，必须托管于操作系统原生凭证库（macOS Keychain / Windows Credential Manager / Linux Secret Service）。Rust 后端启动时从 OS Keychain 取出 KEK 解密 DEK（Data Encryption Key），再解密具体 Provider 密钥；KEK 全程驻留 Rust 安全内存，杜绝「脱库即失守」。
- **测试边界**：连接可用性测试（如最小 `models.list` 探针）同样由 Rust 后端代理发起，前端仅接收脱敏后的成功/失败状态，禁止前端直接持有 Key 发起测试。


**执行边界**：为防范敏感凭证泄漏并收敛网络攻击面，AI 对话流（SSE、tool call、streaming）通过 Tauri IPC 由 Rust 后端执行反向代理（Proxy）。API Key 严格保留在 Rust 后端安全内存中，禁止下发或暂存于前端 Next.js 的 JS 内存。Host 层 CSP 的 `connect-src` 规则锁死在本地环回，杜绝任何外部未授权网络外流。模块创建指令（`/create-app` 等）落到 Rust 后端 `write_generated_module` 入口，受 KI-1 与 KI-3 强制拦截校验。

**流式与中断控制（Streaming & Abort）​**：
- **逐 token 下行**：Rust 后端通过 Tauri Event（`assistant://stream/{session_id}`）将上游 SSE 增量逐 token `emit` 至前端，前端按 `session_id` 订阅并增量渲染；禁止前端直连上游 SSE 端点。
- **中断信号链路**：输入框「停止生成」按钮触发 `cancel_stream(session_id)` 命令，Rust 后端立即 abort 上游连接并 `drop` 对应 task handle；已生成的部分内容标记为 `interrupted` 状态落盘，保证历史可追溯。
- **断线重连**：上游中途断开时，后端按指数退避重试（上限 3 次）；最终失败则向前端 emit `stream-error`，气泡降级为可重试错误态，不丢失已接收的 partial 内容。
- **背压保护**：emit 频率经节流（如 16ms 合批），避免高频 token 事件压垮前端渲染主线程。


**能力发现与规约机制**：采用静态规约与动态上下文融合的三层混合规约（Hybrid Specification Protocol）：
1. **静态基石 (Skill 规约)**：助理在启动时索引内置的 `Module Generation Skill` 描述符，以获知不可违背的 CSP 策略、UI 视觉令牌（Liquid Glass）与 Bridge 权限。
2. **动态感官 (MCP 服务)**：对话中接入本地 MCP 服务（Model Context Protocol），允许 LLM 动态获取当前项目 AST、可用 Vendored 第三方依赖库列表及本地运行时上下文。
3. **拦截检查 (Rust 门禁)**：LLM 动态生成的 SPA 代码必须在落盘前通过 Rust 的 `Contract Linter` 门禁（KI-3），过滤非安全标签（如远程脚本引用），重写依赖路径并拦截越权配置。

**数据持久化**：对话数据（会话 + 消息）存储在 SQLite 专用表（`assistant_sessions` + `assistant_messages`）。读写经由 Rust 端独立且非阻塞的异步通道，绝不与控制系统核心生命周期的 `KI-2` WAL 串行事务队列共用，保障聊天流的流畅性与系统级事务的严格隔离。

**助理 × 创意工坊（Workshop）联动规约**：
- **单一数据源（Single Source of Truth）​**：`/list-apps` 指令与创意工坊的模块列表必须共用同一个 Module Registry（`~/.natives/modules/` 扫描 + SQLite 注册表），杜绝双份状态。AI 生成模块写盘成功后，通过 `db-state-changed`（携带 `version + sequence_id`）通知 Workshop 增量刷新，无需用户手动重扫。
- **来源标记**：AI 生成模块在 `manifest.json` 中打 `source: "ai-generated"` 与生成它的 `session_id`，创意工坊以独立分组/角标展示，区分手动安装与 AI 自生成，并支持「跳回生成该模块的会话」。
- **发现机制**：底座通过 `fs_watch` 监听 `~/.natives/modules/` 目录变更（FSEvents），新模块落盘即触发 Registry 增量更新；同时保留 Workshop 手动刷新入口作为兜底。
- **生命周期一致性**：在 Workshop 中卸载 AI 生成模块，与 `/uninstall-app` 走同一条销毁链路（含 iframe 实例物理驱逐 + 数据保留确认），保证两个入口行为完全一致。

**上下文窗口管理（Context Window）​**：
- **预算分配（Token Budget）​**：每次请求前，Rust 后端按所选模型的上下文上限拆分预算——「系统规约（Skill 描述符）」15% +「历史消息」40% +「动态注入（AST/文件）」30% +「预留输出」15%，四块按优先级分配，总量逼近上限时从低优先级开始裁剪。
- **历史截断策略**：采用「滑动窗口 + 摘要」混合。保留最近 N 轮完整消息，更早的历史由后端异步压缩为摘要写入 session 元数据；原始消息不物理删除（会话回放仍可查看），仅在送入模型时以摘要替代。
- **压缩模型**：用户可在设置页指定一个「压缩模型」用于历史摘要压缩，默认使用当前会话主模型。
- **注入上限与降级**：单文件 / 单次 AST 注入设硬上限（如 64KB，可配置）。超限时降级为「符号级摘要」（仅注入函数/类型签名而非全文），并在注入内容头部标注来源路径与 `[truncated]` 标记，避免模型误判为完整文件。
- **计量与回写**：使用与目标模型匹配的 tokenizer 估算 token。provider 可自带 tokenizer 插件（精确计量），无则降级为启发式估算（chars/3.5）；tokenizer 插件可在设置页卸载。预算与实际消耗回写 UsagePanel（与「Agent 用量追踪」打通），让用户实时感知成本。

---

## 执行引擎（Execution Engine）

助理的 AI 对话执行核心。采用**三 runtime 双轨架构**（学 CodePilot），按用户本机已安装的 CLI 自动分流，无 CLI 时降级到自建引擎并引导安装。

### Runtime 抽象层（Runtime Abstraction）
顶层 `trait AgentRuntime`，三个实现，`stream_chat` 入口按可用性自动分流：

| Runtime | 实质 | 工具来源 | 上下文管理 | 适用场景 |
|---|---|---|---|---|
| **Claude CLI Runtime** | Rust 直接 spawn `claude` 二进制，stdin/stdout 管道通信 | Claude CLI 自带（Read/Grep 等）+ Rust 补的模块生成工具 | CLI 自管，Rust 透传 | 用户已装 Claude CLI |
| **Codex CLI Runtime** | Rust 调 Codex app-server JSON-RPC，订阅 notification 转事件 | Codex 自带 | Codex 自管，Rust 透传 | 用户已装 Codex CLI |
| **Native Runtime** | Rust 自建 agent loop（Vercel AI SDK 风格，落地为 Rust）| Rust 自建工具注册表 | Rust Context Assembler 自管 | 用户未装任何 CLI 时的降级方案 |

**分流逻辑**：`stream_chat` 入口先检测可用 runtime（按优先级 Claude CLI > Codex CLI > Native），选可用的走；无 CLI 时走 Native 并提示用户「未检测到 Claude/Codex CLI，已降级为内置引擎，建议安装以获得更好体验」。

**定位边界**：区别于 Q6 早期决策（不引入 runtime 层）。P3 双轨路线下 runtime 抽象是必须的——CLI 轨与 Native 轨的执行模型根本不同，必须由 trait 统一接口。这是对 Q6 R1 的推翻。

**安全红线**：三 runtime 均须经 Rust 后端代理，API Key 禁止下发前端（CONTEXT.md L40 不变）。CLI 子进程由 Rust spawn，管道通信不出 Rust 内存。

### CLI Runtime 子系统（Claude CLI + Codex CLI）

#### Claude CLI Runtime
Rust 直接 spawn `claude` 二进制（非 Node SDK，避免引入 Node 运行时依赖），stdin 喂 prompt，stdout 读 `--output-format stream-json` 流式输出。CLI 自带工具（Read/Grep 等）由 CLI 自管，模块生成工具由 Rust 注入。

**调用方式**：`tokio::process::Command` spawn `claude --print --output-format stream-json`，管道通信全程在 Rust 内存，符合 L40 安全红线。

#### Codex CLI Runtime
Rust 维护常驻 `codex app-server` 子进程，通过 JSON-RPC over stdio 通信。spawn `codex app-server` 后建立单例 client，用 `thread/start`、`thread/resume`、`turn/start` 等方法驱动会话，订阅 `agentMessage/delta`、`item/started`、`item/completed`、`turn/completed` 等 notification 转为 Tauri Event。Rust 负责子进程生命周期（崩溃重启、超时清理）。

**区别于 Claude CLI 的单次 spawn 模式**——Codex 是常驻会话模型，必须保活 app-server 进程。

#### CLI 写盘权限分流（路径白名单）
CLI runtime 下通用编程能力不削弱——CLI 原生 Edit/Bash 写非模块路径放行；写 `~/.natives/modules/` 路径必须走 Rust 注入的 `write_module` 工具（过 KI-1/KI-3 门禁）。

**实现机制**：
1. system prompt 注入指令引导 LLM「涉及模块生成必须用 write_module 工具」
2. Rust 侧 fsWatch 监听 modules 目录作兜底拦截——检测到非 write_module 的写入即告警/回滚
3. Codex 同理，app-server 的 exec 通过 approval bridge 拦截 modules 路径写入

**定位边界**：这是对 G1「全禁 CLI 写盘」的修正——全禁会削弱通用编程能力，改为路径分流精准守 KI。

#### CLI 上下文注入策略
CLI runtime 下不注入项目上下文（CLI 自己会读 CLAUDE.md/AGENTS.md/扫描项目），仅通过 `systemPrompt.append` 注入模块生成规约（CSP 约束 + KI 规则 + Liquid Glass 视觉令牌 + Bridge 协议，复用 `prompt-context-injector.ts`）。KI 门禁双保险：system prompt 事前引导 + write_module 工具事后拦截。

### Native Runtime 子系统（无 CLI 降级方案）

Native Runtime 是自建轨，含四个子模块——仅当用户无 CLI 时启用，CLI 轨不走这些。

#### 协议适配层（Protocol Adapter）
Native Runtime 内部的 LLM 上游协议抽象。当前落地 OpenAI 兼容协议，未来加新协议（Anthropic 原生 SSE、Ollama）只需新增 trait 实现。区别于顶层 Runtime 抽象——这层只管协议格式，不管执行模型。

#### 上下文装配器（Context Assembler）
仅 Native Runtime 使用。将多源上下文融合为送入 LLM 的 system prompt + messages，采用双轨混合模式：

1. **静态摘要打底**（静态基石层）——每次请求注入：
   - 项目文件树（目录结构 + 文件名）
   - 身份文件全文（`claude.md` / `soul.md` / `user.md` 等约定文件，有 PER_FILE_LIMIT）
   - 代码签名摘要（用 syntect 提取函数/类型签名，小众语言降级为正则提取）
2. **MCP 按需深查**（动态感官层）——LLM 通过 MCP 工具按需查询具体文件内容、项目记忆、会话历史等，不预载全部内容。

**索引机制**：fsWatch 监听项目目录变更，后台增量更新 AST 索引；索引存储于 `~/.natives/assistant-index/{project_hash}/`，按项目 hash 隔离，不污染用户项目目录。请求时直接读已建好的索引，无扫描延迟。

#### Agent Loop（执行回路）
仅 Native Runtime 使用。LLM ↔ 工具的循环执行回路：收 tool_calls → 执行 → 回填 tool_result → 下一轮，无 tool_calls 则结束。

**保护机制**（三 runtime 共用，CLI 轨由 Rust 外层套用）：
- **步数上限**：默认 50 步，可在设置页配置。超限即中断，防死循环。
- **Doom Loop 检测**：相同工具组合连续调用 3 次即判定 doom，emit error 事件中断 loop。固定阈值不可配置（安全机制非调参项）。
- **自愈熔断**：工具执行连续失败 >3 次熔断，停止自愈，将错误升级给用户。
- **进度心跳**：工具执行期间 emit 实际进度（如 `run_terminal` 透传 stdout 行），无进度工具降级为空心跳。

#### 工具注册表（Tools Registry）— Native Runtime 专用
Native Runtime 自建的内置工具集合。CLI 轨用 CLI 自带工具，不走此注册表。当前已有 6 个（`read_file` / `list_dir` / `write_file` / `write_module` / `run_terminal` / `lint_module`），补齐后含记忆检索/会话搜索/反问用户/视觉规约/通知调度/媒体导入等组。

**命名空间**：所有工具保持裸名（不加前缀），与现有 6 个工具风格一致。

### 模块生成工具注入（三 runtime 共用）
CLI 自带的通用工具（Read/Grep 等）由 CLI 提供，但**模块生成相关工具**（`write_module` / `lint_module`，含 KI-1/KI-3 门禁）是 Natives 专属，必须由 Rust 注入 CLI 作为自定义工具/MCP 服务。无论走哪个 runtime，模块生成工具都由 Rust 侧提供。

### 定时调度器（Task Scheduler）
见「定时调度」条目。归入执行引擎子系统，负责定时触发 agent 执行任务。三 runtime 共用——调度器触发的是 runtime 层的 `stream`，不关心具体走哪个 runtime。

**运行方式**：Rust 后端常驻一个 scheduler tokio task，10s 轮询 `scheduled_tasks` 表（存于 assistant DB，与 KI-2 WAL 隔离）找到期任务执行。失败按指数退避 `[30s, 1m, 5m, 15m]` 重试，连续 10 次失败熔断。

**结果呈现**：静默执行 + 落库（存 assistant_messages）+ 系统通知提醒。不打扰用户，用户下次进助理查看新消息——符合工具型定位，不引入人格化主动开口。

**配置入口**：设置页「定时任务」管理面板，用户 CRUD 任务（cron 表达式 + prompt + runtime 选择）。

---

### 模块生成技能（Module Generation Skill）
系统内置的特殊 Skill，描述 AI 生成新模块的完整规约。采用标准 SKILL.md 格式（YAML frontmatter + Markdown body），包含 9 个章节：

1. **Manifest Schema** — domain、schema_version、permissions 模板和约束（KI-1）
2. **Contract ID Rules** — contract_id 由 Rust 内核计算，AI 禁止生成。其仅基于 `domain` 和大版本号决定；HTML/CSS 逻辑细节微调作为“模块热更新”，不改变 `contract_id`，杜绝标识符爆炸与迁移死结（KI-1）
3. **Data Namespace Rules** — 数据写入 `module_data` 表，key 前缀 `{domain}:{schema_version}:`，跨版本共享需声明式 Migration（KI-2 + 数据长青性）
4. **CSP & Vendor Whitelist** — 可用 vendor 库清单（`tauri://assets/vendor/`），禁止外部 CDN（KI-5）
5. **Liquid Glass Visual** — 颜色、间距、圆角等设计令牌（设计约束）
6. **Permissions Declaration** — 生成模块必须在 `manifest.json` 的 `permissions` 中显式声明所需读取的系统变量或能力（例如 `env.read: ["GITHUB_TOKEN"]`）。用户在首次加载或安装时确认授权，且在运行时仅允许通过 `window.natives.env.get` 异步读取已授权变量，杜绝暗中扫描机密。
7. **Migration Limits** — 迁移必须是声明式的 JSON Mapping DSL（例如 `{"rename": {"old": "new"}, "default": {"status": "pending"}}`），严禁包含可执行代码（KI-3），且只能由 Rust 后端的 Migration Runner 串行解析执行，任何校验失败自动回滚。
8. **Lifecycle & Destruction** — 模块覆盖时旧数据保留（version 升级 via migration），卸载时用户确认是否保留数据
9. **create_module Tool Def** — tool 的 JSON schema（参数：name、html_content、permissions 等）

属于基座级 Skill，不放在用户 Skill 目录中。

**生成产出物**：单文件 HTML（`index.html` + `manifest.json`）。CSS/JS 内联在 HTML 中，或引用 `tauri://assets/vendor/` 下预置的库。底座维护一个可 CRUD 的预置库白名单。

**覆盖安全与实例销毁**：
- 覆盖写入检测：若 `/create-app` 或 `/modify-app` 所生成的模块在底座的“温层/热层（后台挂起/前台激活）”中已存在活跃的 iframe 实例，系统会拦截安装并弹窗二次确认：“检测到该应用正在运行，是否强制关闭并应用新版本？”。
- 实例清理：用户点击“确认”后，底座的前端 `IframeManager` 必须强制卸载（Unmount）该模块的 iframe 节点，清空所有事件监听器和挂起引用，完成彻底的物理驱逐与内存垃圾回收，然后再执行后续的代码写入和全新加载。

**应用修改与挂载时序**：
- 静默覆盖与重载（带 Diff 与回滚）：当用户输入 `/modify-app <名称> <意图>` 进行修改时，助理生成的代码变动通过 Tool Call 提交。**非破坏性修改**（仅 HTML/CSS/JS 逻辑微调，不改 `contract_id`、不涉及权限提升或数据迁移）可静默写入以保证流畅，但写入后须在聊天气泡中展示「变更 Diff（修改前/后）+ 一键回滚」入口，旧版本以快照保留，杜绝不可逆损坏。**破坏性修改**（schema_version 升级 / 权限扩张 / 声明式 Migration）必须先弹出二次确认，用户同意后方可提交。
- 严格挂载时序：Tauri 后端提交 SQLite WAL 事务后，广播携带 `version + sequence_id` 的 `db-state-changed` 注册表事件（满足 KI-4 最终一致性）。前端必须收到该事件并按 `sequence_id` 完成 Module Registry 状态调和后，才允许挂载 iframe 容器，防止 `SessionToken` 异步握手竞态失败；乱序到达的旧事件按 `sequence_id` 直接丢弃。
- 构建错误自愈（带熔断）：构建或 Linter 校验失败时，界面保持在助理聊天视窗，报错日志以折叠块在气泡中展示，并允许大模型感知错误自动重试自愈。**自愈设硬上限（≤ 3 次）​**：每次重试须携带上一轮的结构化报错上下文；达到上限仍失败则停止自愈、熔断退出，将最终错误升级给用户并提供「手动编辑 / 放弃」选项，严防无限重试导致的 token 消耗与死循环。完全成功后再滑入预览。


---

## 通信与安全

### Bridge API
插件与基座之间的通信接口，通过 `window.natives.*` 暴露。包含数据读写、主题获取、通知发送、生命周期管理等能力。

### Session Token
插件实例与基座之间的会话凭证。用于验证 postMessage 和 HTTP 请求的合法性。

**握手方式**：两阶段握手 — iframe 加载后主动向基座请求 token，基座验证后下发。插件重载时自动重新请求。详见 [[ADR-0001-session-token-handshake]]。

### 权限（Permissions）
插件通过 `manifest.json` 声明所需能力（如 `env.read`、`notification.send`）。用户安装时确认授权，运行时基座强制检查。

### 来源验证（Source Verification）
基座验证 postMessage 来自正确 iframe 的机制。因 sandbox iframe 的 origin 为 `"null"`，改用 `MessageEvent.source` 窗口引用匹配 — 基座创建 iframe 时保存 `contentWindow`，收到消息时检查 `event.source` 是否匹配。详见 [[ADR-0002-postmessage-origin-verification]]。

---

## 布局与界面

### 三栏布局
Natives 的主界面结构：左侧边栏（模块列表）+ 中间主内容区（iframe 容器）+ 右侧面板（工坊/设置）+ 底部终端。

### 主题（Theme）
Natives 的视觉风格系统。通过 `data-theme` 属性切换，所有颜色/间距/圆角通过 CSS 变量注入。内置三套主题：Terminal Volt（暗色）、Warm Archive（暖色亮色）、Editorial Index（编辑风格）。

### 终端（Terminal）
底部可折叠的完整 PTY 终端，支持 TUI 程序、窗口调整、多会话。启动时自动注入用户配置的环境变量。

---

## 环境与配置

### 环境配置（Env Profile）
一组环境变量的集合（如 API Key、代理地址）。用户可以创建多组配置（如"工作"、"个人"），终端启动时自动注入当前激活的配置。

### 凭证（Credentials）
存储在环境配置中的敏感信息（API Key 等），使用 AES-256-GCM 加密存储在 SQLite 中。

---

## 插件生命周期

### 心跳（Heartbeat）
插件定期向基座发送的存活信号（每 5s）。连续 3 次缺失（15s）标记为无响应，再 10s 无恢复标记为已崩溃。详见 [[ADR-0006-iframe-crash-detection]]。

### 状态分层（State Layers）
插件状态的保留策略：热层（当前可见，JS 内存保留）、温层（最近 5 个后台，隐藏但保留）、冷层（超出温层，销毁）、持久层（插件主动通过 `natives.db.set()` 保存）。详见 [[ADR-0005-plugin-state-preservation-strategy]]。

### 插件间通信（IPC）
插件之间的消息传递机制。因 sandbox 限制，所有消息经基座主进程中转，支持定向发送（`send`）和广播（`broadcast`）。详见 [[ADR-0003-plugin-ipc-main-process-relay]]。

---

## 设计哲学

### 不造轮子（No Domain Wheel Reinvention）
"完全不写"的适用范围是**插件层**：不自己写 AI 客户端、代码编辑器等，直接嵌入现有工具。**基座层必须自建**：容器本身就是轮子，没有现成替代品。详见 [[ADR-0007-domain-wheel-reinvention-clarification]]。

### 内核不变量（Kernel Invariants，KI-1 ~ KI-5）
微内核运行时的 5 条不可违背硬约束，焊死在 Rust 内核层，AI 与上层应用均不可绕过。详见 `docs/architecture/module-workshop-kernel-runtime.md`。

- **KI-1 Kernel-Owned Identity**：AI 生成模块的 `contract_id`/`module_id` 必须由 Rust 内核按 domain + schema_version + 内容指纹计算，AI 禁止生成标识符。控制点：`write_generated_module`。
- **KI-2 Serialized WAL**：物理文件写入 + SQLite 状态更新 + 契约版本变更三者绑进单线程 WAL 事务，崩溃原子回滚。控制点：`module_manager.rs`。
- **KI-3 Contract Enforcement Gate**：AI 生成代码落盘前必过 Contract Linter 门禁，过滤非安全标签、重写依赖路径、拦截越权配置。迁移契约严禁可执行代码，仅允许声明式 JSON Mapping DSL。控制点：`module_manager.rs`。
- **KI-4 Eventual Consistency**：跨沙箱事件必带 `version + sequence_id` 时序标签，消费端状态调和，免疫异步竞争。控制点：iframe-manager。
- **KI-5 Closed Supply Chain**：CSP 锁死 `script-src 'self' tauri://assets`，禁外部 CDN，第三方库全量本地 Vendored。控制点：iframe-manager。

---

## 文件管理

### 文件浏览器
基座内置的文件管理界面。支持网格视图（缩略图卡片）和列表视图（详细行），嵌入三栏布局的中间内容区。侧边栏提供快捷入口（桌面、文档、下载等）和收藏夹。

### 文件预览
右侧面板中的文件内容预览。支持 Markdown WYSIWYG 编辑（Milkdown）、HTML 沙箱实时渲染、代码语法高亮、图片/视频/音频/PDF 内联播放、压缩包内容列表。

### 文件搜索
Cmd+K 全局搜索的文件扩展。支持模糊文件名搜索（评分算法）和 `content:` 前缀全文搜索，范围可在当前文件夹和全盘之间切换。

### 原子写入
文件写入的安全机制：写入临时文件 → fsync → rename。防止并发编辑（如 Agent 和用户同时编辑同一文件）导致数据损坏。通过 mtime 冲突检测实现。

---

## AI 工作台

### Agent 变更监控
实时显示 Agent 对文件的修改。文件卡片在 Agent 写入时闪烁（强度与变更频率成正比），跟随模式自动跳转到 Agent 当前编辑的文件。

### 跟随模式
终端与文件浏览器的联动模式。终端跟随模式：切换目录时终端自动 cd。文件跟随模式：文件视图自动跳转到 Agent 当前编辑的文件。

### 变更收件箱
聚合本次会话中所有被 Agent 修改的文件列表，方便用户快速回顾工作成果。

### 会话回放
通过时间轴滑块逐步重放 Agent 触摸过的文件，帮助用户理解 Agent 的工作过程。

### 项目记忆
扫描 Claude Code 和 Codex 的会话日志（`~/.claude/projects/` 和 `~/.codex/sessions/`），显示任何项目文件夹的历史 AI 会话。支持恢复会话。

### Skills X-Ray
扫描本机所有 Agent Skills（5 个来源目录），显示触发统计、健康状态，支持启用/禁用/卸载。

### Agent 用量追踪
显示 Claude Code 的 5 小时窗口用量、周配额、本地 Token 统计，以及 Codex 的用量和计划类型。

### RTK 用量分析
显示 RTK（Rust Token Killer）的 Token 节省统计、命令历史、使用趋势。

### AI 文件整理
"AI 提议→人工审核→基座执行"的文件整理工作流。AI 只读取文件元数据（不读内容），每个建议附带理由，用户逐项审核后执行，支持一键撤销。

---

## 工具

### 发布向导
Node 项目的发布管理工具。检查 package.json、git 状态、CHANGELOG、gh CLI，支持一键版本号递增、CHANGELOG 更新、GitHub Release。

### 截图快递
监控系统截图目录，新截图时弹出浮动卡片，提供三个操作：发送到终端（作为 Agent 上下文）、保存到素材目录、打开标注编辑器。

### 图片标注
内置的图片编辑工具。支持画笔、箭头、文字、模糊/遮挡等标注操作，可进行格式转换和压缩。

### 飞入终端（Fling to Terminal）
选中文本后出现"Send to Terminal"按钮，点击后文本以飞行动画发送到终端，使用 bracketed paste 模式注入，终端面板发光反馈。

### 热度发光（Heat-based Glow）
文件卡片在 Agent 修改时显示与变更频率成正比的发光效果。`--heat` CSS 变量驱动 glow 强度，涟漪动画从图标中心扩散，左上角显示变更计数角标。

### 骨架屏（Skeleton Loading）
数据加载时显示的灰色脉冲占位符，替代"Loading..."文本。支持文本行、卡片、头像、表格四种变体，提升感知性能。

### 键盘快捷键帮助
Cmd+/ 打开的快捷键面板，展示所有可用键盘快捷键。毛玻璃背景，kbd 标签样式。

### Toast 通知
全局共享的通知系统，支持 info/success/error/warning 四种类型，自动 3s 消失，底部右侧定位。

### 噪音过滤
变更收件箱和跟随模式自动排除系统文件（.git、node_modules、.next、dist 等），避免干扰。变更收件箱支持去重计数（同一文件多次变更显示 ×N）。

### 导航历史
文件浏览器支持 Cmd+[ / Cmd+] 后退/前进，维护历史栈，工具栏显示 ← → 按钮。

---

## 版本与兼容性

### 语义化版本（SemVer）
Natives 和插件都遵循语义化版本：主版本.次版本.修订号。主版本号变更表示不兼容的 API 变更，次版本号表示向后兼容的功能新增，修订号表示向后兼容的问题修正。

### minNativesVersion
插件在 `manifest.json` 中声明的最低基座版本要求。主版本号不同则阻止加载，次版本号不同则警告但允许运行。

### 插件更新（Plugin Update）
数据保护式更新流程：备份 module_data → 替换插件文件 → 保留 module_data → 重新加载。用户手动触发，不自动更新。

