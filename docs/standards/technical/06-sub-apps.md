# 06 · 子应用接入规范（Sub-Apps Specification）

> 本规范定义 Natives 体系中扩展子应用（Extension Apps）的架构分层、资源契约、能力边界、数据分账及标准接入流程。
> 架构基准见 [ADR-0026](../../adr/0026-sub-apps-shared-host-and-resource-distribution.md)，取代 ADR-0025 中关于独立 App Host 的全部决策。

---

## 一、核心原则与架构分层

子应用采用 **“扩展内置 UI 逻辑 + 共享 Core Host 通道 + 跨平台纯资源包分发”** 的三层架构：

```text
┌─────────────────────────────────────────────────────────────┐
│  Chrome / Chromium 浏览器渲染层 (MV3 Extension Surface)     │
│  ├─ app.html (统一 App Surface 舞台容器)                    │
│  ├─ extension/apps/<appId>-ui.js (构建期内置 UI 与业务逻辑) │
│  └─ extension/app-module-registry.js (唯一静态模块映射表)    │
└──────────────────────────────┬──────────────────────────────┘
                               │ Native Messaging (共享直属 Port)
                               ▼
┌─────────────────────────────────────────────────────────────┐
│  共享原生宿主 (native-file-host / com.natives.file_manager)  │
│  ├─ Apps Store: 注册表、安装事务、版本控制、锁管理          │
│  ├─ Resource Engine: apps:read_resource (权威有界读取)      │
│  └─ Core Domain: 严格受限文件系统 / 工作区核心能力          │
└──────────────────────────────┬──────────────────────────────┘
                               │ 本地文件系统落盘
                               ▼
┌─────────────────────────────────────────────────────────────┐
│  持久化数据层 (~/.natives/)                                  │
│  ├─ apps/<appId>/packages/<version>/ (只读静态资源包)       │
│  ├─ apps/<appId>/data/<appId>.db (应用私有业务数据库)       │
│  └─ OS Keychain: com.natives.app.<appId> (敏感 Secret)      │
└─────────────────────────────────────────────────────────────┘
```

---

## 二、接入规则（MUST）

### 1. 代码归属（Code Ownership）
- **R-APP-01**：子应用的前端界面与业务逻辑代码**必须**作为静态 ES Module 编译进 Natives 扩展包内。
- **R-APP-02**：`extension/app-module-registry.js` 是唯一的子应用 UI 映射入口。禁止通过 URL 参数、动态 `<script>` 标签、`eval`、`new Function`、远程 WebAssembly 或在线 DSL 解释器加载任何未打包的外部代码。
- **R-APP-03**：未安装的子应用代码保持静态，不得在后台预热或执行任何网络与文件操作；卸载时必须完全释放 stage DOM 与相关事件监听。

### 2. 资源契约（Resource Contracts）
- **R-APP-04（应用 ID 命名规范）**：`app_id` 必须匹配 `^[a-z0-9][a-z0-9._-]{0,63}$`，全局唯一且具备向前兼容性。
- **R-APP-05（包格式与载荷规范）**：
  - 必须采用 `.nap` 压缩格式（gzip 压缩，单包单载荷）；
  - 包类型（`kind`）只允许 `data`（数据包）与 `resource`（静态资源包）；严禁包含 `runtime` 或任何可执行二进制；
  - 跨平台统一声明 `platform: "any", arch: "any"`；
  - 单包网络压缩大小 $\le 5,242,880\text{ bytes}$（$5\text{ MiB}$）；单包解压载荷上限 $\le 20,971,520\text{ bytes}$（$20\text{ MiB}$）。
- **R-APP-06（载荷结构安全白名单）**：
  - `data` 载荷：必须是合法结构化 JSON，由 Host 在写入前使用严格 parser 校验；只作为静态只读数据消费，严禁注入动态脚本代码；
  - `resource` 载荷：仅允许标准图片格式（PNG、JPEG、WebP），Host 写入前必须核验文件头签名（Magic Bytes）；
  - **红线**：严禁 Mach-O、ELF、PE 等二进制程序头；严禁脚本注释头（`#!/`）；严禁 HTML 标签（`<!DOCTYPE`、`<html`、`<script`）。
- **R-APP-07（双向版本兼容）**：
  - Catalog 必须声明 `catalogVersion: 2`；
  - 必须同时声明 `minExtensionVersion` 与 `minHostVersion`；当客户端版本不足时，前端与 Host 必须拒绝安装并明确提示用户升级 Natives 客户端。

### 3. 能力边界（Capability Boundaries）
- **R-APP-08**：子应用在 Native 侧**必须**共用已有的 `com.natives.file_manager` 连接，禁止在系统中注册新的 Native Messaging Host，禁止申请新的操作系统级应用程序权限。
- **R-APP-09（受限资源读取接口）**：
  - 子应用读取安装包资源只能调用 `apps:read_resource(appId, packageId, [offset], [length])`；
  - 目标路径由 Host 权威决定并限定在 `~/.natives/apps/<appId>/packages/<version>/<packageId>`；
  - 前端严禁向 Host 传递任何绝对或相对路径参数；
  - Host 必须校验调用方应用是否已安装且启用，并实施单次原始读取上限（$\le 512\text{ KiB}$）以保证完整 base64 JSON 响应严格小于 Native Messaging 的 $1\text{ MiB}$ 上限。
- **R-APP-10**：应用 ID 用于隔离不同子应用的数据与资源读取目录，**不得宣称其构成了扩展内恶意代码的安全沙箱**。任何新增的 Native 本地操作必须作为显式领域接口随 Host 统一更新与安全审查。

### 4. 数据分账与存储规则（Data Rules）
- **R-APP-11**：解压后的资源文件为只读，存储于 `apps/<appId>/packages/<version>/`；新版本升级必须写入新版本目录，旧版本在事务提交完成后清理。
- **R-APP-12**：应用私有数据存储于 `apps/<appId>/data/`（如 SQLite 业务库），与只读资源完全解耦；升级应用不得覆盖或重置用户个人数据。
- **R-APP-13**：卸载应用时，默认必须保留用户的 `data/` 目录；仅当用户在交互中显式勾选并完成二次危险确认后，方可清除数据。
- **R-APP-14**：持久化敏感凭据（如第三方平台 Token、私有 API Key）必须托管于 OS Keychain，命名空间为 `com.natives.app.<appId>`；禁止落盘到应用数据库、前端 storage 或日志中。

---

## 三、子应用标准接入步骤

1. **实现前端 UI 模块**：
   - 在 `extension/apps/<appId>-ui.js` 中导出 `mountApp(ctx)` 函数；
   - 在 `extension/app-module-registry.js` 中注册 `<appId>: './apps/<appId>-ui.js'`。
2. **定义静态资源与数据**：
   - 准备跨平台数据文件（JSON）及必要媒体资源（PNG/WebP）；
   - 使用 `package-demo.mjs` 规范的打包流程打包为 `.nap` 压缩包，生成双重 SHA-256 摘要与准确字节大小。
3. **声明 Catalog 条目**：
   - 在 `extension/apps/catalog-v2.json` 中添加应用条目，配置名称、多语言描述、版本、图标、`minExtensionVersion`、`minHostVersion` 与 packages 资源数组。
4. **多语言与文案同步**：
   - 在 `extension/_locales/zh_CN/messages.json` 和 `extension/_locales/en/messages.json` 中补齐应用所需的所有交互文案与错误映射。
5. **本地检查与自动化门禁**：
   - 运行 `npm run apps:check`，确保 Manifest 格式、包体积预算、资源合法性与签名检查全部通过；
   - 运行 `npm run perf:check` 确保包体与性能无回归。
6. **发布上线**：
   - 随 Natives 扩展发布 UI 代码与更新支持；
   - 在 GitHub Releases 发布签名验证的跨平台资源包。
