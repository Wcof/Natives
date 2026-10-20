# Natives 性能与内存回收验收方案

> 更新：2026-10-04。本文是当前性能门禁、生命周期整改方案和剩余验证项；早期架构审计已移至
> `docs/archive/legacy-development/application-performance-remediation.md`。

实施对象是 Extension、`native-file-host`、`file-manager-core`、`model-host` 和内置模块。
性能优化只修改已证实的瓶颈，安全、数据完整性、可访问性和 Native EOF 回收规则优先级更高。

## 当前基线

### 空间时间组件的页面 owner

Time、Binary Time、Work Hours、Since、Countdown 和待办专注倒计时共用页面级可见 tick。活动工作区由 Core 限制最多 64 个组件；每个 renderer 注册回调并在 dispose 时解除，页面至多一个时间 tick、一个 visibility 监听。隐藏／pagehide 停止 tick，BFCache 恢复后立即按绝对时间校准，不补执行后台累计次数。最后订阅解除后移除 timer 与监听。Web Surface 的会话续租由原 owner 单列，不能混记成组件计时器。需实测 E/W 的 1／5／20 组件与隐藏恢复，并保留修复前计数；不据此宣称总 CPU／RSS 已达标。

| 对象 | 目标 | 当前判断 |
|---|---:|---|
| Extension 发布体积 | ≤ 360 KiB | 由 `extension:check` 复核 |
| `native-file-host` Release 体积 | ≤ 4 MiB | 由 `perf:check` 复核 |
| Host 空闲 RSS | ≤ 12 MiB | 由 `perf:check` 复核 |
| Model Host 空闲 RSS | ≤ 256 MiB | 由 `perf:check` 复核 |
| Host EOF 退出 | ≤ 2 s | 必须实测 |
| 10,000 项目录查询 p95 | ≤ 50 ms | 必须实测 |
| Extension 空闲 CPU | ≤ 5% | 必须实测 |
| Watcher 合并窗口 | 250 ms | 代码与实测一致 |

一次通过只代表该构建、该平台和该数据集通过，不能外推到未验收平台。

## 生命周期约束

```text
newtab/空间：只渲染可见 Widget
  页面隐藏 → 暂停时钟、轮播和刷新
  最后一个 AI Widget 销毁 → 断开 Model Native Port

files.html：唯一 Files Native Port
  隐藏且无任务 60 秒 → 断开
  pagehide/关闭 → 断开 → Host EOF → 2 秒内退出

内置模块：owner page → App Host → 受限 loopback iframe
  页面隐藏 60 秒 → 停止模块并释放端口
```

所有 interval、listener、Blob URL、Native Port、Watcher 和临时文件都必须有对称 disposer。
Service Worker 不创建 Port、不轮询、不保活；Model Host 常驻只有用户显式开启时才允许。

## 已完成的代码整改

- App Center 已移除模块下载、目录、种子和独立发布生产链；模块代码随完整产品交付。
- Host 的旧安装事务和启动恢复调用已删除；产品配置保留验签、staging、原子切换和恢复安全原语。
- App Store 只投影内置模块；清除数据保留代码、注册、activation 和显示/启用偏好。
- Extension 清理了重复主题监听和旧安装状态字段；关闭模块的运行锁覆盖 activation 与数据库事务。
- 文件 Host 保持有界分页、取消、并发、路径授权和 EOF 清理。

## 必须补齐的证据

### P0：稳定基线

1. `perf:check` 每次先用 Cargo 构建当前 Release Host，再测体积、RSS、CPU、EOF 和目录 p95。
2. Model Host 冷启动至少采样 5 次，分开报告编译、Keychain、Usage 初始化和首个快照耗时。
3. Extension 分别输出 `newtab`、`files`、`apps`、`model-settings` 的发布依赖体积。

### P1：真实浏览器生命周期

在干净 Chrome profile 中重复 30 分钟：打开/关闭文件页、切换空间主题、增删 AI Widget、打开/隐藏内置模块。
记录 Renderer/Host RSS、CPU、Native Port、监听器、interval、Blob URL 和 loopback 端口数量；结束时空闲 60 秒，按性能标准的 RSS 增长 ≤15% 判定，资源数量应回到预热基线。不得用进程重启或强制 GC 制造回落。

### P1：数据规模与取消

使用 50 万条用量事件和 10,000 项目录，验证查询 p95、峰值内存、取消后的任务数与连接数归零。
任何超时、取消或页面关闭都必须停止后续依赖请求，不能把未知结果记为成功。

### P2：平台证据

macOS arm64/x64 和 Windows 分别执行安装、升级、修复、卸载保留数据、Native EOF 与空闲 CPU 验收。
缺真实设备或签名时标为 `unknown`/`blocked`，不以模拟器或静态产物代替。

## 门禁

```sh
rtk npm run standards:check
rtk npm run extension:check
rtk npm run perf:check
rtk npm run perf:files
rtk env -u CARGO_TARGET_DIR cargo fmt --check
rtk env -u CARGO_TARGET_DIR cargo test --workspace
```

若性能变化来自业务修复，记录同一设备、同一构建、同一数据集的 before/after；没有可比证据就不宣称优化完成。

## 进程生命周期整改与交接（2026-10-04）

### L0 · 目标、授权与事实边界

本节是待实施方案。用户要求整体检查启动、退出、僵尸进程、内存和资源回收，并将方案交给另一位 Agent 实施；本次只完成代码审查和隔离测试，未实施以下业务修复、未终止正在使用的进程。实施状态唯一记录在根 ROADMAP.md。

目标是把分散的生命周期责任整理为所属 Host 内的明确模块，修复无界积压、缺少 reap、阻塞退出和任务越过 owner 生命周期的问题。沿用 ADR-0031、ADR-0020、ADR-0033、ADR-0035、ADR-0037 的现有进程和领域边界，不新增常驻 supervisor、系统服务、登录启动项、统一业务 daemon、第二 IPC 或第二数据权威。

实施前读取 AGENTS.md、ROADMAP.md、docs/README.md、现行 technical/01～05（含端口注册表）及上述适用 ADR。涉及模块或 Extension 时补读对应规范。保留工作区现有修改；尤其不得覆盖本轮更新按钮、模型路由、单输入桥接和投资模块的改动。此次整改不实施 CLIProxy v8 迁移、不手动升级内核、不修改用户数据、Keychain、生产注册或签名产品源，不默认提交、推送、安装或发布。

审查证据及限制：

| 编号 | 当前证据 | 实施入口 |
|---|---|---|
| F1 | SDK `NewManager(buffer)` 未使用 buffer，Publish 向 slice 追加；队列无容量上限，Stop 不等待 dispatcher，出队未清空旧槽位 | `third_party/cliproxyapi/sdk/cliproxy/usage/manager.go` |
| F2 | Hub 同步逐客户端广播；Writer 持锁执行阻塞 I/O；Usage Close 无期限等待 worker，可能形成慢客户端 → 广播 → 用量 worker → Close 阻塞链 | `model-host/main.go`、`internal/nativeio/nativeio.go`、`internal/usage/plugin.go` |
| F3 | 打开应用、终端、目录只有 Start，没有 Wait/reap；macOS/Linux 长期存活父进程下存在僵尸风险 | `model-host/internal/agentclients/launch.go`、`internal/authfiles/manager.go` |
| F4 | 日志采集无 context，Engine Close 未 join；推理、OAuth、更新取消与最终等待未统一；accept channel 发送及连接 Read 无退出联动，客户端数量未设上限 | `model-host/main.go`、`internal/host/engine.go`、`inference.go`、`kernel_handlers.go` |
| F5 | dev 按 debug/release 路径终止全部匹配进程，250 ms 后 SIGKILL；不能证明实例失效或属于当前启动会话 | `extension/dev.mjs` |
| F6 | Files cancel 后 join 无统一期限；同步 locate 不接入任务取消；App accept 仅检查 shutdown 标记，实际退出依赖进程级兜底 | `crates/native-file-host/src/{main,session,search,dispatch}.rs`、`crates/app-runtime/src/{http/mod,controller/stopping}.rs` |
| F7 | Windows singleinstance 忽略目录参数，使用固定命名管道；开发与正式命名空间隔离不完整，未做 Windows 实测 | `model-host/internal/singleinstance/instance_windows.go` |
| F8 | 日志遍历限制匹配文件数和读取字节，但未限制遍历节点、目录扇出及错误列表总量，也不能取消 | `model-host/internal/usage/collector*.go` |

2026-10-04 隔离 Go 生命周期检查报告 8 项通过、2 个 package，覆盖单实例、非驻留最后连接关闭、常驻 EOF、浏览器退出和桥接重启。项目进程及直接子进程的当次快照未见 Z；单次 RSS 不能证明无泄漏。慢客户端、无界队列、启动器 reap、采集中退出、Windows 实机及 30 分钟真实 Chrome 循环仍待补证据。不得把历史绿测试写成本节整改已经完成。

### L1 · 生命周期 owner 与模块边界

| 对象 | owner 与责任 | 保持的行为 |
|---|---|---|
| Chrome bridge | 单一 stdin reader、管道、直接 worker 子进程及 Wait；重启沿用同一输入流 | 保留退出码 75 换装重启和有限重试；不因 Chrome bridge 退出杀掉用户允许的常驻 worker |
| Model worker | 单实例锁／控制 listener、客户端、Engine 和后台任务 | resident=false 且最后合法客户端关闭才退出；顶栏持有的 stdin 也是合法客户端，不能只数浏览器页面 |
| Model Engine | Proxy、只读 AI 查询服务、Usage、采集、推理、OAuth、更新任务 | 配置和 Secret 归属保持；gateway stop 与整个 Engine close 是不同动作 |
| Files Host | 页面 Port、watch、导入、文件任务及工具子进程 | EOF 后取消、回收；不改变路径授权及文件事务结果 |
| App Runtime | 当前 appId 的 lease、token、HTTP、模块、业务任务和 DB | 单 appId、动态端口、最多 4 个活动 Runtime；EOF 后 2 秒内退出 |
| 顶栏 | 自己启动的 NSTask、stdin、输出 drain、订阅和请求 | 顶栏退出关闭自己的连接；用户开启的 resident 保持，不误杀其他客户端 |
| 开发 launcher | 当前仓库开发产物及本会话子进程 | 2026-10-06 用户明确恢复启动前自动停止当前仓库 debug/release 白名单 Host（含其他 dev／Chrome 连接与共享 worker）；核验路径与 PID 启动时间，先 TERM／有界等待再 KILL，最后复查占用；正式安装与其他项目不在范围。Ctrl+C 仍清理本会话子进程 |

建议在 Model Host 内建立 `internal/lifecycle/`，只容纳有真实调用方的任务登记、进程启动／reap、取消和等待原语；clientHub 另按连接职责拆成局部文件。业务数据、模型路由、resident 判断仍归 Engine／现有领域模块，不能搬进通用 supervisor。Go 内部接口最少表达：登记任务、拒绝 stopping 后新任务、取消、按共同 deadline 等待、记录退出结果；实现用已有 context、sync 和标准库，不引入新依赖或泛化任务框架。

Rust 分别在现有 Files session 和 App controller 内收口。只有证明两个以上生产调用方确实共享稳定逻辑，才考虑已有 core crate 的小原语；不预建跨语言进程 SDK。顶栏复用 NSTask terminationHandler，不引入新的 resident agent。

每个资源登记用途、owner、取消方式、等待方式和释放时机。状态为 running → stopping → stopped，Close／shutdown 幂等；进入 stopping 后不再接收任务。业务锁只用于短暂换状态／取出 handles，不持锁等待网络、子进程、广播或 join。任务的登记与 stopping 判定必须原子，避免 Add/Wait 竞态。

### L2 · 启动、异常恢复与子进程回收

1. macOS/Linux 继续复用现有单实例锁和 socket。取得实例锁后，才能处理该命名空间的失效 socket；锁仍被持有时先有限等待连接／握手，连接失败返回明确错误，禁止因此杀掉持锁进程。锁文件保留稳定 inode，不能删除重建制造双实例。
2. Windows 管道名称由当前用户身份与规范化配置命名空间派生，开发／正式隔离，同时核对 ACL、并发启动及输入目录长短、大小写／路径等价。旧固定管道不可静默兼容连接到未知实例；升级时遇合法旧 worker，返回可操作的重启提示，不强杀。不恢复固定 App Runtime 端口。
3. 所有 Start/spawn 点按表审计，涵盖 Agent client、OpenDir、worker、内核 git/tar/go、mdfind、pdftotext、launcher 和顶栏。短启动器异步等待并收集退出结果；长期用户应用异步 reap，但不绑定“Host 关闭就终止应用”。Wait 与释放 child handle 各只执行一次，处理 Start 失败、stdin/输出管道失败、取消和自然退出竞态。
4. 僵尸已经退出，需由父进程 reap，不向其发送信号试图释放堆内存。PPID=1、运行时间长、同名或健康检查暂时失败都不能单独证明失效。只终止当前 owner 创建、身份重新核验一致且策略允许终止的任务；外部用户应用、其他会话和用户允许的常驻实例不能顺带清理；开发启动的明确授权范围按下一条处理。
5. 2026-10-06 用户明确恢复 dev 启动自动停止旧 Host：范围只包含当前仓库 debug/release 精确可执行路径白名单，覆盖 Chrome 启动的连接、共享 Model worker 与旧 dev 顶栏。先停客户端／顶栏，最后停 worker；每次发信号前核验路径、命令与启动时间，TERM 后等待最多 2s，仍存活再核验并 KILL，最后复查占用，未释放或新占用仍阻止构建／重密封。其他项目、正式安装目录、路径前缀伪匹配与 PID 复用不得终止。Ctrl+C 仍只清理本会话 children。恢复信息不是额外终止授权，不自动调用 sudo 或修改生产注册。

### L3 · 运行期间的有界资源与计费完整性

**SDK 用量队列与持久化。** 保持公开 SDK 方法和真实计费原子／去重口径。让 constructor 容量真实生效，默认沿用 512；为记录尺寸和总字节设置上限，采用有界 FIFO，消费后清空引用。限制统计元数据长度，移除非必要响应 body 和 context 引用；不能截断金额、tokens 或身份造成错误计费。与 Natives 的 512 槽队列一起核算总容量，避免仅修其中一层。满队列采用可取消背压，且消费进度不能依赖客户端广播。

已完成供应商请求的用量不得因 downstream request context 取消而丢弃。producer 停止、队列 drain、dispatcher join 和 plugin Close 的顺序归 Engine 生命周期；gateway 重启不反复注册 plugin，不重开已关闭的 dispatcher，Stop／Start 的并发和重复调用必须测试。

先复用现有 Usage Store 和唯一写入 owner，在持久化完成后发送可合并的更新通知。不能把“进内存队列”当成“账单已持久化”。验证最大队列／慢存储能在退出预算内保存已接收用量；若实测不能兼顾，必须在编码该路径前向用户提交有证据的持久化／恢复契约方案（例如同 owner 的脱敏恢复日志），不能暗自加第二 DB、schema 或磁盘队列，不能静默丢记录／虚报完成，也不能擅自放宽 EOF 预算。本节不授权数据迁移。

**客户端与广播。** 控制连接采用有限 admission 和连接关闭联动；初始实现上限 32，测试达到上限及第 33 个连接被拒绝，不新增配置 UI。每客户端独立有界输出，状态通知按事件类型合并；请求响应不可静默丢弃，超限必须关闭并返回现有结构化错误。慢客户端写超时后只移除该客户端，不阻塞其他客户端和 Usage 写入。socket 可设置写 deadline；stdio 要用关闭自有写端等可解除阻塞的方式，不能用“额外 goroutine + 超时返回”留下旧 goroutine。事件队列初始上限 64／客户端、总输出字节 ≤2 MiB／客户端、单次写等待 ≤250 ms；数值是本次内部起点，需提供最大占用实测，不得扩大已有产品资源预算。

**后台任务。** 采集、OAuth、更新属于 Engine；推理／会话请求还要归属发起客户端或既有 broker generation。复用现有 owner 校验，不相信页面提供任意 owner。断开某客户端只取消其任务，不误取消其他客户端；最后非驻留 owner 关闭或整个 Engine close 才取消 Engine 任务。ctx、done／WaitGroup 与清理对称；过期结果定期由现有活动触发回收或任务完成时回收，不加永久扫描 timer。

采集保留连接建立触发、10 秒防抖、增量游标和真实数据行为；不能为降 CPU 删除自动采集。context 贯穿来源、遍历、解析和 DB 操作；限制访问节点、文件、读取字节、单条事件及错误列表，达到限制明确 partial/budget_exceeded。现有每来源 5,000 文件／512 MiB 读取限制不能当作 RSS 预算，解析按流／小批次进行；游标只提交完整且成功持久化的记录，取消后重扫不漏计或重复计费。

**内存回收与观测。** 清空完成任务、消费槽位及关闭客户端引用；关闭 HTTP body、idle connections、timer、watch 和文件句柄。审计 SDK goroutine、refresh watcher、默认 Usage manager 在 gateway 多次启停后的数量。按现有脱敏日志输出资源计数／退出原因和期限，不新增 unauthenticated 诊断 HTTP、UI 监控页或永久轮询。RSS 超预算先定位 retain/backlog；禁止通过定时强制 GC、定时重启、提高预算或关闭功能掩盖问题。

### L4 · 确定性退出与数据安全

整个 shutdown 使用一个绝对 deadline，各阶段共享剩余时间，不能多个局部超时相加超过契约。非驻留 Model worker 从最后合法连接断开起 ≤1 s，Files/App 从 stdin EOF 起 ≤2 s；bridge 退出与 worker 退出分别采样。resident=true 时关闭某页面只清理连接及该 owner 的资源，用户关闭 resident 后仍有顶栏／其他客户端则继续按正常 owner 策略运行。

统一顺序：

```text
原子进入 stopping／拒绝新任务
→ 停止 listener 接收并关闭自有客户端，解除 accept/read/write 阻塞
→ 取消 owner 任务和 provider I/O，撤销 App bearer
→ 停止用量 producer，持久化／排空已接收记录
→ 等待任务、dispatcher 和自有短命子进程
→ 关闭数据库／网络客户端／watch／timer
→ 释放实例 lease／锁，退出进程
```

Model：listener 和 clients 先于可能阻塞的 Engine Close 收口；accept channel 发送必须能被取消，未交付 connection 当场 Close。审计 gatewayMu 与 Runtime.Start/Stop 的长锁，已有 5 秒 close timeout 不能代替 1 秒契约。任务完成／退出前不能再次写已关闭 store、发广播或把 stopped 改回 running；Stop 失败不可吞掉。

Files：locate 与长操作纳入现有有界任务／取消机制，长目录遍历检查 cancellation 和总工作量，不只限制命中数。Watch、导入、copy/archive/search 等 sibling 路径都核验；mdfind 输出流式限额，不先完整 wait_with_output 再检查长度；pdftotext 读到 EOF 后仍要限制 child wait，并 join 读取线程。超时 kill 后仍 wait/reap。避免 deadline 到期后后台任务继续改用户文件；保留事务／staging 的恢复与清理约定，不宣称强杀即完成写入。

App：可主动停止 accept，登记 HTTP／业务 threads 并等待；用 RAII 保证请求计数在异常返回时回落，保证 panic 不占掉并发槽。模块取消和 DB 持锁路径纳入 deadline，不改变模块数据 authority。保留 2 秒进程级 watchdog 作为兜底，但兜底触发要记录 timeout 阶段，不能记作优雅释放成功；不能每次重复 shutdown 再生成 watchdog 线程。只有正常退出完成资源回收，或明确报告超时恢复路径，才能交付。

顶栏：核对 launch 失败／服务就绪超时／重复 ensure／NSTask terminationHandler 的并发，关闭自身 stdin 和 drain handlers，不清理其他 session 的 worker。SIGTERM、stdin EOF、请求异常、初始化半失败均复用适用 shutdown；SIGKILL 无法执行 cleanup，依赖 OS 释放锁和 SQLite／文件恢复，下次启动验证且不删除真实业务数据。

### L5 · 分阶段施工清单

按以下依赖顺序执行，阶段完成后更新实际证据；不得先大搬文件再猜接口。可以由一个 Agent 全部实施；如另行委派，主 Agent 必须提供本节任务编号和允许／禁止范围，并亲自核对 diff 与证据。

| 阶段 | 目标、入口与允许范围 | 依赖／已确定决策 | 验收与交付 |
|---|---|---|---|
| T0 | 核对 F1～F8、所有 spawn／cancel／close caller，采集隔离 baseline；只读业务代码，夹具放 `.tmp/host-lifecycle/` | 当前 dirty diff 优先，不假定历史结论仍适用；不用真实用户库或生产 Keychain | owner／资源／调用链表、可复现失败测试、构建与版本信息；标明未复现项 |
| T1 | 在 Model 内建立最小 lifecycle 原语，修 F3 的启动器 reap；范围为 Go lifecycle、agentclients、authfiles、process 和对应 tests | T0；短 helper 与用户外部应用分别处理；保持单 reader／75 restart | L01、L02、L03、L13；Wait 恰一次、失败不残留；实际模块接口与改动说明 |
| T2 | 修 F2、F4、F8：Hub、nativeio、Engine、采集／推理／OAuth／更新及 close tests | T1；L1 owner 和 L3 连接／输出上限；不改变路由与更新行为 | L04～L07、L10、L13；慢客户端隔离、任务取消且 join、退出期限与存储时序 |
| T3 | 修 F1 的 SDK 队列及 Natives 用量链，范围为 usage manager/plugin/store 生命周期、对应测试和 fork patch | T2 广播不阻塞持久化；遵循 L3 计费完整性门槛，不自行迁移数据 | L08、L09、L13；最大容量／字节／FIFO／drain／重复启停证据，SDK 补丁重放一致 |
| T4 | 修 F5、F7，并核对顶栏；范围为 singleinstance、dev launcher、NSTask 生命周期及相关测试 | T1/T2；保留 namespace／权限；开发启动按 L11 清理当前仓库白名单 Host | L02、L03、L11、L12；开发重启明确覆盖同仓库旧 Host，其他项目／安装实例与 PID 复用不误杀，Windows 平台未做必须标明 |
| T5 | 修 F6：Files session/search 和 App controller/http；只修改直接生命周期及测试，不改 Fund 业务 | T0 证据、L4 deadline／恢复契约；Rust 在主 checkout 单 target 验证 | L10、L14、L15；正常／故障 EOF、锁释放、请求计数、取消和重开一致性 |
| T6 | 最终集成、真实 Chrome soak 和平台复核；范围为现有 perf／浏览器脚本、证据与受影响文档 | T1～T5 全部集成，不并行叠加 benchmark 与 build/race；不改预算／断言掩盖缺陷 | L01～L16 逐项结果、before/after 原始样本、门禁、未验证／阻塞项和交付报告 |

每阶段失败先定位再修；同方向两轮未解决时重新核对契约与证据，不叠加卡住的测试进程。T3 的持久化／退出矛盾若出现，只阻塞依赖的计费路径，其余独立整改继续。

### L6 · 验收矩阵

测试使用当前源码构建的隔离 Host、假 helper 和测试数据库。API 替身验证失败边界，真实 Chrome + 实际 Native 验证用户流程，两者分别标注。不得为了造僵尸启动真实终端／用户应用或清理现有进程。每个故障用例设总 timeout、记录 own PID／identity，并在 finally 只清理自己的夹具进程。

| 编号 | 场景 | 必须证明的结果 |
|---|---|---|
| L01 | 连续启动并结束 100 个短 helper；启动失败、输出失败、取消与自然结束并发 | 每 child 恰一次 Wait；测试父进程存活时无 Z／未释放 child handle；不阻塞 UI，外部应用不被 Host close 杀掉 |
| L02 | 20 个同 namespace 并发连接；两个 namespace 同时启动；崩溃后残留 socket | 每 namespace 一个 worker；健康实例复用；拿不到锁不删 socket；crash 后能安全重开 |
| L03 | 非常驻多个页面／顶栏；常驻开关；浏览器正常退出与被终止 | 最后合法客户端关闭才退出；常驻 worker 保持单实例；关闭设置可生效；没有误杀其他 owner |
| L04 | 客户端不读输出、只发半帧、连接达到 32 上限、达到输出字节上限 | 其他客户端和 Usage 持久化继续；第 33 连接被拒；坏客户端及时移除；goroutine／句柄回落 |
| L05 | listener 正在 Accept、channel 正在送 connection、writer 正阻塞时 shutdown | 阻塞被解除；未交付 connection 关闭；重复 Close 幂等，结束后无后续回调 |
| L06 | 采集遍历／解析／入库时 EOF；超大单文件、目录扇出、很多坏文件 | context 生效、总节点／字节／错误列表有界、退出 ≤1 s；游标不越过未提交记录，重扫不重复或漏计 |
| L07 | 推理、OAuth、更新过程中 owner disconnect／SIGTERM；另一个 owner 仍在工作 | 仅所属任务取消并等待；保留更新事务恢复；不关闭 store 后继续写、不杀其他客户端 |
| L08 | 用量产速大于消费；下游短时阻塞；512 容量边界；不同记录尺寸 | 两层队列计数／字节有界、背压可观测、出队引用释放，真实 tokens／费用／去重正确；通知合并不丢账单 |
| L09 | 满队列 close、数据库 busy、producer 与 Stop 竞态；gateway 启停 50 次 | 已接收用量全部持久化或明确失败／恢复状态；无静默丢失；dispatcher 数量回基线，Close 不无限等、不自动复活 |
| L10 | Model 1 s、Files/App 2 s：正常 EOF 和上述故障负载 | bridge 与 worker 分别记录退出时间；deadline 不按阶段叠加；正常／watchdog／失败清楚区分，超预算不能通过 |
| L11 | dev 启动／Ctrl+C，有当前仓库旧 dev／Chrome Host／worker、release、其他项目、PID 复用及失效登记 | 启动沿 2026-10-06 用户授权自动停止当前仓库精确白名单 Host；先停客户端／顶栏，再停共享 worker；TERM／2s 等待／身份复查后 KILL，未释放或新占用仍阻止构建。其他项目／正式安装不受影响；Ctrl+C 只清理 own children |
| L12 | Windows 不同用户／dev-prod 配置 namespace、ACL、同时启动与旧管道 | 正确隔离和拒绝未知旧实例；真实 Windows 证据单列，模拟／交叉编译不得算平台通过 |
| L13 | 更新按钮同主版本换装失败／成功重启；重复更新；路由文本和凭据回归 | 保留 75 和同一 stdin reader；没有重复 worker／遗留构建进程；现有更新／路由安全测试不退化，v8 仍按真实兼容性处理 |
| L14 | Files locate/search、pdf helper 超时、watch/import/archive/batch 处理中 EOF | 工具输出与 wait 有界，kill 后 reap，读取线程结束；task／watch／导入归零，重开后文件与事务状态一致 |
| L15 | App DB busy、HTTP 半包／慢请求／异常返回、重复 shutdown、至少 50 次打开关闭 | 停止 accept、token 撤销、计数释放，≤2 s；同 appId lease 可重取；watchdog 有可识别失败记录、没有数据半提交 |
| L16 | Release 对比与 30 分钟 Chrome 循环 | 下述基线、资源预算与证据要求全部满足；未验平台不标通过 |

L16 细则：

- 同设备／系统／Chrome／源码构建类型／数据集，Release before/after 各至少 5 个样本；记录二进制摘要、冷启动、p50/p75/p95、峰值及稳态 RSS、CPU、PID 树、线程／goroutine、FD、连接、Port 和队列计数。当前 `perf:model-host` 构建 debug，不能单凭该脚本声称 Release 通过；增加隔离 Release 采样入口或准确验证现有入口，仍使用唯一 `target/`。
- 真实 Chrome 独立 profile，预热 5 分钟后循环 30 分钟，每 10 秒采样；包含 Home、Files、Model 设置、增删 AI Widget、顶栏、模块打开／隐藏／关闭及 resident 开关。结束后空闲 60 秒，RSS 增长 ≤15%；资源计数回到允许常驻资源的预热基线，非驻留关闭对象归零。持续增长趋势仍需解释，不能只比较两个端点。
- Model 空闲 RSS ≤256 MiB、CPU ≤5%、非驻留 EOF ≤1 s；Files Release RSS ≤12 MiB、CPU ≤0.5%、EOF ≤2 s；App EOF ≤2 s、每 appId ≤1 实例、同用户活动实例 ≤4、未运行模块零专用资源，Fund Ready RSS 不回归 >10%；总空闲 CPU 等继续遵循性能标准。
- 用量 10k／100k／500k、目录 10k／100k；覆盖查询、取消、队列峰值及回落，10k 目录首 100 项查询 p95 ≤50 ms。真实用户账本／日志不能作为 fixture，不发送真实持仓或供应商请求。隔离环境必须同时避免扫描真实 HOME 日志和访问真实 Keychain，不能仅换 configDir 就宣称完全隔离。
- macOS arm64、macOS Intel、Windows 分别记录实际证据；没有设备／权限的项明确 blocked/未验证，不伪造通过。RSS 正常不会代替 handle／thread／process 的回收证据。

### L7 · 检查命令、证据与执行提示词

开发中先运行最小相关失败／回归用例，Go 定向 race，Rust 先 no-run 区分编译与执行。Rust 所有验证从主 checkout 根运行，移除 CARGO_TARGET_DIR，不设第二 target、不 cargo clean。测试 SDK 修改时进入其既有 module 跑受影响 package，补丁和 `NATIVES_FORK.md` 同步，核验补丁可重放；当前依赖仍为 v7，路径不能照搬 v8。

生产代码实施后的最终门禁：

```sh
rtk npm run model:host:check
rtk env -u CARGO_TARGET_DIR cargo fmt --check
rtk env -u CARGO_TARGET_DIR cargo test --workspace
rtk npm run perf:check
rtk git diff --check
```

检查所有改动 Go 文件的 gofmt 和相关 package race；`perf:check` 当前包含 extension:check、perf:files、apps:check 与 standards:check，同次通过可覆盖重复门禁。涉及模块 UI 再执行 modules:ui:build 与 modules:ui:check 并核对真实资源；顶栏改动要复用实际 clang 构建／脚本测试和真实退出路径，不能运行 `npm run statusbar` 无意启动用户顶栏。现有浏览器脚本能覆盖的路径优先复用，新增测试只针对缺口。

临时日志／夹具在 `.tmp/host-lifecycle/`；需要交付和复用的验证证据归现有约定位置，建议 `docs/development/evidence/host-lifecycle/`，只存脱敏原始样本与摘要，不提交二进制／真实用户数据。至少交付：需求 F1～F8 → T0～T6 → L01～L16 对应表、实际模块接口、源代码与 fork patch、失败复现及修复后证据、所有命令与退出码、Release 对比／soak 原始数据、未验证项及根因限制。只更新受影响的现有方案、导航和 ROADMAP，不制造重复进度源。README 仅在使用方式改变时同步。

将以下内容交给实施 Agent：

> 请实施 `/Users/ldh/Downloads/project/AiNative/Natives/docs/architecture/application-performance-remediation.md` 的“进程生命周期整改与交接”章节，按 T0～T6 顺序完成，逐项核验 L01～L16。先读当前 AGENTS.md、ROADMAP.md、docs/README.md 和适用规范／ADR，检查 dirty diff，保留其他人的改动及现有更新按钮／路由修复。目标是各 Host 内的生命周期模块、子进程 reap、有界队列和背压、客户端／任务取消与等待、确定性退出、开发与正式实例隔离。禁止新增通用 daemon、第二 IPC／DB、登录启动项，开发启动只允许 L11 明确授权的当前仓库白名单清理，禁止按进程名清扫或终止其他项目／正式安装实例，不手动升级内核、不实施 v8 迁移、不修改真实用户数据／Keychain、不默认提交／安装／发布。先稳定复现，再最小修复与集成；计费数据不可静默丢失，若持久化与退出预算实测冲突，先提交具体契约方案，不绕过门禁。真实 Chrome、Release、Windows 等证据分别报告，未运行不能标通过。最终交付实际改动、门禁、逐项验收证据及未解决事项，并更新 ROADMAP。
