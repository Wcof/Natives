# ADR-0036：空间投资卡的短命只读投影查询 owner（Space Investment Card Snapshot Owner）

- 状态：已接受（accepted，2026-09-29）
- 决策者：产品方（用户）
- 关联文档：ADR-0031（统一内置应用 Runtime）、ADR-0032（动态端口）、ADR-0035（退役固定端口）、
  `docs/standards/technical/06-built-in-modules.md`、`docs/standards/technical/02-security.md`、
  `docs/standards/technical/04-performance.md`、`docs/standards/product/01-positioning.md`
- 方案来源：`/Users/ldh/Downloads/project/plan/02-空间投资数据契约.md`（2026-09-29）

---

## 1. 背景

ADR-0031 规定内置模块业务数据只能由该模块的 App Runtime 进程读取，`extension/app.html`
是典型 owner page。空间（`space.html`）新增的投资卡片需要展示基金持仓等真实业务摘要，
但空间卡片本身由扩展内置 renderer 渲染，不能：

- 直读 `~/.natives/apps/fund/data/fund.db`（打破 Fund 唯一业务 writer）；
- 把金额复制进 Core / Chrome storage（形成第二数据权威）；
- 经 Service Worker 保活模块进程（违反 R-P6 / Service Worker 无 Native Port 红线）。

同时，当前代码存在生命周期缺口：`space.js` 的 `leaveEmbeddedView()` 清除嵌入引用但不
释放会话；`createEmbeddedAppSurface()` 的 5 分钟空闲回收只在页面 hidden 时生效。卡片
短命查询若与孤儿嵌入会话共存，会同 appId 锁竞争。

## 2. 决策

### 2.1 space.html 可作为一次性只读投影查询 owner（窄例外）

在满足以下全部条件时，**`space.html` 页面 controller 可以作为 `fund` 模块的一次性只读
投影查询 owner**，临时成为 App Runtime Protocol v2 会话的持有者：

1. **可见性门控**：页面可见、当前 Workspace 存在至少一张未隐藏的投资卡、且本页没有
   正在释放的 Fund 嵌入详情会话。
2. **激活校验先行**：先经 Core `apps:get(fund)` 取得可信 `runtimeHost` 与
   `productGeneration`，确认模块已启用；未启用或未登记直接进入不可用状态，不连接 Runtime。
3. **readiness 前置**：`app:handshake` 后先调 `app:data_status`；仅当响应 `readiness=ready`
   才允许 `app:start`。`uninitialized / migration_required / recovery_required /
   incompatible / unreadable` 一律断开并显示「请打开投资完成初始化／更新或检查数据」，
   卡片绝不触发建库、迁移或 journal 恢复；`readiness` 字段缺失按 unsupported 处理。
4. **固定只读命令**：唯一允许的业务命令是 `app:card_snapshot`，参数仅限 `app:start`
   返回的 `instanceId` 与 `generation`；无任意 route、SQL、路径或自定义字段。
5. **一次响应立即断开**：收到响应（或超时/失败）即断开 Native Port，Runtime 经 stdin
   EOF 在 ≤2 秒内退出。页面级所有投资卡共享这一次查询；无后台轮询、无自动重试。
6. **单实例竞争让步**：收到 `APP_RUNNING_ELSEWHERE` 时显示「投资模块正在另一页面使用」，
   不广播停机、不自动重试、不为展示卡片抢占其他标签页的 Fund 会话。

### 2.2 不授权的内容

- 不创建通用 Widget→模块 RPC 平台；`app:card_snapshot` 是当前唯一授权的卡片命令，响应
  上限 64 KiB（严格小于 Native 帧 512 KiB），超限按错误处理。
- 不授权空间页面渲染期间常驻连接、定时刷新或 BFCache 之外的持久化；刷新浏览器后本页
  内存快照消失，不写 Chrome storage 或 Core。
- 不授权 `app:card_snapshot` 响应包含逐笔流水、个人路径、账户标识、Secret 或 HTML。
- 不授权绕过 readiness 直接 `app:start`；启动过程仍可能设置 WAL／元数据，因此该路径
  只能称为「短命只读投影查询」，不是操作系统级纯只读进程。

### 2.3 生命周期收口（同 P0 工程修正）

- `space.html` 从 Fund 嵌入详情返回 Workspace 时，必须先显式 `dispose()` 嵌入会话
  （停止 idle 计时、断开 Native Port、移除本 surface 的监听器），完成后才允许卡片查询。
- 卡片跳转 Fund 详情前，先取消进行中的卡片查询并断开 Port。
- 嵌入会话 owner 必须持有并释放自己的 `visibilitychange` 监听器；页面隐藏 60 秒回收
  （R-P8）优先于代码注释中的 5 分钟意图。
- 迟到响应按页面级 query generation 丢弃；`pagehide`、卡片全部移除、Workspace 切换、
  隐藏、超时与 Native Port 异常都取消本次查询并释放 Port。

### 2.4 深链导航意图

卡片「查看持仓／查看资产／查看目标」使用现有 `space.html?app=fund` 同页嵌入入口，附加
固定 `view` 标记（白名单 `holdings | assets | goal | allocation`）。Shell 在 iframe 握手
完成后转发通用 `{type:'navigate', view}` 纯导航意图；未知值由模块回到默认视图。意图不含
URL、账户数据或 HTML。

## 3. 后果与门禁影响

- `docs/standards/technical/06-built-in-modules.md` R-A6 的「`app.html` 是唯一 owner
  page」表述按本 ADR 修订：`app.html` 是完整模块界面 owner；`space.html` 仅在本 ADR
  约束下作为一次性只读投影查询 owner。
- `docs/standards/product/01-positioning.md` R-P3（Widget 只做轻量投影）补注：投资卡
  经 `app:card_snapshot` 固定命令取数，属于「现有领域 client 查询真实数据」的授权形态，
  页面级去重按 R-P4 执行。
- `docs/standards/technical/02-security.md` R-S2 补注：`app:card_snapshot` 参数白名单、
  64 KiB 响应上限与 readiness 前置是 Host 边界校验的一部分。
- 性能验收：冷启动到投影响应目标 2.5 秒（p75），15 秒为异常终止上限；真实 Chrome 需
  实测「新标签页＋单卡」「20 张卡共享一次查询」「Fund 详情已开启」「空间与详情往返」
  「页面关闭」场景与 EOF ≤2 秒。若实测证伪本路径，回到产品决策，不得以旁路读库替代。

## 4. 明确不做

- 不做第二写实例、不做共享会话总线（若锁冲突实测频繁，另立架构决策）。
- 不在 Core / Extension 通用层出现 `appId == fund` 业务分支；卡片 key 白名单是扩展
  构建期注册的静态 renderer 目录，不属于运行时业务分支。
- 不把投影快照写入任何持久层。
