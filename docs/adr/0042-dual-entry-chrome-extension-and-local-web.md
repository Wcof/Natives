# ADR-0042：Chrome 扩展与本机独立 Web 双入口架构（Dual-Entry Architecture: Chrome Extension and Local Standalone Web）

- **状态**：已接受（accepted，2026-10-15）
- **决策者**：产品与架构团队
- **修订**：
  - ADR-0031（修订其“Extension 为唯一业务界面”及“单活动 Surface 独占 Runtime 进程”约束，放行本机受限 Web 页面入口并升级为同 `appId` 多 Surface 共享单个 Worker 实例）
  - ADR-0035（保持 App Runtime 动态端口 `127.0.0.1:0` 决策不变；在 `05-port-registry.md` 中正式登记产品级 Web 入口固定端口）
  - ADR-0037、ADR-0038（将模块推理代理与 Secret Keychain 能力代理抽象为通用平台层，同时服务扩展 Shell 与 Web Shell）
  - ADR-0029（Launcher 支持无扩展普通 Chrome 直接唤起独立 Web 页面）
- **关联文档**：
  - `docs/standards/technical/01-layering.md`
  - `docs/standards/technical/02-security.md`
  - `docs/standards/technical/03-data.md`
  - `docs/standards/technical/04-performance.md`
  - `docs/standards/technical/05-port-registry.md`
  - `docs/contracts/managed-app-contract.md`
  - `/Users/ldh/Downloads/project/plan/memory/dual-entry-migration/01-architecture.md`
  - `/Users/ldh/Downloads/project/plan/memory/dual-entry-migration/02-contracts-security.md`

---

## 1. 背景与问题

Natives 原架构严格定义为“Chrome 扩展驱动的本地桌面产品”（ADR-0020/ADR-0023/ADR-0031）：
- 唯一业务界面是 Chrome Extension 页面（`space.html`、`files.html`、`apps.html`、`app.html`）；
- 通信完全依赖 Chrome Native Messaging 管道，由页面 Native Port 按需拉起 Rust `native-file-host` 与 `natives-app-runtime`，由 stdin EOF 驱动 2 秒确定性退出；
- App Runtime（如 Fund 模块）采用“单进程实例对单页面 Surface”模型，一旦出现第二个页面握手，既有 token 立即失效；
- 普通无扩展环境下的浏览器无法直接使用 Natives 的文件管理、空间、应用中心、投资终端及 AI 路由能力。

为了让未安装 Chrome 扩展的普通用户也能在浏览器中直接访问 Natives，同时保留既有 Chrome 扩展的完整功能与体验，必须支持**“Chrome 扩展＋本机独立 Web 双入口”**。然而，双入口改造不能退化为构建第二套独立后端、建立常驻 Daemon、复制数据库或削弱现有安全与生命周期边界。

---

## 2. 核心架构决策

### 2.1 整体拓扑：单产品二进制与统一领域进程

```text
Chrome 扩展页面 ── Native Messaging Relay ─┐
                                          ├─ 本机服务入口（native-file-host 受限服务模式）
普通网页 ───────── HTTP 会话 / SSE ────────┘     ├─ Files / Workspace / AppStore：原领域 Owner
                                                ├─ 私有通信 ──> 单实例 Model Host（复用已有 Hub）
                                                └─ 私有通道 ──> 每 appId 单个 App Runtime 进程
                                                                    └─ 模块业务与 SQLite 库
扩展 / Web 页面通用模块 Shell ── sandbox iframe ──> 模块动态 loopback 端口
```

1. **唯一可执行文件演进**：
   - 现有的 `native-file-host` 增加受限服务监听模式（`--service`）；
   - 原有的 Chrome Native Messaging 模式作为轻量级 Relay 运行，连接受 OS 锁保护的单实例服务，不再独立打开第二份 Core 数据库写入器。
2. **单一领域权威（Single Domain Authority）**：
   - Files、Workspace、Core App Store 权威由 `native-file-host` 持有；
   - Provider、Quota、AI Key 与 Usage 权威由 `model-host` 单实例持有；
   - Fund 账本、持仓、流水与资讯权威由 `natives-app-runtime`（fund 模块）持有；
   - 严禁因引入 Web 入口而创建 `web-fund.db` 或任何第二同步数据副本。
3. **严格回环监听（Loopback Only）**：
   - 所有服务一律强制绑定 `127.0.0.1`，严禁绑定 `0.0.0.0` 或暴露于局域网/公网。

### 2.2 端口分配与注册（Port Registry）

依据 `docs/standards/technical/05-port-registry.md` 规则，在 8765–8799 区间内正式分配：
- **8770**：Natives 本机独立 Web 正式环境入口（Production Web Entry）；
- **8771**：Natives 本机独立 Web 本地开发/测试入口（Development Web Entry）；
- **8766**：保持 ADR-0033 规定的 Model Host 只读 AI 数据查询端口职责不变；
- **动态端口 `127.0.0.1:0`**：保持 ADR-0035 规定的 App Runtime 纯动态分配端点不变，仅向通过安全核验的 Shell 报告。

### 2.3 Web 安全模型与双重鉴权链路

针对普通浏览器访问，确立双重信任链与零特权泄漏原则：

1. **首次配对与会话交换（Bootstrap & Pairing）**：
   - **自动化启动链路**：Launcher/CLI 生成一次性 256-bit 随机 Bootstrap Token（有效期 <=60 秒），通过 URL Fragment（`#token=...`）传递给打开的页面。前端读取后立即执行 `history.replaceState` 清空 URL，并通过同源 POST 请求交换会话；
   - **手动访问链路**：用户在无扩展浏览器手动输入 `http://127.0.0.1:8770` 时，呈现配对界面。用户从 Natives 诊断或 CLI 获取短期配对码（10 位随机 Base32，5 分钟有效，单个码最多 5 次失败，全实例每分钟限速 10 次）；
   - 严禁通过查询参数、Referer 或静态日志泄露 Token。
2. **会话保持与 CSRF 防御**：
   - 会话通过 Host-only、HttpOnly、`SameSite=Strict` Cookie 保持，Path 为 `/`。正式环境与开发环境使用不同 Cookie 名称；
   - 所有写操作（POST/PUT/DELETE）强制要求：有效 Cookie、精确 `Origin: http://127.0.0.1:8770`（或 8771）、标准 `Host` 头校验、有效 CSRF Token，以及 `Content-Type: application/json`；
   - 拒绝跨站表单提交与未知 Host 请求。
3. **沙箱边界与文件安全**：
   - Web 页面不能通过传本地绝对路径或 Shell 命令执行操作；
   - 文件上传下载通过有界流与临时 capability 凭据完成，继续执行 root 白名单、规范化路径（canonicalize）及 symlink/reparse 越界检查。

### 2.4 App Runtime 多 Surface 会话与租约生命周期

修订 ADR-0031 的单 Surface 独占逻辑，升级为**“单 appId 单进程，多 Surface 独立租约”**模型：

1. **会话标识与鉴权三元组**：
   - `workerEpoch`：每次 Runtime 实例启动生成一次，实例重启旧 token 立即失效；
   - `surfaceId`：每个页面 Shell 单独随机生成，归属于特定的 Native Relay 或 Web Session；
   - `generation`：每个 Surface 内 iframe 重建或导航递增；
   - `module bearer`：32 字节 CSPRNG 生成，最长有效 15 分钟，绑定 `appId + workerEpoch + surfaceId + generation + capabilities`。仅保留在 iframe 内存，严禁透传给父 Shell 或作为通用 Cookie。
2. **租约与生命周期回收（Lease & Graceful Shutdown）**：
   - **租约心跳**：可见状态下的 Surface 每 20 秒发送一次心跳续约，租约 TTL 为 75 秒；
   - **隐藏休眠**：页面隐藏（hidden）后停止普通高频轮询，60 秒后释放非任务租约；
   - **单窗口关闭**：关闭单个 Surface（调用 `DELETE /web/surfaces/{id}` 或断开对应 Native Relay）仅撤销该 `surfaceId` 的资源，不影响其他正在访问该模块的窗口；
   - **最后租约回收**：当某个内置模块的最后一个 Surface 租约释放且无正在执行的保护级写入任务时，进入 60 秒宽限期；宽限期结束未收到新租约则进入优雅停机（shutdown），并在 `<=2 秒` 内彻底退出进程，释放数据库连接与端口。

### 2.5 单一写入器与乐观并发（Optimistic Concurrency）

- 两个入口同时打开时，共享相同的底层数据库与持久化配置；
- 对同一业务记录的并发修改，强制遵循基于 `revision` 或 `version` 的乐观锁机制。当发生版本冲突时，后提交方返回 `409 Conflict`，保留用户草稿并提示最新数据，严禁 Last-Write-Wins 覆盖；
- 空间组件拖拽排序等操作，在 `pointermove` 过程中禁止持久写入，拖拽释放（drop）后触发至多一次逻辑提交。

---

## 3. 规范条目精确修订（Standards Amendments）

本决策对现行技术规范进行限定性修订，未提及的原则与约束继续完全生效：

1. **`docs/standards/technical/01-layering.md`**：
   - **R-T1 修订**：除 Chrome Extension 外，放行由 `native-file-host` 服务模式承载的本机独立 Web 界面。两者地位平等，共享同一套业务控制器与领域协议；
   - **R-T4 修订**：跨信任域通信在 Native Messaging 之外，正式放行经过 Bootstrap/Pairing 认证的本地回环 HTTP/1.1 与 SSE 通道；
   - **R-T5 修订**：服务存活机制支持多连接租约聚合。扩展断开但独立 Web 会话仍活跃时，服务保持运行；所有客户端均离开且空闲 60 秒后触发退出；
   - **R-T7 修订**：Runtime 进程实例与 `appId` 保持 1:1 独占，但允许同 `appId` 下接入多个受信 Surface，由内部多 Surface 会话状态机分发鉴权。
2. **`docs/standards/technical/05-port-registry.md`**：
   - 登记 8770（正式 Web）与 8771（开发 Web）。

---

## 4. 交付与验证退出条件

本地隔离验证使用 `natives_dev` 编译标签的 Model Host 时，可在显式隔离
`NATIVES_MODEL_HOST_CONFIG_DIR` 下，以 `NATIVES_MODEL_HOST_AIQUERY_PORT` 选择测试
回环端口，Tokenusage 使用既有 `NATIVES_MODEL_HOST_AIQUERY_URL` 指向同一实例。
这只隔离既有只读查询面的测试监听；未带标签的正式二进制忽略该覆盖，仍固定
8766。不能借用真实用户 8766 数据或把测试端口作为新的产品服务。

双入口迁移完成的判定标准：
1. 真实 Google Chrome（无扩展配置）能通过 `http://127.0.0.1:8770` 完整使用全部页面；
2. Chrome 扩展继续正常使用，两入口同时打开互不抢占；
3. 完成 V01～V17 真实端到端业务流程测试；
4. 全矩阵 16 种视觉组合（两入口 × 双主题 × 双语 × 宽窄窗）截图审阅通过；
5. 满足 09 规定的性能与资源预算（冷启动 p75 <=2.5s，交互 p95 <=100ms，空闲 CPU <=2.0%）。
