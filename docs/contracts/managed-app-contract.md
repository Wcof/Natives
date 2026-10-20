# Natives 内置模块契约 v3

> 状态：CURRENT · 日期：2026-10-04
> 依据：ADR-0027、ADR-0029、ADR-0031、`docs/standards/technical/06-built-in-modules.md`、`docs/standards/technical/07-built-in-app-development.md`
> 本契约取代旧的独立模块包、运行时下载、独立 Native Host executable 及每应用动态 manifest 契约。
> ADR-0039 的包内服务与扩展约束见第 14 节；此次澄清不改变 Product Manifest v2 或 App Runtime Protocol v2 的线格式版本，也不表示源码整改已经完成。

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
      "capabilities": ["local-data", "platform.secret"],
      "views": ["home", "positions", "settings"],
      "ui": {
        "path": "modules/fund/ui",
        "treeSha256": "<hex>"
      }
    },
    {
      "appId": "tokenusage",
      "entryRoute": "app.html?app=tokenusage",
      "moduleApiVersion": 1,
      "dataSchemaVersion": 1,
      "capabilityVersion": 1,
      "capabilities": [],
      "views": [],
      "ui": {
        "path": "modules/tokenusage/ui",
        "treeSha256": "<hex>"
      }
    }
  ]
}
```

> 示例展示双模块形态；实际清单以 `modules/registry.json`（fund + tokenusage）为准，
> 安装器按 registry 迭代产出。`dataSchemaVersion` 示例值为占位，以各模块真实
> manifest 为准，不从本文示例推断当前 schema。

规则：

- `appId` 匹配 `^[a-z0-9][a-z0-9._-]{0,63}$`，发布后不改名。
- 路径必须是清单根内相对路径，拒绝绝对路径、`..`、symlink/reparse point 逃逸。
- `bytes` 与实际字节精确一致；SHA-256 用最终签名后文件计算。
- 清单原文受产品签名保护；正式构建拒绝开发信任根。
- 产品更新验证全部固定文件后才切换 generation。
- `views`（ADR-0038）：模块接受的深链导航 token 白名单；Shell 只转发白名单内
  token，未声明模块保持纯 token 正则行为。字段随签名清单→激活收据 v3→
  `apps:get` 投影传递，与 `capabilities`／`requires` 同样受编译期一致性校验。

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
- ADR-0042 双入口下，通用 Shell 和空间摘要经唯一 Core Relay／认证 Web RPC 申请模块会话，由 Core 按 `appId` 复用一个产品 Runtime Worker；页面不再直接启动上述 Runtime Host。旧注册记录不构成第二启动路径。
- 每 appId 至多一个活动运行实例；同用户至多四个活动模块实例。

## 7. App Runtime 协议 v2

握手与运行请求：

| 方法 | 作用 |
|---|---|
| `app:handshake` | 传入 `protocolVersion: 2`、`appId`、`productVersion`、`activationGeneration`，Runtime 选定模块并绑定上下文 |
| `app:start` | 启动 loopback HTTP 服务器并返回动态端口、instanceId、generation |
| `app:status` | 查询真实运行与数据状态 |
| `app:session` | 对当前 Surface 执行 rotate / issue / revoke；不撤销其他 Surface |
| `app:stop` | 释放当前 Surface；最后 owner 离开后按 ADR-0042 宽限期回收 Worker |
| `app:data_status` | 返回 schema、迁移和恢复状态 |
| `app:card_snapshot` | ADR-0036 授权的短命只读模块摘要，响应上限 64 KiB |
| `app:secret` | 模块 Secret 的 Keychain 窄写入／掩码状态／确认删除（ADR-0038） |

Core 注入 `surfaceId`，页面提供的同名字段不受信任。Native Relay 每个 Port 生成独立随机身份，EOF 释放；Web RPC 以 `X-Natives-Surface-Id` 绑定已认证 Cookie 会话及租约，跨会话或跨模块拒绝。每个 Surface 的 generation、challenge 集合和 bearer 独立；续期替换本 Surface bearer，撤销或重新打开不影响其他 Surface。已失效租约释放对应模块 owner，不能只减少网页租约计数而遗留 Runtime。

Launcher 的入口选择只保存在自身偏好中，顶栏按相同偏好跳转。独立网页跳转经
Core 的 `--launcher-web [space|files|tokenusage]` 准备服务及一次性会话；目标是
固定白名单，不接受任意路径／URL，stdout 不携带 bootstrap token。

Runtime 校验 bearer 后，向模块传递不可由 HTTP body／header 自报的 `RequestContext`（principal、surfaceId、appId、workerEpoch、generation、capabilities）。生产 HTTP adapter 必须调用带上下文的模块入口；上下文缺失默认拒绝。Fund 的资讯采集、机会刷新和通知任务按 Surface 绑定 taskId／requestId；跨 Surface 状态、取消和重放拒绝，释放仅取消对应任务代际。Runtime 状态同时报告 activeRequests／activeTasks，保护在途任务不被空闲回收截断。

空间投资卡继续使用 `fund.cards.v1`。所有字段在同一数据库读事务中生成，
`payload.dataRevision` 标识业务版本。`registeredFundHoldings` 保留既有 wire key，
其范围是全部账本证券持仓（股票、ETF 和基金），展示名称为“投资持仓摘要”。
资产、目标和风险配置复用 Fund 现有领域查询；增量 `cash` 仅包含汇总金额，
`groupAllocation` 仅包含分组名称、目标／实际比例、金额和失效引用数量，
`portfolio` 仅包含组合的汇总证券指标与每日收益，不包含历史、逐项标的或账户。
金额使用十进制字符串／金额对象；缺失保持 null。禁止转发首页完整响应、
账户／资产／分组 ID、绑定关系、个人路径、Secret 或 HTML。分组卡最多展示
前 6 组并明确剩余数量；风险配置检查继续明确标注风险类别口径。

### `app:secret` schema（ADR-0038）

仅可信 Extension／认证 Web Shell 表单经 Core 可调用；sandbox iframe 结构上无端口能力。
未知字段、越界参数与错误会话在产生任何副作用前拒绝：

```json
{ "method": "app:secret", "params": {
    "instanceId": "<app:start 返回值>",
    "generation": "<app:start 返回值>",
    "op": "set | status | delete",
    "accountId": "^[a-z0-9][a-z0-9:._-]{0,63}$",
    "value": "<仅 set，≤8192 bytes，UTF-8>",
    "confirm": "<仅 delete，必须为 true>" } }
```

响应：`set`/`delete` → `{ "ok": true, "op": "...", "accountId": "..." }`；
`status` → `{ "ok": true, "exists": bool, "updatedAt": "<RFC3339 或 null>" }`。
没有读取明文的 op。能力门：激活收据中该模块 `capabilities` 必须含
`platform.secret`，否则 `APP_CAPABILITY_DENIED`。Keychain locked／denied →
可恢复 `APP_KEYCHAIN_LOCKED`；未实现后端（Linux/Windows 本期）→
`APP_KEYCHAIN_UNSUPPORTED`；不降级明文存储。Secret 仅存
service=`com.natives.app.<appId>`、account=`accountId` 的 OS Keychain 条目。

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
- CORS 仅允许有效 bearer、声明的方法和头；当前模块 API 声明 `GET`、`POST`、`PUT`、`DELETE`，响应与预检必须一致；禁用 cookie/credentials。
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

## 14. 包内模块服务与扩展（ADR-0039）

当前统一接口以 `crates/app-runtime-core/src/module.rs` 为生产定义，准确签名和
字段示例见 [技术 07](../standards/technical/07-built-in-app-development.md)。
`BuiltInAppModule` 管理 descriptor、initialize、start、data_status、health、
shutdown；`start()` 返回 `Arc<dyn ModuleService>`，由 Runtime 的受控 listener
调用 UI／业务 API／卡片服务，不在生命周期锁中处理业务 HTTP。
`ModuleContext` 的目录、Secret 窄能力、取消与 limits 注入不扩大模块权限。

包内领域通过明确输入／输出的业务操作协作；HTTP／Native 与后台任务调用同一
用例，不反向引用 handler。所属应用的 Store／迁移 owner 保持唯一数据库权威，
跨领域写入由一个事务 owner 编排；内部领域不能增加 appId、独立库、Host 或
分发身份。公共状态、动作装配、SDK 适配、资源归属与检查要求统一见
[技术 08](../standards/technical/08-module-boundaries-and-contracts.md)。

新增应用继续实现现行 Library 契约、静态工厂注册与 UI 构建，声明实际元数据、
能力和 views，随完整产品清单验签／初始化／关闭验收。registry、编译描述、
产品清单、激活投影与 Client 的版本和能力必须一致；不兼容协议或未来 schema
在业务写入前拒绝，不以页面自报版本／verified 或相同主版本代替兼容证明。

源码内核更新仅允许核验后的隔离开发身份与开发源；正式身份在下载、构建、
换装前拒绝，正式代码继续沿唯一产品交付链。此规则不创建在线产品更新引擎，
也不授权 CLIProxy 主版本迁移。

上述目标的施工与逐项证据见[模块整改 Plan](/Users/ldh/Downloads/project/plan/mode/README.md)，
实施状态以项目 ROADMAP 为准，契约存在不等于全部检查已经实现。

### 双入口模块投影失效通知（ADR-0042 收尾）

模块持有业务写入权威；页面在成功完成业务写请求后，以当前 iframe 的 `generation`／`challenge` 向 Shell 发送 `natives:data-changed`，不携带业务值或路径。Shell 验证来源、会话及固定字段，合并短时间通知，再调用 Core `app:invalidate {appId, originId}`。Core 以传输层注入的 Surface 验证已绑定的模块，禁止跨模块或未握手调用，不拉起新 Worker、不写库，仅发送 `module_changed` 失效提示。`originId` 是 Shell 的随机去重标识，不是鉴权凭据。

其他同模块 Surface 通过现有 Core 事件总线接收提示并重取权威快照；发送方忽略自身通知。断线／序号缺口触发重取，隐藏页面及打开编辑表单时延后至恢复可见／表单关闭，不丢草稿，不重放写请求。Shell dispose 释放订阅与合并 timer；不增加逐卡轮询、数据库副本或独立连接通道。

Web Cookie 换会话时，同浏览器各 document 通过仅含随机标识的 storage 失效信号重查 `/web/session`；信号不传 Cookie、CSRF 或 token。旧的异步会话检查不得覆盖新会话。服务端只撤销本次交换请求 Cookie 对应的旧会话及其 Surfaces，不影响其他浏览器会话。模块 Shell 在同一 Runtime 仍存活时重建自己的 Surface 握手并换发 bearer，保留现有 iframe 和表单，不重放写入。Runtime 端口确实变化时保留旧页面并明确提示复制草稿后重新打开，不伪造自动恢复。
### 双入口模块维护操作

模块禁用由 Core 在同一 appId 的调度门内协调：存在受保护任务或执行中请求时返回
`APP_BUSY`，不改变启用状态；否则撤销该模块全部 Surface 并停止所属 worker 后保存
禁用状态。成功失效事件携带 appId 与 maintenance 标记，两入口的 Shell 均关闭对应
iframe，不能只依赖同浏览器 BroadcastChannel。其他模块不受影响。

清数据仍要求用户先关闭该模块的全部 Surface；有 owner 或任务时拒绝，不强停业务。
全部 owner 已释放后，可在管理操作中结束空闲宽限并等待自有 worker 退出，再执行
已有双重确认、范围与回执校验的清理。无效参数不得触发 worker 停止。

用户重新确认清理时，新确认范围覆盖的同模块 pending 回执一并完成；原回执仅记录
自身确认范围内实际清理的部分。范围未覆盖的凭据等旧意图仍待处理，不能自动扩大
本次清理。已完成旧回执再次重放不停止 worker，也不删除随后创建的数据。
