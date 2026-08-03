# 个人创意模块分批整改方案

> 基线：`deploy@1b4b1792932e2e24160091c7700a2c092d01e5f2`，2026-08-03。
> 原则：先资源安全，再统一运行身份，再扩项目类型。复用现有 Catalog adapters、LocalRuntimeManager 进程组逻辑、Docker CLI adapter、Workshop 发布门禁和 child WebView；不创建第二套运行器。

## 批次 0：危险操作与资源安全

- 目标：先消除假 stopped、Provider 越权、外部导航和交易误启动风险。
- 当前问题：Local Stop 忽略错误并清身份；orphan 强杀不验证；Host 直接调用 Provider；WebView 后续导航可去公网；Compose 尚无危险 command 门禁。
- 涉及模块：Tauri Host、Local Runtime、Agent Daemon、Embed Browser。
- 涉及文件：`src-tauri/src/creative_app/local/lifecycle.rs`、`runtime.rs`、`ai.rs`、`service.rs`、`browser.rs`、`commands/creative_app.rs`、`src-agent-daemon/src/production.rs`、`crates/capability-gateway/src/tools/`。
- 生产调用链变化：Stop 只有在资源退出并验证后写 stopped；本地 AI 规划改走 Host 脱敏扫描 → Daemon Run/Tool → Host 校验；WebView 所有导航保持 loopback。
- 数据库变化：本批不迁移；停止失败保留 process identity、port、URL，并写结构化 cleanup error。
- Protocol/RPC：新增最小的 `creative.local.analyze` Daemon 方法或受控 Tool Result；不得让 Host 直接调 provider-adapters。
- Agent Tool：分析 Tool 只接收虚拟 `/project` 脱敏扫描摘要，返回候选 plan，不接收/返回凭证或绝对路径。
- Runtime：传播 `runtime.stop` 错误；TERM/KILL 后核实 PGID/PID/port；失败保持非 stopped。
- Renderer：停止失败显示“仍可能运行”，保留再次停止/打开日志/手工处理入口。
- 样式：使用现有 danger/warning token 和 ConfirmDialog，不加新设计系统。
- 资源回收：close/kill/port 验证均可观测；Browser close 失败不清状态。
- 安全边界：禁止 Docker prune、默认禁止 volume/image 删除、local-only navigation、任何 `trade` command 默认高风险阻断。
- 精准测试：`stop_failure_keeps_identity_and_non_stopped_state`；`term_timeout_kills_group_and_releases_port`；`local_ai_routes_through_daemon`；`browser_rejects_external_navigation`。
- 验收标准：不能出现 UI stopped 而 PID/PGID/port 仍存在；Host 无本地项目 Provider 直调；WebView 不能离开 loopback；危险交易 plan 无明确授权不可执行。
- 回滚：保留旧 API 形状；只回滚到“拒绝启动/停止失败”而不是假成功。
- 依赖：无；必须最先完成。

## 批次 1：统一 Application / StartupPlan / RuntimeInstance

- 目标：为三源统一身份和运行实例建立单一权威，不改现有 driver。
- 当前问题：`modules`、`external_creative_apps`、`local_creative_apps` 三表投影；app 行内塞运行字段；无 RuntimeInstance/PreviewTarget。
- 涉及模块：Host DB、assistant-protocol、creative_app service/adapters、Renderer types。
- 涉及文件：`src-tauri/src/db.rs`、`creative_app/model.rs`、`store.rs`、`adapters/*`、`commands/creative_app.rs`、`crates/assistant-protocol/`、`src/types/generated/`、`src/lib/tauri-adapter.ts`。
- 生产调用链变化：Catalog 先读统一 `applications` 身份，再 join 现有 source detail；start 先创建/复用 RuntimeInstance，driver 只操作实例资源。
- 数据库变化：新增 `applications`、`startup_plans`、`runtime_instances`、`preview_targets`；三源表保留 source detail 外键。`runtime_instances` 至少含 id/application_id/plan_id/status/owner_kind/pgid/compose_project/resolved_urls/cleanup_status/timestamps/failure。
- Protocol/RPC：新增/更新 wire types，唯一来源放 `assistant-protocol`，前端绑定只生成不手改。
- Agent Tool：本批不开放新 Tool；先稳定数据契约。
- Runtime：start 使用 application lock + CAS；同一 app 最多一个 active runtime。
- Renderer：Catalog/卡片使用 application_id；所有按钮携带 runtime_instance_id（未运行时为空）。
- 样式：不改视觉，仅替换数据来源。
- 资源回收：每个资源记录 owner runtime id；stopped 仅在 cleanup_status=completed 后成立。
- 安全边界：迁移不得复制或删除用户项目；旧三表数据幂等 backfill；ID 冲突显式失败。
- 精准测试：migration backfill；三源 list ID 稳定；双 start CAS；restart 创建新 instance 且旧实例 completed。
- 验收标准：同一 Application 从助理卡片、Catalog、详情、Preview 看到同一 ID；每个 active 资源可追溯到一个 RuntimeInstance。
- 回滚：保留旧表与兼容读取一个版本；迁移为只增不删，回滚只切读路径。
- 依赖：批次 0。

## 批次 2：Runtime Owner 与资源回收

- 目标：RuntimeInstance 成为进程、Compose、日志、健康、URL、Preview 和临时资源的唯一 Owner。
- 当前问题：app_id HashMap、detached reader、无取消树、启动锁不可抢占、orphan 只有粗状态。
- 涉及模块：Host Runtime service、Local Process driver、Docker driver、Browser manager。
- 涉及文件：`creative_app/local/runtime.rs`、`lifecycle.rs`、`logs.rs`、`docker.rs`、`install.rs`、`browser.rs`、`service.rs`、`src-tauri/src/lib.rs`。
- 生产调用链变化：`start(runtime_id)` 创建 root CancellationToken 和 task set；所有 spawn/reader/poll/browser/temp resource 通过 owner registry 注册；Stop 先 cancel 再按 driver 清理。
- 数据库变化：补充 `cleanup_status`、`last_heartbeat`、`owner_pid`、`exit_code`、`resource_ledger_json` 或等价规范化资源表。
- Protocol/RPC：start/stop/restart 返回 RuntimeInstance；事件包含 runtime_id、stage、cleanup_status。
- Agent Tool：未来 start/stop Tool 只能按 application_id 请求，Daemon/Host 返回 runtime_id；Agent 不持 PID/Compose 命令。
- Runtime：标准顺序为 cancelling tasks → TERM → grace → KILL → wait/reap → port/container/network check → temp cleanup → stopped。
- Renderer：展示 `starting/waiting_for_health/stopping/cleanup_running/orphaned`；允许启动中 Stop。
- 样式：只新增状态 Badge/Progress，复用 token。
- 资源回收：reader、health、URL、Docker logs、WebView、listener、temp file 全部绑定 token + JoinHandle；设清理超时和失败明细。
- 安全边界：不误杀身份不匹配 PID；不对 Docker 使用全局清理；Compose 只按稳定 project/file 操作。
- 精准测试：忽略 TERM 强杀；启动中取消；reader 结束；health 结束；重复 Stop 幂等；Host crash reconcile；端口释放。
- 验收标准：资源矩阵每一行都能回答创建者、owner id、cancel、stop、timeout、恢复；场景 6、7、9、10 通过。
- 回滚：driver 保留，但 Runtime service 可 feature flag 切换；已创建实例不得降级为 app 行内假状态。
- 依赖：批次 1；必须先于批次 4/5 增加类型。

## 批次 3：助理创建应用闭环

- 目标：普通助理能把创建意图交给正式创作流程，并返回结构化应用结果。
- 当前问题：普通 surface 只能写文件；草稿工具仅创作台可用；没有注册/结果卡。
- 涉及模块：Assistant UI、Daemon tool surface、creative draft、Host publish gate、Catalog。
- 涉及文件：`AssistantWorkbench.tsx`、`ConversationTimeline.tsx`、`components/assistant/blocks/`、`CreativeHome.tsx`、`CreationSession.tsx`、`use-assistant-run.ts`、`production.rs`、`tools/creative_draft.rs`、`commands/creative_draft.rs`。
- 生产调用链变化：普通助理识别创建意图 → 受控 `create_creative_draft`/handoff Tool → creative-draft Run → 结构化草稿卡 → 用户发布/启动并预览。
- 数据库变化：draft 增加 `created_by_run_id`/origin conversation；Application 记录 creation_origin。
- Protocol/RPC：Tool Result 包含 draft_id、conversation_id、status、suggested_name；发布结果含 application_id/startup_plan_id。
- Agent Tool：模型可创建草稿或请求 handoff，但正式 publish 仍只能由用户 Host command；普通 surface 不获得正式目录写权限。
- Runtime：发布后的静态 Application 通过统一 start，而不是把“文件已写”当 running。
- Renderer：新增结构化“应用草稿/已发布应用”卡片，按钮为继续创作、发布、启动并预览、打开目录；部分成功可恢复。
- 样式：结果卡复用 Assistant card/interaction shell 和现有 Button/Badge。
- 资源回收：启动并预览走批次 2 owner；卡片关闭不停止 Runtime，除非用户显式 Stop。
- 安全边界：Agent 不直接写 SQLite、不调用 publish、不绕过 Gateway；草稿路径由 id 解析。
- 精准测试：普通 assistant 工具广告；draft handoff；发布同一 ID；注册失败保留草稿；重复 Tool call 幂等。
- 验收标准：场景 1、2 从普通助理消息开始完整通过；消息卡和个人创意列表为同一 application_id。
- 回滚：可关闭普通助理 handoff，但保留个人创意创作台；不回滚发布门禁。
- 依赖：批次 1、2。

## 批次 4：本地项目导入与智能启动

- 目标：把扫描结果升级为多候选 StartupPlan，不再把路径登记当可运行。
- 当前问题：仅 HTML/Vite/Vue；绝对路径无持续授权；AI schema 无新 runtime。
- 涉及模块：Host 目录授权、scanner、plan validator、import wizard。
- 涉及文件：`local/path.rs`、`scan.rs`、`plan.rs`、`model.rs`、`commands/creative_app.rs`、`WorkshopPage.tsx`（拆分向导）、`local-creative.ts`。
- 生产调用链变化：选择目录 → 保存 macOS bookmark/授权引用 → 有界扫描 → 多候选计划 → 用户确认 → Application/StartupPlan。
- 数据库变化：ProjectSource 保存 canonical path、display path、volume identity、bookmark blob/authorization status、last_verified_at；StartupPlan 版本化。
- Protocol/RPC：scan result 返回 candidates、evidence、risks、required confirmations；不返回 secret 内容。
- Agent Tool：智能分析走 Daemon 受控 Tool；只能提议，Host validator 最终裁决。
- Runtime：新增类型仅定义 plan，不在本批绕过 owner。
- Renderer：自动识别可改；展示证据、cwd、command argv、端口、health、风险、依赖副作用；失败保留表单。
- 样式：拆成复用 Stepper/Form/Alert；不继续堆进 WorkshopPage。
- 资源回收：扫描句柄及时关闭；卷断开标 missing，不无限重试。
- 安全边界：拒绝 symlink escape、shell 字符串、secret 文件、未授权外置卷；不修改项目。
- 精准测试：HTML/Vite/Python/Compose 冲突矩阵；中文空格路径；symlink；卷断开/重挂；多 lockfile。
- 验收标准：场景 3、11 通过；每个候选计划可解释且可编辑。
- 回滚：旧 HTML/Vite plan 作为候选 driver 保留。
- 依赖：批次 2。

## 批次 5：Docker Compose Runtime

- 目标：让本地现有 Compose 项目复用现有 Docker adapter，安全启动选定服务。
- 当前问题：Compose 只存在于 GitHub Release 轨；local plan 无 Compose 字段。
- 涉及模块：scanner、StartupPlan、Docker driver、Runtime owner、import wizard。
- 涉及文件：`local/scan.rs`、`model.rs`、`docker.rs`、`install.rs`（提取复用而非复制）、`adapters/local.rs`、`WorkshopPage` 拆分组件。
- 生产调用链变化：识别 4 种 Compose 名 → `docker compose config` → 列 services/profiles/ports/command → 确认 → unique project up → inspect/health → stop/down 当前 project。
- 数据库变化：StartupPlan Docker detail 保存 compose absolute path、project name seed、profiles、services、build_required、up/down args、port/health strategy；RuntimeInstance 保存实际 project name。
- Protocol/RPC：返回 preflight 和 service topology；事件带 service id。
- Agent Tool：Agent 只请求 start/stop Application，不接收任意 docker argv。
- Runtime：复用 `docker.rs` argv adapter；支持 Docker/Compose version、config、up、ps、inspect、logs、stop/down；部分失败补偿。
- Renderer：服务/profile/build/端口/volume 副作用确认；Docker 未启动提供明确恢复。
- 样式：服务列表、风险 Badge、主次操作统一。
- 资源回收：logs follow（若增加）绑定 token；Stop 默认 stop 或 down 语义可见；volume 默认保留；验证本 project 容器/network。
- 安全边界：禁止 prune；禁止未确认 `--volumes`/`--rmi`；project name 稳定唯一；禁止影响已有非本实例容器。
- 精准测试：两个同名 compose 目录隔离；部分服务失败；已有容器；端口冲突；stop 不影响另一个 project。
- 验收标准：场景 4、8 通过。
- 回滚：关闭 local Compose 候选；GitHub Docker 轨继续工作。
- 依赖：批次 2、4。

## 批次 6：URL、健康检查与内置浏览器

- 目标：URL 来源可解释、running 有真实健康证据、Preview 与 Runtime 绑定。
- 当前问题：单显式 URL/HTTP；200–499 过宽；BrowserState 只绑 app；存储跨应用。
- 涉及模块：Runtime probes、Docker inspect、Preview manager、Browser UI。
- 涉及文件：`local/runtime.rs`、`docker.rs`、`browser.rs`、`service.rs`、`WorkshopPage` browser 子组件、`tauri.conf.json`。
- 生产调用链变化：显式 plan → Compose/inspect → stdout hint → framework default → 用户配置，按优先级生成候选；health 成功后 running；PreviewTarget 绑定 runtime id。
- 数据库变化：保存 resolved_urls、selected_preview_target、health evidence、checked_at。
- Protocol/RPC：结构化 probe event；Browser commands 必须携带 runtime_id/preview_target_id。
- Agent Tool：open_preview 只接受 Application/Runtime id，由 Host 解析 local URL。
- Runtime：HTTP/TCP/Docker health/process alive 可组合；有 timeout/cancel/degraded；停止取消 probe。
- Renderer：地址、重连、外部打开、状态覆盖层、日志/浏览器 tab；停止后不显示旧页。
- 样式：复用 toolbar/button/token，补 focus/aria/小窗口。
- 资源回收：WebView close 必须确认；按 app 隔离 storage 或关闭时清理；WebSocket 随 WebView 销毁。
- 安全边界：始终 loopback；Embed 无 Workshop Bridge；不放宽 iframe sandbox。
- 精准测试：0.0.0.0→127.0.0.1、随机端口、多 URL、base path、WebSocket、X-Frame 替代、close 释放。
- 验收标准：场景 1–5、10 的 URL/health/preview 部分通过。
- 回滚：系统浏览器作为显式降级，不伪装内置 Preview 成功。
- 依赖：批次 2、5。

## 批次 7：个人创意 UI 与设计系统统一

- 目标：删除重复创建心智，按“创作/导入/运行”形成清晰信息架构。
- 当前问题：WorkshopPage 2,277 行；旧模板流与新创作台并存；状态/按钮/像素重复。
- 涉及模块：Creative UI、Assistant reusable components、i18n、design tokens。
- 涉及文件：`WorkshopPage.tsx`、`components/creative/*`、`components/ui/*`、`globals.css`、`src/i18n/zh.ts`、`en.ts`。
- 生产调用链变化：不改后端；UI 只消费统一 Application/Runtime 投影。
- 数据库变化：无。
- Protocol/RPC：无新增；删除旧字段前先证明无调用者。
- Agent Tool：结果卡展示 Tool Result，不重造 AI store。
- Runtime：按钮严格按后端 actions/runtime status 可用。
- Renderer：删除 `createTemplate` 和 `onInstall` 死接缝；拆 Creator、Catalog、ImportWizard、RuntimeDetails、Logs、Browser；保留一份 Assistant Run hook。
- 样式：复用 Modal/ConfirmDialog/Toast/EmptyState/Skeleton/全局 btn/token；统一 dark/focus/disabled/危险确认。
- 资源回收：UI 组件 unmount 清事件、ResizeObserver、log subscription、WebView。
- 安全边界：状态不只靠颜色；危险操作二次确认；敏感 env 仅显示 key/掩码。
- 精准测试：键盘/ARIA、中文长路径、小窗口、状态 action matrix、日志内存 cap、i18n key 同步。
- 验收标准：个人创意首页 10 秒内回答项目/状态/来源/主操作；无重复“创建”；日志 Renderer 有界。
- 回滚：组件拆分可逐块回滚，旧模板不得恢复。
- 依赖：批次 1、3、6。

## 批次 8：Freqtrade 安全验收

- 目标：以通用 Compose 能力验证高风险项目，不硬编码产品识别分支。
- 当前问题：默认 Compose command 为 `trade`，配置未审计时可能实盘；当前 scanner 无 plan。
- 涉及模块：通用风险扫描、Compose override、Runtime、Preview。
- 涉及文件：批次 4/5/6 文件及 Freqtrade 专项 fixture（复制最小脱敏结构到测试目录，不引用用户项目写测试）。
- 生产调用链变化：静态识别危险 command → 默认阻断 → 用户选择 webserver/已验证 dry-run → Natives-owned override → unique project start → 8080 health/Preview。
- 数据库变化：StartupPlan 保存 risk flags 和 explicit approval，不保存用户 config 内容/secret。
- Protocol/RPC：plan risk 包含 `may_trade_real_funds`、command evidence、required_confirmation。
- Agent Tool：Agent 不能自行确认交易风险；必须向用户请求明确授权。
- Runtime：首选官方非交易 `webserver`；若 dry-run，仅在可验证配置投影证明 dry_run 后允许。
- Renderer：醒目风险摘要、只读计划预览、禁止“一键默认 trade”。
- 样式：使用 danger Alert/ConfirmDialog，风险不只靠红色。
- 资源回收：Stop/down 仅当前 unique project，保留 user_data volume/bind mount；验证端口和容器。
- 安全边界：不修改配置/策略、不读取或输出 secret、不启动真实交易、不删 volume、不影响原容器。
- 精准测试：危险 trade 默认拒绝；webserver plan；API 未启用；8080 冲突；原容器不受影响。
- 验收标准：场景 5 通过，且审计日志可证明没有 live trade、项目写入、volume 删除。
- 回滚：禁用 Freqtrade 安全验收入口；通用 Compose 仍可用于低风险项目。
- 依赖：批次 5、6。

## 批次 9：泄漏、重启与端到端测试

- 目标：以失败注入证明资源闭环，而不是只证明按钮/Struct 存在。
- 当前问题：测试集中纯函数；无真实进程、Docker、WebView、crash E2E。
- 涉及模块：全链路测试 harness、Host、Daemon、Renderer。
- 涉及文件：新增最少的 `src-tauri/tests/creative_runtime_*`、Daemon contract test、Renderer interaction tests；不新增生产抽象。
- 生产调用链变化：无；仅可观测性与故障注入 hook（test-only）。
- 数据库变化：测试临时 DB 验证 migration/reconcile/CAS。
- Protocol/RPC：契约测试锁定 Tool 广告 ⊆ Handler、Runtime event 顺序。
- Agent Tool：普通/creative surface 广告与权限交叉测试。
- Runtime：覆盖重复 start、TERM 忽略、部分 Compose 失败、Host/Daemon crash、卷断开。
- Renderer：覆盖结果卡、状态、Stop 失败、Preview close、日志 cap。
- 样式：快照只验证关键 token/状态，不建脆弱全页截图墙。
- 资源回收：每个场景结束检查 PID/PGID、container/network、logs process、port、WebView、listener、temp files、runtime row。
- 安全边界：测试 Docker 使用独立标签/project，绝不 prune/volume delete；Freqtrade 使用脱敏 fixture，不碰用户目录。
- 精准测试：下方 11 个最终场景逐一实现；Rust 最少运行相关 crate/target，不先跑 workspace 全量。
- 验收标准：所有场景有失败前证据和清理后证据；无“测试结束由全局清理兜底”。
- 回滚：测试本身可独立回滚；发现失败时阻断发布，不降级断言。
- 依赖：批次 0–8。

## 最终验收场景映射

| 场景 | 主要批次 | 必须证明的清理证据 |
|---|---|---|
| 1 助理创建静态应用 | 3、6 | Preview 销毁、静态 stopped 路由不可访问 |
| 2 助理创建 Vite | 3、2、6 | Node 进程组退出、端口释放 |
| 3 导入现有项目 | 4、2 | Runtime completed、句柄关闭 |
| 4 Docker Compose | 5、6 | 仅当前 project 停止，其他容器不变 |
| 5 Freqtrade | 8 | 非实盘、保留数据、8080 释放 |
| 6 重复启动 | 1、2 | 仅一个 active RuntimeInstance |
| 7 普通进程强停 | 2 | TERM→KILL→wait/reap→port free |
| 8 Docker 部分失败 | 5、2 | 补偿清理、无 logs task |
| 9 Daemon/Host 崩溃 | 2、9 | reattach 或 orphaned，不重复启动 |
| 10 关闭 Preview | 6 | WebView/WebSocket/listener 释放，Runtime 继续 |
| 11 外置卷断开 | 4 | missing、无无限重试、重挂验证 |
