# 产品架构 02 · 功能治理与无假数据红线

> **版本**: 1.2.0 · **日期**: 2026-07-26  
> **关联 ADR**: [ADR-0012](../../adr/0012-product-identity-workshop-scope.md)、[ADR-0013](../../adr/0013-creative-app-dual-source.md)、[ADR-0014](../../adr/0014-creative-app-creator-workbench.md)  
> **关联源文件**: `src/lib/error-classifier.ts`、`src/components/ui/EmptyState.tsx`、各功能组件  
> **承接**: 根 `CLAUDE.md`「No fake data」；能力广告 ⊆ 可调（引擎契约）

---

## 一、本篇要约束什么

「功能怎么长出来」需要统一的纪律，否则会沦为各写各的。本篇约束三件事：**功能优先级如何标注**、**用户可见字段必须有真实来源（无假数据红线）**、**空/错/加载态如何处理**。其中「无假数据」是 Natives 的核心信任底线，单列一节。

---

## 二、功能优先级标注

#### R-F1 · 功能必须有优先级标签
- **等级**：SHOULD
- **分类**：命名
- **规则**：每个功能（User Story、组件、IPC/协议接口）**应该**标注优先级 `P0` / `P1` / `P2`，语义如下：

| 标签 | 含义 | 丢掉的后果 |
|------|------|-----------|
| `P0` | 必备。构成产品最小可用闭环。 | 产品不可发布。 |
| `P1` | 重要。显著提升体验，但有替代路径。 | 体验明显下降，但可用。 |
| `P2` | 锦上添花。 | 可延后，不影响核心。 |

- **为什么**：与 ADR-0012 的 P0/P1/P2 阶段语义对齐，统一排期与回归范围。
- **检查方法**：新增 User Story / 大功能时在 PR 描述中带上 `P?`，且不与 ADR-0012 阶段「必须不」冲突。

---

## 三、🔴 无假数据红线（核心信任约束）

这是 Natives 最严肃的产品约束，**违反即视为缺陷**，详见 `CLAUDE.md` 的「Anti-fake data principle」。

#### R-F2 · 用户可见字段必须有真实来源
- **等级**：MUST
- **分类**：无假数据
- **规则**：任何**用户在界面上能看到的数值/状态/文本**，**必须**有可追溯的真实数据来源。禁止：
  1. 写死的占位数字（如 `usage: 0`、`count: 42`、`progress: 100`）。
  2. 凭空捏造的「示例」条目（如永远不存在的模块名、假的文件列表）。
  3. 用 `Math.random()` 或时间戳伪造的「看起来在动」的指标。
- **正例**：Agent 用量面板显示 Claude Code 的 5h 窗口用量 → 来源是真实扫描 `~/.claude/` 的会话日志。
- **反例**：磁盘用量卡片暂时没接好，先填 `used: 128GB, total: 512GB` 让 UI「不空」→ 违反。正确做法是显示加载态或「暂无数据」。
- **为什么**：用户用 Natives 管理的是自己的真实环境（文件、凭证、AI 用量）。假数据会误导决策，直接破坏信任，且很难事后被发现。
- **检查方法**：
  - review 时对每个可见数值追问「这个值从哪来？」答不上来即不合规。
  - 暂时无法接真实数据时，**必须**显示空态/加载态（见 R-F4），而不是编一个值。

#### R-F3 · 估算值必须显式标注
- **等级**：MUST
- **分类**：无假数据
- **规则**：若某字段是**估算**而非精确值（如本地 token 统计、近似磁盘占用），**必须**在 UI 上标注其为估算（文案如「约」「估算」），或隐藏该字段。**禁止**把估算当精确值展示。
- **为什么**：估算合理，但伪装成精确值等同于欺骗。
- **检查方法**：文案中是否体现不确定性。

#### R-F4 · 数据缺失时显示空态，而非假数据
- **等级**：MUST
- **分类**：无假数据、交互
- **规则**：当真实数据未加载、为空、或不可用时，**必须**渲染统一的空态组件（`src/components/ui/EmptyState.tsx`）或加载态。**禁止**用假数据「撑场面」。
- **正例**：`<EmptyState title="还没有通知" hint="模块的消息会出现在这里" />`
- **反例**：通知列表为空时，塞三条假通知让截图好看 → 违反。
- **为什么**：空态是诚实的产品信号，也是引导用户下一步行动的机会。
- **检查方法**：所有列表/面板组件都**必须**处理 `空` 与 `加载中` 两个状态。

---

## 四、错误展示纪律

错误分类逻辑集中在 `src/lib/error-classifier.ts`（12 个类别 + userMessage/actionHint/retryable）。前端展示**必须**走这套分类，不得各自手写错误文案。

#### R-F5 · 错误必须经过分类器
- **等级**：MUST
- **分类**：状态、无假数据
- **规则**：面向用户的错误**必须**通过 `classifyError()` 处理，使用其返回的 `userMessage` + `actionHint`。**禁止**把原始异常字符串（含堆栈、英文技术细节）直接展示给用户。
- **正例**：`showErrorToast(err, moduleId)` → 内部调用 `classifyError`。
- **反例**：`alert(err.message)` 把 `SQLITE_CONSTRAINT: FOREIGN KEY...` 直接弹给用户 → 违反。
- **为什么**：统一错误分类保证用户看到的是「能做什么」而非「系统哪里坏了」，也避免泄露内部实现。
- **检查方法**：搜代码中是否还有直接展示 `error.message` / `String(err)` 给用户的地方。
- **延伸**：需要新增错误类别时，先扩 `error-classifier.ts` 的 `ErrorCategory` 与 `ERROR_META`，再在 UI 使用。

#### R-F6 · 可重试错误必须提供重试入口
- **等级**：SHOULD
- **分类**：交互
- **规则**：当 `ClassifiedError.retryable === true` 时，**应该**在错误展示处提供「重试」按钮；不可重试错误**应该**说明原因而非留一个无效按钮。
- **为什么**：分类器已经算出了 `retryable`，不用就是浪费。
- **检查方法**：错误页/错误 toast 是否随 `retryable` 切换按钮。

---

## 五、功能树（按产品维度）

以下按产品维度梳理所有功能。每个功能标注优先级（`P0`/`P1`/`P2`）和所属组件。各域按 ADR-0012 的三面（Hub / Workshop / Embed）归类，标注在域标题后（【Hub】【Workshop】【Embed】【全局】）；G–J 为按代码现状补录的新域，每个条目均可追溯到真实源文件或已冻结的 ADR 设计（后者显式标注状态，遵守 R-F2 无假数据红线）。

### A. 文件管理（File Manager）【Hub】— P0

```
文件管理
├── 文件浏览（File Browser）          P0
│   ├── 目录导航（面包屑、前进/后退）  P0
│   ├── 视图切换（网格 / 列表）        P0
│   ├── 网格视图：
│   │   ├── 文件图标（扩展名徽章 + 图标）       P0
│   │   ├── 图片/视频缩略图                    P0
│   │   ├── 视频播放角标                       P0
│   │   └── 项目类型角标（node/web/py/rs/go/git）P1 ← 已移植自 Natives2
│   ├── 列表视图：
│   │   ├── 文件名 + 格式化时间 + 大小          P0
│   │   ├── 隐藏文件标识                       P0
│   │   └── 项目类型标签（行内）                P1
│   ├── 收藏功能（星标）                       P1
│   └── 文件高亮动画（flash + 呼吸边框）       P1
│
├── 文件操作                                P0
│   ├── 单击选中 + 双击打开                   P0
│   ├── 右键上下文菜单                       P0
│   │   ├── 打开 / 预览 / 编辑               P0
│   │   ├── 在终端打开 / 磁盘用量             P0
│   │   ├── 复制路径                         P0
│   │   ├── 收藏 / 取消收藏                   P1
│   │   ├── 重命名 / 新建文件 / 新建文件夹     P1
│   │   └── 移到废纸篓                       P0
│   ├── 拖拽文件（drag & drop）              P1
│   └── 快捷键支持（重命名、删除等）           P1
│
├── 变更追踪                                P1
│   ├── 实时文件变更监听（file-flash 事件）   P1
│   ├── 变更角标（"改·N"，热度呼吸）          P1
│   └── 变更涟漪动画（editRipple）           P2
│
├── 项目自动识别（project badge）            P1
│   ├── Node（package.json）                P1
│   ├── Web（index.html）                   P1
│   ├── Python（requirements.txt/setup.py） P1
│   ├── Rust（Cargo.toml）                  P1
│   ├── Go（go.mod）                        P1
│   └── Git（.git 目录）                    P1
│
└── 搜索与快速定位                           P1
    ├── ⌘K 全局搜索                         P1
    └── 命令面板（cmd palette）              P2
```

### B. 终端管理（Terminal）【Hub】— P0

```
终端管理
├── 多 Tab 终端                              P0
│   ├── 创建/关闭终端会话                     P0
│   ├── 终端 Tab 切换                         P0
│   ├── Tab 上下文颜色（按项目配对）            P1
│   └── 终端跟随（follow mode）               P1
├── 终端集成                                  P0
│   ├── 在终端打开目录                        P0
│   ├── 文件导航 → 终端 cd 同步               P1
│   └── 终端 → 文件浏览器定位                 P1
└── 终端 UI                                   P1
    ├── 主题适应（终端核/暖色/编辑式）          P1
    └── 字体配置（Nerd Font 优先）             P1
```

### C. Settings 设置【全局】— P0

```
设置（Settings）
├── 主题与样式                                P0
│   ├── 三皮肤切换（terminal / warm / editorial） P0
│   ├── 语言切换（zh-CN / en）                P0
│   └── 液态玻璃视觉微调（模糊/光泽/配色）      P1
├── 供应商管理（AI Provider）                  P0
│   ├── 预设供应商库（38 家 + 中英双语）        P0
│   ├── 多 API Key 管理                       P1
│   └── 自定义供应商                           P1
├── 布局设置                                  P1
│   ├── 侧边栏宽度                            P1
│   ├── 面板宽度                              P1
│   └── 终端高度                              P1
└── 关于                                      P1
    ├── 版本信息                              P1
    └── 系统信息                              P1
```

### D. Dashboard 仪表盘【Hub】— P1

```
Dashboard
├── Token 用量概览（TokenHero）               P1
├── 用量趋势图表                              P2
├── Skills 面板（Claude Code / Codex）        P1
├── 模型统计表                                P2
├── 磁盘用量                                  P2
└── Kanban 看板                               P2
```

### E. 预览与查看（Preview）【Hub】— P1

```
预览与查看
├── 代码预览（高亮 + 跟随）                    P1
├── Markdown 实时渲染                         P1
├── 图片/视频预览                             P1
├── PDF 预览                                  P2
└── 文件跟随（follow mode + 产物卡片）         P1
```

### F. 安全与信任【全局】— P0

```
安全与信任
├── 凭证加密（AES-256-GCM / ChaCha20-Poly1305） P0
├── 插件 iframe 沙箱（allow-scripts allow-forms） P0
├── 权限审核日志                               P1
└── 最小权限原则（capabilities 白名单）         P0
```

### G. 助理与 Agent 引擎（Assistant / Native Engine）【Hub】— P0

主要依据：`src/components/assistant/`、`src-agent-daemon/src/`、`src/components/settings/EngineCapabilitiesPanel.tsx`；能力权威清单见 `crates/assistant-protocol/src/v2/methods.rs`（`IMPLEMENTED_METHODS`）。

```
助理与 Agent 引擎
├── 会话与工作台                                        P0
│   ├── 三栏工作台（时间线 + 输入区 + 活动面板可拖拽）     P0 ← AssistantWorkbench.tsx
│   ├── 多会话管理（列表/分页/重命名/归档/删除/Fork）      P0 ← AssistantWorkspaceContext.tsx、AssistantSidebarSection.tsx
│   ├── 项目维度分组（项目重命名/移除）                    P1
│   ├── 会话模式（chat / agent / goal）                  P0
│   └── 每会话草稿持久化（防抖 + IME 保护）               P1 ← MessageInput.tsx
├── 输入区（Composer）                                  P0 ← MessageInput.tsx
│   ├── 发送 / 运行中排队 / ⌘Enter 插话（interject）      P0
│   ├── 权限档位切换（只读 / 询问 / 完全访问）             P0
│   ├── 模型与供应商选择器                                P0 ← ModelSelectorDropdown.tsx
│   ├── 文件附件 + @ 文件提及                             P1 ← FileMentionPopover.tsx
│   ├── 历史提问回溯（上下键，按项目隔离）                 P2
│   └── 斜杠命令弹层（交互完整；命令列表当前为空，显示诚实空态） P2 ← SlashCommandPopover.tsx、src/lib/assistant-slash.ts
├── 时间线与内容块渲染                                   P0 ← ConversationTimeline.tsx、blocks/index.tsx
│   ├── 17 种结构化内容块（text/reasoning/tool_call/diff/plan/subagent 等） P0
│   ├── Markdown 渲染 + XSS 安全过滤                     P0 ← MarkdownText.tsx、markdown-safety.ts
│   ├── 思考过程展示（折叠/耗时计时/工具调用分组）         P1 ← ThinkingActivity.tsx
│   ├── 内联 Diff（hunk + diffstat + 在 Monaco 打开）    P1 ← DiffViewer.tsx
│   └── 历史消息分页（加载更早）                          P1
├── 检查点与回滚                                         P0 ← src-agent-daemon/src/checkpoint.rs
│   ├── Run 级文件快照（写前/写后捕获、终态固化）           P0
│   ├── 回滚预演（冲突清单；有冲突 fail-closed 拒绝撤销）   P0
│   ├── 会话级「撤销全部文件改动」卡片                     P1 ← ConversationTimeline.tsx
│   └── 对话级回退 run.rewind（后端已实现，前端未接线）    P2
├── 权限审批与用户交互                                   P0
│   ├── 权限请求卡片（once/this_run/session/project 四种批准作用域） P0 ← PermissionRequestCard.tsx
│   ├── 审批期间接管输入区（防绕过）                       P0 ← AssistantWorkbench.tsx
│   ├── AskUser 提问卡片                                 P1 ← AskUserPromptCard.tsx
│   └── 计划审批卡片（planMarkdown 批准/拒绝）             P1
├── 子代理分派（Subagent）                               P1 ← SubagentAssignmentModal.tsx、subagent_store.rs
│   ├── 分派确认弹窗（default/random/custom + 10s 倒计时） P1
│   ├── 仅有效 Key 进池 + 失败自动换 Key 判定              P1
│   ├── 运行中换 Key / 换路由（subagent.switchRoute）     P1
│   ├── 隐藏子会话 + 父子会话跳转                          P1
│   └── 父心跳 + 孤儿子代理回收                            P1
├── 提示队列（Prompt Queue）                             P1 ← PromptQueuePanel.tsx、prompt_queue_store.rs
│   ├── 队列面板（编辑/删除/拖拽排序/立即发送）             P1
│   └── 安全点调度 + 守护进程重启后恢复                    P1
├── Goal 模式                                            P1 ← GoalStatusBar.tsx
│   ├── 状态条（运行状态映射/实时耗时/token 用量）          P1
│   └── 暂停 / 继续 / 删除                                P1
├── 活动面板（六页签）                                    P1 ← ActivityInspector.tsx
│   ├── 运行 / 任务 / 产物 / 上下文用量                    P1
│   ├── 审计页签：Git 分支/变更列表/逐行 diff/Commit/Push   P1
│   └── 事件页签（仅开发者模式）                           P2
├── 命令面板与快捷键（⌘⇧P，含 Fork/停止/重试等）           P1 ← CommandPalette.tsx
├── 连接与恢复                                            P0
│   ├── 连接横幅（7 种状态，浮层不挤压布局）               P0 ← ConnectionBanner.tsx
│   ├── 引擎恢复整页（致命/协议不兼容时阻断，不假装可用）   P0 ← EngineRecoveryPage.tsx
│   ├── 前端能力门控（未广播的 RPC 不渲染入口）             P0 ← src/lib/assistant-workspace/capability-gate.ts
│   └── 一键复制诊断信息                                   P1
├── 运行编排（daemon）                                    P0 ← run_manager.rs、authority.rs
│   ├── Run 生命周期（幂等键去重/快照恢复/级联取消）        P0
│   ├── 执行权威（生产走 UDS，禁止静默降级进程内）          P0
│   └── Provider 请求限流（RPM + 429 冷却）                P1 ← governor.rs
├── 运行时桥接                                            P0
│   ├── Native 引擎（默认执行路径）                        P0 ← production.rs
│   ├── Claude Code CLI 桥接（宿主中介权限；默认 fail-closed 只读） P1 ← cli_runtime_bridge.rs
│   └── Codex CLI 桥接（Stub，恒不可用，选中即降级 Native） P2 ← codex_runtime_bridge.rs
├── 能力子系统（daemon）
│   ├── MCP 运行时（stdio/HTTP/SSE、信任门控；mcp.call 有意禁用、OAuth 流未实现） P1 ← mcp_runtime.rs
│   ├── Skills（项目/用户级扫描、信任 + 启用注入；前端仅只读列表） P1 ← skill_store.rs
│   ├── 定时任务（后端 CRUD + 到期真实拉起 Run；前端仅只读列表） P1 ← scheduler_store.rs
│   ├── 记忆 Memory（关键词检索，无 embedding；暂无用户界面） P2 ← memory_store.rs
│   ├── 扩展 Extension（发现/信任/启停；无安装更新，前端只读） P2 ← extension_store.rs
│   └── 后台任务存储（task.list / wait / cancel）           P1 ← task_store.rs
└── 引擎设置页
    ├── 执行引擎面板（三引擎选择/五态状态解释/偏好漂移警告/工具开关/自愈熔断；能力矩阵为静态说明文案，非实时探测） P0 ← RuntimePanel.tsx
    └── 引擎能力面板（限流设置可写；MCP/调度/扩展/Skills 只读列表） P1 ← EngineCapabilitiesPanel.tsx
```

### H. 个人创意工坊（Workshop，三来源）【Workshop / Embed】— P0

主要依据：`src-tauri/src/creative_app/`、`src-tauri/src/module_manager.rs`、`src-tauri/src/contract_linter.rs`、`src-tauri/src/http_server.rs`、`src/components/shell/WorkshopPage.tsx`；设计冻结见 ADR-0013、ADR-0014。三来源中 internal web-module 属 Workshop 面（Unique Origin iframe + Bridge）；GitHub 容器与本地项目以子 WebView 打开，属 Embed 面。

```
个人创意工坊（三来源：internal web-module / GitHub 容器 / 本地项目）
├── 统一目录与生命周期                                   P0 ← creative_app/adapters/mod.rs、service.rs
│   ├── 三来源合并列表（运行中优先排序、来源徽章）         P0
│   ├── 统一启动/停止/删除/重启（internal 不支持重启）     P0
│   ├── 12 态状态机 + 非法迁移拒绝 + 孤儿瞬态收敛          P0 ← state_machine.rs
│   ├── 启动对账（以 Docker/本地进程实况为权威）           P0 ← install.rs、local/lifecycle.rs
│   ├── 全局变更互斥锁（同时仅一个安装/生命周期写）        P1
│   └── 目录事件驱动刷新（db-state-changed）              P0 ← src/hooks/useCreativeAppCatalog.ts
├── 来源一：内部 web-module【Workshop】                  P0 ← module_manager.rs
│   ├── manifest 校验 + 目录扫描 + 同步入库               P0
│   ├── 安装（目录/.zip/拖拽）/ 更新（失败自动回滚）/ 卸载 / 启停 P0
│   ├── 权限授权（导入逐项勾选、撤销、审计日志）           P0 ← commands/module.rs
│   ├── AI 生成模块原子写盘 + 热上架（temp→fsync→rename） P0 ← write_generated_module
│   ├── 契约门禁（写盘前强制 lint_html + lint_manifest）  P0 ← contract_linter.rs
│   ├── 内核持有 contract_id（SHA256 计算，拒绝 AI 传入）  P0
│   └── 生成内容一键回滚（rollback_module_html）          P1
├── web-module 运行时沙箱【Workshop】                    P0 ← http_server.rs
│   ├── 本地 HTTP 静态服务（/modules、/local-projects）   P0
│   ├── Unique Origin iframe（无 allow-same-origin + 运行时断言） P0 ← src/lib/iframe-manager.ts
│   ├── Host/Origin 校验 + CSP + 路径穿越防护             P0
│   ├── Session Token 握手 + 心跳                         P0 ← token_manager.rs、iframe-sandbox-manager.ts
│   └── Bridge SDK（db/settings/lifecycle/meta）          P0 ← bridge_sdk.js
├── 来源二：GitHub 容器【Embed】                         P1 ← github.rs、probe.rs、docker.rs
│   ├── Release 解析与容器资产探测（natives.app.json / compose） P1
│   ├── 安装向导（一键 / 手动选 tag / Token 三态 / 实时进度） P1 ← WorkshopPage.tsx
│   ├── Compose 端口重写仅绑 127.0.0.1 + 安全解压         P0
│   ├── Docker CLI 编排（argv 数组、无 shell）            P1
│   ├── 容器日志查看                                      P1
│   └── 可复用子 WebView 打开（仅 loopback URL 白名单）    P1 ← browser.rs
├── 来源三：本地项目【Embed】                            P1 ← creative_app/local/
│   ├── 项目扫描 + LaunchPlan 生成/校验（拒绝 shell 元字符） P1 ← scan.rs、plan.rs
│   ├── 本地项目向导（四步；AI 启动建议先预览脱敏载荷再确认） P1
│   ├── 本地进程守护（进程组/树杀/孤儿处置）               P1 ← runtime.rs、lifecycle.rs
│   ├── 滚动文件日志 + 双重脱敏                            P1 ← logs.rs
│   ├── 白名单依赖安装（先预览确切 argv 再执行）           P1 ← deps.rs
│   ├── 加密 env 持久化                                   P1 ← store.rs
│   └── AI 故障诊断（启动失败时）                          P2 ← ai.rs
├── 工坊页面（WorkshopPage）                             P0 ← WorkshopPage.tsx
│   ├── 作品卡片列表 + 四入口添加菜单 + 拖拽 .zip 安装     P0
│   ├── 内置浏览器视图（前进/后退/刷新/重启/复制 URL）     P1
│   ├── 删除确认（按来源附 Docker 卷/镜像选项）            P1
│   └── 模板创建对话框（硬编码占位 HTML；ADR-0014 判定为假创作入口，待创作台替换） P2
├── 创作台（Creator Workbench）（设计冻结，未实施）       P0 ← ADR-0014、docs/architecture/creative-app-creator-workbench.md
│   ├── P0 主流程：描述想法→生成草稿→沙箱预览→对话修改→确认发布→继续迭代 （设计冻结）
│   ├── 草稿存储（creative_drafts v10 迁移 + 修订文件）    （设计冻结）
│   ├── 受限草稿工具四件套 + 创作会话工具 allowlist        （设计冻结）
│   ├── publish_creative_draft 唯一发布门禁（汇入 write_generated_module） （设计冻结）
│   ├── 草稿沙箱预览路由 /drafts/{draftId}                （设计冻结）
│   └── 「继续创作」反向路径（以已发布模块为种子开新草稿）  （设计冻结）
└── 发布向导（Release Wizard）                           P2 ← release_wizard.rs
    ├── 后端：就绪检查/版本写入/步骤序列/执行              P2 （后端就绪，前端零调用点）
    └── 前端对话框为模拟进度占位（违反 R-F2，待整改或下架） P2 ← src/components/release/ReleaseWizardDialog.tsx
```

### I. Hub 环境与凭证注入【Hub】— P0

主要依据：`src-tauri/src/env_manager.rs`、`credential_broker.rs`、`key_lease.rs`、`provider_key_manager.rs`。与 F 域分工：F 管加密算法与沙箱红线，本域管环境组、注入、租约、分发等功能层。

```
Hub 环境与凭证注入
├── 环境组（Env Profile）                                P0 ← env_manager.rs、commands/env.rs
│   ├── 多环境组 CRUD + 默认组单选互斥（后端与 IPC 就绪；暂无管理界面） P0
│   ├── 变量加密落库 + 旧密文透明升级迁移                  P0
│   └── 环境组变更实时广播（db_state_changed）             P1
├── 终端环境注入                                          P0 ← commands/terminal.rs
│   ├── 新建终端按 Profile 注入（未指定回落默认组）         P0
│   ├── 注入不覆盖调用方已有 key                           P0
│   └── 终端工具栏 Profile 切换器（预选默认组）             P0 ← Terminal.tsx
├── 凭证代理（Credential Broker）                        P0 ← credential_broker.rs、production_credentials.rs
│   ├── Daemon 按需索取单条凭证（永不批量下发）             P0
│   ├── 无可用 key 时 fail-closed（禁止伪造/离线 mock key） P0
│   ├── key_id 省略时回落主 key / 最近激活 key             P1
│   ├── 全局出站代理随凭证下发（代理 URL 加密存储）         P1
│   ├── 错误脱敏 + 内存凭证 Drop 零化                      P0
│   └── Embedded / UDS 双 Authority 模式                  P0 ← daemon_authority.rs
├── 密钥租约（Key Lease）                                P1 ← key_lease.rs
│   ├── 短时租约元数据（TTL 120s、绑定 run_id、拒绝无 run 租借） P0 ← assistant-protocol v2 credential.rs
│   ├── 子代理副 key 独占租借（事务防抢占 + 公平轮转）      P1
│   ├── 主 key 降级兜底（每次运行仅一次，标记 fallback）    P1
│   ├── 运行终态统一释放（无超时自动回收——已知缺口）        P1
│   └── key 失效标记回写（test_status + 脱敏错误文本）     P1
└── Provider Key 管理（信封加密封装层）                   P0 ← provider_key_manager.rs、commands/provider.rs
    ├── KEK 自动生成持久化 + 每 key 独立 DEK               P0
    ├── 前端仅见掩码，完整 key 不出 IPC                    P0
    ├── 主 Key 唯一性 + 设主前置校验（须测试通过）          P1
    ├── 子代理 Provider/Key/Model 三级绑定弹窗             P1 ← SubagentAssignmentModal.tsx
    └── 运行记录展示所用 key 标签（含 fallback 标识）       P1
```

### J. Provider 路由与账号池（Sub2API）【Hub】— P1

主要依据：`src-tauri/src/provider_accounts.rs`、`src-agent-daemon/src/routing.rs`、`src-agent-daemon/src/loopback.rs`、`src/components/settings/provider-routing/`；设计见 `docs/architecture/provider-routing-sub2api.md`。

```
Provider 路由与账号池
├── 路由设置 UI                                          P1 ← src/components/settings/provider-routing/
│   ├── 供应商设置三页签（管理/路由/子智能体）              P1 ← ProviderSettingsTabs.tsx
│   ├── 路由总开关 + 请求整流器开关                        P1 ← RoutingPanel.tsx
│   ├── 自动故障转移绑定编辑器（增删/排序/启用/选池或 key）  P1
│   ├── 本地回环 API 配置（端口 / Token 轮换一次性明文展示） P1
│   ├── 全局出站代理配置（当前只写不回读——已知缺陷）        P2
│   └── 子智能体可用供应商白名单                            P1
├── Sub2API 账号池                                       P1 ← provider_accounts.rs、Sub2ApiAccountPool.tsx
│   ├── 创建账号池（作为一个 provider）                    P1
│   ├── 批量导入（token/JSON/JSONL 多格式、预览两步、去重指纹、10MiB/2000 条上限） P1
│   ├── 凭据信封加密 + sessionToken 直接丢弃               P0
│   ├── 平台/类型白名单（openai/anthropic/gemini，其余拒绝不伪装可用） P0
│   ├── 账号表格（搜索/多选/批量删除/状态徽标）             P1
│   ├── 账号级代理导入（账号代理优先，全局代理兜底）         P1
│   └── 账号暂停/启用/编辑（未实现，状态仅展示）             P2
├── 路由执行（daemon）                                    P1 ← src-agent-daemon/src/routing.rs
│   ├── 路由计划加载（关闭或读库失败时仅直连，不破坏既有 Run） P1
│   ├── 跨目标故障转移（首个可见 delta 后禁止重放）          P1
│   ├── 熔断器（3 次失败/60s 冷却/半开单探测，状态落 assistant.db） P1
│   ├── 池内账号选择（priority + 最少连接 + 会话亲和轮转 + 并发租约） P1
│   ├── OAuth token 自动刷新（到期前 180s，加密回写）       P1
│   └── 超时策略（首字节 60s / 流空闲 120s）               P1
├── 本地回环 API（loopback）                              P1 ← src-agent-daemon/src/loopback.rs
│   ├── /v1/chat/completions、/v1/responses、/v1/messages + /health、/v1/models P1
│   └── Bearer 强制鉴权 + 32MiB 请求上限 + 非流式 600s     P1
└── 请求整流器（rectifier）                               P2 ← request_rectifier.rs
    └── max_tokens 抬升至 64000 / 回环侧 thinking 归一（无条件前置改写，弱于设计文档的错误触发重试） P2
```

---

## 六、本篇合规自检清单

- [ ] 我新增的可见字段都有真实数据来源，没有占位值或 `Math.random()` 伪造（R-F2）。
- [ ] 估算值已标注「约/估算」或隐藏（R-F3）。
- [ ] 我的列表/面板都处理了「空」和「加载中」状态，用 `EmptyState` 而非假数据（R-F4）。
- [ ] 我的错误展示走了 `classifyError()`，没有把原始异常抛给用户（R-F5）。
- [ ] 新功能已标注 `P0/P1/P2`（R-F1）。
