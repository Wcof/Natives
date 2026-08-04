# Creative OS 疑似问题逐项核查

> 基线：`deploy@9584c3c2`。严重度按本任务 P0–P3；“测试证据”中的源码单测只能证明相应纯逻辑，不替代真实 Tauri/Docker/崩溃实验。

## 01 应用身份与 Source 解析

- 核查状态：**CONFIRMED**；严重程度：**P0**；可信度：高
- 涉及文件/关键函数：`commands/creative_app.rs:328-354,386-415` `creative_app_browser_show/close`；`runtime_store.rs:40-59` `find_or_create_application`；`adapters/mod.rs:78-98` `resolve`
- 触发条件/调用链：GitHub Docker app → Browser Show/Close → 先按 `LocalProject` find-or-create → `or_else(ExternalGithub)`。
- 源码证据：find-or-create 对不存在身份执行 `INSERT OR IGNORE` 后成功返回，因此 `or_else` 永远不会处理 GitHub；`applications` 只唯一 `(source,source_id)`，允许同一 source id 的两种 source 行。
- 测试证据：现有 identity 单测只验证同一 source 幂等，未覆盖跨 source Browser。
- 当前实际行为/影响：产生伪 `local_project` application；真正 external runtime 的 PreviewTarget 查不到，Catalog/preview identity 分裂，属于数据错误。
- 是否需要修复/初步方向：是；Browser 必须调用唯一 `resolve_source`/按 summary 的 application id 查找，禁止读路径创建身份。
- 仍需验证：在真实用户 DB 统计并清理已经产生的幽灵 identity。

## 02 PreviewTarget 写入顺序

- 核查状态：**CONFIRMED**；严重程度：**P0**；可信度：高
- 涉及文件/关键函数：`commands/creative_app.rs:337-354,391-415`；`runtime_store.rs:252-318`；`browser.rs:46-78,115-125`
- 触发条件/调用链：show 先 DB DELETE+INSERT 后 WebView；close 先删 DB 后 close WebView；DB 错误被吞。
- 源码证据：创建/导航/尺寸/显示失败会留下已选 Preview；close 失败则 Preview 已删但 WebView/BrowserState 尚活；DB 失败时 WebView 仍可显示；replace 两语句无事务。
- 测试证据：store 单测仅覆盖成功 replace/clear，无失败注入。
- 当前实际行为/影响：DB、BrowserState、真实 WebView 三套状态可互相矛盾。
- 是否需要修复/初步方向：是；Operation/Window 记录 pending，外部动作成功后提交，失败补偿；DB 写失败不得静默。
- 仍需验证：Tauri add/navigate/close 故障注入。

## 03 Local Static Stop 后 URL

- 核查状态：**CONFIRMED**；严重程度：**P0**；可信度：高
- 涉及文件/关键函数：`http_server.rs:119-127,374-510`；`local/lifecycle.rs:490-494`；`local/runtime.rs:867`
- 触发条件/调用链：启动 static 得到 `/local-projects/{source-id}/...` → stop → 再请求旧 URL。
- 源码证据：`lookup_local_project_root` 只按 id 读目录，不检查 Application/Runtime/Preview 状态；static stop 无资源动作，仅清 source row URL/port。
- 测试证据：未发现 stop 后 HTTP 404/410 测试。
- 当前实际行为/影响：文件仍存在即继续 200；旧 Browser 仍可加载，停止语义不可信且 URL 能力不可撤销。
- 是否需要修复/初步方向：是；instance-scoped、不可猜 token/path，路由验证 active runtime；stop 先吊销 route，再清 Preview/Window。
- 仍需验证：真实 Host HTTP stop 前后响应与 cache 行为。

## 04 全局 MutationLock

- 核查状态：**PARTIALLY_CONFIRMED**；严重程度：**P1**；可信度：高
- 涉及文件/关键函数：`service.rs:14-58`；`commands/creative_app.rs:184-210`；`lib.rs:357-389`
- 触发条件/调用链：任一长 Docker/install/stop/delete/restart 与另一 app 操作并发。
- 源码证据：单 `Arc<Mutex<()>>`；stop/delete/restart/install 跨 await 持有，watchdog 每 2 秒也获取。Node health 已移到锁外，故“所有 health 都在锁内”不成立；Compose/external health 仍在。
- 测试证据：无跨 app 并发时序测试。
- 当前实际行为/影响：不同 app 无谓串行，长操作阻塞 stop/reconcile；同 app 排他仍有必要。
- 是否需要修复/初步方向：是；application-keyed lock + 有界全局 install/Docker semaphore，DB invariant 兜底。
- 仍需验证：实际长 health 下 stop/reconcile 延迟。

## 05 RuntimeInstance 是否真正拥有资源

- 核查状态：**CONFIRMED**；严重程度：**P1**；可信度：高
- 涉及文件/关键函数：`local/runtime.rs:56-117` `LocalRuntimeManager`；`runtime_store.rs:125-176`；`adapters/mod.rs:101-267`
- 触发条件：同 app 连续运行、晚到 task/health、Host crash。
- 源码证据：Child/log/task/cancel registry 均以 source app id 为键；Docker label/project 也以 app id；RuntimeInstance 由 adapter 在 driver 外创建并事后镜像资源。
- 测试证据：现有 runtime tests 验证单 app task cleanup，不验证两次 instance 隔离。
- 当前实际行为/影响：RuntimeInstance 是记账记录而非 owner；晚到事件可能落到新一轮运行。
- 是否需要修复/初步方向：是；复用现有 manager，但 registry/resource event/driver API 全部以 runtime id 驱动。
- 仍需验证：启动-停止-立即重启时的日志/health 串线。

## 06 活跃实例数据库唯一性

- 核查状态：**CONFIRMED**；严重程度：**P0**；可信度：高
- 涉及文件/关键函数：`runtime_store.rs:107-143`；`db.rs:930-949`
- 触发条件：并发调用或绕过全局进程锁的写入。
- 源码证据：`has_active_instance` 与 `create_instance` 是分离 SELECT/INSERT；没有 `WHERE status IN (...)` partial UNIQUE index 或单事务 CAS。
- 测试证据：`start_cas_keeps_one_active_instance` 只做顺序读写，未尝试第二次 insert/并发。
- 当前实际行为/影响：DB 允许同 app 多个 starting/running/stopping，资源与 active lookup 不确定。
- 是否需要修复/初步方向：是；数据库部分唯一索引 + 原子 insert/transition，迁移前先检测冲突。
- 仍需验证：真实 DB 是否已有重复 active rows。

## 07 状态迁移 affected rows

- 核查状态：**CONFIRMED**；严重程度：**P1**；可信度：高
- 涉及文件/关键函数：`runtime_store.rs:145-199,320-374`；`store.rs:78-90`；`local/store.rs:54-115`
- 触发条件：missing row、旧状态不匹配、并发 stop/start。
- 源码证据：guarded UPDATE（如 `status='starting'`）不检查 execute 返回数；source update/delete 同样忽略 affected rows。
- 测试证据：`mark_running_does_not_resurrect...` 只断言最终状态，API 仍返回 `Ok`。
- 当前实际行为/影响：调用者误认为迁移成功，无法区分幂等、冲突或数据丢失。
- 是否需要修复/初步方向：是；统一最薄 CAS helper，0 行返回 typed conflict/not-found。
- 仍需验证：无。

## 08 Restart 事务与失败语义

- 核查状态：**CONFIRMED**；严重程度：**P0**；可信度：高
- 涉及文件/关键函数：`adapters/mod.rs:234-314`；`local/lifecycle.rs:440-550`
- 触发条件：Local stop 的 PGID/port/Compose 清理失败后 restart。
- 源码证据：local stop 持久化 `CleanupFailed` 但最终返回 `Ok(summary)`；adapter 因此 mark runtime stopped；restart 接着创建新 instance。
- 测试证据：无 stop partial-failure → restart 阻断的端到端测试。
- 当前实际行为/影响：旧资源未释放时启动新资源，端口冲突、进程/容器失控、DB 状态互相矛盾。
- 是否需要修复/初步方向：是；cleanup 未 verified 必须 Err/typed outcome；restart 是 stop-success barrier 后的新 Operation。
- 仍需验证：Compose `ps` 失败与 TERM ignore 故障注入。

## 09 Stop、Delete、Window 关系

- 核查状态：**CONFIRMED**；严重程度：**P1**；可信度：高
- 涉及文件/关键函数：`adapters/mod.rs:234-298`；`commands/creative_app.rs:386-415`；`WorkshopPage.tsx:257-274`
- 触发条件：通过 Host/API stop/delete，或 Browser close 失败。
- 源码证据：lifecycle adapter 不控制 Window/Preview；只有当前 Renderer action 在部分路径主动 close；delete application 直接删 preview rows，但不证明 WebView 已关闭。
- 测试证据：无 stop/delete/window 组合测试。
- 当前实际行为/影响：Runtime 停止与窗口内容脱节；删除可能留下 child WebView。
- 是否需要修复/初步方向：是；Close Window 与 Stop Runtime 分离，但 stop/delete 必须 revoke endpoint 并使相关窗口进入离线/关闭策略。
- 仍需验证：窗口存活时 Host stop/delete 的真实行为。

## 10 崩溃恢复与 Orphaned

- 核查状态：**PARTIALLY_CONFIRMED**；严重程度：**P0**；可信度：中高
- 涉及文件/关键函数：`lib.rs:328-389,430-440`；`local/lifecycle.rs` reconcile/poll；`install.rs` reconcile；`db.rs:1204-1397`
- 触发条件：SIGKILL/崩溃、Child 活着但 Host 句柄丢失、Compose 部分存活。
- 源码证据：有启动 reconcile、2 秒 watchdog、normal-exit shutdown、PID identity 与 heartbeat；但 Child handle 不可恢复，资源 ledger/Compose project backfill 不全，错误多被忽略。Compose local backfill 被误标 `local_process`。
- 测试证据：有纯 migration/reconcile tests；无 Host crash/Docker Desktop 故障 E2E。
- 当前实际行为/影响：优于“完全无恢复”，但不能证明 reattach/cleanup，可能粗略 orphan 或假 stopped。
- 是否需要修复/初步方向：是；driver-specific inspect + identity proof + operation recovery；无法证明归属时 fail closed。
- 仍需验证：真实 Host crash、PID reuse、Docker restart。

## 11 单例 Child WebView

- 核查状态：**CONFIRMED**；严重程度：**P1**；可信度：高
- 涉及文件/关键函数：`browser.rs:12-18,46-78`
- 触发条件：同时打开两个 app。
- 源码证据：固定 label `creative-app-browser`，存在时只 navigate 复用；BrowserState 只有一个 active app/url。
- 测试证据：无多 app Tauri test。
- 当前实际行为/影响：第二个 app 覆盖第一个；无法构成多窗口/多任务伪 OS。
- 是否需要修复/初步方向：是；引入真实 WindowInstance registry，label 使用 window id。
- 仍需验证：多 Child/WebviewWindow 的平台上限。

## 12 WindowInstance 缺失

- 核查状态：**CONFIRMED**；严重程度：**P1**；可信度：高
- 涉及文件/关键函数：`browser.rs`、`db.rs`、Creative model 全域搜索
- 触发条件：恢复位置、最小化、焦点、后台运行、多个窗口。
- 源码证据：无类型/表/API；bounds 是一次性命令参数，未持久化。
- 测试证据：静态搜索无命中。
- 当前实际行为/影响：视觉 panel 不能由真实 OS 状态驱动。
- 是否需要修复/初步方向：是；WindowInstance 只表达窗口，不复制 Runtime。
- 仍需验证：Tauri close/minimize event ordering。

## 13 BrowserProfile 与 Cookie 隔离

- 核查状态：**NEEDS_RUNTIME_VERIFICATION**；严重程度：**P1**；可信度：高（缺模型）/未知（平台行为）
- 涉及文件/关键函数：`browser.rs:61-75`；`src-tauri/capabilities/default.json`
- 触发条件：两个 app 同 origin/端口变更、登录/清除登录状态。
- 源码证据：无 BrowserProfile、data directory/incognito/clear API；单 child 被跨 app 复用。
- 测试证据：未执行 macOS WebKit cookie/storage 实验。
- 当前实际行为/影响：无法声明隔离或稳定持久登录；很可能受 WebKit data store/同 origin 规则影响，但不能仅凭源码定论。
- 是否需要修复/初步方向：需要设计；先做最小实验再选 per-profile WebviewWindow/Child 策略。
- 仍需验证：cookie/localStorage/cache/service worker 跨 label/重启/端口行为。

## 14 OAuth 导航

- 核查状态：**NEEDS_RUNTIME_VERIFICATION**；严重程度：**P1**；可信度：高（当前不支持公网导航）
- 涉及文件/关键函数：`service.rs:66-105`；`browser.rs:65-66`
- 触发条件：localhost app 跳转 OAuth provider。
- 源码证据：所有后续 navigation 都限制 loopback，公网 OAuth 必被阻断；没有临时窗口、域名授权、callback correlation。
- 测试证据：无真实 OAuth 测试。
- 当前实际行为/影响：安全上已 fail closed，但登录类 app 的 OAuth 流不完整。
- 是否需要修复/初步方向：是（需求到批次 3 时）；临时 OAuth surface + per-app allowlist + loopback callback，不放宽普通 child。
- 仍需验证：WebKit popup/callback/cookie sharing。

## 15 下载、上传、剪贴板与新窗口

- 核查状态：**PARTIALLY_CONFIRMED**；严重程度：**P2**；可信度：中高
- 涉及文件/关键函数：`browser.rs`、capabilities、Creative UI 全域搜索
- 触发条件：文件选择、下载、`window.open`、clipboard。
- 源码证据：Creative Browser 没有 download/upload/new-window handler 或 per-app permission；主 UI 有通用 clipboard/file 能力不等于 Embed 获得能力。
- 测试证据：无 Child WebView 平台实验。
- 当前实际行为/影响：功能行为依赖底层默认，权限和审计不明确；不能断言全部不可用。
- 是否需要修复/初步方向：是；每项单独能力门禁、用户选择路径、下载记录、新窗口拦截。
- 仍需验证：各平台默认行为与 capability 边界。

## 16 Source 与 Runtime 类型覆盖

- 核查状态：**CONFIRMED**；严重程度：**P1**；可信度：高
- 涉及文件/关键函数：`model.rs:42-56,523`；scanner
- 触发条件：Python/Binary/attached localhost/remote URL。
- 源码证据：Source 仅 internal/external_github/local_project；Runtime 仅 Workshop/static/node/compose/run。scanner 的 Python/Dockerfile/Makefile 可只是 evidence。
- 测试证据：enum/scan tests 与静态搜索。
- 当前实际行为/影响：目标类型不能被正式注册和运行。
- 是否需要修复/初步方向：是，按实际用户场景逐 driver 增量加入，禁止预留万能 runtime。
- 仍需验证：各目标 runtime 的真实优先级。

## 17 Managed、Attached、Remote 能力

- 核查状态：**CONFIRMED**；严重程度：**P1**；可信度：高
- 涉及文件/关键函数：`model.rs:914-920` `OpenTarget`; lifecycle adapters
- 触发条件：添加已运行 localhost 或远程 Web。
- 源码证据：`LocalUrl` 是 managed runtime 输出，不是 Attached source；没有 remote registry。现有 stop 总是假设 Natives 管理资源。
- 测试证据：静态搜索。
- 当前实际行为/影响：无法诚实表达“可打开但不可停止”。
- 是否需要修复/初步方向：是；ownership mode 是 profile/driver 属性，Attached/Remote stop 为 no-op/不提供动作。
- 仍需验证：Remote 与 ADR-0012 Embed 边界一致性。

## 18 LaunchPlan 单服务限制

- 核查状态：**CONFIRMED**；严重程度：**P1**；可信度：高
- 涉及文件/关键函数：`model.rs:445-521` `LaunchPlan/ComposePlanDetail`
- 触发条件：frontend+backend+worker、多端点 Compose。
- 源码证据：单 program/script/cwd/port/open/health；Compose 仅一个 optional service/host port，启动却可拉起整个 project。
- 测试证据：model/plan tests 无多服务结构。
- 当前实际行为/影响：无法表达每 service 状态、日志、endpoint、补偿。
- 是否需要修复/初步方向：是；保留 LaunchProfile 顶层，Compose driver inspect 后生成 ServiceInstance/Endpoint。
- 仍需验证：现有用户 Compose 配置兼容映射。

## 19 Python 与 Binary Runtime

- 核查状态：**CONFIRMED**；严重程度：**P1**；可信度：高
- 涉及文件/关键函数：`scan.rs`；`model.rs` runtime enums；`local/lifecycle.rs`
- 触发条件：Python WebUI 或本地二进制。
- 源码证据：scanner 能发现 Python/Makefile/Dockerfile，但 plan/driver 不支持；无可执行文件 hash/签名/argv approval。
- 测试证据：scan tests 只验证证据/风险。
- 当前实际行为/影响：被识别但不能受管运行。
- 是否需要修复/初步方向：是，Node owner 稳定后复用 ProcessDriver；Binary 需显式 executable approval/hash。
- 仍需验证：第一批 Python framework 范围。

## 20 Agent 启动计划安全

- 核查状态：**PARTIALLY_CONFIRMED**；严重程度：**P1**；可信度：高
- 涉及文件/关键函数：`local/ai.rs`；`local/plan.rs`；`scan.rs`；Daemon creative protocol
- 触发条件：AI 建议或 Agent 创建 app。
- 源码证据：当前 AI 已通过 UDS，不再 Host 直调 Provider；建议会经过 Host `validate_launch_plan`，argv/cwd 结构化，交易类 Compose 默认阻断。尚无统一多 driver manifest/permission/Operation 审计，普通助理到 managed app 的正式闭环仍有限。
- 测试证据：AI/plan/risk 单测存在；无恶意跨 driver E2E。
- 当前实际行为/影响：关键旧旁路已修，但扩展 runtime 前仍需统一 Host policy。
- 是否需要修复/初步方向：是；Agent 只提交 versioned proposal，Host validate + 用户批准，永不直接 shell/SQLite/WebView。
- 仍需验证：Daemon tool advertisement 与 handler/permission 交叉测试。

## 21 健康检查范围

- 核查状态：**CONFIRMED**；严重程度：**P1**；可信度：高
- 涉及文件/关键函数：`local/runtime.rs:469-519`；`docker.rs:459-485`；LaunchPlan health fields
- 触发条件：TCP-only、Docker health、多 endpoint、登录重定向、外部 redirect。
- 源码证据：主要是单 HTTP/TCP readiness，200–499 可过；无 ready/degraded/unhealthy 持续状态、多 probe 聚合、Docker health truth。
- 测试证据：纯 health tests 有；无多服务/redirect 安全 E2E。
- 当前实际行为/影响：端口开或 4xx 也可能被视作 ready，运行退化不可表达。
- 是否需要修复/初步方向：是；probe 类型化、redirect 限域、readiness 与 liveness 分开，保持状态集合最小。
- 仍需验证：各 driver 的 probe 优先级。

## 22 日志实例隔离

- 核查状态：**CONFIRMED**；严重程度：**P2**；可信度：高
- 涉及文件/关键函数：`local/logs.rs:321-362`；`local/runtime.rs:56-61`；Docker logs
- 触发条件：同 app 多次运行或 Compose 多服务。
- 源码证据：registry/path/API 以 source app id，不带 runtime/service id。
- 测试证据：日志 cap/脱敏/轮转通过，但无跨 instance test。
- 当前实际行为/影响：历史运行混合、晚到 reader 串线、无法按服务过滤。
- 是否需要修复/初步方向：是；路径与事件至少带 runtime id，service id 可选；保留 app 级聚合视图。
- 仍需验证：旧日志迁移/保留策略。

## 23 CPU、内存、端口监控

- 核查状态：**CONFIRMED**；严重程度：**P2**；可信度：高
- 涉及文件/关键函数：Creative backend/frontend 全域搜索
- 触发条件：Task Manager/资源异常。
- 源码证据：无 CPU/memory sampler、metrics table/event；只有启动/停止时端口检查和 health。
- 测试证据：静态搜索。
- 当前实际行为/影响：伪 OS 任务管理器无真实资源数据。
- 是否需要修复/初步方向：是但后置；先做按需 snapshot，不先造历史时序库。
- 仍需验证：采样成本/跨平台 API。

## 24 端口冲突

- 核查状态：**PARTIALLY_CONFIRMED**；严重程度：**P1**；可信度：高
- 涉及文件/关键函数：`local/runtime.rs` port selection/check；`docker.rs` published-port inspect；plan validation
- 触发条件：auto port bind 后被抢、非 Docker 进程占固定端口、并发 starts。
- 源码证据：Node fixed port 检查存在；auto port 是 bind 后释放再 spawn 的 TOCTOU；Docker 检查偏 Docker published state；无 reservation/lease。
- 测试证据：port helper tests 存在，未覆盖竞争者抢占。
- 当前实际行为/影响：不是完全无检查，但并发/跨运行时冲突仍可发生。
- 是否需要修复/初步方向：是；Endpoint/Operation 分配阶段建立短租约，driver 失败需释放。
- 仍需验证：macOS Docker Desktop 端口错误表现。

## 25 Operation Journal

- 核查状态：**CONFIRMED**；严重程度：**P1**；可信度：高
- 涉及文件/关键函数：schema、service/install/lifecycle 全域
- 触发条件：install/start/stop/delete 中途崩溃或部分失败。
- 源码证据：只有 source 状态、last_error、runtime row；无 operation id、phase、compensation、actor/approval/journal。
- 测试证据：静态搜索。
- 当前实际行为/影响：恢复只能猜测瞬态状态，危险操作不可完整审计。
- 是否需要修复/初步方向：是；一个小 append/update journal，记录阶段和补偿，不引入工作流引擎。
- 仍需验证：保留上限与敏感字段脱敏。

## 26 CSP

- 核查状态：**PARTIALLY_CONFIRMED**；严重程度：**P1**；可信度：高
- 涉及文件/关键函数：`http_server.rs:14-19,90-127`
- 触发条件：导入的本地 static app 加载外部 script/connect/frame。
- 源码证据：Workshop/Local CSP 存在，但 Local 允许广泛 `https:`、`wss:`、`unsafe-eval`；不能按 app/profile 审批。Child 的远程页面不走此 CSP。
- 测试证据：响应 header 单测只证明 header 存在。
- 当前实际行为/影响：不是“无 CSP”，但策略与应用能力/信任域不匹配。
- 是否需要修复/初步方向：是；Workshop 维持冻结策略，Local/Remote surface 使用独立、可审计的 origin policy。
- 仍需验证：常见 Vite/React app 在收紧策略下的兼容性。

## 27 Host、Origin、Path Traversal

- 核查状态：**NOT_REPRODUCED**；严重程度：**P3**；可信度：高
- 涉及文件/关键函数：`http_server.rs:16,155-200,374-500`
- 触发条件：伪造 Host、编码 `..`、symlink escape。
- 源码证据：Host 仅 localhost/127.0.0.1/::1；path decode 后拒绝绝对/parent；最终 canonical containment 校验。
- 测试证据：HTTP/path tests 覆盖典型 traversal。
- 当前实际行为/影响：本轮未重现绕过；风险主要在“合法 source id 路由未绑定 runtime”（问题 03），不是 traversal。
- 是否需要修复/初步方向：该疑点无需单独修复；保持回归测试。
- 仍需验证：平台特有 Unicode/Windows path（macOS 基线外）。

## 28 Bridge Body Limit

- 核查状态：**CONFIRMED**；严重程度：**P1**；可信度：高
- 涉及文件/关键函数：`http_server.rs:55-66,561-580`
- 触发条件：大请求或并发慢请求。
- 源码证据：上限为 64 MiB；`.take(MAX_POST_BODY)` 读取会截断而非按 Content-Length/超限字节明确 413；listener 每连接再 spawn OS thread，无并发上限。
- 测试证据：无超限/并发压力测试。
- 当前实际行为/影响：内存/线程 DoS 与语义性截断，属于本地信任边界问题。
- 是否需要修复/初步方向：是；较小端点级上限、超限 413、有限 worker/concurrency。
- 仍需验证：chunked/无 Content-Length 行为。

## 29 Tauri Capability 隔离

- 核查状态：**NOT_REPRODUCED**；严重程度：**P3**；可信度：中高
- 涉及文件/关键函数：`src-tauri/capabilities/default.json`；`browser.rs:65-75`
- 触发条件：Embed child 尝试调用主应用 IPC/plugin。
- 源码证据：default capability 的 windows/webview 都仅 `main`；child label 为 `creative-app-browser`，没有 remote capability grant。
- 测试证据：配置静态检查；未做运行时恶意页面实验。
- 当前实际行为/影响：源码未支持“Child 获得 main capability”疑点；主窗口本身权限较宽是另一审计域。
- 是否需要修复/初步方向：无需因该疑点改动；新增动态 labels 时必须继续默认拒绝。
- 仍需验证：Tauri 版本下 child invoke 的真实拒绝响应。

## 30 WorkshopPage 复杂度

- 核查状态：**CONFIRMED**；严重程度：**P2**；可信度：高
- 涉及文件/关键函数：`WorkshopPage.tsx`（2,012 行）
- 触发条件：修改任一安装/本地/创作/Browser/log 流程。
- 源码证据：同组件持有大量 GitHub wizard、本地 wizard、Catalog、Browser、日志、创建状态；虽已有 creative 子组件，编排仍集中。
- 测试证据：`wc -l` 与 state/callback 静态统计。
- 当前实际行为/影响：回归面大，Window Manager 再塞入会恶化。
- 是否需要修复/初步方向：是但不在本轮；按真实业务 flow 拆 controller/hook，不建立空泛组件层。
- 仍需验证：React profiler 不是本问题必要条件。

## 31 前端 Busy State

- 核查状态：**PARTIALLY_CONFIRMED**；严重程度：**P2**；可信度：高
- 涉及文件/关键函数：`useCreativeAppCatalog.ts:109-135`；`WorkshopPage.tsx`
- 触发条件：同 app 重复操作、跨 app 长操作、Host Browser show 失败。
- 源码证据：per-app `busyIds` 能阻止同 app UI 重入且操作后 reload；跨 app UI 被允许但后端全局排队；Browser show fire-and-forget，错误不回滚 panel。
- 测试证据：hook/creative TS tests 通过；无真实长操作 UI E2E。
- 当前实际行为/影响：不是单一全局 busy，但与后端锁/Operation 状态不一致。
- 是否需要修复/初步方向：是；UI 从 Operation/Window event 投影，而不是叠加更多 boolean。
- 仍需验证：双 app 并发的可见反馈。

## 32 Dock、多窗口与 Task Switcher

- 核查状态：**CONFIRMED**；严重程度：**P1**；可信度：高
- 涉及文件/关键函数：Creative components、Browser state、schema 全域
- 触发条件：伪 OS 多任务。
- 源码证据：无 Creative Dock/window registry/focus/minimize/restore/task switcher；只有单 Browser panel。
- 测试证据：静态搜索。
- 当前实际行为/影响：当前是应用目录+单预览，不是由真实运行状态驱动的伪 OS。
- 是否需要修复/初步方向：是；第二批在 WindowInstance 之上做最小 Dock/任务切换器。
- 仍需验证：产品选择主窗口内多窗还是原生多窗。

## 33 ApplicationSurface

- 核查状态：**CONFIRMED**；严重程度：**P1**；可信度：高
- 涉及文件/关键函数：`OpenTarget`、`PreviewTarget`、LaunchPlan
- 触发条件：同 app 的 Main/Admin/Docs/Metrics/Setup。
- 源码证据：每 app 只有一个 `open_url`，PreviewTarget 只是一个 selected URL；无 surface name/kind/endpoint relation。
- 测试证据：静态搜索。
- 当前实际行为/影响：多服务 app 无法公开多个界面，Window 无稳定 surface 身份。
- 是否需要修复/初步方向：是；Endpoint 是运行地址，Surface 是稳定产品入口，二者分开。
- 仍需验证：首批真正需要的 surface 类型，避免预留大全。

## 34 数据迁移与 Manifest 版本

- 核查状态：**PARTIALLY_CONFIRMED**；严重程度：**P1**；可信度：高
- 涉及文件/关键函数：`db.rs:892-1090,1204-1397`；`runtime_store.rs:82-104`；`model.rs:445` 起
- 触发条件：旧 DB 升级、plan schema 演进、Compose active row backfill。
- 源码证据：v12–v14 和幂等 backfill 已存在，保留三源数据；但 active plan/runtime 无唯一 invariant，plan version 固定 1，缺 ApplicationVersion/manifest migration contract；local Compose backfill owner 只分 static/other，误记 local_process。
- 测试证据：migration/backfill tests 存在并在 targeted Rust suite 中运行；没有跨多个真实历史 DB fixture。
- 当前实际行为/影响：不能说“无迁移”，但未来伪 OS 实体/manifest 兼容与回滚不足。
- 是否需要修复/初步方向：是；逐表 additive migration、schema_version parser/upgrader、兼容旧 plan、先修 backfill 分类。
- 仍需验证：用户 DB 各版本采样（脱敏副本）。

## 汇总

| 状态 | 数量 |
|---|---:|
| CONFIRMED | 22 |
| PARTIALLY_CONFIRMED | 8 |
| NOT_REPRODUCED | 2 |
| FALSE_POSITIVE | 0 |
| OBSOLETE | 0 |
| NEEDS_RUNTIME_VERIFICATION | 2 |

| 严重度 | 数量 |
|---|---:|
| P0 | 6 |
| P1 | 21 |
| P2 | 5 |
| P3 | 2 |

“Host 直调 Provider”“Child 可导航公网”“Renderer 日志无限增长”属于旧报告中的已整改事实，但本任务 34 类问题更宽，因此分别归入 20/14/31 的剩余问题评价，而未额外增加 `OBSOLETE` 行。
