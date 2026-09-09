# 06 · 官方托管应用接入规范（Managed Apps Specification）

> 本规范定义官方托管应用（managed_local App）的交付架构、接入契约、生命周期、数据分账与安全边界。
> 架构基准见 [ADR-0027](../../adr/0027-managed-apps-independent-delivery.md)（取代 ADR-0026 的 Apps 目标决策）。
> 接口唯一来源：[官方托管应用契约 v1](../../contracts/managed-app-contract.md)。
> 历史规范（扩展内置 UI + 共享 Core Host + 纯资源包）见 ADR-0026 历史正文，不再作为新应用接入依据。

---

## 一、核心原则与架构分层

官方托管应用为独立发布的原生可执行程序：业务代码与构建后的 UI 静态资源嵌入程序内，按需启动、停止与回收。Core（native-file-host / App Store）只负责安装管理、登记、签名校验与 Native Host 注册；应用业务与业务数据归应用自身。

```text
┌─────────────────────────────────────────────────────────────┐
│  Chrome / Chromium 扩展 (MV3)                               │
│  ├─ apps.html (应用中心：安装/更新/启停/卸载管理)            │
│  └─ app.html (通用壳：每应用一个 owner 页面)                │
└──────────────┬──────────────────────────────┬───────────────┘
               │ Native Messaging (短连核验)  │ Native Messaging
               ▼                              │ (直属 Port，Chrome 按需启动)
┌──────────────────────────────┐   ┌──────────▼────────────────┐
│  Core Host (native-file-host)│   │  官方 App Host (独立程序)  │
│  App Store：安装事务、收据、 │   │  内嵌静态 UI + 业务接口    │
│  签名核验、注册、清理        │   │  127.0.0.1 动态端口        │
└──────────────┬───────────────┘   └──────────┬────────────────┘
               │ activation.json 只读投影     │ app.html 受限 sandbox iframe 呈现 UI
               ▼                              ▼
┌─────────────────────────────────────────────────────────────┐
│  持久化数据层 (~/.natives/apps/<appId>/)                     │
│  ├─ activation.json (Core 生成的只读激活投影)                │
│  ├─ runtime/<version>/app (已验证可执行程序，受限权限)       │
│  ├─ data/ imports/ cache/ backups/ (应用独占，默认保留)      │
│  └─ OS Keychain: com.natives.app.<appId> (敏感 Secret)       │
└─────────────────────────────────────────────────────────────┘
```

app.html 只提供可信壳、主题/语言、生命周期与安全握手，不执行下载的业务 JS；业务代码不进入扩展执行上下文。独立 Native Host 注册与 sandbox 是 ADR-0027 授权的有界例外；Service Worker 仍无状态、无 Port、无轮询；文件页与 Model Host 保持既有边界。

---

## 二、接入规则（MUST）

### 1. 代码归属与交付（Code Ownership & Delivery）
- **R-APP-01**：官方应用的 UI、业务程序与资源**必须**作为独立原生可执行程序交付；UI 静态资源在构建期嵌入该程序。每个目标平台恰好一个 runtime 载荷（gzip 单载荷 `.nap`，wire ≤ 32 MiB、payload ≤ 128 MiB，精确长度与双 SHA-256 校验）。
- **R-APP-02**：应用业务代码**禁止**进入扩展包或扩展执行上下文；扩展与 Core 不包含任何单个应用的业务逻辑。协议兼容时，新增/升级应用**禁止**要求重新发布扩展或 Core。
- **R-APP-03**：扩展与 Core **禁止**提供动态代码下载执行、`eval`/`new Function`、远程 WASM 或在线 DSL 解释器；app.html 不加载应用之外的任何远程代码。
- **R-APP-04**：通用运行协议、鉴权、限额、锁与退出逻辑**必须**由共享编译期支持库 `crates/app-host-support` 提供，标准样例与各官方应用共用；禁止各应用手写第二套安全协议。

### 2. 身份与契约（Identity & Contracts）
- **R-APP-05（应用 ID 命名规范）**：`app_id` 必须匹配 `^[a-z0-9][a-z0-9._-]{0,63}$`，全局唯一且向前兼容；`fund` 保持既有 ID 不改名。
- **R-APP-06（runtimeHost 计算）**：Native Host 名称**必须**由 Core 计算：`com.natives.app.a` + app_id 的完整 SHA-256 小写十六进制；映射持久化在安装收据中。Catalog、页面与请求参数**禁止**提供 Host 名称、可执行路径、启动参数或安装脚本。
- **R-APP-07（契约单一来源）**：Catalog v3、Core Apps protocol v4、App protocol v1 的类型与校验资料**必须**从支持库单一来源生成；扩展页面与 Host **禁止**各维护不同枚举。未支持的 kind/权限/协议**必须**拒绝并返回明确错误。
- **R-APP-08（双向版本兼容）**：Catalog 条目必须声明 `minExtensionVersion`、`minHostVersion` 与 `appProtocolVersion`；版本不足时前端与 Host **必须**拒绝并明确提示升级。

### 3. 安装与生命周期（Install & Lifecycle）
- **R-APP-09**：安装**必须**走既有 App Store 安装事务（install_begin/install_package/install_finish/install_commit/install_abort/recover），Core 重新核验签名目录、artifact hash、payload hash、平台与架构；Host 侧**禁止**信任前端 alreadyVerified 标记或前端提供的 hash。
- **R-APP-10**：安装后不自动运行；运行实例由应用持有 OS runtime 锁与四个全局运行槽文件锁保证，每 appId 至多一个业务实例，每 OS 用户命名空间至多四个活动应用实例。运行中**禁止**更新、卸载、停用或清数据。
- **R-APP-11**：EOF/停止至应用退出 ≤ 2 秒；stdin EOF、OS 终止与正常 stop 共用取消/关闭路径。没有确定性 shutdown 的应用**禁止**发布。
- **R-APP-12**：更新仅在应用停止后进行；安装器不接触业务库；候选 `--health` 不取 runtime 锁、不迁移数据；首次真实打开由应用自行备份并迁移业务库。迁移失败恢复备份；已接受新写入后**禁止**静默恢复旧备份。

### 4. 承载隔离（UI Isolation）
- **R-APP-13**：app.html 的应用 iframe sandbox 精确为 `allow-scripts allow-forms`，**必须不**包含 `allow-same-origin`、`allow-top-navigation`、`allow-popups`/downloads；应用 UI 无 chrome.runtime Native 能力、不能导航顶层、不能访问 Core、其他应用与外部网络。
- **R-APP-14**：应用本地 HTTP 只绑定 127.0.0.1 系统分配端口；会话鉴权、两阶段握手、generation/token 撤销、CORS（仅 Origin: null + 有效 bearer）、限额与 Host header 校验**必须**按契约实现；禁用 cookie 与 credentials。
- **R-APP-15**：应用 ID 用于数据目录与注册隔离，**不得宣称其构成对恶意原生程序的 OS 级沙箱**。官方发布签名审查是信任前提；第三方 native 包明确拒绝。

### 5. 数据分账与 Secret（Data & Secrets）
- **R-APP-16**：Core App Store（natives.db）与应用业务库分别迁移、分别写入；应用**禁止**连接 natives.db 写业务表；Core **禁止**创建 portfolio/transactions/nav 等业务表。
- **R-APP-17**：应用私有数据存于 `apps/<appId>/data/`、导入原件存于 `imports/`；升级与默认卸载**必须**保留；清除用户数据与 Keychain 是独立危险操作，**必须**二次确认，失败保留 cleanup_pending 收据可重试。
- **R-APP-18**：持久 Secret **必须**进 OS Keychain，命名空间 `com.natives.app.<appId>`（fund 为 `com.natives.app.fund`），命名与删除匹配规则由 app-host-support 与 Core 共享 fixture 固定；**禁止**落盘 SQLite、前端 storage 或日志。Keychain 锁定/拒绝**必须**可恢复，不得回退明文文件。

---

## 三、标准接入步骤

1. **实现 App Host**：以 `crates/app-host-support` 为依赖，实现契约 §5 运行协议（handshake/start/status/data_status/session/stop）、静态 UI 嵌入与业务路由；提供 `--health` 与 `--inspect-data` 受限模式。
2. **数据与迁移**：业务库使用版本化增量迁移与一致性备份，按契约 §4.1 记录 `data/.migration.json` journal。
3. **打包**：先平台代码签名/公证，再计算载荷与压缩包 hash，产出各平台 `.nap`；禁止签名后修改二进制。
4. **声明 Catalog v3 条目**：名称、多语言文案、版本、兼容要求、`appProtocolVersion`、permissions、packages（每平台一个 runtime 载荷）、changelog。
5. **多语言与文案同步**：`extension/_locales/zh_CN/messages.json` 与 `extension/_locales/en/messages.json` 同步补齐。
6. **黑盒契约检查**：通过契约 §9 的统一黑盒套件（签名安装、崩溃切点、隔离、并发、停止/EOF、回收、迁移、数据保护）。
7. **发布**：候选包与签名 Catalog 交付官方发布仓库；不要求重新发布扩展/Core；正式对外发布另行取得授权。
