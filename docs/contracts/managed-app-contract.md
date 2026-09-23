# Natives 内置模块契约 v3

> 状态：CURRENT · 日期：2026-09-14
> 依据：ADR-0027、ADR-0029、ADR-0031、`docs/standards/technical/06-built-in-modules.md`、`docs/standards/technical/07-built-in-app-development.md`
> 本契约取代旧的独立模块包、运行时下载、独立 Native Host executable 及每应用动态 manifest 契约。

## 1. 产品不变量

1. Natives 是唯一安装、更新和发布的产品。
2. 基金等官方内置应用是内置模块，源码归入 Natives Monorepo，静态编译入统一产品级 `natives-app-runtime`，UI 随完整安装包交付。
3. 模块首次打开只做用户数据初始化/迁移，不下载代码。
4. App Center 只提供打开、显示/隐藏、偏好和数据管理。
5. 模块不得创建独立桌面产品、独立可执行文件、独立系统入口或独立安装记录。
6. Extension/Core 不包含单个模块的业务逻辑。
7. 第三方原生模块和远程执行代码不在当前契约范围。

## 2. 固定产品布局

### 2.1 系统产品源

正式 macOS 产品使用：

```text
/Applications/Natives.app
/Library/Application Support/Natives/
├─ product-manifest.json
├─ product-manifest.sig
├─ extension/
├─ hosts/
│  ├─ native-file-host
│  ├─ model-host
│  └─ natives-app-runtime
└─ modules/
   └─ fund/
      └─ ui/
```

本地候选使用独立的 `Natives-Local` 产品身份和系统源。Windows/Linux 路径只在对应平台实机验收后写入打包规范，不从 macOS 推断。

系统源只含签名代码和固定资源。安装器不得在其中保存用户 DB、activation、偏好或 Secret。

### 2.2 当前用户私有根

```text
~/.natives/
├─ natives.db
├─ apps/
│  └─ <appId>/
│     ├─ activation.json
│     ├─ data/
│     │  └─ fund.db
│     ├─ imports/
│     ├─ cache/
│     └─ logs/
└─ model/
   └─ usage.db
```

**绝对规则**：用户数据目录（`~/.natives/apps/<appId>/`）严禁存放可执行文件（彻底废弃 `runtime/<version>/app` 模式）。可执行代码全部属于系统产品源下的 Natives Product Code。

## 3. Product Manifest v2

组合清单使用 Schema 2：

```json
{
  "schemaVersion": 2,
  "product": "natives",
  "version": "0.1.0",
  "platform": "darwin",
  "arch": "arm64",
  "launcher": {
    "path": "launcher/Natives",
    "bytes": 1,
    "sha256": "<hex>"
  },
  "extension": {
    "path": "extension",
    "treeSha256": "<hex>"
  },
  "appRuntime": {
    "protocolVersion": 2,
    "path": "hosts/natives-app-runtime",
    "bytes": 1,
    "sha256": "<hex>"
  },
  "modules": [
    {
      "appId": "fund",
      "entryRoute": "app.html?app=fund",
      "moduleApiVersion": 1,
      "dataSchemaVersion": 1,
      "capabilityVersion": 1,
      "ui": {
        "path": "modules/fund/ui",
        "treeSha256": "<hex>"
      }
    }
  ]
}
```

规则：

- `appId` 匹配 `^[a-z0-9][a-z0-9._-]{0,63}$`，发布后不改名。
- 路径必须是清单根内相对路径，拒绝绝对路径、`..`、symlink/reparse point 逃逸。
- `bytes` 与实际字节精确一致；SHA-256 用最终签名后文件计算。
- 清单原文受产品签名保护；正式构建拒绝开发信任根。
- 产品更新验证全部固定文件后才切换 generation。

## 4. 产品配置事务

Extension 首次握手并通过 origin 校验后，Host 自动执行一次 per-user 产品投影：

```text
读取可信系统源
→ 验证 Product Manifest v2 与平台签名
→ 验证 Launcher / Extension / Hosts / Modules UI 字节与哈希
→ 注册单一 App Runtime Native Host（com.natives.app_runtime）
→ 为全部固定模块写 activation.json 元数据投影
→ 清理旧版 legacy per-app host manifest 与旧 runtime 目录
→ 单事务更新产品 version/generation
→ 原子提交
```

要求：
- 相同清单重复执行幂等，generation 不重复增加。
- 旧版本不得覆盖已配置的新版本。
- 任一步失败可重试；活动版本保持完整，不出现半配置模块。
- 安装器不以 root 执行业务迁移或写用户数据。

## 5. Core Apps 协议（v5）

Core 协议方法：

| 方法 | 作用 | 副作用 |
|---|---|---|
| `apps:handshake` | 核对 Chrome caller origin 与协议版本（v5），按需完成产品投影与统一 Host 注册 | 有（首次或产品版本变化） |
| `apps:list` | 返回固定模块 + 用户偏好 + 产品配置状态 | 无 |
| `apps:get` | 返回一个模块的真实登记/运行元数据 | 无 |
| `apps:product_status` | 返回产品源和当前 generation | 无 |
| `apps:set_enabled` | 启用/停用模块运行资格 | 有 |
| `apps:set_sidebar` | 显示/隐藏导航入口并排序 | 有 |
| `apps:clear_data` | 按确认范围清除模块用户数据 | 有 |
| `apps:health` | Core 登记健康 | 无 |
| `apps:open_onboarding` | 打开随包固定指南 | 只启动短命浏览器打开命令 |

## 6. App Runtime Host 身份与启动

- 整个产品在操作系统 Native Messaging 目录仅注册唯一的 App Runtime Native Host：
  - 生产：`com.natives.app_runtime` 指向 `hosts/natives-app-runtime`；
  - 本地：`com.natives.local.app_runtime` 指向开发构建的 `natives-app-runtime`。
- 废除每模块动态计算和注册 Native Host Manifest（`com.natives.app.a<hash>` 彻底废弃）。
- 用户在 Chrome 中通过 `app.html?app=fund` 打开模块时，连接 `com.natives.app_runtime`，由 Chrome 拉起一个独立的 `natives-app-runtime` 进程实例。
- 每 appId 至多一个活动运行实例；同用户至多四个活动模块实例。

## 7. App Runtime 协议 v2

握手与运行请求：

| 方法 | 作用 |
|---|---|
| `app:handshake` | 传入 `protocolVersion: 2`、`appId`、`productVersion`、`activationGeneration`，Runtime 选定模块并绑定上下文 |
| `app:start` | 启动 loopback HTTP 服务器并返回动态端口、instanceId、generation |
| `app:status` | 查询真实运行与数据状态 |
| `app:session` | rotate / issue / revoke iframe session |
| `app:stop` | 进入统一 shutdown 并退出当前 Runtime 进程 |
| `app:data_status` | 返回 schema、迁移和恢复状态 |

Runtime 处理流程：
```text
收到 app:handshake
→ 校验 Chrome Origin
→ 校验 Product Version
→ 校验 appId
→ 查询 compile-time ModuleRegistry
→ 校验 activation
→ 获取 runtime lock
→ 实例化模块并注入 ModuleContext
→ 启动 loopback API 路由
```

## 8. iframe 与 loopback

- `app.html` iframe sandbox 精确为 `allow-scripts allow-forms`。
- App Runtime 进程只绑定 `127.0.0.1:0` 并验证 Host header。
- iframe load 后先生成至少 128-bit challenge；Host 签发 32-byte CSPRNG bearer。
- token 绑定 appId、instanceId、generation、challenge，最长 15 分钟。
- `postMessage` 校验保存的 contentWindow；`Origin: null` 仅用于 CORS，不作为身份。
- CORS 仅允许有效 bearer、声明的方法和头；禁用 cookie/credentials。
- iframe reload/navigation、hidden stop、pagehide 和显式 stop 立即撤销旧 token。
- 单帧、HTTP body/response、并发、速率和超时使用固定上限。

## 9. 生命周期

```text
ABSENT → BOOTING → SELECTING_MODULE → INITIALIZING → READY → STOPPING → ABSENT
```

- `app.html` 隐藏 60 秒且无 in-flight 请求时调用 stop。
- stdin EOF、用户停止、页面关闭和 OS 终止共用取消/关闭路径。
- shutdown 顺序：拒绝新请求 → 撤销 session → 关闭 loopback listener → 取消任务 → 刷新 SQLite → 释放锁 → 退出进程（`exit(0)`）。
- **硬指标：Native Port 断开（stdin EOF）到进程退出时间 $\le 2$ 秒**。
- 停止后当前模块的进程、iframe、token、端口、timer、SQLite 连接全部归零。
- 页面恢复后只能由用户动作或 BFCache 恢复逻辑重新打开一次。

## 10. 数据与迁移

- Core DB 只写模块登记、偏好、product generation、activation 和清理收据。
- App Runtime 独占 `apps/<appId>/data/`；Core 不创建业务表，App 不连接 Core DB。
- schema 迁移：backup → prepared journal → migrate → verify → commit。
- 失败恢复备份；已接受新业务写入后不静默恢复旧备份。
- 清数据按 data/imports/cache/logs/credentials 分范围，必须二次确认和 requestId 幂等。
- 清除失败保留可重试收据；代码、activation 和模块偏好默认保留。
- Secret 只进入 `com.natives.app.<appId>` Keychain namespace。

## 11. App Center 投影

固定模块即使当前用户尚未完成本次握手，也必须在 App Center 中可见。卡片状态来自：

- Product Manifest v2 是否声明；
- 系统源载荷是否存在且可验证；
- 当前用户 product generation 是否配置；
- enabled/showInSidebar 偏好；
- activation 是否存在；
- App Runtime 最后一次真实健康或数据迁移状态。

允许动作：打开、显示/隐藏、启用/停用、偏好、清数据、打开安装指南。
不允许展示模块级安装、下载、更新、卸载或独立发布。

## 12. 本地与正式 Gate

### A-Local

- 隔离身份、注册、数据根和 Keychain；
- 真实 fund 作为 Monorepo 内置模块随完整候选编译；
- Product Manifest v2 / hash、配置事务和 App Runtime 协议 v2 通过；
- Chrome 页面真实打开、隐藏、关闭和 ≤2s 进程彻底回收通过。

### B-Local

- 从完整本地安装包安装；
- Launcher → Chrome → 扩展引导 → Natives → App Center → fund 全链路；
- 重装、修复、N→N+1、断网首次打开和数据保留通过。

### Release

- 正式 Extension ID、平台签名、公证/验签和生产信任根；
- 开发 key/fixture 扫描为 0；
- 无任何 `fund-host`、`*.nap` 或旧每模块 manifest 引用；
- macOS/Windows 各自实机安装和浏览器注册；
- 性能、安全、数据和 30 分钟生命周期门禁；
- 另行取得发布授权后才上传。

## 13. 完成定义

- 产品只有一个安装/更新链；
- 产品只有一个 `natives-app-runtime` 可执行程序；
- 模块始终可见且没有独立分发语义；
- Product Manifest v2 与平台签名覆盖实际最终字节；
- 运行实例彻底随页面关闭回收（≤2s）。
