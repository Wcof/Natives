# ADR-0031：统一内置应用 Runtime 与 Monorepo 模块架构（Unified Built-in App Runtime and Monorepo Module Architecture）

- 状态：已接受（accepted-target，2026-09-14），成为 Natives 内置应用架构的唯一最高权威决策。
- 决策者：产品方（用户）
- 取代范围：
  - **取代 ADR-0027 / ADR-0029 中以下决策**：废除“每模块一个 App Host executable（如 fund-host）”、“每模块一个 Native Messaging Host（如 com.natives.app.a<hash>）”、“用户目录 runtime/<version>/app payload 可执行文件复制”、“独立 executable 激活与健康探测”。
  - **延续并保留的核心原则**：Natives 是唯一产品；内置应用仅打开时按需拉起独立进程；stdin EOF / 页面关闭 2 秒内彻底退出进程并释放全部资源；数据目录物理隔离（`~/.natives/apps/<appId>/data/`）；独立 SQLite 库与 Migration；受限 sandbox iframe + 127.0.0.1 动态端口 loopback 呈现；Bearer Token 鉴权；Keychain 凭据隔离；严禁全局常驻 daemon 或开机启动；严禁动态插件/远程代码加载。
  - **明确概念与边界**：
    - **Core != App Runtime**：Core（`crates/native-file-host`，`com.natives.file_manager`）是 Chromium 文件管理与基础应用注册中心，严禁编译进任何子应用（Fund 等）的业务逻辑；App Runtime（`crates/app-runtime`，产出 `natives-app-runtime` 二进制，对应 Native Host `com.natives.app_runtime` / 本地开发 `com.natives.local.app_runtime`）是 Natives 产品级官方内置应用通用运行时。
    - **Single Runtime Binary / Multi Process Instance**：整个 Natives 产品只编译并注册**一个**通用的 `natives-app-runtime` 原生程序，但通过 Chrome Native Messaging 机制，在用户打开不同内置应用（如 Fund、未来 App A）时分别按需启动**独立的 Runtime 进程**。一个 Runtime 进程一次只承载并激活一个 `appId`，生命周期完全由页面连接拥有。
    - **Built-in App vs Internal Module**：Built-in App（如 Fund）具有独立 appId、独立数据目录、独立 UI、独立 App Center 卡片；Internal Module（如 Fund 内的 Portfolio, Ledger, NAV, Import, Storage, Migration）属于 Built-in App 的内部组件，严禁成为 App Center 卡片、Native Host 或独立安装对象。
- 关联文档：ADR-0020、ADR-0027、ADR-0029、`docs/standards/`、`docs/contracts/managed-app-contract.md`。

---

## 1. 背景与核心矛盾

在 ADR-0027 与 ADR-0029 落地过程中，虽然已明确“Natives 是唯一产品，所有官方模块随 Natives 统一构建与安装，取消独立发布与在线下载”，但在底层运行模型上仍延续了“每个模块一个独立 Native Host 可执行文件（如 `fund-host`）并在系统注册独立 Native Messaging Host（如 `com.natives.app.a<hash>`）”的模式。

这种“一模块一可执行文件”模式带来了不可忽视的系统与工程摩擦：
1. **系统安全与签名膨胀**：每个模块单独一个 executable，在 macOS 上需要逐个做 Gatekeeper 评估与公证，在 Windows 上触发独立安全扫描；
2. **Native Messaging 注册混乱**：每次安装或激活模块，都需要在 OS 目录动态写入 `com.natives.app.a<hash>.json`，引入不必要的文件落地、权限与状态竞争；
3. **用户目录写入可执行文件风险**：将可执行文件复制到 `~/.natives/apps/<appId>/runtime/<version>/app` 并 `chmod 755`，混淆了“产品可执行代码”与“用户应用数据”的边界；
4. **多仓库割裂**：`Natives-App-Fund` 独立于 `Natives` 主仓库，增加了跨仓库联调、CI 校验与版本同步的心智负担。

为了彻底解决以上问题，必须进行架构整改：将内置应用收敛为 Monorepo 模块并使用统一的 App Runtime 二进制。

---

## 2. 最终架构决策

### 2.1 产品与代码架构：Monorepo 内置应用模块

1. **唯一产品**：Natives 是唯一产品。Fund 等内置应用为 Natives 的官方组成部分，源码归入 Natives 主代码库的 `modules/` 目录下（如 `modules/fund/`）。
2. **Library 而非 Executable**：各内置应用模块（如 `fund-module`）在 Rust 侧实现为 Library crate，不再拥有独立的 `main()` 函数或独立产出的 Native Messaging 可执行文件。
3. **统一编译**：所有官方 Built-in App 模块在编译期静态链接入 `crates/app-runtime`，生成唯一的产品级可执行文件 `natives-app-runtime`。
4. **严禁动态插件**：禁止使用 `dlopen`、`.dylib`、WASM 插件、远程 JS/DSL 或动态下载代码。内置应用列表由编译期 `ModuleRegistry` 决定。

### 2.2 运行模型：Single Runtime Binary / Multi Process Instance

1. **一个可执行程序**：Release 产物仅包含 `native-file-host`（Files Host）、`model-host`（AI Host）、`natives-app-runtime`（App Runtime Host）及 `Natives.app`（Launcher）。严禁产生 `fund-host`、`fund-app` 或 `*.nap` 二进制。
2. **独立按需进程**：
   - 当用户在 Chrome 中通过 `app.html?app=fund` 打开 Fund 时，扩展连接 `com.natives.app_runtime`（本地开发为 `com.natives.local.app_runtime`），Chrome 启动一个 `natives-app-runtime` 进程实例。
   - 握手协议升级为 **App Runtime Protocol v2**：前端发送 `app:handshake`，指定 `appId: "fund"`。
   - 该 Runtime 进程核验身份后从 `ModuleRegistry` 查找 `fund`，按需实例化 `FundModule`，绑定专属数据目录（`~/.natives/apps/fund/`），获取单实例运行时锁，并启动 127.0.0.1 动态端口 HTTP 服务器提供 UI 与业务 API。
   - 若用户同时打开另一个官方应用（如 Future App A），Chrome 会拉起另一个独立的 `natives-app-runtime` 进程实例，执行对应的模块。
3. **未运行模块零开销（≈0 核心内存）**：
   - 未被当前进程激活的模块，绝不执行初始化代码：不打开 SQLite 数据库、不启动 HTTP 路由、不创建 Worker/Timer、不分配专用堆内存。
4. **进程级生命周期隔离与彻底释放**：
   - 当 `app.html` 页面关闭、hidden 超时 60 秒或 Native Messaging 端口断开（stdin EOF）时，Runtime 进程触发 `shutdown()`，释放锁并直接退出进程（`exit(0)`）。
   - 硬指标：**stdin EOF 到进程完全退出时间 $\le 2$ 秒**。
   - 进程退出后，操作系统直接回收全部堆内存、线程栈、SQLite 缓存与网络套接字，绝不残留孤儿进程或锁泄漏。

### 2.3 数据与权限隔离

1. **物理目录隔离**：
   - 模块数据严格存放在 `~/.natives/apps/<appId>/`。
   - 模块严禁自行探测 `HOME` 或推导产品路径，所有数据根路径（`data/`, `imports/`, `cache/`, `logs/`）由 App Runtime 在初始化时通过 `ModuleContext` 统一注入。
2. **独立数据库**：
   - 严禁模块业务表汇入 `natives.db`。Core DB 仅保留应用的安装/显示/排序元数据投影；Fund 业务数据完全独占 `~/.natives/apps/fund/data/fund.db`。
   - 模块独立维护自己的 Data Schema Version、Migration 逻辑与备份恢复。
3. **用户目录不存可执行文件**：
   - 废除 `~/.natives/apps/<appId>/runtime/` 目录。可执行文件只存在于产品安装源（`/Library/Application Support/Natives/hosts/`）或构建产物中。

### 2.4 Native Messaging Host 注册收敛

系统原生 Native Messaging 注册由多个收敛为统一的 1 个：
- 生产环境：`com.natives.app_runtime.json` 指向 `hosts/natives-app-runtime`。
- 本地开发：`com.natives.local.app_runtime.json` 指向 target 目录的 `natives-app-runtime`。
- 不再随应用增减而动态写入或注销系统 Native Messaging Manifest。

### 2.5 协议与版本体系

1. **协议升级**：
   - Product Manifest Schema: 1 → 2
   - Core Apps Protocol: 4 → 5
   - App Runtime Protocol: 1 → 2
   - Built-in Module Contract: 2 → 3
2. **版本对齐**：
   - 用户可见版本只有 `Natives x.y.z`。App Center 界面不再展示 Fund 的独立 releaseVersion。
   - 模块内部保留 `moduleApiVersion`、`dataSchemaVersion`、`capabilityVersion` 等工程契约版本，用于向后兼容与迁移判定。

---

## 3. 验收与迁移保障

1. **旧版本升级平滑迁移**：
   - 自动检测旧版 `~/.natives/apps/fund/runtime/` 及 `com.natives.app.a<hash>.json`；
   - 停止旧进程，清理旧 manifest 与旧 executable 文件；
   - 严禁删除 `fund.db`、导入文件与 Keychain 凭据；
   - 自动接管并验证新 Runtime 能正常读取既有数据。
2. **静态架构检查与无特例 Gate**：
   - `crates/native-file-host/**`、`crates/app-runtime-core/**`、`extension/app.js`、`extension/apps.js` 严禁出现具体模块（如 `fund`）的业务分支判断。
   - 生产环境禁止出现 `fund-host` 或 `*.nap` 文件引用。
