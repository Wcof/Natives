# macOS 菜单栏常驻与个人概览浮窗

> 状态：已有实现，但 `5627e3e4` 二次审计未通过；修复计划见第 9 节。
> 初始基线：`d25aa644d2f6a71d1d50752627fc9f8032ce7438`（2026-08-09）。
> 当前复核基线：`5627e3e43cf7b5113bc8e17f8c813ed43a1a78a4`（2026-08-10）。
> 产品面：Hub 系统外壳；不是 Workshop、Embed、web-module 或 Capability 执行轨。  
> 并行构建与低磁盘规则：[`../development/natives-agent-build-cache-and-disk-policy.md`](../development/natives-agent-build-cache-and-disk-policy.md) 第 10 节。

## 1. 目标与非目标

目标：

- macOS 主窗口点击红色关闭后默认隐藏，Host、Agent Daemon、Jobs、终端与受监督业务进程继续运行。
- 菜单栏显示 Natives template icon；点击切换轻量浮窗。
- 浮窗只展示设置页“个人概览”已有的真实聚合指标。
- `Cmd+Q` 或浮窗“退出”才执行幂等进程清理并真正退出。
- Popup 不加载 Shell、AssistantWorkspace、Workshop 或主窗口的全局副作用。

非目标：

- 截图中的 Token 配额、目标百分比、剩余天数、自然周目标、90 天热力图。
- CPU/内存卡片、动态 Tray 数字、登录时启动。
- 第二套 usage cache、短周期轮询、自定义 Tray 框架或新插件。
- 没有测量证据时销毁/重建隐藏的主 WebView。

## 2. 现有底座与阻断

可复用：

- Tauri 2 core 已提供 Tray，不增加依赖。
- `src-tauri/src/commands/widget.rs` 已有 400×600 无边框 WebviewWindow 骨架。
- `src/app/RootClient.tsx` 与 `src/components/shell/ShellLayout.tsx` 已有 widget 路由分支。
- `PersonalOverview.tsx`、`useUsageData.ts`、`personal-overview-data.ts` 已有真实缓存读取、手动同步、汇总与趋势计算。
- `usage_get_cached` 只读 Host 快照，不触发 180 天扫描。
- single-instance 与 Daemon supervisor 已存在。

生产阻断：

| ID | 问题 | 收敛要求 |
|---|---|---|
| MB-P0-01 | 任意窗口 `CloseRequested` 都会清理终端、创意进程和 Daemon | 窗口关闭只隐藏；清理集中到真实退出路径 |
| MB-P0-02 | `theme_ready_signal` 固定显示 `main` | theme-ready 必须针对调用窗口；Popup 就绪前不可见 |
| MB-P0-03 | widget 分支仍挂载 Shell 与 Assistant providers | `RootClient` 最早分流轻量 surface |
| MB-P0-04 | 无 Tray、失焦隐藏、显式 Quit、Reopen、多屏定位 | 建立 Host menubar 域和生命周期测试 |
| MB-P0-05 | 当前 ControlHubWidget 指标不符合个人概览 | 迁移或替换旧 widget，禁止并存第二套浮窗框架 |
| MB-P0-06 | 系统磁盘数据把所有挂载盘求和；Host usage scanner 直读 Daemon DB | 修正根卷语义；usage 通过 Host 投影或最小 Daemon summary 收敛权威 |

## 3. 冻结契约

| 项目 | 契约 |
|---|---|
| Window label | `menubar` |
| Renderer route | `?surface=menubar` |
| Open/close | Tray 左键 toggle；失焦、Escape、CloseRequested 只 hide |
| Main close | `prevent_close()` + `hide()` |
| True quit | `Cmd+Q` / Popup Quit → 幂等清理 → app exit |
| Data view | 当前时区、30d、无项目过滤 |
| Cached read | Popup hidden→visible 时调用现有 `usage.getCached` |
| Manual refresh | 只有用户明确点击刷新才调用 `usage.sync` |
| Cross-window event | `usage:snapshot-changed` |
| Shared calculation | 将个人概览纯计算移动到 `src/lib/`，Settings 与 Menubar 共用 |
| Permissions | 独立 `menubar` capability，仅授予所需窗口/事件/只读数据能力 |

Tray/Popup 命令应归 `commands/menubar.rs`。命令必须验证调用窗口 label；Popup 不能继承 main 的 shell、filesystem、dialog、credential 或 Workshop 权限。

## 4. 生命周期

| 事件 | 行为 |
|---|---|
| App setup | 创建唯一 Tray；主窗口继续走现有 FOUC guard |
| 主窗红色关闭 | prevent close、隐藏；后台任务不受影响 |
| 首次 Tray 点击 | 懒创建透明/无边框/不可缩放 Popup，主题就绪后显示 |
| 再次 Tray 点击 | toggle hide/show |
| Popup 失焦、Escape、关闭 | 仅隐藏 |
| 打开 Natives | 隐藏 Popup；show/unminimize/focus 主窗 |
| 打开个人概览 | 恢复主窗并导航到 Settings / Personal Overview |
| 第二实例 | 复用已有实例，隐藏 Popup，恢复主窗 |
| macOS Reopen | 没有可见窗口时恢复主窗 |
| Cmd+Q / Quit | 一次性清理终端、本地创意进程、Daemon 和受监督子进程后退出 |

主窗口隐藏时保留 WebView 状态，但必须 visibility-gate 非必要 Renderer 定时器、iframe heartbeat、动画和更新检查；Host Jobs、Daemon supervision 和正在运行的任务继续工作。

## 5. 浮窗指标与数据诚实性

| 显示项 | 计算/来源 |
|---|---|
| 今日 Token | 本地日期范围内所有非空 `daily.totalTokens` 求和 |
| 近 30 天 Token | `aggregateUsageMetrics().totalTokens` |
| 输入 / 输出 Token | `totalInputTokens / totalOutputTokens` |
| 近 30 天趋势 | 共享 `buildOverviewTrend()` |
| 平均每会话消息 | `(userMessages + assistantMessages) / unique sessions` |
| 活跃项目 | daily/session 中非空 `projectId` 去重 |
| 会话数 | `sourceId + sessionId` 去重 |
| 消息数 | user + assistant message 计数 |
| 已登记项目 | Host `project.list().length` |
| 更新时间/覆盖 | snapshot `generatedAtMs` + source states |
| 根卷存储 | 修正后的 root volume used/total/available；修复前隐藏 |

规则：

- 无缓存显示空态与“打开 Natives 同步”，不得显示假 `0`。
- 陈旧缓存仍可显示，但必须显示真实更新时间。
- 部分来源必须标记覆盖范围，不能把已知子集伪装成全量。
- 不在 Tray title、日志或错误中暴露 Token、项目路径、Provider 输入或凭证。
- 趋势图同时提供可访问的日期和值文本。

## 6. 并行实施计划

并发上限为“主 Agent + 3 个 Subagent”。三个工作包先并行，随后复用一个 Subagent 做专项 QA。

| 负责人 | 文件所有权与交付 |
|---|---|
| native_menubar_lifecycle | `src-tauri` Tray、Popup、定位、close/hide/quit、single-instance、Reopen、capability、template icon；`lib.rs` 生命周期改动独立提交 |
| lightweight_menubar_renderer | `RootClient` 早分流、`components/menubar/*`、loading/error/empty/partial/stale、键盘/a11y、zh/en |
| overview_metrics_integrity | 共享纯计算、usage update event、coverage 语义、根卷存储、Host/Daemon usage 权威收口 |
| 主 Agent | 冻结契约、审阅权限/冲突、整合生命周期状态机与真机验收 |

合并顺序：

1. 指标共享 helper 与测试。
2. Native Host 与 Renderer 并行提交。
3. 先合并执行引擎方案的 `run.watch` Host 装配，再 rebase 本方案的 `lib.rs` 生命周期提交。
4. 合并到唯一集成 HEAD 后统一执行全门禁和一次 Tauri/Release smoke。

## 7. 验收标准

- 关闭主窗后 Tray、Daemon、Jobs、终端、活跃 Run 和本地创意进程继续存在。
- Popup 关闭/失焦/Escape 不触发全局清理。
- Cmd+Q/显式退出后 Daemon 与受监督子进程全部回收。
- 第二实例不会创建第二个 Tray、Host 或 Daemon；Dock Reopen 可恢复主窗。
- Popup 与同一时刻设置页个人概览的 30d 汇总一致。
- 无缓存、部分来源、陈旧缓存、磁盘错误分别显示真实状态，无假零值。
- 多显示器、负坐标、缩放、刘海屏和菜单栏自动隐藏场景中 Popup 不越界。
- Popup 隐藏时无 usage polling/scan、无图表动画，不加载 Shell/Assistant bundle。
- 同设备 Release 构建下：冷启动、热 IPC、Widget CPU、总 RSS 符合 `technical/04-performance.md`；记录修改前后数据。
- 完整门禁只在唯一集成 HEAD 执行一次，遵守共享构建与低磁盘策略。

## 8. Goal 启动提示词

> 本节是首次实现时的历史提示词；二次审计后的集成修复以
> [`MODULAR_ARCHITECTURE_REMEDIATION.md`](MODULAR_ARCHITECTURE_REMEDIATION.md) 第 11–12 节为准。

```text
请创建并持续执行一个 Goal：为 Natives 增加生产级 macOS 菜单栏常驻与个人概览浮窗。

使用独立分支 `codex/macos-menubar-overview` 和仅源码 Worktree；若调用方已经创建对应分支/Worktree，则复用而不是再创建。开始前完整阅读 AGENTS.md、docs/README.md、docs/architecture/macos-menubar-personal-overview.md、docs/development/natives-agent-build-cache-and-disk-policy.md 第 10 节以及该方案引用的 standards。本文第 1–7 节是范围、契约、工作包和完成门槛唯一来源；不要复制新的进度文档。

使用主 Agent + 3 个 Subagent，并按第 6 节文件所有权并行。主 Agent 先冻结 label、route、command、event、data view 和生命周期。两个 Goal 会同时修改 src-tauri/src/lib.rs：本 Goal 不改 run.watch；把 Tray/窗口生命周期做成独立原子提交，并在执行引擎分支的 run.watch 装配合并后 rebase。

当前磁盘约 17 GiB 可用。进入低磁盘模式：任务 Worktree 不安装/软链接 node_modules，不生成自己的 target/.next/release/app/dmg，不运行 npm typecheck/test/perf:check、cargo workspace test/check/clippy 或 tauri build。开发期只运行 git diff --check、cargo fmt --check、纯逻辑测试和无需完整依赖的静态检查。需要 Rust 定向测试时必须由集成负责人使用主仓库唯一 /Users/ldh/Downloads/project/AiNative/Natives/target 串行运行。

每批结束记录 df、主 target/node_modules 大小、worktree 和构建进程。自动清理只允许本 Goal 自己创建、已验证为 ignored/generated 且无进程占用的精确绝对路径；禁止 cargo clean、git clean -fdx、通配符删除，禁止触碰主 target、node_modules、~/.cargo、~/.npm、SQLite、用户项目、证据和未提交文件。提交完成后保留 Worktree 供集成者合并，不自行删除未合并工作。

必须使用 Tauri 2 core Tray，不安装新插件。主窗红色关闭只隐藏，Cmd+Q/显式退出才清理进程。Popup 必须懒创建并在 RootClient 最早分流，不得挂载 Shell、AssistantWorkspace、Workshop 或更新检查；隐藏时无轮询、扫描和动画。只展示本文第 5 节的个人概览真实指标，不实现截图中的配额、目标、剩余天数、90 天热图、CPU/内存或动态 Tray 数字。缺失、部分、错误和陈旧数据必须诚实显示。

严格完成本文第 7 节全部验收。两个方案开发完成后，由集成负责人合并到同一 HEAD，只在主工作区串行执行共享策略第 10.5 节全门禁及一次 Tauri/Release smoke。真机生命周期、数据一致性、权限或性能证据未完成时 Goal 不得 complete。最终报告列出状态机、指标来源、capability 权限、macOS 矩阵、定向测试、磁盘前后数据、最终统一门禁、剩余 blocker 与回滚点。
```

## 9. 二次审计与修复计划（2026-08-10）

### 9.1 当前实现不能通过的原因

| ID | 审计发现 | 用户影响 | 修复要求 |
|---|---|---|---|
| MB-R1 | `RootClient` 静态导入并先挂载主窗口 providers，随后才判断 Menubar surface | 浮窗不轻量，可能触发主 Shell/Assistant 副作用 | 在 provider/static main bundle 前 early split；用 bundle test 证明 |
| MB-R2 | Menubar 首次 mount 没有完成个人概览所需项目列表加载 | 活跃项目/已登记项目等首屏为空或不一致 | 复用 Settings 的 cached usage/project 数据入口，首次显示即读取一次 |
| MB-R3 | hidden 状态 interval 仍触发 React state update | 关闭浮窗后仍持续耗电、唤醒 Renderer | visibility-gate/cancel interval 与 animation；再次显示时单次 reconcile |
| MB-R4 | 全量 Rust/Tauri/Release 与真机生命周期未执行 | close/hide/quit、second instance、Reopen 未获生产证据 | 在同一 candidate 上完成真机矩阵和唯一 Release 构建 |
| MB-R5 | 根卷、usage authority、错误三态仍依赖未通过的 Host/Daemon 整改 | 指标可能不真实或 Sidecar 不可用 | 先完成 DB authority/Broker/readiness，再做同一快照一致性验收 |

### 9.2 最小整改路径

1. 保留现有 Tauri 2 Tray、Window label、route、usage helper 和个人概览数据结构；不创建第二套 widget、
   store 或轮询器。
2. Renderer 把 `surface=menubar` 识别移到最早入口，以 lazy/dynamic main surface 边界阻止 Menubar
   引入 Shell、AssistantWorkspace、Workshop、更新检查和主窗口 Provider。
3. Popup hidden→visible 时只做一次 `usage.getCached` 与 project list reconcile；手动刷新才调用 sync。
   hidden 时停止非必要 interval、图表 animation 和 state update，但不停止 Host/Daemon/Jobs/活跃 Run。
4. Native lifecycle 复用唯一 owner：Close/focus loss/Escape 只 hide；`Cmd+Q`/显式 Quit 才做幂等清理；
   second instance 和 Dock Reopen 恢复主窗；多屏定位 clamp 到当前 screen work area。
5. 用同一 cached snapshot 同时渲染 Settings 与 Menubar，逐项比较第 5 节指标；missing/partial/stale/error
   均显示真实状态，禁止假零。
6. 该整改并入模块化 Goal 的 Subagent B/C，不再单独创建 Menubar Worktree 或构建缓存。

### 9.3 验收证据

- 单元/interaction：early split 不挂载主 providers；首次显示加载 project+usage；hidden 不增加 timer-driven
  render；Escape/Close 只 hide；错误/空/partial/stale 四态和键盘焦点可达。
- Host contract：唯一 Tray/Popup、幂等 quit、second instance、Reopen、失焦、窗口 label/capability 检查。
- 真机矩阵：主窗关闭后活跃 Run/Daemon/终端继续；显式退出全部回收；多屏/负坐标/缩放/刘海屏不越界。
- 数据：同一时刻 Settings 与 Menubar 的 30d 汇总、项目数、覆盖和更新时间一致。
- 性能：hidden CPU/唤醒、Menubar bundle、冷启动/热 IPC/总 RSS 有修改前后数据；全部产品路由 gate 通过。
- 构建：只使用模块化 Goal 的唯一 candidate、共享 target 和一次 Tauri/Release；任一项未执行则不能完成。
