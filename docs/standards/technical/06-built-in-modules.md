# 技术 06 · 内置模块

> 版本：5.0.0 · 日期：2026-09-14
> 依据：ADR-0027、ADR-0029、ADR-0031、[`../../contracts/managed-app-contract.md`](../../contracts/managed-app-contract.md)

#### R-A1 · 模块随完整产品交付

- **等级**：MUST
- 所有官方内置应用模块代码、UI 和固定资源在构建期进入完整 Natives 安装包。
- 模块源码归入 Natives Monorepo 的 `modules/<appId>`，作为 Library crate 静态编译入统一产品级 `natives-app-runtime`。
- 不存在模块级发现、传输、独立安装、独立更新、独立卸载或独立 Release。
- 新增/删除/升级模块只能发布新的完整 Natives 版本。

#### R-A2 · 产品身份唯一

- **等级**：MUST
- 模块使用稳定 appId 作为内部注册与数据身份，不创建独立 `.app`、Dock、LaunchServices 或产品安装记录。
- App Center 始终显示产品声明的模块，并提供打开、显示/隐藏、偏好和数据管理。

#### R-A3 · 产品安装与用户初始化分离

- **等级**：MUST
- 安装器只放置签名产品文件、统一 App Runtime 可执行文件与固定模块资源，以及最小浏览器注册，不以 root 写用户 DB/activation 或运行业务迁移。
- 用户应用根目录（`~/.natives/apps/<appId>/`）严禁保存任何可执行文件（彻底废弃 `runtime/<version>/app` 模式）。
- 第一次打开模块才以当前用户身份初始化或迁移数据；断网可完成。
- 更新、重装、修复保留显示偏好、用户数据和 Keychain。

#### R-A4 · Product Manifest v2 是完整性来源

- **等级**：MUST
- 组合清单声明产品版本、统一 `appRuntime`（可执行文件相对路径、字节数、SHA-256、Protocol 版本）以及内置模块列表（appId、entryRoute、moduleApiVersion、dataSchemaVersion、capabilityVersion、UI 资源树摘要）。
- 安装/更新层验证清单、平台签名和实际字节后才激活。
- 页面、模块或请求不能提供安装路径、Host 名称、hash 或 verified 结论。

#### R-A5 · 统一 Runtime 二进制与独立按需进程

- **等级**：MUST
- 所有官方 Built-in App 共享且仅使用唯一受签名的 `natives-app-runtime` 可执行文件；整个产品在操作系统 Native Messaging 目录仅注册唯一的 App Runtime Native Host（`com.natives.app_runtime`，本地开发为 `com.natives.local.app_runtime`）。
- 严禁为任何内置应用生成或注册独立 Native Messaging Host（如废弃的 `com.natives.app.a<hash>`）。
- 每个打开的内置应用实例按需启动一个独立的 `natives-app-runtime` 进程实例（Single Runtime Binary / Multi Process Instance）。一个 Runtime 进程一次只能激活并承载一个 appId。
- 每 appId 至多一个业务实例，同用户至多四个活动 App Runtime 进程。
- App Runtime 进程通过 Native Port 按需启动，在 `127.0.0.1:0` 动态端口提供受鉴权业务与 UI；不得访问 Core DB。
- 未被当前进程激活的模块不占用任何专用堆内存或系统资源（≈0 dedicated runtime memory）。

#### R-A6 · 通用 owner page 与 Protocol v2 握手

- **等级**：MUST
- `extension/app.html` 只按 appId 查询登记、核验状态、连接统一 App Runtime Host（`com.natives.app_runtime` / `com.natives.local.app_runtime`），通过 App Runtime Protocol v2 的 `app:handshake` 显式选定模块并承载 sandbox iframe。
- Extension/Files Host 不包含基金或其他模块的业务分支。
- App iframe 精确使用 `allow-scripts allow-forms`，遵守 `02-security.md` 的 handshake、CORS、token 和限额。

#### R-A7 · 确定性生命周期与进程级回收

- **等级**：MUST
- 安装后不自动运行；打开 UI 才启动实例。
- hidden 60 秒、pagehide、显式停止和 Native Port 断开（stdin EOF）进入同一取消/关闭路径。
- **硬指标：Native Port 断开（stdin EOF）到 App Runtime Process 退出时间 $\le 2$ 秒**。
- 进程退出后，操作系统直接回收全部资源：Module Process = 0, Listener = 0, Timer = 0, Worker Thread = 0, SQLite Connection = 0, Network Client = 0。
- 20 次打开/关闭循环测试无孤儿进程、无孤儿端口、无锁泄漏。

#### R-A8 · 业务数据独立

- **等级**：MUST
- 模块业务数据严格存放于 `~/.natives/apps/<appId>/data/`，由当前承载该模块的 App Runtime 进程唯一写入。
- 模块所有工作目录由 Runtime 注入，禁止模块自行探测系统或推导用户目录。
- Core 只拥有登记、偏好、activation 和产品收据，不创建业务表。
- 内部模块（如持仓、账本、NAV、导入等）通过包内窄接口协作，不拥有安装记录、卡片、生命周期或数据库 authority。

#### R-A9 · 迁移与清数据

- **等级**：MUST
- 模块迁移先备份、写 journal、验证后提交；失败恢复，已接受新写入后不静默回退。
- 清数据与隐藏、停止、修复、产品更新分离；清数据二次确认并可重试。
- Secret 只在 `com.natives.app.<appId>` Keychain namespace。

#### R-A10 · 本地与正式验收分列

- **等级**：MUST
- 本地模式使用隔离身份和开发签名；正式产品拒绝开发 key/fixture。
- 完整候选用真实基金验证；sample 只作为共享底层逻辑 fixture。
- A-Local/B-Local 不能冒充平台签名、公证和发布 Gate。

## 接入步骤

1. 在 `modules/<appId>/` 内实现业务与构建期 UI，Rust 侧实现为 Library crate。
2. 实现统一 `BuiltInAppModule` 契约，注册进 App Runtime 的编译期 `ModuleRegistry`。
3. 声明 appId、协议、权限、版本、数据 schema 和 Product Manifest v2 条目。
4. 静态编译入 `natives-app-runtime`，严禁产出独立模块 executable 或独立 Native Host。
5. 验证首次初始化、迁移、回退、清数据、隐藏/关闭回收（$\le 2$ 秒退出）和真实浏览器 UI。
6. 随完整 Natives 候选执行本地与正式两套 Gate。

## 合规自检

- [ ] 模块没有独立可执行程序或独立 Native Messaging Host。
- [ ] Product Manifest v2 验证实际字节和平台身份。
- [ ] App Center 中模块可见且状态真实。
- [ ] Core、Extension 无模块业务逻辑。
- [ ] 运行和数据隔离、退出 ≤2 秒彻底回收。
