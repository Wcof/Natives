# ADR-0025：Apps 打包与扩展应用运行时（App Packaging & Extension App Runtime）

- **状态**: 已接受（2026-09-06）
- **决策者**: 产品方（用户）
- **取代范围注记**:
  - ADR-0022 的 Apps Web Surface 运行时（Tauri `BrowserStateHandle` 托管 child WebView、R-P11 的 WebView 实例预算）在**当前 Chrome 扩展生产面**不再适用，由本 ADR D2/D17 取代；ADR-0022 的主题/外观决策不受影响。
  - ADR-0023 资源预算表中「Native Host 单架构 Release 二进制 ≤ 3 MB」一行由本 ADR D19 重述（目标 3 MiB，过渡硬 Gate 4 MiB）；「扩展 ZIP ≤ 250 KB」已被 ADR-0024 修订为 300 KiB，本 ADR D4 沿用 300 KiB 并增加分层 Gate。
  - ADR-0020 §4 的 Apps 领域模型（App / RuntimeSpec / RuntimeInstance / Surface）**扩展**而非取代：本 ADR 新增 `PackageSpec` 与 App Store 数据模型。
- **保留**: ADR-0020 §6 Secret ownership（OS Keychain）、ADR-0023/0024 的按需 Host 生命周期（stdin EOF 2 秒退出、无 daemon/开机启动/保活）、AGENTS.md 的 production 范围约束。
- **关联**: `docs/standards/`（product/01、02-feature-spec、technical/01–04）、ADR-0020/0022/0023/0024、`Natives-App-Fund/docs/reference/fundval-local-audit.md`（FundVal-Live 行为规格审计，Phase R0 产出）。

## 背景

Apps 领域在 ADR-0020 §4 已冻结术语（App / RuntimeSpec / RuntimeInstance / Surface，运行与呈现分离），但从未定义**打包、分发、安装、升级、卸载**机制。第一个正式 Extension App（`fund`，个人基金资产管理）要求：

1. 用户可在「设置 → 应用中心」在线安装/更新/卸载；
2. 卸载默认保留个人财富数据（`fund.db`），二次确认后彻底删除；
3. 安装物是独立 Rust 二进制（`fund-host`），经 Native Messaging 按需启动，与 `native-file-host` 完全分预算；
4. 浏览器侧 Fund UI 必须随扩展构建进入扩展包（MV3 不允许在线下载执行代码）。

同时存在三处必须先冻结的约束冲突：

- **预算漂移**：`docs/standards/technical/04-performance.md` 写 `native-file-host ≤ 3MB`，而 `scripts/perf/check-native-host.mjs` 硬 Gate 为 4 MiB。本 ADR D19 统一口径。
- **扩展预算余量**：当前扩展估算 283,245 bytes，预算 307,200 bytes（300 KiB），余量 23,955 bytes。App Center UI 必须在该余量内落地，且 Apps Framework 自身增量 ≤ 20 KiB（gzip 估算）。
- **Supply chain**：在线分发的二进制与 Catalog 必须具备签名、哈希、大小门禁，否则 App Center 就是供应链攻击入口。

Fund 参考实现 FundVal-Live（AGPL-3.0）已完成本地源码审计（Phase R0）：只提取行为规格（SourceRegistry、fallback 链、穿透算法、持仓回放不变式），**不复制任何源码**，技术栈（Django/React/Tauri/PostgreSQL/Redis）全部禁止引入。

## 决策

### D1 · Apps Domain 唯一

Extension App **必须**属于 ADR-0020 §4 定义的 Apps Domain。完整模型冻结为：

```text
Apps
├── App            已安装的权威记录（natives.db `apps` 表）
├── RuntimeSpec    如何启动 Runtime（host name、argv、协议版本）
├── RuntimeInstance 一次按需运行的 Native Messaging 连接
├── Surface        呈现入口（app.html?app=<id>）
└── PackageSpec    安装包描述（NAP artifact 的元数据，见 D6/D7）
```

禁止创建第二 Registry 或平行领域：

```text
ExtensionApps / PluginApps / MiniApps / ModuleApps / WidgetApps … 全部禁止
```

约束不变：**App 不是 Widget；运行不等于呈现；只有一个 Apps Registry**。`apps` 表是唯一安装事实来源；`chrome.storage.local` 里的任何 App 状态都只是可丢弃的 UI 缓存（见 D15）。

Fund App 的注册形态：

```text
App { id: "fund", kind: "extension_app" }
```

V1 的 `kind` 词表只有 `extension_app`。

### D2 · 浏览器执行代码不得在线下载安装

Chrome MV3 要求扩展的浏览器执行逻辑来自提交的扩展包，远程资源只能是**数据**。因此：

**禁止**（全部）：

```text
在线下载 JS/WASM 并 import()/execute
在线下载 JSON/DSL 并解释成任意应用逻辑（通用解释器）
远程 eval / new Function / 动态 <script>
Catalog 提供 scriptUrl / moduleUrl / wasmUrl 字段
```

特别是：**禁止实现「JSON → 任意应用逻辑」的解释器**——那会重新形成 Plugin Runtime（ADR-0020 §2 停止建设项），且存在 MV3 审核与 XSS 双重风险。

**正确形态**：浏览器端 App UI（App Center UI、App Shell、Fund UI）全部随扩展构建进入包内：

```text
extension/
├── apps.html          App Center Surface（设置级页面）
├── apps.js
├── app.html           App Surface Shell
├── app.js
└── apps/
    └── fund-ui.js     Fund UI（build-time 存在）
```

未安装时 `fund-ui.js` **零执行**：不 import、不创建 DOM、不启动 Host、不请求网络、不运行 Timer。安装后 `app.html?app=fund` 校验安装态 → lazy `import('./apps/fund-ui.js')` → 连接 `fund-host`。

> 浏览器 UI 代码 build-time 存在、runtime 未安装时为 0 执行；在线真正安装的是 Fund Native Runtime、资源包与静态数据包。

如果 Catalog 中出现当前扩展不认识的 `appId`（无对应 UI module），UI 必须显示「需要更新 Natives 后才能安装」，而不是在线拉取 UI 代码。

### D3 · 5 MiB 分包预算（MUST）

统一二进制单位：`1 MiB = 1,048,576 bytes`。

| Gate | 值 | 说明 |
|---|---:|---|
| `APP_PACKAGE_MAX_WIRE_BYTES` | `5,242,880`（5 MiB） | 单个下载 Package（`.nap`）硬上限；超出 CI **直接失败**，不得 Warning 后继续 |
| Required Packages 最大数量 | `3` | 防「无限拆包绕过 5 MiB」 |
| Required Packages 总下载量 | `15,728,640`（15 MiB） | 超出 FAIL |
| 全部 Package 数量上限 | `16` | 含可选包 |
| 单 Package 解压后 Payload | `20,971,520`（20 MiB） | 防压缩炸弹（5 MiB gzip → 500 MiB payload） |
| 应用安装代码 + 静态资源总体 | `52,428,800`（50 MiB） | 不含用户个人数据（`fund.db`/cache/imports/history） |

边界必须精确到字节并有 CI 测试：

```text
5,242,879 bytes → PASS
5,242,880 bytes → PASS
5,242,881 bytes → FAIL
```

**5 MiB 是优化 Gate，不是鼓励微服务**：若 `fund-host > 5 MiB`，第一响应必须是 strip、LTO、`panic=abort`、依赖审计、feature pruning、release profile、HTTP client 比较、SQLite feature 比较、debug symbol 清理；只有当某模块同时具备**独立生命周期 + 独立安全边界 + 独立崩溃隔离价值**时，才允许拆分为新的 Native Runtime。禁止为过 Gate 制造 `fund-core / fund-yjb / fund-eastmoney / fund-stock / fund-analysis` 五个进程。

**Gate 变更规则**：修改 5 MiB 数字本身必须新增 ADR，附文件级 size attribution、至少两个替代方案、为何不能优化、性能/安全影响，批准后修改。CI 禁止出现 `ALLOW_OVERSIZE=1`、`SKIP_APP_SIZE_CHECK`、baseline waiver 等豁免。

### D4 · Core Extension 预算三层 Gate

当前代码实际 Gate 为 307,200 bytes（300 KiB，ADR-0024 修订；`scripts/perf/check-extension-bundle.mjs` `BUDGET = 300 * 1024`），实测估算 283,245 bytes，余量 23,955 bytes。正式定义三层：

| Gate | 阈值 | 行为 |
|---|---:|---|
| Hard Gate | 扩展估算包 ≤ 300 KiB（307,200 bytes） | 任何情况下不得突破，CI exit 1 |
| Warning Gate（NEAR_BUDGET） | ≥ 290 KiB | CI 明确打印 `WARNING: Extension bundle is within 10KiB of hard limit.`，不阻断 |
| Apps Framework 增量 Gate | 相对 Phase A1 记录的 baseline，新增 Apps Framework ≤ 20 KiB（gzip 估算） | 超出必须文件级归因并优化 |

输出字段固定为 `current / budget / headroom / deltaFromBaseline`。现有 `perf:extension` 脚本口径不变，只增输出字段。

### D5 · Bundle Recovery 前置（Phase A1 Gate）

在加入任何 App Center UI 之前，必须把当前扩展估算从 ≈276.6 KiB（实测后以 A1 基线为准）优化到 **≤ 270 KiB**，作为 Apps Framework 的开发基线。只允许安全优化：重复 CSS、重复 SVG、重复字符串/字符串表、重复 helper、重复 locale、dead code、lazy import。**禁止**以删除功能、牺牲契约、破坏现有 Widget 换取体积。

### D6 · NAP 包格式（Natives App Package V1）

NAP（扩展名 `.nap`）V1 **不是**任意文件 ZIP——避免任意目录解压、`../../../` path traversal 与复杂 archive parser。V1 冻结为：

> **一个经过 gzip 压缩的单 Payload Artifact。**

```text
fund-runtime-darwin-arm64.nap   →  解压后就是 fund-host 可执行文件
fund-industry-map-v1.nap        →  解压后是行业映射静态数据文件
```

- 压缩统一使用 **gzip**：浏览器侧用原生 `DecompressionStream("gzip")` 解压，不在扩展 bundle 增加 gzip 库。
- 每个 `.nap` 对应 Catalog 中一条 `PackageSpec`，`kind` 决定安装路径（见 D7），NAP 本身不携带路径信息。

### D7 · Package Catalog 描述目标，路径由 Core 决定

Catalog（`catalog-v1.json`）每条 Package 描述：

```json
{
  "packageId": "fund-runtime-darwin-arm64-1.0.0",
  "kind": "runtime",
  "platform": "darwin",
  "arch": "arm64",
  "version": "1.0.0",
  "wireSize": 2097152,
  "payloadSize": 3145728,
  "artifactSha256": "…",
  "payloadSha256": "…",
  "url": "https://…/fund-runtime-darwin-arm64-1.0.0.nap"
}
```

**安装路径由 Core 根据 `kind` 决定**，Catalog 禁止提供 `targetPath` 或任何路径字段：

| kind | 安装路径（唯一） |
|---|---|
| `runtime` | `~/.natives/apps/<appId>/runtime/<version>/` |
| `data` | `~/.natives/apps/<appId>/packages/<packageId>/` |

从根上杜绝任意路径写入。完整性链（D8）保证 Catalog 与 artifact 不可被篡改，`url` 指向的字节必须与 `artifactSha256` 精确匹配。

### D8 · 完整性与签名

```text
Signed Catalog + Artifact SHA-256 + Payload SHA-256
```

- Catalog 文件对：`catalog-v1.json` + `catalog-v1.sig`，签名算法 **Ed25519**；公钥编译进 Natives Extension（build-time 常量），私钥只存在于 Catalog 发布流程。
- 安装验证链（顺序固定，任一失败进入 rollback）：

```text
下载 Catalog 原始 bytes
→ 验证 Ed25519 signature
→ 解析 JSON（schema 校验）
→ 下载 .nap（流式检查累计字节 ≤ 5 MiB，超限立即中止）
→ 验证 artifactSha256
→ gzip 解压（检查 payloadSize ≤ 20 MiB，超限中止）
→ 发送 Host（Base64，见 D12）
→ Host 验证 payloadSha256
→ 安装
```

- 签名 Gate 测试必须覆盖：官方 Catalog PASS；Catalog 改 1 byte FAIL；未知公钥 FAIL；空 signature FAIL。
- Hash Gate 测试必须覆盖：正确 hash PASS；artifact 改 1 byte FAIL；payload hash 不匹配 FAIL。
- Path Security Gate（纵深防御，NAP V1 虽无任意路径仍需测试）：`../`、`/Users/`、`C:\`、`~/`、symlink 任何试图逃出 `~/.natives/apps/<appId>/` 的输入必须被拒绝。

### D9 · Core App Manager 不给 native-file-host 增加 HTTP 栈

`native-file-host` 是极轻量 Rust Host。**禁止**为应用商店引入 `reqwest`、HTTP runtime、TLS stack、GitHub client 进入 Core Host。Catalog 与 Package 下载由 `apps.html` 用浏览器原生 `fetch()` 完成。Native Host 只负责：

```text
验证（payloadSha256 / 大小 / 路径）→ 落盘 → Registry 写入
→ 安装事务 → Native Messaging Host 注册 → 健康检查 → commit / rollback
```

### D10 · Package 经 Base64 传入 Native Host（V1 无 streaming）

Chrome 消息限制：Extension → Native Host 单条 64 MiB；Native Host → Extension 单条 1 MB。5 MiB `.nap` Base64 后 ≈6.99 MiB，远低于 64 MiB，因此 V1 **不引入**复杂 streaming protocol：

```text
apps.html
→ fetch 下载 ≤5 MiB .nap（流式累计字节检查）
→ DecompressionStream("gzip") 解压（payload ≤20 MiB 检查）
→ Base64 编码（检查 payload Base64 后 < 64 MiB）
→ port.postMessage({ cmd: "apps:install_package", payloadB64, … })
→ native-file-host 验证并落盘
→ Host 返回 { state, error?, revision }（必须 < 1 MiB，超长分页）
```

Host → Chrome 单条消息硬约束 **< 1 MiB**；Fund 侧协议（fund-host ↔ app.html）进一步收紧到 **≤ 900 KiB**，长数据一律分页（`limit` + `offset`/`cursor`），禁止一次发几年历史净值。

### D11 · 应用目录布局（`~/.natives/apps/`）

```text
~/.natives/
├── natives.db                     # 唯一权威库（App Store 表在库内，见 D13）
├── apps/
│   └── fund/
│       ├── runtime/
│       │   ├── 1.0.0/
│       │   │   └── fund-host      # 可执行文件（strip）
│       │   └── current            # 指向 active 版本（文件内容=版本号）
│       ├── packages/              # kind=data 的资源包
│       ├── data/
│       │   └── fund.db            # 个人财富数据（只有 fund-host 可写）
│       ├── cache/
│       ├── imports/               # 用户导入的 CSV 等
│       └── staging/               # 安装事务暂存（commit 后清空）
└── logs/
    └── apps/
        └── fund/
```

**唯一目录例外**：macOS/Windows/Linux 的 Native Messaging Host Manifest 必须写入浏览器规定目录（`~/Library/Application Support/Google/Chrome/NativeMessagingHosts/` 等）。这属于 OS Integration Stub，允许出现在 `~/.natives/` 之外，但 Manifest **只能包含** `name / path / type / allowed_origins`，**禁止**包含 Token、Secret、用户财富数据、账号或配置。Core Installer 必须使用真实 caller origin（Chrome 启动 Host 时提供的 origin）生成 `com.natives.app.fund` 的 Host Manifest。

### D12 · Registry 唯一权威：`natives.db` + App Store 表

`~/.natives/natives.db` 是 App 安装状态的唯一权威。**禁止**恢复历史 `modules` / `module_permissions` / plugin runtime 表语义。新增表（capability-based migration，见 D13）：

**`apps`**

```sql
app_id        TEXT PRIMARY KEY
kind          TEXT NOT NULL CHECK(kind IN ('extension_app'))
name          TEXT NOT NULL
version       TEXT NOT NULL
enabled       INTEGER NOT NULL DEFAULT 1
show_in_sidebar INTEGER NOT NULL DEFAULT 1
sidebar_order INTEGER NOT NULL DEFAULT 0
runtime_spec_json TEXT NOT NULL   -- host name / argv / protocol version
surface_json    TEXT NOT NULL     -- 入口 surface 描述（app.html 路由）
manifest_json   TEXT NOT NULL     -- Catalog 中该 App 的完整条目快照
installed_at  TEXT NOT NULL
updated_at    TEXT NOT NULL
revision      INTEGER NOT NULL    -- 乐观并发
```

**`app_packages`**

```sql
app_id          TEXT NOT NULL REFERENCES apps(app_id) ON DELETE CASCADE
package_id      TEXT NOT NULL
kind            TEXT NOT NULL CHECK(kind IN ('runtime','data'))
version         TEXT NOT NULL
platform        TEXT NOT NULL
arch            TEXT NOT NULL
wire_size       INTEGER NOT NULL
payload_size    INTEGER NOT NULL
artifact_sha256 TEXT NOT NULL
payload_sha256  TEXT NOT NULL
installed_path  TEXT NOT NULL
installed_at    TEXT NOT NULL
PRIMARY KEY (app_id, package_id, version)
```

**`app_permissions`**

```sql
app_id     TEXT NOT NULL REFERENCES apps(app_id) ON DELETE CASCADE
permission TEXT NOT NULL
granted    INTEGER NOT NULL DEFAULT 0
granted_at TEXT
PRIMARY KEY (app_id, permission)
```

**`app_install_transactions`**

```sql
install_id     TEXT PRIMARY KEY
app_id         TEXT NOT NULL
from_version   TEXT
to_version     TEXT NOT NULL
state          TEXT NOT NULL   -- 见 D14 状态机
staging_path   TEXT NOT NULL
started_at     TEXT NOT NULL
completed_at   TEXT
error_code     TEXT
error_message  TEXT
```

### D13 · Migration 规则：跟随 capability-based，不动全局 `user_version`

Workspace Store 已采用 capability-based migration（`CREATE TABLE IF NOT EXISTS` + `table_has_column` + `ensure_column`），因为 `PRAGMA user_version` 属于共享历史数据库的单一游标。**App Store 必须跟随同一模式**，对自己的 4 张表负责；**禁止**用一个新的全局 `PRAGMA user_version = N` 覆盖整个 `natives.db` 的演进。

### D14 · 安装状态机（禁止布尔 installed）

不使用 `installed = true/false`。完整状态词表：

```text
catalog_resolved → downloading → artifact_verified → decompressing
→ payload_verified → staging → runtime_registering → health_check
→ committing → installed
```

任意阶段失败：

```text
<任意状态> → rolling_back → failed
```

**安装事务**：只有 `commit` 成功后 `apps` 表才出现/更新正式安装记录。失败时：staging 删除、runtime registration 回滚、package receipt 回滚、App Registry 不出现 installed。`app_install_transactions` 行保留 `failed` 记录供诊断（不静默消失）。

### D15 · 升级不覆盖当前 Runtime

```text
runtime/
├── 1.0.0/
├── 1.1.0/
└── current          # 内容 = active 版本号
```

升级流程：当前 1.0.0 → 安装 1.1.0 到 `staging/` → 验证（hash/size）→ 移入 `runtime/1.1.0/` → 更新 Host Manifest 指向新版本路径 → health check → `current` 写入 `1.1.0` → commit。**任何一步失败，`current` 继续指向 1.0.0**，新版本 staging/半成品删除。

### D16 · 卸载：默认保留个人数据

默认卸载：

```text
停止 Runtime → 删除 Native Host registration（Manifest 文件）
→ 删除 runtime/ → 删除 packages/ → 删除 App Registry 行
→ 刷新 navigation projection
```

**不删除** `data/fund.db`、`imports/`。

第二操作「删除应用及全部个人数据」必须**二次确认**，才允许删除 `fund.db` 及 `cache/`、`imports/`。重新安装时，Installer 检测现存 `fund.db` → 执行 fund-host migration → 恢复全部个人数据。

### D17 · 全局 Sidebar 解耦与 AppNavigationProjection

现状约束：`files.html` 使用 `FilesSidebar`（已支持动态 sections）；`space.html` 拥有独立 Sidebar DOM 与 `sidebar-controller.js`。**禁止**只改 `files-sidebar.js` 并假设 Home 自动生效。

新增共享模块 `extension/app-navigation-projection.js`，统一输出：

```json
{
  "revision": 42,
  "items": [
    { "appId": "fund", "label": "基金", "icon": "fund", "route": "app.html?app=fund", "order": 0 }
  ]
}
```

- **Source of Truth 是 `natives.db` App Store**；Projection 只是 UI 缓存，存 `chrome.storage.local`，key `natives.apps.navigation.v1`。Projection **只允许包含** `appId / label / icon / route / order / revision`，**禁止**包含 secret、package、token、业务数据。
- **必须有 Projection 的原因**：`space.html`/newtab 保持 0 Native Port 启动成本（ADR-0024 允许短连接，但 Sidebar 首帧不得等待 Host 往返）。安装/卸载后由 App Store mutation 成功方生成 Projection → 写 `chrome.storage.local` → 两个 Surface 消费。
- 更新规则：只有 App Store mutation 成功（install/uninstall/enable/disable/sidebar order）后才更新 Projection 并带 `revision`。Files 页面 Host 已连接时，若 Host revision ≠ Projection revision，自动重建 Projection。
- 呈现规则：0 App → Sidebar 不显示「应用」分组；1 App → `应用 └── 基金`；多 App → 分组内按 `order` 排列。

### D18 · 导航语义：typed NavigationTarget（禁止复用 filesystem path）

`FilesSidebar` 动态 item 目前把 `dataset.path` 当文件路径。App **禁止**伪装成 path。新增 typed 导航：

```json
{ "kind": "filesystem", "target": "/Users/…" }
{ "kind": "app",        "target": "fund" }
```

Sidebar 渲染统一走 `NavigationTarget`；`app.html?app=fund` 不得出现在 filesystem path 字段中。

### D19 · 预算口径统一：Core Host vs Fund Host

`docs/standards/technical/04-performance.md` 的「native-file-host ≤ 3MB」与 `scripts/perf/check-native-host.mjs` 的 4 MiB 硬 Gate 属于文档/CI 漂移。本 ADR 统一：

| 对象 | 目标 | 硬 Gate |
|---|---:|---:|
| `native-file-host`（Core） | ≤ 3 MiB | **4 MiB（过渡值，不得再提高）** |
| `fund-host`（App，经 NAP） | ≤ 4 MiB wire 目标 | **5 MiB wire（D3）** |

- 本次 Apps Framework 对 `native-file-host` 的 binary delta **≤ 64 KiB**；若 App Store 逻辑导致超 64 KiB，必须做依赖归因（哪个 crate 贡献多少字节）并优化。
  - **Gate A2 实测（2026-09-06，metadata-only install flow）**：以同一 release profile（`opt-level="z"` / `lto="fat"` / `codegen-units=1` / `panic="abort"` / `strip="symbols"`）在「无 App 代码基线 `2a33e827`」与「App Store 数据层 + `apps:*` dispatch」间公平对比，二进制 1,763,504 B → 1,830,048 B，**delta = 66,544 B（≈ 65 KiB）**，超 64 KiB 目标 1,008 B（1.5%）。绝对值仍远低于 4 MiB 过渡硬 Gate（≈44% 余量）。
  - **依赖归因**：未新增任何外部 crate（`Cargo.toml` 依赖集不变，仅改 `[profile.release]`）。delta 全部来自 App 自身代码的编译产物——`app_dispatch` 单符号 17.9 KiB、`app_store::query` 约 2.6 KiB、App 类型 serde 单态化 + 异常表约 45 KiB；基线共享的 serde/alloc/rusqlite 无增量。
  - **已实施优化**：release profile 优化使 raw delta 从 165 KiB → 65 KiB；registry 行类型精简为 `Serialize`-only（去 Debug/Clone/PartialEq/Eq/Deserialize 单态化）、移除未用的 `PackageReceipt` 死代码。残余 1,008 B 低于 Mach-O 单 section 页粒度，无 ADR 规范代码可再删；后续 Phase A5 真实包处理接入后按同一 profile 重新核算。
- `native-file-host` ≠ `fund-host`：Core 预算与 Fund 预算完全分开，**禁止**为 Fund 把基金业务编进 `native-file-host`。
- Fund Host 性能预算（V1）：EOF 退出 ≤ 2s；Idle CPU ≤ 0.5%；Idle RSS 目标 ≤ 24 MiB / 硬 Gate ≤ 32 MiB；GPU 0；background timer 0。

### D20 · Core Host Apps API（`domain:action` 命名空间）

继续使用 `native-file-host`（V1 不做 crate rename）。新增 `crates/native-file-host/src/app_store/`（`mod.rs` / `schema.rs` / `types.rs` / `query.rs` / `mutation.rs` / `tests.rs`）与 `app_dispatch.rs` / `app_install.rs` / `app_runtime_registry.rs`。接口全部使用新 `domain:action` 命名空间（不扩老式无命名空间命令）：

```text
apps:list            apps:get
apps:install_begin   apps:install_package   apps:install_commit   apps:install_abort
apps:uninstall       apps:set_enabled       apps:set_sidebar
apps:health          apps:repair
```

**Core App Manager 的能力边界**（禁止清单）：不得请求东方财富/养基宝、不得解析基金、不得计算收益、不得读 `fund.db`、不得做股票分析。它只知道「存在一个 `appId = fund`」。

### D21 · Fund Native Runtime 生命周期

独立 Native Messaging Host：`com.natives.app.fund`，二进制 `fund-host`。

```text
用户打开 Fund → app.html → connectNative("com.natives.app.fund")
→ Chrome 启动 fund-host → Port 通信
→ 最后 Port 关闭 → stdin EOF → ≤2 秒退出
```

禁止：开机启动、daemon、tray、background polling、常驻 scheduler。同步策略 V1：Fund 页面可见 + 市场交易时段才允许 30s quote refresh；页面隐藏暂停；页面关闭 Host 退出。**V1 不做**每日定时任务/Celery 式后台同步（如未来需要，单独 ADR）。

### D22 · App Surface 与 Browser App Module Registry

统一 Surface：`app.html?app=fund`。`app.html` 流程：读取 authoritative App（经已连接 Core Host 或 Projection 提示后按需短连接确认）→ 确认已安装 → 查 RuntimeSpec → 查本地 UI module registry → lazy import `apps/fund-ui.js` → 连接 `fund-host`。

`extension/app-module-registry.js` 是 build-time 常量表：

```js
{ fund: "./apps/fund-ui.js" }
```

Catalog 出现注册表之外的 `appId` → UI 显示「需要更新 Natives 后才能安装」。

### D23 · Fund 数据边界

- `~/.natives/apps/fund/data/fund.db` **只有 `fund-host` 允许写**；Core Host 禁止打开 `fund.db`；Renderer 禁止打开任何 SQLite。
- Fund 不访问 `natives.db`。
- 养基宝 token/refresh/cookie **禁止**写入 `fund.db` / `natives.db` / log / frontend / `chrome.storage`；统一 OS Keychain，namespace `com.natives.app.fund`，DB 只存 `secret_ref`。
- Fund 禁止自建模型配置（OpenAI/DeepSeek key）；AI 能力必须消费 Natives AI Domain（model-host 单一模型资源管理）。
- Fund Host 禁止调用系统 `curl` 或提供 arbitrary shell/process；HTTP 栈选型必须先做独立 size spike（`fund-host-http-size-spike`：候选 HTTP client × TLS 实现 × SQLite 配置，输出 binary raw / gzip wire / RSS / cold start，选择满足安全+跨平台+≤5 MiB wire 的最轻方案）。

### D24 · Fund App 分仓与浏览器 UI 归属

- Native 业务在独立仓库 **`Natives-App-Fund`**（与 Natives Core 分仓维护）：`src/{protocol,domain,storage,sources,sync,portfolio,analysis}/`、`migrations/`、`fixtures/`、`docs/reference/`、`packaging/`、独立 CI（`cargo fmt` / `cargo test` / `fund:package` / `fund:package:size` / `fund:protocol:test` / `fund:lifecycle:test` / `fund:source:fixture:test`）。
- 浏览器 Fund UI 在 **Natives Core** `extension/apps/fund-ui.js`——因为浏览器 UI 必须 build-time 集成进扩展包，Native Runtime 可以独立发布。这是 D2 的直接推论。
- Fund 领域模型（不按 FundVal-Live Django Model 照搬）：`Account / Asset / Position / Transaction / Portfolio / Fund / FundHolding / Security / Quote / SourceConnection / SyncRun`。

### D25 · Catalog 分发与 Public Store Gate

- Catalog 第一阶段为独立仓库 `Natives-App-Catalog`，内容 `catalog-v1.json` + `catalog-v1.sig`。
- 开发阶段：GitHub + GitHub Releases 托管即可；生产建议切自有域名 + CDN（R2）。客户端协议不变，换取更窄 host permission 与更稳定 URL。
- **Catalog URL 是 build-time 固定常量**，禁止开放用户配置任意 Catalog URL（供应链入口不得变成任意地址）。
- **Public Chrome Web Store Gate**：开发/私有阶段可完成 Native Runtime 在线安装机制；正式提交 Chrome Web Store 前必须单独完成「在线下载 Native executable」政策审计。若公共商店认定此分发模式不合适，生产发行切换为「Natives 原生 Installer 负责 Native Runtime 分发」，App Center 保留安装/激活/卸载语义。**不得为赶上线规避审核**。

### D26 · App Center UI 与数据来源

设置菜单新增「应用中心」入口 → `apps.html`（Settings-level App Center Surface，视觉沿用 Model Settings 的左导航+右内容布局）。**禁止**在 `space.html` 里直接启动 Core Native Port 做 App Center。

页面内容（V1）：App 卡片（名称/描述/版本/安装大小/所需权限/数据保存位置）+ 状态与动作按钮（未安装[安装] / 安装中[进度] / 已安装[打开][卸载] / 有更新[更新] / 异常[修复][卸载]）。

数据来源合并：

```text
CatalogEntry + InstalledApp = AppCenterItem
```

**`CatalogEntry` 不能提前写成 `App`**——只有安装成功后才成为 `apps` 表记录。

### D27 · Fund Domain 与数据表（最低集）

`fund.db` 最低包含：

```text
accounts                -- id/name/type/source/parent_id/created_at/updated_at
source_connections      -- 数据源连接与健康状态
assets                  -- asset_id/symbol/name/exchange/currency（同一 510300 全局一个 Asset）
positions               -- account_id/asset_id/quantity/cost_basis/market_value/source/as_of/updated_at
transactions            -- account_id/asset_id/type/quantity/price/amount/fee/trade_date/source
fund_master             -- 基金主数据
fund_nav                -- UNIQUE(fund_id, nav_date) 历史净值（增量同步）
fund_holdings           -- UNIQUE(fund_id, report_date, security_id)；必须含 report_date + disclosure_date
security_master         -- 证券主数据
security_quotes_latest  -- 实时行情 latest（防无限 Tick 表）
portfolio_snapshots     -- date/total_value/cash_value/invested_value/net_deposit/daily_pnl
sync_runs               -- 每次同步：范围/来源/added/updated/unchanged/耗时/状态
analysis_snapshots      -- 评分与指标快照
fund_meta               -- 每基金同步游标（last_nav_date 等）
```

关键不变式（承接 Phase R0 行为规格 B1–B14）：

- 收益计算必须基于**交易 + 入金 + 出金**，不是「今天资产 − 昨天资产」。
- `fund_holdings` 的 `report_date` / `disclosure_date` **不能省略**（主动基金披露持仓 ≠ 实时持仓）。
- 所有外部事实数据带 source provenance：`source / fetched_at / as_of / confidence`（必要时 `disclosure_date`）；UI 可显示「数据来源：东方财富 · 数据日期：2026-06-30 · 获取时间：2026-09-06」。
- Fallback 必须可观察：`requested_source / resolved_source / fallback_level / fetched_at`，禁止悄悄换源后 UI 仍显示原源。
- Source Health 五态：`healthy / degraded / unavailable / auth_required / rate_limited` + 轻量 circuit breaker（禁止接口坏掉后每 30s 无限失败）。
- Quote 数据区分 `security_quotes_latest` 与 daily history，禁止每 30s 永久写一行。
- Unknown 不算 0：估值数据 unavailable 时 `valuation_score = unknown`，综合评分输出 `confidence / coverage`。
- SourceRegistry 各 Source 不允许互相调用；fallback 由 Registry 统一控制。

### D28 · Fund 业务开发顺序（V1）

```text
手工数据（F2）→ 页面成立 → 养基宝（F3）→ 基金公共数据 EastMoney（F4）
→ 股票行情按需（F5）→ 资产穿透（F6）→ UI 完成（F7）
→ 风险指标（F8）→ Fund Score（F9）→ AI Explanation（F10）
```

- V1 手工输入：基金代码/持有份额/成本、股票代码/数量/成本、现金。
- 同花顺 V1 走 CSV Import + 手工录入；不为 XLSX 在 Browser Bundle 引入大型解析库（确需 XLSX 先做 Binary/Bundle Size Spike）。
- 支付宝 V1 不逆向；路径 = 支付宝 → 养基宝 → Natives Fund，或手工录入。
- 股票行情只对**当前需要的股票**请求（基金 holdings 驱动），禁止全 A 股采集器。
- Analysis V1 是 deterministic `FundScore`（底层持仓/趋势/估值/集中度/数据新鲜度/风险），可复现、可测试、模型无关；辅助意见必须附 Evidence/Risk/Data Freshness/Confidence；AI 只解释确定性结果，不直接决定买卖。

### D29 · CI Gate（Core 侧）

`scripts/apps/` 新增：

```text
check-app-manifest.mjs        App 结构/权限/词表校验
check-package-budget.mjs      5 MiB / 15 MiB / 20 MiB / 50 MiB 精确边界
check-catalog-signature.mjs   Ed25519 签名 Gate
check-app-security.mjs        禁止项扫描（在线执行代码/任意路径/第二 Registry…）
check-app-projection.mjs      Projection 字段白名单
```

`package.json` 新增脚本：`apps:contract:check` / `apps:package:check` / `apps:catalog:check` / `apps:security:check` / `apps:check`。`perf:check` 必须包含现有 `perf:files` + `apps:check`。`perf:extension` 口径不变（Hard ≤ 307,200 bytes），增加 `deltaFromBaseline` 输出与 NEAR_BUDGET（≥290 KiB）警告。

## 后果

### 正面

- Apps Domain 第一次拥有完整的打包/分发/安装/升级/卸载/注册闭环，且全部落在现有 `native-file-host` + 扩展页面上，零新常驻进程。
- 供应链防线（签名 Catalog + 双 SHA-256 + 精确字节 Gate + 路径白名单）使 App Center 不是攻击入口。
- Fund 与 Core 的边界以「Core 知道 Fund 是 App，但不知道基金业务」与「Fund 知道基金/股票/数据源，但不知道 Workspace/Files 内部」双向钉死。
- 预算漂移（3 MB vs 4 MiB）在 ADR 层统一，文档与 CI 恢复同一 Source of Truth。

### 成本与约束

- 扩展预算余量仅 23,955 bytes，App Center 必须先完成 Bundle Recovery（D5）再动 UI；20 KiB 增量 Gate 使 App Center 必须极致精简（复用 settings 页面骨架与 tokens）。
- `fund-host` 的 HTTP 栈选型被 5 MiB Gate 前置（spike），不能「先 cargo add reqwest 再看」。
- Chrome Web Store 上线前存在一次政策审计风险点（在线下载 Native executable），已有原生 Installer 回退方案（D25）。
- Sidebar 双 Surface 消费 Projection 要求 A3 同时改 `files.html` 与 `space.html` 两条链，Gate A3 必须双页验证。

## 禁止清单（汇总，执行期逐条检查）

```text
恢复 Plugin Runtime / 创建第二 App Registry / 把 Fund 当 Widget
恢复 Tauri / 引入 React/Vue / 引入 iframe/WebView
Core 访问 fund.db / Fund 访问 natives.db / Fund 自己管理模型 Secret
Service Worker 持有 Native Port / space-newtab 为 App Center 启动 Core Host
后台常驻 Fund Host / 为过 5 MiB 拆无意义微服务
在线下载 JS/WASM 执行 / Catalog 提供任意 script / Catalog 提供任意写盘路径
直接 Copy FundVal-Live 大段代码 / 支付宝逆向作为 V1 必选项
为 XLSX 引入巨大前端库 / 保留新旧两套 Sidebar 状态 / 保留新旧两套 Apps Authority
CI 豁免（ALLOW_OVERSIZE / SKIP_APP_SIZE_CHECK / baseline waiver）
```

## 验收 Gate（里程碑索引）

| Milestone | 内容 | Gate |
|---|---|---|
| M1 · Apps Framework | App Center + Demo 安装 → Sidebar → 打开 → Runtime 按需启动 → 关闭退出 → 卸载消失 | A0–A6 全通过；**必须停一次**，通过后才允许开始 Fund |
| M2 · Personal Wealth | Fund 安装 + 手工录入（基金/股票/现金）→ 总资产/收益/持仓 | F0–F2 |
| M3 · YJB Sync | 养基宝扫码 → 账户 → 基金持仓 → Natives | F3 |
| M4 · Fund Intelligence | 基金持仓股票 → 价格 → 权重 → 基金穿透 → 全资产穿透 | F4–F6 |
| M5 · Decision Support | 风险 / 评分 / 置信度 / AI 解释 / 辅助意见 | F7–F10 |

V1 Definition of Done：`设置 → App Center → Fund → Install → Signed Catalog → Download .nap → 5MiB Gate → SHA256 → Install Transaction → Native Host Registration → Health Check → Commit → Sidebar「应用 └── 基金」→ 打开 → fund-host 按需启动 → 手工录入 → 养基宝扫码 → 同步持仓/净值 → 穿透 → 行情 → 真实股票暴露 → 行业暴露 → 风险与评分 → 关闭 → fund-host ≤2s 退出`；卸载后 Sidebar 消失、fund-host/Manifest/Package/Registry 删除、`fund.db` 保留；重装检测 `fund.db` → migration → 恢复全部个人数据。
