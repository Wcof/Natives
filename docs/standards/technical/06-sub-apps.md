# 06 · 官方托管扩展包接入规范（Managed Extension Packages Specification）

> 本规范定义官方托管扩展包（managed_local Package）的交付架构、接入契约、生命周期、数据分账与安全边界。
> 架构基准见 [ADR-0027](../../adr/0027-managed-apps-independent-delivery.md)（取代 ADR-0026 的 Apps 目标决策）。
> 统一套件、系统源与本地/正式验收补充见 [ADR-0029](../../adr/0029-unified-suite-preinstalled-apps.md)；以下均为实施目标，不表示当前代码已满足。
> **2026-09-12 路线收敛（用户最终决定）**：Natives 是一个完整应用，基金等均为**内置模块**，随完整安装包一起安装、更新和修复；取消模块独立下载/安装/更新/卸载、在线 Catalog 驱动新 appId 与 Fund 独立 nap 发布。本篇中与该决定冲突的"独立构建/签名/升级/回滚、协议兼容时不重发 Core/扩展、Catalog 条目发布"等表述一律按内置模块语义执行：签名/Catalog/事务能力只在完整产品安装/更新层核验固定模块文件。数据、Secret、沙箱、锁与 EOF 回收约束不变。
> 接口唯一来源：[官方托管应用契约 v1](../../contracts/managed-app-contract.md)。
> 历史规范（扩展内置 UI + 共享 Core Host + 纯资源包）见 ADR-0026 历史正文，不再作为新应用接入依据。

---

## 一、核心原则与架构分层

Natives 是唯一的用户产品。托管扩展包（如基金扩展包）是 Natives 应用中心管理的可选扩展能力，属于 Natives 的组成部分，不是独立桌面产品；扩展包内部的持仓、账本、净值、导入和数据迁移等是包的内部模块，不应分别变成可安装应用，也不拥有独立安装记录、生命周期或产品身份。

必须严格区分四类对象与概念：
1. **产品归属（Product Ownership）**：Natives 是唯一产品主体；托管扩展包以 Natives 内的应用入口（`app.html?app=<id>`）呈现，严禁作为独立 Mac App 注册到 `/Applications`、Dock 或 LaunchServices。
2. **交付（Delivery，2026-09-12 收敛）**：模块源码可独立构建与版本化，但**随 Natives 完整安装包交付**——安装、更新、修复与 Release 全部与 Natives 联动；不存在模块独立下载、独立安装、独立升级或独立回滚。
3. **运行进程（Runtime Instance）**：运行时是否使用本机进程属于实现细节；运行载荷由受控本机实现承载，按需启动、停止与回收。
4. **页面呈现（Surface）**：扩展内的通用 `app.html` 作为呈现会话，通过受限 sandbox iframe 展示 UI，业务代码不进入扩展执行上下文。包内模块不拥有独立生命周期。

```text
┌─────────────────────────────────────────────────────────────┐
│  Chrome / Chromium 扩展 (MV3)                               │
│  ├─ apps.html (应用中心：打开/显示隐藏/设置/数据管理)        │
│  └─ app.html (通用壳：每扩展包一个 owner 页面)              │
└──────────────┬──────────────────────────────┬───────────────┘
               │ Native Messaging (短连核验)  │ Native Messaging
               ▼                              │ (直属 Port，Chrome 按需启动)
┌──────────────────────────────┐   ┌──────────▼────────────────┐
│  Core Host (native-file-host)│   │  受控 App Host (扩展包载荷)│
│  App Store：安装事务、收据、 │   │  内嵌静态 UI + 业务接口    │
│  签名核验、注册、清理        │   │  127.0.0.1 动态端口        │
└──────────────┬───────────────┘   └──────────┬────────────────┘
               │ activation.json 只读投影     │ app.html 受限 sandbox iframe 呈现 UI
               ▼                              ▼
┌─────────────────────────────────────────────────────────────┐
│  持久化数据层 (~/.natives/apps/<appId>/)                     │
│  ├─ activation.json (Core 生成的只读激活投影)                │
│  ├─ runtime/<version>/app (已验证可执行载荷，受限权限)       │
│  ├─ data/ imports/ cache/ backups/ (扩展包独占，默认保留)    │
│  └─ OS Keychain: com.natives.app.<appId> (敏感 Secret)       │
└─────────────────────────────────────────────────────────────┘
```

**目标架构说明（ADR-0027/0029 修订，2026-09-12）**：目标架构是**受控独立应用 Host**——官方包以 `managed-native-host` 载荷交付（业务代码与构建期嵌入 UI），由 Chrome 按连接按需启动、Core 掌控安装/激活/生命周期；Core 不承载任何包业务，也不提供 `app:invoke` 业务分发生产入口。历史上的"单 Runtime builtin/ManagedApp Registry dispatch"路线已被 ADR-0027 取代，仅作为已删除的历史实现保留记录。

app.html 只提供可信壳、主题/语言、生命周期与安全握手，不执行下载的业务 JS；业务代码不进入扩展执行上下文。独立 Native Host 注册与 sandbox 是 ADR-0027 授权的有界例外；Service Worker 仍无状态、无 Port、无轮询；文件页与 Model Host 保持既有边界。

---

## 二、接入规则（MUST）

### 1. 运行模型（Managed Native App Host）

官方托管子应用采用 **Managed Native App Host** 运行模型。由 Core 统一作为应用生命周期权威，管理受控的独立 Native Host（如 `fund-host`），以兼顾业务代码隔离与独立构建；模块代码只随完整 Natives 产品版本统一更新，没有独立升级事务（2026-09-12 收敛）。

Managed Native App Host 必须满足以下刚性标准：
- **无独立 .app**：严禁创建 `/Applications/Fund.app` 或任何独立的 `.app` bundle；
- **无系统桌面暴露**：严禁注册 Dock 入口、LaunchServices、URL Scheme 或文件关联；
- **无开机启动与独立 Daemon**：严禁创建 Login Item、LaunchAgent、LaunchDaemon 或系统级常驻服务；
- **受控私有目录运行**：活动载荷固定安装在 Core 管辖的私有根 `~/.natives/apps/<appId>/runtime/` 下；
- **Core 掌控生命周期**：安装、验证、启用、停用、启动、停止、更新、回滚与激活投影均由 Core App Store 事务统一管理；
- **支持独立更新**：2026-09-12 收敛后取消——内置模块不独立版本化发布或独立升级，代码随 Natives 完整产品版本统一更新；本条仅保留为运行载荷在产品包内可按模块版本化的内部构建追踪语义。

禁止将托管子应用编译进 Core（禁止 Fund builtin 化），亦不引入 WASM Runtime。运行时进程隔离与 `runtime.lock` / 单实例互斥机制必须严格保留。

### 2. 代码归属与交付（Code Ownership & Delivery）
- **R-APP-01**：官方扩展包的 UI、业务程序与资源**必须**作为内置模块随 Natives 完整安装包交付（2026-09-12 收敛：不再独立打包发布 `.nap`，不经 Catalog 或分块安装获取）；运行载荷由受控本机实现承载，UI 静态资源在构建期嵌入该载荷，每个目标平台恰好一个载荷，其精确长度与双 SHA-256 摘要登记进签名产品组合清单并由产品安装/更新层核验。活动载荷安装路径固定在私有数据根 `~/.natives/apps/<appId>/`，**严禁**写入 `/Applications`、独立 `.app` bundle、系统 Dock 或 LaunchServices 产品注册。
- **R-APP-02**：扩展包业务代码**禁止**进入 Core / 浏览器扩展执行上下文；**允许**（且必须）在构建期嵌入自身托管载荷。扩展与 Core 不包含任何单个扩展包的业务逻辑。协议兼容时，新增/升级扩展包**禁止**要求重新发布扩展或 Core（2026-09-12 收敛：此句改读为——模块代码随完整 Natives 产品版本统一交付，不发生运行期的"单独升级扩展包"事务；取消独立升级后该句不再构成独立发布许可）。
- **R-APP-03**：扩展与 Core **禁止**提供动态代码下载执行、`eval`/`new Function`、远程 WASM 或在线 DSL 解释器；app.html 不加载扩展包之外的任何远程代码。
- **R-APP-04**：通用运行协议、鉴权、限额、锁与退出逻辑**必须**由共享编译期支持库 `crates/app-host-support` 提供，标准样例与各官方扩展包共用；禁止各扩展包手写第二套安全协议。
- **R-APP-04.1（包内模块边界）**：持仓、账本、NAV、导入、存储、迁移等模块为扩展包内部模块，**必须**通过包内窄接口协作；内部模块**禁止**分别拥有安装记录、应用中心卡片、独立生命周期或独立数据权威。

### 3. 身份与契约（Identity & Contracts）
- **R-APP-05（应用 ID 命名规范）**：`app_id` 必须匹配 `^[a-z0-9][a-z0-9._-]{0,63}$`，全局唯一且向前兼容；`fund` 保持既有 ID 不改名。契约声明必须包含 `parentProduct: "natives"` 与 `packageRole: "managed_extension"`。
- **R-APP-06（runtimeHost 计算）**：正式 Native Host 名称**必须**由 Core 计算：`com.natives.app.a` + app_id 的完整 SHA-256 小写十六进制；本地开发前缀按契约 §4.2 隔离并共用同一命名函数。映射持久化在安装收据中。Catalog、页面与请求参数**禁止**提供 Host 名称、可执行路径、启动参数或安装脚本。
- **R-APP-07（契约单一来源）**：Catalog v3、Core Apps protocol v4、App protocol v1 的类型与校验资料**必须**从支持库单一来源生成；扩展页面与 Host **禁止**各维护不同枚举。未支持的 kind/权限/协议**必须**拒绝并返回明确错误。
- **R-APP-08（双向版本兼容）**：签名产品组合清单必须声明产品版本、固定模块构建版本与 `appProtocolVersion` 兼容范围；版本不足时前端与 Host **必须**拒绝并明确提示「更新 Natives」，不存在模块级升级通道。

### 4. 安装与生命周期（Install & Lifecycle）
- **R-APP-09**：内置模块**没有独立安装事务**——`install_begin` / `install_chunk` / `install_finish` / `install_commit` / `install_abort` 等模块分发方法对当前产品必须在任何存储/网络/注册变更前明确拒绝（2026-09-12 收敛）。模块文件随完整产品安装/更新落盘，产品配置复用同一套验证、锁、journal 与恢复原语，在锁内核验签名组合清单、逐文件摘要、平台与架构；Host 侧**禁止**信任前端 alreadyVerified 标记或前端提供的 hash。旧无签名整包 `install_package` 方法已删除，不保留第二条安装链路。
- **R-APP-10**：安装后不自动运行；运行实例由扩展包持有 OS runtime 锁与四个全局运行槽文件锁保证，每 appId 至多一个业务实例，每 OS 用户命名空间至多四个活动应用实例。运行中**禁止**更新、卸载、停用或清数据。
- **R-APP-11**：EOF/停止至应用退出 ≤ 2 秒；stdin EOF、OS 终止与正常 stop 共用取消/关闭路径。没有确定性 shutdown 的扩展包**禁止**发布。
- **R-APP-12**：更新仅在扩展包停止后进行；安装器不接触业务库；候选 `--health` 不取 runtime 锁、不迁移数据；首次真实打开由扩展包自行备份并迁移业务库。迁移失败恢复备份；已接受新写入后**禁止**静默恢复旧备份。

### 5. 承载隔离（UI Isolation）
- **R-APP-13**：app.html 的扩展包 iframe sandbox 精确为 `allow-scripts allow-forms`，**必须不**包含 `allow-same-origin`、`allow-top-navigation`、`allow-popups`/downloads；扩展包 UI 无 chrome.runtime Native 能力、不能导航顶层、不能访问 Core、其他应用与外部网络。
- **R-APP-14**：扩展包本地 HTTP 只绑定 127.0.0.1 系统分配端口；会话鉴权、两阶段握手、generation/token 撤销、CORS（仅 Origin: null + 有效 bearer）、限额与 Host header 校验**必须**按契约实现；禁用 cookie 与 credentials。
- **R-APP-15**：扩展包 ID 用于数据目录与注册隔离，**不得宣称其构成对恶意原生程序的 OS 级沙箱**。官方发布签名审查是信任前提；第三方 native 包明确拒绝。

### 6. 数据分账与 Secret（Data & Secrets）
- **R-APP-16**：Core App Store（natives.db）与扩展包业务库分别迁移、分别写入；扩展包**禁止**连接 natives.db 写业务表；Core **禁止**创建 portfolio/transactions/nav 等业务表。
- **R-APP-17**：扩展包私有数据存于 `apps/<appId>/data/`、导入原件存于 `imports/`；升级、重装与修复**必须**保留（模块代码随整包交付，不存在模块卸载语义）；清除用户数据与 Keychain 是独立危险操作，与代码操作分离，**必须**二次确认，失败保留 cleanup_pending 收据可重试。
- **R-APP-18**：持久 Secret **必须**进 OS Keychain，命名空间 `com.natives.app.<appId>`（fund 为 `com.natives.app.fund`），命名与删除匹配规则由 app-host-support 与 Core 共享 fixture 固定；**禁止**落盘 SQLite、前端 storage 或日志。Keychain 锁定/拒绝**必须**可恢复，不得回退明文文件。

---

### 7. 统一套件与预装（Suite & Preinstallation；2026-09-12 收敛为整包统一交付）

- **R-APP-19（一次安装）**：完整 Natives 安装包必须随附主程序薄入口、主 Host 和全部内置模块代码；本期完整候选必须包含真实 fund，不以样例或占位 UI 代替。首用只做当前用户权限下的数据初始化/迁移，用户不需逐个安装、执行命令或再次下载；不存在"安装基金"语义。浏览器扩展的安装/启用是一次产品配置，不得伪称普通 `.pkg` 可以静默绕过浏览器政策。
- **R-APP-20（单一事务）**：产品安装/更新复用 R-APP-09 同一套验证、流式落盘、锁、journal、激活和恢复逻辑，核验组合清单内的固定模块文件。系统安装器只放置主程序、主 Host、固定模块文件及最小浏览器注册；不得直接写用户 App Store、activation 或业务库，不以 root 执行业务迁移。macOS 系统源目录限于契约 §3.2 声明的受限目录；这不改变活动载荷与数据的私有根约束。取消离线预装源（Suite Seed）与 Seed Reconciliation 链路。
- **R-APP-21（幂等与用户选择）**：产品重装/升级必须保留更新的模块版本、启用/导航偏好及用户移除选择；不自动降级、重新安装已移除模块或清除数据。组合清单移除条目不等于卸载。程序回退与数据恢复分离，兼容时只回退代码，不覆盖新写入。
- **R-APP-22（开发与发布）**：本地模式按契约 §4.2 使用隔离身份和信任策略，可使用本机 ad-hoc/开发签名而不要求 Developer ID/公证；不能豁免验签、精确构建摘要、格式、权限、协议与数据保护。生产构建必须拒绝开发信任根和 fixture。用真实内置基金验证完整产品组合；独立样例仅作为覆盖共享底层规则的低层测试 fixture。A-Local/B-Local 与正式 Release Gate 分列，禁止把未完成的真实浏览器/平台验收写成通过。

## 三、标准接入步骤

1. **实现 App Host**：以 `crates/app-host-support` 为依赖，实现契约 §5 运行协议（handshake/start/status/data_status/session/stop）、构建期嵌入的静态 UI 与业务路由；提供 `--health` 与 `--inspect-data` 受限模式。每个官方包恰好一个受控独立 Native Host 载荷；Core 不 dispatch 包业务（`app:invoke` 生产入口已随单 Runtime 路线移除），业务请求走 app.html → 直连 Native Port → 包自身 loopback 通道。
2. **数据与迁移**：业务库使用版本化增量迁移与一致性备份，按契约 §4.1 记录 `data/.migration.json` journal。
3. **打包**：按契约 §4.2 选择明确的本地开发或正式发布身份；先完成该模式要求的平台签名处理，再计算模块载荷与全部固定文件的最终摘要，登记进签名产品组合清单（2026-09-12 收敛：不再产出独立发布的模块 `.nap`）。正式分发另须完成实际适用的公证与平台验证，禁止签名/哈希后修改二进制。
4. **声明组合清单条目**：名称、多语言文案、版本、兼容要求、`appProtocolVersion`、permissions、固定入口与构建版本随完整产品组合清单登记（2026-09-12 收敛：不再发布独立签名 Catalog 条目）。
5. **多语言与文案同步**：`extension/_locales/zh_CN/messages.json` 与 `extension/_locales/en/messages.json` 同步补齐。
6. **黑盒契约检查**：通过契约 §9 的统一黑盒套件（签名核验、崩溃切点、隔离、并发、停止/EOF、回收、迁移、数据保护）。
7. **候选与整包组合**：产出载荷、摘要、来源版本和验收证据，全部进入同一 Natives 完整产品候选；不给 Core/扩展增加 appId 业务分支。按相同事务验证产品安装、重装和整包更新。
8. **发布**：模块随完整 Natives 产品版本发布，不存在独立模块 Release 或"兼容更新不重发扩展/Core"语义；本地候选通过不代表正式可发布。完整 Release Gate 通过并另获授权后，才上传官方发布仓库或对外推送。

后续子应用开发者必须提交：稳定 appId、锁定版本的支持库依赖、构建期 UI、真实业务、声明的 schema 读写范围、可取消生命周期、迁移/回退与数据保护测试，以及 local/production 分列证据。内部模块沿用该应用身份，不新增安装对象。具体工作包与证据只维护于 [唯一实施方案](../../development/app-center-fund-implementation-plan.md)，本规范不维护第二份进度。
