# ADR-0026：子应用共用 Host 与纯资源包分发架构（Sub-Apps Shared Host & Resource Distribution）

- **状态**: 已接受（2026-09-08）
- **决策者**: 产品方（用户）
- **取代范围注记**:
  - **取代 ADR-0025 中独立 App Host 相关的决策**：废除为每个子应用独立下载二进制（`runtime` 包）、在 OS Native Messaging 目录注册独立 Host Manifest（`app_host_manifest.rs`）、设置可执行权限（`chmod 755`）及启动独立进程进行 `--health` 健康探测的机制。
  - **继承并保留 ADR-0025 的核心安全防线**：
    - 扩展内置 UI 模块：子应用界面和逻辑随 Natives 扩展构建期打包发布，`app-module-registry.js` 作为唯一构建期映射表，严禁动态远程下载或执行 JS/WASM/DSL。
    - 供应链完整性：继续采用 `.nap` 压缩包与 Ed25519 签名验证，维持单包 $\le 5\text{ MiB}$（wire）、解压 $\le 20\text{ MiB}$（payload）、双重 SHA-256 摘要防御。
    - 数据所有权与分账：资源只读安装于 `packages/`；用户数据隔离在 `data/`，升级与卸载默认保留，清空数据必须显式二次确认。
    - 密钥归属：敏感 Secret 必须保存在 OS Keychain 中，严禁落盘 SQLite 或前端 storage。
- **关联**: `docs/standards/technical/06-sub-apps.md`、ADR-0020、ADR-0025、`docs/standards/technical/02-security.md`、`docs/standards/technical/03-data.md`。

---

## 背景

ADR-0025 确立了 DLC 模式的应用分发架构，但原设计为每个子应用分发独立的原生二进制程序（`runtime` 包，如 `demo-host` / `fund-host`），并在操作系统 Native Messaging 目录注册独立的 Host JSON 文件。在实际跨平台落地与演进中，暴露出以下核心矛盾：

1. **操作系统执行与授权摩擦**：每次安装或升级子应用二进制，均涉及新可执行文件的落地、权限变更及操作系统的应用隔离与安全审查，无法实现“一次授权、安全复用”。
2. **多平台构建与体积冗余**：每个子应用都需要针对 `darwin-arm64`、`darwin-x64`、`linux-x64`、`windows-x64` 等分别编译二进制，极大地增加了供应链打包复杂性。
3. **架构过度设计**：大部分子应用的核心是前端界面与结构化业务数据/媒体资源，底层 Native 交互完全可以由已有且经过严格安全审计的 Core Host（`native-file-host`，`com.natives.file_manager`）统筹承载。

因此，决定将架构彻底收敛：**扩展内置子应用界面和逻辑 → 共用现有 Host → GitHub Release 仅下载跨平台数据与资源。**

---

## 决策

### D1 · 共用 Host，删除独立运行链路
1. **统一连接**：`app.html` / `app.js` 不再寻找和连接子应用独立 Host，统一直接连接页面的 `com.natives.file_manager`（Core Host）。
2. **无守护进程与保持生命周期**：保留页面直属 Native Messaging 连接。页面关闭即断开，隐藏空闲 60 秒自动断开，Host 收到 `stdin` EOF 后 $\le 2\text{ 秒}$ 退出。Service Worker 绝不持有连接，不引入任何常驻守护进程或通用插件系统。
3. **彻底移除 `demo-host`**：删除 `crates/demo-host` crate 及其全部打包、调用、测试依赖。Demo 应用改为展示从资源包读取的数据与图片，并反映共享 Host 的运行状态。

### D2 · 纯资源包标准与格式白名单
1. **单一载荷 `.nap` 压缩包**：复用 gzip 压缩、双重 SHA-256（artifactSha256 + payloadSha256）、$5\text{ MiB}$ wire limit 与 $20\text{ MiB}$ payload limit。
2. **仅限只读数据与资源**：包类型只允许 `data`（结构化数据）和 `resource`（静态媒体资源）。废除 `runtime` 类型。
3. **结构与内容安全审查**：
   - `data` 包：载荷必须为合法结构化 JSON，由 Host 校验解析后落盘；JSON 仅作数据读取，不得包含可执行代码或 DSL。
   - `resource` 包：仅允许标准图片（PNG、JPEG、WebP），写入前由 Host 严格校验文件魔数签名。
   - **绝对禁止**：严禁可执行二进制（Mach-O、ELF、PE）、动态库、WASM、Shell 脚本（`#!/`）、HTML（`<!DOCTYPE`、`<html`）或脚本标签。
4. **跨平台统一**：资源包统一声明 `platform: "any", arch: "any"`，无需多平台分包构建。

### D3 · Catalog v2 与双向版本门禁
1. **模式升级**：Catalog 升级为 `catalogVersion: 2`，由新文件 `catalog-v2.json` 与 `catalog-v2.sig` 发布，与旧版独立隔离。
2. **零包应用支持**：允许子应用 `packages` 数组为空，支持纯扩展内嵌、不需要外部资源的轻量应用。
3. **双向版本门禁**：分别声明 `minExtensionVersion`（与扩展清单版本比对）和 `minHostVersion`（与 Host 握手版本比对）。未知应用或版本不足时明确提示更新，绝不尝试动态下载远程代码补齐。
4. **移除运行时配置**：Catalog 清单中移除 `runtime_spec.host` 字段。

### D4 · 受限资源读取接口（`apps:read_resource`）
1. **协议升级**：Apps 协议版本提升至 `3`（`appsProtocolVersion: 3`），握手强校验；拒绝旧版运行时安装请求。
2. **受限读取接口**：
   - 接口声明：`apps:read_resource(appId, packageId, [offset], [length])`。
   - 权威路径控制：前端只传 `appId` 和 `packageId`，实际文件路径由 Host 权威定位在 `~/.natives/apps/<appId>/packages/<version>/<packageId>`；绝对禁止前端传入文件系统路径。
   - 安全审查：Host 强制校验该应用是否已安装且处于启用状态（`enabled == 1`），校验资源是否属于该应用，校验读取范围（单次原始读取最大 $512\text{ KiB}$），返回 base64 数据及实际总大小。
   - 边界守卫：本次不开放通用 SQL、Shell、文件系统遍历或 Secret 读取接口。后续应用若有原生能力需求，必须随 Host 版本统一编译扩展领域接口。

### D5 · 数据库迁移与旧资源清理
1. **幂等版本化迁移**：
   - 检查并回滚未完成的旧版本安装事务。
   - 扫描并清理历史遗留的 `com.natives.app.*.json` 注册文件，严格保障不误删 `com.natives.file_manager.json` 与 `com.natives.model_host.json`。
   - 原有应用的启用状态、侧栏排序和用户数据（`~/.natives/apps/<appId>/data/`）完整保留。
   - 旧版本残留的二进制运行时不作为有效资源激活。

---

## 效果与影响

1. **显著降低用户摩擦**：安装应用和更新资源不再触发操作系统级别的外部程序启动警告或防病毒拦截，体验平滑。
2. **工程与包体极大瘦身**：消除子应用的多平台交叉编译流水线与 release 矩阵构建；扩展包保持在 $300\text{ KiB}$ 预算内，且无需依赖任何外部子进程。
3. **更强的供应链安全性**：所有可执行代码 100% 随扩展签名审核发布；下载渠道仅流转白名单静态资源，从根本上消除了可执行文件被供应链劫持执行的风险。
