# 官方托管应用契约 v1

> 状态：active-target；随 [ADR-0027](../adr/0027-managed-apps-independent-delivery.md) 在实施方案 A0 提交生效（accepted-target 同批；active-target 表示可据此实施并修改生产路径，不代表实现已完成）。
> 2026-09-11 按 [ADR-0029](../adr/0029-unified-suite-preinstalled-apps.md) 增补统一套件、预装和分级验收；本地签名例外仅按 §4.2 生效，其他 MUST 不豁免。实现与验收证据见唯一实施方案。
> **2026-09-12 路线收敛（用户最终决定）**：Natives 是一个完整应用，基金等均为**内置模块**，随完整安装包一起安装、更新和修复；取消模块独立下载/安装/更新/卸载、在线 Catalog 驱动新 appId 与 Fund 独立 nap 发布。本文中与该决定冲突的表述按以下语义执行：签名 Catalog/验签/安装事务能力只在**完整产品安装/更新层**核验组合清单内的固定模块文件；应用中心的模块操作只剩打开/显示隐藏/设置/数据管理；更新统一为"更新 Natives"。数据、Keychain、沙箱、会话鉴权、锁与 EOF 回收 MUST 全部保留。凡冲突处以此注记为准。
> 本文是应用中心与应用开发者共享的接口唯一来源；实现状态和任务进度不写在本文。
> 范围：官方 managed_local 应用。外部 Web URL 仅保留类型语义，本期不得注册或执行。
> 版本：Catalog v3 / Core Apps protocol v4 / App protocol v1。实现前检查当前 HEAD，若号码已占用则顺延并同步全部 fixture，禁止覆盖已发布协议。

## 1. 产品对象与权威

Natives 是唯一的用户产品。托管扩展包是 Natives 应用中心管理的可选组成部分，内部模块（如持仓、账本、净值等）不拥有独立安装对象或产品身份。

| 对象 | 含义 | 唯一权威 |
|---|---|---|
| AppDefinition | 应用身份、用途、兼容要求与交付声明 | 签名产品组合清单（2026-09-12 收敛：不再有独立签名 Catalog） |
| Installation | 扩展包安装记录（已安装版本、包收据、启用状态、侧栏设置），不是 Mac 应用安装记录 | Core App Store |
| RuntimeInstance | 实现层进程/端口实例（一次真实应用进程、锁、会话与当前工作状态），不是新的产品 | 持有运行锁的应用 Host |
| Surface | Natives 内的应用呈现会话（通用 app.html 中一个应用呈现会话） | 该 app.html 页面 |
| UserData | 用户业务记录与导入原件；由扩展包拥有，但只能经扩展包接口访问 | 对应扩展包的业务库/数据目录 |
| Credential | 外部服务凭据 | OS Keychain，业务库仅保存 opaque reference |

安装状态与运行状态分离；Core DB 不通过持久化 running 布尔值判断进程存活。
显示 running 必须取得当前 owner 的真实响应；无响应先显示 unknown/checking，锁被占用且当前 profile 找不到 owner 时显示“应用正在另一会话中使用”。仅凭锁和 Native origin 无法识别具体 Chrome profile；不得声称准确检测到哪个 profile，也不能猜测 stopped。

## 2. 应用声明

组合清单沿用已有 app_id 命名风格，不引入第二 App Registry（2026-09-12 收敛：不再发布独立签名 Catalog）。schema 字段由协议代码生成校验资料，禁止扩展页面和 Host 各维护不同枚举。

| 字段 | 要求 |
|---|---|
| parentProduct | 固定为 `"natives"`；声明归属于 Natives 主产品，不是独立桌面软件 |
| packageRole | 固定为 `"managed_extension"` 或 `"managed-app"`；声明为 Natives 托管子应用/扩展包 |
| app_id | 稳定且全局唯一；沿用现有合法 ID，fund 不改名 |
| kind | 当前只接受 managed_local；web_link 是未来语义，当前返回 unsupported |
| runtimeImplementation | 声明为 `managed-native-host`；由受控独立 Native Host（如 fund-host）承载业务代码与构建期嵌入的 UI，Core 管控其生命周期；禁止将业务编译进 Core（禁止 Fund builtin），亦不引入 WASM |
| package_id / app-exec | `app-exec` 仅表示 Package Internal Runtime Entry（包内部受控运行入口），绝对不得解释为 Standalone Application、独立 .app 或独立用户安装对象 |
| moduleManifest | 非权威描述性清单，仅列出包内模块及其说明（如 portfolio、ledger、nav、import、storage、migration）；不产生新的安装对象，不构成强制逐模块版本协议，Core 不据此校验；缺失或与实际不符不阻塞安装，仅作为审查参考 |
| name / description | 真实名称及 zh_CN/en 文案 |
| publisher | 官方发布身份；绑定本地信任根，不仅依赖一个 official 标签 |
| version | 应用 SemVer，与 Core/App 协议版本分离 |
| minExtensionVersion / minHostVersion | 安装管理端的最低版本 |
| appProtocolVersion | 1；独立运行协议 |
| dataSchema | 当前版本及可读/可写版本范围；用于决定代码回退是否安全 |
| permissions | 实际使用的能力声明；只展示或约束确实实现的能力 |
| packages | 每目标平台恰好一个 runtime 载荷（gzip 单载荷格式，wire ≤ 32 MiB、payload ≤ 128 MiB，精确长度与双 SHA-256 校验）；2026-09-12 收敛后该载荷是完整产品包内的构建产物，摘要登记进产品组合清单，不再独立发布或下载；禁止额外可执行文件、动态库、`.app` bundle 或脚本进入载荷 |
| runtimeApiVersion | 1；独立运行协议版本，随契约 §5 演进 |
| minCoreVersion | 包要求的最低 Core 版本（安装管理端协商用） |
| entryRoute | 包 UI 入口路由，如 `"app.html?app=<appId>"` |
| publishedAt / changelog | 有来源的发布时间和变更说明 |
| runtime budgets | 声明内存/请求/缓存限额，不能提升 Core 的绝对上限 |

### 2.1 runtime 载荷与 manifest 约束（ADR-0027 修订，2026-09-12）

每个官方包以受控独立 Native Host 载荷交付：`runtimeType: "managed-native-host"`，业务代码与构建期嵌入的 UI 在载荷内，Chrome 按连接按需启动，Core 只管安装/登记/激活/生命周期，不执行包业务。manifest 收敛为：`appId`、`runtimeType`、`runtimeApiVersion`、`minCoreVersion`、`capabilities`、`entryRoute`、`packages`（每平台一个 `.nap` runtime 载荷）。示例（仅为示例性，具体 schema 以协议代码生成的校验资料为准）：

~~~json
{
  "appId": "fund",
  "runtimeType": "managed-native-host",
  "runtimeApiVersion": 1,
  "minCoreVersion": "x.x.x",
  "capabilities": [],
  "entryRoute": "app.html?app=fund",
  "packages": { "darwin-arm64": { "artifactSha256": "…", "wireSize": 0, "payloadSha256": "…" } }
}
~~~

不存在 Core 内 builtin dispatch：Core 不再提供 `app:invoke` 业务分发生产入口，也不持有任何包业务静态依赖（单 Runtime/ManagedApp Registry 路线已由 ADR-0027 取代并从生产代码移除）。业务请求路径为：app.html 短连 Core 核验安装记录 → 直连包 Native Port → 包自身 loopback 业务接口。

载荷安全不放松：artifact_sha256/payload_sha256、Ed25519 catalog 签名、包身份、版本与 origin 校验对 managed-native-host 载荷同样适用，全部保留。

不允许声明任意系统路径、shell 命令、启动参数、安装脚本、任意 Native Host 名称或扩展脚本 URL。

正式 runtimeHost 由 Core 计算：com.natives.app.a + app_id 的完整 SHA-256 小写十六进制。避免 app_id 中的连字符与 Native Host 命名限制产生冲突；Keychain namespace 仍沿用 com.natives.app.<appId> 语义，fund 保持 com.natives.app.fund。映射结果持久化在安装收据中，不由页面猜测或 Catalog 覆盖。本地开发使用 §4.2 的隔离前缀，由同一共享命名函数计算，不由页面或 Catalog 切换。

## 3. 包与目录

首版一个平台包就是 gzip 单载荷 .nap，载荷为平台原生可执行程序；UI 的 HTML/CSS/JS/图片在构建时嵌入该程序。没有压缩目录树，没有安装时 npm/pip/cargo，也不要求用户预装语言运行时。该描述属 legacy executable 路线：整改后普通 .nap 是 Managed App Resource Package（manifest.json、ui/、assets/、migrations/、metadata/），生产包内禁止 app-exec、fund-host、*.app、*.dylib、Mach-O executable；仅当 runtimeType=native-exec（默认禁止，见 §2）时才允许原生可执行载荷。

每个包必须包含版本、平台、架构、wire_size、payload_size、artifact_sha256、payload_sha256。先做所选分发模式要求的平台签名处理，再计算最终载荷 hash 和压缩包 hash；正式模式还须完成适用的公证与实际平台验证。签名/哈希后修改二进制会使交付失败。

2026-09-12 收敛：完整产品（Natives）的首次使用**没有模块获取链路**——不存在在线 Catalog、模块资产下载或"未随附应用在线获取"，本节原有的浏览器模块下载 URL 校验规则随之取消（仅作历史记录）。首用只做当前用户产品配置（§3.2）：从已验证的系统源核验组合清单，为全部固定模块准备私有活动载荷、浏览器注册与 activation，不做任何代码下载。产品更新统一为"更新 Natives"完整产品版本，其来源与网络校验由实施方案 P5 定义，不继承已取消的模块下载语义。

Host 在可信边界核验签名产品组合清单原文与全部固定模块文件的实际字节摘要。不能只信任前端 alreadyVerified 或前端传来的 hash。签名私钥只在发布凭据中，禁止进源码、安装包或测试证据。（2026-09-12 收敛：模块 Catalog/资产的浏览器下载链路取消；产品级更新通道的网络校验规则由实施方案 P5 另行定义。）

目录由 Core/支持库根据标准用户根路径和签名身份计算：

~~~
~/.natives/
  natives.db                         # App Store，Core 写
  apps/
    .locks/<appId>.install.lock       # 安装/更新/清理串行
    .locks/<appId>.runtime.lock       # OS 排他锁，运行期间一直持有
    .locks/runtime-slot-<0..3>.lock    # 跨页面/浏览器的四个运行槽，持锁代表占用
    <appId>/
      activation.json                # Core 生成的只读激活投影，不是第二权威
      runtime/<version>/app[.exe]     # 已验证可执行程序，受限权限
      data/                          # 应用独占；升级、重装与修复保留（无模块卸载）
      imports/                       # 用户导入原件；默认保留
      cache/                         # 有界、可清理
      staging/<installId>/           # 失败可恢复，最多一个安装事务
      backups/<migrationId>/         # 业务 schema 迁移备份，有界保留
~~~

Native Host manifest 与 Windows 注册表属于 Core 的安装收据，不允许应用自己随意登记。**受限浏览器注册例外**：活动包代码、数据和收据只存在于 Natives 私有根 `~/.natives/apps/<appId>/`；浏览器发现 Host 所需的最小注册元数据是明确例外——macOS/Linux 写入相应浏览器的 Native Messaging Hosts 指定目录（Chrome、Chromium、Chrome for Testing 分别核验，CfT 从 146 起默认位置不同），Windows 注册表仅指向 manifest JSON；manifest 再指向 Natives 私有目录内的受验签载荷。禁止任意注册路径、任意 Host 名称，禁止广泛删除不属于本产品收据的其他注册项。实际扩展 origin 和 OS 平台差异由现有安装器机制统一处理，不接受请求体自报 origin 授权。

原生程序按用户权限运行。namespace 是数据组织约定，不能宣称阻止恶意原生程序访问其他用户文件。官方发布审查是当前信任前提，第三方 native 包明确拒绝。活动载荷安装目录仅属于 Natives 私有数据根（`~/.natives/apps/<appId>/`）；严禁写入 `/Applications`、独立 `.app` bundle、系统 Dock 图标或 LaunchServices 产品注册，不得伪装成独立桌面软件。

### 3.1 激活投影的读取契约

Core App Store 是安装权威；activation.json 是 Core 在安装/启用/停用时生成的原子投影。App Host 只读该投影，不猜测或直连 natives.db 的内部 schema。

投影使用严格 JSON，字段固定为 receiptVersion=1、appId、runtimeHost、activeVersion、generation、activationState（ready/maintenance/removed）、enabled、appProtocolVersion、payloadSha256、allowedOrigins、catalogEvidenceSha256。不得包含用户数据、凭据、任意启动参数或可覆盖标准路径的字段。

路径固定在标准 apps/<appId>/activation.json；Core 创建父目录为仅当前用户可写，文件权限 POSIX 0600 或对应 Windows 用户 ACL，拒绝符号链接/重解析点逃逸，写入采用 temp+fsync+rename。App ID 来自程序编译期身份，与安装目录、投影和 Catalog 四方一致；不能从网页参数决定读取路径。

catalogEvidenceSha256 指向标准 apps/.catalog-evidence/<sha256>.json 与同名 .sig：Core 保存已验证的原始签名目录，支持库用锁定的官方信任根验证后，核对本程序身份、版本、平台和载荷摘要。启动时校验 current_exe 的实际摘要与投影、签名条目一致。allowedOrigins 来自 Core 核验过的 Chrome 启动 origin，应用再与自己的真实 argv origin 比对。

签名保护发布身份与不可变包信息；enabled/generation 是 Core 的本地控制状态，由受限写入与 OS ACL 保护。这不是抵御同用户恶意原生程序篡改的认证系统，不额外引入一个把密钥放同盘的伪安全签名器。

每次变更先写持久 journal，再将投影原子改为 maintenance；随后修改注册与 DB，最后写与已提交收据一致的 ready 投影。整个过程持有 install→runtime 锁。崩溃时 App Host 遇到缺失/maintenance/不一致即拒绝启动，由 Core recover 恢复；不能推测启用或自行修复投影。停用写 enabled=false；卸载写 removed/删除投影，均先阻断新启动。catalog-evidence 仅在没有当前/上版收据与 journal 引用时清理。

### 3.2 统一套件、只读系统源与产品配置

一个完整产品包含 Core Files Host、现有 Model Host、薄打开入口、稳定的扩展身份与安装说明、签名产品组合清单、固定解压扩展目录及全部固定内置模块文件（2026-09-12 收敛：无预装清单、无逐应用包）。首个完整候选必须包含真实 fund；独立样例仅作为覆盖共享底层规则的低层测试 fixture，必须标为测试，不宣称基金可用。

macOS 正式系统源为 `/Library/Application Support/Natives/`，本地候选为 `/Library/Application Support/Natives-Local/`。目录及父级由系统安装器保护，root-owned、普通用户不可写；存主 Host、固定扩展目录 `ChromeExtension/`（含 manifest.json）与全部固定内置模块文件；不存离线种子、用户数据库、activation 或业务数据。活动模块载荷仍在每用户私有根，Native Messaging 注册仅在浏览器指定位置。

**可见主产品入口（2026-09-13，ADR-0029）**：同一安装器必须安装 `/Applications/Natives.app`，具备 Info.plist、正常 GUI 可执行入口、图标和系统应用登记；用户从“应用程序”双击即可打开 Chrome。未连接扩展时显示薄引导窗口，定位上述随包目录、提供浏览器所需操作；已连接则通过本次真实前台握手交接。不得标为隐藏/纯后台应用，不强制固定 Dock，不另建业务桌面界面；关闭引导或成功交接即结束 Launcher。模块仍不得创建独立 `.app`/系统图标。开发候选使用 `/Applications/Natives Local.app`、独立 bundle ID/名称/图标标识，并继续使用 Natives-Local 系统源及私有根，不覆盖正式入口。Windows/Linux 按对应平台实操声明支持。

**离线引导资源（2026-09-13）**：完整指南位于主产品 `Contents/Resources/onboarding/index.html`，CSS、经典脚本、图片和中英文内容全部本地随包，由 launcher.files 纳入同版本摘要/签名。PKG Distribution 的 conclusion 使用同一内容来源生成静态摘要，不依赖脚本或外网；首次双击则由 Launcher 明确在 Chrome 打开完整指南。HTML 只负责展示/语言切换/可降级复制，不使用扩展 API、Native Bridge、file 扫描、localhost HTTP、任意命令或新 URL Scheme；不能用查询参数、手工勾选或按钮点击证明安装/连接。实际状态由本次验证握手和产品配置确定。主/开发版本的路径、名称和扩展身份从各自完整产品元数据构建，不在浏览器猜测。详细交互与验收见唯一实施方案 §1.3。

`bundle-manifest.json`（签名产品组合清单）≤256 KiB，旁置 Ed25519 detached signature；沿用既有信任根与签名工具，不创建第二套密钥协议（2026-09-12 收敛：清单只描述完整产品内固定文件与固定模块，无 Catalog 条目、无模块下载 URL）。最小字段为：

| 字段 | 要求 |
|---|---|
| bundleSchemaVersion / suiteVersion | 1 / 套件 SemVer；与各应用版本独立 |
| distribution / platform / arch | production 或 local-development；精确匹配当前构建身份与机器 |
| extensionId / minExtensionVersion | 真实、稳定、可核验的扩展身份与最低版本；不使用假商店 ID |
| launcher | bundleId、version、files（relativePath/size/sha256）；只描述固定主产品 `.app` 内的入口和资源，纳入同一组合签名与平台验证 |
| hosts | role（file/model）、version、relativePath、payloadSha256；主 Host 校验适用平台身份 |
| modules | appId、version、entryRoute、多语言名称/说明、artifactPath、payloadSha256；与包内固定模块文件精确匹配，无独立安装记录或下载 URL |

hosts/modules/扩展的相对路径限制在受信系统源内；launcher.files 的相对路径只限制在本节固定主产品 `.app` 根内。根位置由生产/隔离开发构建策略固定，不能由页面或包参数任意改写。各自拒绝绝对路径、..、符号链接/重解析点逃逸、重复 ID 和未知字段；不允许命令或安装脚本。清单只描述不可变交付资产，不记录 enabled/running/用户数据，不是第二 App Registry。签名公钥可分发，私钥禁止随包；本地开发系统源的来源摘要还须满足 §4.2。

系统安装器只安装文件及必要主 Host 注册，不能猜测 `$SUDO_USER` 的家目录并写其业务数据/activation，也不运行子应用或迁移。首次配置在扩展的现有产品配置页完成：经过真实扩展 origin 校验的前台主 Host 连接，在当前 OS 用户权限下执行一次幂等「完成 Natives 配置」——校验当前签名组合清单，为全部固定模块准备私有活动载荷、浏览器注册与 activation，绑定同一 `productGeneration`；全部必需模块与注册验证成功后才标记本用户产品就绪，完成前不提供虚假「打开」，应用中心不触发配置、不显示「安装基金」。普通安装包不能替用户绕过浏览器扩展安装/启用政策，这一步在产品安装说明中明确一次性完成。

产品配置在 Core 内从已验证系统源读取固定模块文件，复用既有安装事务的验证、锁、journal、health、commit 和 recover 原语，不经另一套复制后写 DB 的捷径（2026-09-12 收敛：无种子字节流、无首次连接预装事务）。不开放网页传任意本地包路径的 RPC。当前用户连接关闭则取消或安全收尾，下一次前台连接恢复；沿用每事务十分钟上限，不启动 Service Worker Port、常驻配置 daemon 或后台重试。health 不运行基金业务或迁移。

在现有 App Store 元数据/收据上持久化已配置的 `productGeneration`、已处理的产品版本与用户显示/停用偏好，供通用状态投影使用；不另建注册库，无预装来源语义。新增投影字段应通过协议类型统一生成并做兼容检查；名称和枚举不得由 UI 自行发明。

| 当前状态 | 同一或更新产品的处理 |
|---|---|
| 首次、未配置 | 完成一次产品配置：校验组合清单并准备全部固定模块载荷/注册/activation，绑定同一 productGeneration；不自动运行业务 |
| 同版本且摘要一致 | 校验/恢复所需注册与投影，不重复配置、不重置偏好 |
| 系统源版本低于已配置产品 | 拒绝隐式降级，提示获取能承接数据的完整新版；不重置偏好与数据 |
| 系统源版本更高 | 属于产品更新流程（维护窗口与整包切换，见实施方案 P5），不由应用中心或模块 open 隐式补装 |
| 同版本但摘要不同 | 报冲突，拒绝覆盖 |
| 用户曾移除或停用 | 保留选择并转为入口关闭偏好；整包更新不自动打开，开启入口只改偏好，代码已随产品交付 |
| 运行锁占用、未完事务或损坏投影 | 显示 busy/需要恢复；按既有 recover 处理，不强杀或猜测成功 |
| 新清单没有原应用 | 不自动卸载，不清除其数据 |

全部固定模块与注册在同一 `productGeneration` 事务内核验；任一必需模块失败则产品不标记就绪，并提示「完成/修复 Natives 配置」，不能部分成功却展示整体可用。重装主产品不等于恢复业务备份。旧用户级主 Host 注册可能遮蔽系统注册：只在校验既有 Natives 身份和目标后提示/修复，禁止宽泛删除浏览器目录或其他 Host。所有实际支持的浏览器及 profile 必须分别验证。

## 4. 安装、更新与清理

2026-09-12 收敛：本节的验证、流式落盘、锁、journal、health、commit、recover 与清理机制**只在完整产品安装/更新与当前用户产品配置层使用**，核验组合清单内的固定模块文件。`apps:install_begin / install_chunk / install_finish / install_commit / install_abort / suite_prepare / uninstall / rollback` 不再对模块分发开放——页面发起的此类请求必须在任何存储/网络/注册变更前返回固定协议错误码并提示「更新 Natives/重新加载扩展」，不得静默回退在线 Catalog。`apps:clear_data` 与 `apps:uninstall` 分离：清数据只处理用户确认的数据范围，保留代码、注册、activation 与显示/启用/排序偏好，不写 removed 意图（数据重置细则见实施方案 §4.3）。管理实现修改现有协议类型，不创建 parallel v2 store。

> 实现注记：v4 传输协议落地后，`install_begin` 曾只接受签名 Catalog（`catalogBase64` + `signature`，Ed25519 固定信任根验签，Core 自行选包）；旧的无签名整包 `install_package` 方法已删除。2026-09-12 收敛后这些方法仅作为产品层复用的传输/校验机制保留，模块分发语义见上。分块方法实现为 `install_chunk`。

### 4.0 安装传输协议（Core Apps v4）

Native 请求/响应均限制为最多 512 KiB UTF-8 JSON，严格小于 Chrome 1 MiB 出站上限，不利用入站大帧搬运整个可执行程序。

| 方法 | 必填输入 | 成功输出/确定语义 |
|---|---|---|
| apps:install_begin | catalogBase64（原文≤256 KiB 签名 Catalog JSON）、signature（base64 Ed25519） | Core 用固定官方公钥验签并自行选包；返回 installId、packageId、chunkSize=262144、nextOffset=0 |
| apps:install_chunk | installId、packageId、offset、dataBase64（解码≤256 KiB）、chunkSha256 | 只传原始 gzip artifact；Core 计算 chunk hash 并原子记录有效长度，返回 nextOffset |
| apps:install_finish | installId、packageId、artifactBytes | 长度必须等于签名 wire_size；校验整体 artifact hash，流式 gunzip→受限临时载荷并核对长度、payload hash、格式/架构；返回 staged |
| apps:install_commit | installId | 全部校验与 health 已完成后按 journal 激活；返回新的安装快照 |
| apps:install_abort / recover | installId（recover 可恢复本命名空间未完成事务） | 停止解压/写入、清理或恢复 staging，原安装版本保持可用 |

Catalog、签名、包字节都不能由页面标记已验证后省略 Host 检查。前端只流式下载原始压缩字节，解压由 Host 使用成熟 gzip 实现执行，不在页面构造 128 MiB 解压数组。

同一个包顺序上传、同时最多一个 chunk 在途。offset 等于当前持久有效长度才追加；较小 offset 仅在完整重发已确认 chunk 且长度/hash/实际已存字节一致时返回同一 nextOffset；重叠改写、不同数据、乱序或超长返回 APP_PACKAGE_INVALID。断连不继续后台下载；重开应用中心先 recover，复用已确认 staging 长度重新下载并跳过相同前缀；服务不支持可靠续传时从网络字节零开始，不跳过校验。

finish 前崩溃以 journal 和实际文件长度恢复；发现半 chunk 截回最后确认长度。取消需终止当前读取/解压，并清理本事务文件，不影响已提交版本。压缩与解压上限在流式处理每一步检查，ZIP bomb/尾随多载荷/截断 gzip 拒绝。下载/传输停滞 30 秒可重试失败；总事务最长十分钟，无后台自动重试。

### 4.0.1 安装事务

1. Core 验证签名目录、兼容性、平台和资源上限，取得 install 锁，再取得 runtime 锁；顺序固定。
2. runtime 锁不可得则返回 APP_BUSY 或 APP_RUNNING_ELSEWHERE，绝不删正在运行的版本或业务数据。Core 不长期等待锁、不杀未知进程。
3. 创建可恢复事务；分块下载、校验、落盘至 staging；取消/失败保留原安装版本。
4. 验证可执行格式、目标架构、平台签名；以固定 --health 参数启动候选程序，输出有界、限时五秒，必须回收进程。
5. --health 只检查包完整性、资源可读与协议定义；不取 runtime 锁、不启动业务、不接触生产 DB/Keychain/网络。避免与安装器持锁形成死锁。
6. journal 记录旧 manifest/注册表/版本指针/收据。原子激活候选；任一步失败均按 journal 恢复。文件与 DB 不能假称一个原子事务，recover 必须覆盖每个崩溃切点。
7. 提交后保留一个上版代码，清理更旧版本及 staging。运行锁在整个版本切换后释放；不得在持 DB Mutex guard 时调用再次加锁的 snapshot/query。
8. 更新不会自动运行应用。第一次真实打开时，应用自行完成业务库备份、迁移与验证后才进入 ready。
9. 模块卸载语义已取消（2026-09-12 收敛）；历史卸载收据仅在受控产品迁移中按 journal 收尾。清除用户数据和 Keychain 是独立的显式危险操作，与代码操作分离，沿用二次确认。
10. Windows 被占用文件、锁住的 Keychain 或无法删除注册表：显示 retryable cleanup_pending，保留清理收据；禁止返回成功再默默留下残留。

平台安全提示正常展示。禁止通过移除 quarantine、关闭签名验证或使用任意 shell 安装器实现“无感安装”。

正式分发的平台检查必须区分：macOS 为 Developer ID 签名、固定发布者 Team ID、notarytool 的已接受公证产物及实际 Gatekeeper 启动证据；安装时校验代码完整性和身份，不把 Catalog 自报 notarized=true 当公证验证。裸 CLI 的公证/离线票据支持在 A1 核验，不把不适用的 spctl/stapler 输出当成功；首次离线无法验证则明确不可用，不移除 quarantine。Windows 为 WinVerifyTrust/Authenticode 与固定发布者身份校验，签名/吊销状态不确定不静默接受。Linux 不要求不存在的统一 OS 签名机制，以官方 Ed25519 发布签名、载荷 hash、ELF 架构和受限文件权限作为验收。平台验证调用限时 30 秒并回收；本地模式的例外仅见 §4.2，开发资产不能进入正式产品组合。

### 4.1 业务迁移失败与代码回退

应用支持库在 data/.migration.json 记录 migrationId、from/toSchema、from/toAppVersion、state、backupId、hasCommittedNewWrites；状态为 prepared→migrating→verified→committed，失败为 restored/failed。字段不含业务内容或 Secret，原子写入。业务 DB 的 schema version 是实际数据版本权威，journal 负责记录跨步骤恢复；二者不一致时先恢复/修复，禁止 ready。

第一次迁移在接收任何用户写入前完成；备份在 backups/<migrationId>/，使用一致性快照。升级成功后最多保留两份已验证迁移备份；失败中/恢复未完成的备份禁止自动清除，也不再发起另一迁移堆积备份，要求先修复。备份和其他个人数据一样默认保留（模块代码随整包交付，无模块卸载）。

App Host 必须实现固定只读模式 --inspect-data：Core 在持 install→runtime 锁时执行，限时五秒、输出不超过 64 KiB；该模式不再获取 runtime 锁、不迁移、不联网、不访问 Keychain，只返回实际 schema、journal 状态、最后写入版本、已提交新写入标记和可验证的兼容性。禁止页面提供可执行路径或任意参数。

运行中失败通过 app:data_status/app:failed 返回同样的有界状态，供壳展示；Core 回退不能只信任页面转述，必须自行执行已安装且验签版本的 --inspect-data 核验。

新增 apps:rollback 为显式管理操作：仅当应用已停止、业务库与上版的读写 schema 范围相容，且 migration journal 一致时，按安装 journal 原子恢复上版 manifest/activation/收据。已经接受新写入但 schema 仍相容时只回退代码，不恢复备份。schema 不兼容、inspect 不可用或状态不确定时，标记 repair_required 并拒绝回退；不删除新版数据。

迁移失败且新写入尚未开放时，由候选应用恢复其备份，报告 APP_MIGRATION_FAILED 并退出；Core 保持候选版本为需要修复，用户可以重试修复或执行通过上述核验的“回退上一版本”。不能只恢复 DB 后仍把安装卡片显示正常。崩溃发生在迁移/恢复中则先以当前版本重开执行 journal 恢复，成功前禁用业务写入和不安全代码回退。

### 4.2 本地开发、候选与正式分发

本地例外仅解决 Developer ID/公证凭据缺失，不保证系统一定放行；真实 Gatekeeper/Native Host 失败仍是失败。不得自动删除 quarantine、关闭系统校验或脚本代点“仍要打开”。

| 项目 | local-development | production |
|---|---|---|
| 构建 | 专用非生产构建身份 + 显式开发入口；可为性能测试使用优化构建 | 正式构建，不能运行时切成开发策略 |
| macOS 身份 | 本机 ad-hoc/开发签名；不要求 Developer ID/公证 | 固定 Developer ID/Team ID、适用公证及真实启动证据 |
| 组合清单 / bundle | 开发信任根验签 + 精确本机构建摘要核验 | 仅正式信任根；拒绝开发根、fixture 和开发载荷 |
| 校验 | wire/payload/hash/平台/架构/路径/权限/协议全部保留 | 同左，加正式平台身份 |
| 注册与数据 | 独立命名空间，不触碰日常环境 | 固定正式身份和既有数据 |
| 完成含义 | A-Local/B-Local，仅限记录的机器/浏览器/构建 | 原完整 A/B 门禁及所有声明平台 Release Gate |

本地候选数据根为 `~/.natives-local/`，系统源见 §3.2。现有 `apps:dev` 可继续使用 `dist/apps-dev/state/` 等专用临时根，但必须由受控开发入口固定，不能将页面/普通环境变量自报路径当授权，也不能指向正式根。开发源可位于受控构建目录，验其所有权、路径边界和本次实际构建摘要；任意下载文件不能仅改成 fixture 就被接受。

共享身份函数区分分发模式：正式 runtimeHost 保持 `com.natives.app.a<sha256(appId)>`，开发为 `com.natives.local.app.a<sha256(appId)>`；主 file/model Host、扩展身份也使用相互不遮蔽的开发身份。开发 Keychain namespace 为 `com.natives.local.app.<appId>`，正式沿用 `com.natives.app.<appId>`，由支持库与 Core 共用命名和精确清理 fixture；Model Host 的本地测试凭据同样隔离。不得自动导入或删除正式数据/凭据。

必须有反向测试：正式构建拒绝开发 Catalog/清单/载荷、仅传开发环境变量仍拒绝、源被替换/hash 不符拒绝、开发注册不会覆盖正式注册、开发清理不影响正式数据和 Keychain。非生产构建策略不得进入正式二进制；可用同一代码中的受控构建配置实现，不新建平行运行时。

A-Local 用真实内置基金验证完整产品组合的所选本机平台工程链路（2026-09-12 收敛：取消"先独立样例再进入 B"的前置条件；已有独立样例仅作为覆盖共享底层规则的低层测试 fixture）。B-Local 包含真实基金与完整产品组合的端到端业务。Developer ID、公证、生产浏览器身份/审核及其他平台 pending 单列为 Release Gate，不伪造 passed，也不阻止无依赖的本地工作。完整门禁矩阵与证据仅维护于 [实施方案](../development/app-center-fund-implementation-plan.md)。

## 5. 运行协议

### 5.1 启动顺序

1. app.html 从 URL 只读取 appId。
2. 短连 Core apps:get，核验已安装/启用/兼容性，返回appId、activeVersion、runtimeHost、activationGeneration、enabled及安装状态；随后关闭 Core Port。`activationGeneration` 是**单调递增的无符号整数（u64，从 1 起，Core 在每次启用/停用/升级/回滚/恢复投影变更时 +1）**，来源为激活投影 `activation.json` 的 `generation` 字段；它与 App Store 管理用的安装 `revision`（另一独立计数器）是两个不同概念，**禁止混用或互相替代**——页面/壳不得把 revision 当作 activationGeneration，Host 启动校验只使用投影 generation。
3. 同 profile 查找已有同 appId 页面并激活。手工复制页面或同时打开时仍由 OS runtime 锁防止第二个业务实例。
4. app.html 使用 runtime.connectNative(runtimeHost)，Chrome 启动应用。
5. 应用支持库从 Chrome 启动实参核验 origin，并与 Core 安装的受限注册信息匹配；页面参数不能覆盖。
6. app:handshake 核对 appId、安装版本和 App protocol；不匹配立即断开。
7. app:start 获取本应用 runtime 锁，再尝试取得四个全局运行槽之一；没有空槽则释放锁并返回 APP_RUNTIME_LIMIT。取得后验证当前安装收据仍启用且版本一致，再执行迁移、绑定 127.0.0.1:0 并返回 ready。两把锁均持有到退出，崩溃由 OS 释放。
8. 壳校验返回的数值端口、固定本地地址和会话 generation，构造 UI URL，创建 sandbox；不直接导航到应用任意返回的 URL。

appHost 只写业务库；对 Core 安装收据只读。取得 runtime 锁后再校验启用状态，防止“先读 enabled、随后被停用”的竞态。

### 5.2 最小方法与事件

| 消息 | 含义 |
|---|---|
| app:handshake | appId/version/protocol/data-schema；不启动业务或网络 |
| app:start | 启动唯一实例，返回 port、generation 和真实状态；幂等 |
| app:status | 手动重连/核验当前实例；不触发健康轮询 |
| app:data_status | 实际数据 schema、迁移 journal 和代码回退兼容性；不返回业务记录 |
| app:stop | 停止接收操作、取消可取消任务、结束有界事务、关闭监听并退出 |
| app:session | 为当前 iframe generation 签发/撤销短期会话能力 |
| app:state 事件 | ready / busy / idle / failed，含可取消性和真实工作标识 |

Native 单帧严格小于 1 MiB，id/requestId 和错误结构沿用既有风格。消息超长、未知方法、错版本、超时均返回明确错误，不启动 fallback。

业务 HTTP 路由属于应用自身；不经 Core 转发，不存在 Core app:call(method, params) 这种任意业务代理。

#### App v1 的 wire 字段

所有帧 UTF-8 JSON ≤512 KiB，外层沿用 Native Messaging 的四字节长度前缀。请求为 {id, method, params}；成功为 {id, ok:true, result}；失败为 {id, ok:false, error:{code, message, retryable}}。id 为1～64字节字符串；未知字段拒绝，业务错误不输出堆栈/路径/Secret。事件为 {event, instanceId, sequence, data}，sequence 单调递增，不能作为持久化业务事件平台。

| 方法 | params | result |
|---|---|---|
| app:handshake | protocolVersion:1、expectedAppId、expectedVersion | protocolVersion:1、appId、appVersion、dataSchemaRange、state:"stopped"；身份不符即失败 |
| app:start | requestId、expectedActivationGeneration | instanceId、port（1～65535）、generation、state:"ready"；迁移未完成不返回ready |
| app:status | instanceId | instanceId、state（ready/busy/idle/stopping/failed）、operation或null |
| app:data_status | instanceId | currentSchema、migrationState、lastDataWriterVersion、hasCommittedNewWrites、previousVersionCompatible |
| app:session | instanceId、op（rotate/issue/revoke）、generation（rotate时为旧值）、challenge（issue必填） | rotate返回newGeneration；issue返回generation、challenge、token、expiresAt；revoke返回revoked:true |
| app:stop | instanceId、reason（user/hidden/page_closing/maintenance）、requestId | stopped:true；仅在监听关闭、停止接收请求、事务收尾、Token撤销后回复 |

instanceId、generation、challenge 至少128位随机值（base64url）；会话 token 为32字节 CSPRNG、base64url编码，在对应 instance/generation 内有效，最长15分钟，Host以单调时钟计时。过期后业务请求返回APP_SESSION_INVALID；UI在新的实际请求/用户动作时重新握手，不设置后台刷新定时器。stop/revoke/load/navigation 立即撤销，无论TTL是否到期。

app:state 的 data={state, operation}；operation={id, kind, cancellable, startedAt, deadlineAt}或null，均为真实执行状态，deadlineAt必须有界。应用程序不通过任意字符串请求壳执行系统方法。重复 start 对同一连接/requestId返回同一实例，payload变化为conflict；新连接不能接管已有实例。

收到 stop 回复后壳断 Port。连接中断是UI能直接观察的退出信号；Core在后续维护操作中能取得runtime锁，才可继续安装/清理。测试必须另外证明PID退出、端口不可连和锁释放，不能把一个stopped:true当资源回收。stdin EOF、OS终止与正常stop共用取消/关闭路径；两秒预算从收到stop或EOF开始，支持库不得依赖浏览器肯定会替它清理。

### 5.3 通用支持库

A2 创建一个小型编译期 app-host-support Module，由标准样例和基金共用：Native framing、origin 校验、runtime 锁、loopback 服务、会话鉴权、请求限额、EOF 退出。业务只提供静态资源、业务路由和有界关闭/数据迁移回调。

它不是独立常驻进程，不动态加载代码，不提供数据库模型、SourceRegistry、业务调度或自动重启。发布固定版本/commit，基金通过锁定版本依赖，不用开发机绝对 path 依赖。协议 schema/fixture 从这里导出，避免两份定义。

优先抽取现有 Native framing/path/Keychain 共用资产；新 HTTP 解析、密码学不得手写。需要的维护中库只进入 app-host-support 或对应应用；Core 新增签名验证依赖须通过原体积门禁。

## 6. 生命周期与多个页面

| 场景 | 确定行为 |
|---|---|
| 安装后未打开 | 不连接 App Host，不预热、不轮询、不运行 |
| 打开已有应用 | 激活当前 profile 已有 Surface，不增加业务实例 |
| 隐藏且无任务、无未保存输入 | 60 秒后销毁 iframe，断开 App Port；保留轻量 stopped 壳和重开按钮 |
| 隐藏但操作未完成 | 不因 idle 计时器强杀；请求可取消任务停止或等待有界操作完成；完成后开始回收计时 |
| 未保存编辑 | 应用持续保存本地草稿；收到持久化确认前不得把草稿状态报告为 clean |
| 用户按停止 | 先说明未完成操作，取消/安全收尾；确认退出后显示 stopped |
| pagehide/关闭/浏览器退出 | 不依赖异步 unload 存盘；Port 断开触发 EOF，应用在两秒内关闭监听、取消工作并退出 |
| 恢复打开 | 启动新进程，读取已保存记录和草稿；不承诺恢复任意内存计算 |
| Host 崩溃 | 显示 failed、可重试；不自动无限重启 |
| 同 appId 锁被占用且当前 profile 无可控制 owner | 第二实例退出并显示“应用正在另一会话中使用”；不推断具体profile，当前页不能伪造停止成功 |
| OS 用户命名空间最多四个活动应用实例 | 由运行槽文件锁保证，并发启动不能穿透上限；不自动终止 busy 应用，提示用户先停止其他应用 |

应用 UI 不作为唯一 busy 权威；业务 Host 才决定操作是否结束。普通外部网络请求最长30秒、首版单个前台长操作最长五分钟，均可取消；DB事务必须可结束/中断以满足两秒退出。超过期限报告失败并回收，不延长成后台常驻。没有确定性 shutdown 的应用不能发布。

apps:get与实际start之间发生安装版本变化时，App Host核验expectedActivationGeneration失败返回APP_INSTALLATION_CHANGED。壳只重新apps:get核验并重试一次，仍变化则提示重试，不能无限循环或按过期收据启动。

中心停止通过 chrome.runtime.sendMessage 通知匹配 appId/owner 的 app.html；使用 app:stop 并断 Port，等待退出证据。不得用 tabs.sendMessage 向扩展页传消息（该方法用于 content scripts）；不得让 Service Worker 持有运行状态或连接。

更新、停用、卸载前请求 owner 停止；真正的互斥仍由 Host 文件锁保证。其他 profile 锁未释放时返回失败，UI提示在对应浏览器会话停止。

## 7. UI 隔离与本地鉴权

- app.html 只执行扩展内通用壳代码。iframe sandbox 精确为 allow-scripts allow-forms；禁止 same-origin/top-navigation/popups/downloads。
- 扩展 CSP 限制 script-src 为自身，frame-src 仅允许固定的 127.0.0.1 来源模式；不得开放任意远程域或动态 import 下载代码。
- 应用 UI 只加载本应用本地资源。优先把源 ES Modules 构建成一个静态脚本包，避免 opaque origin 下模块资源 CORS 差异；由 A1 真实浏览器验证。
- 应用 HTTP 只绑定 127.0.0.1 的系统分配端口，不监听 LAN。Host header 必须与绑定地址/端口一致；拒绝 DNS rebinding、代理头伪装和任意重定向。
- UI 的业务请求采用 Authorization bearer，能力至少 128 bit CSPRNG，绑定 appId/instance/generation；不得出现在 query、URL fragment、日志、持久 storage 或磁盘。敏感业务数据不能由无鉴权 GET 返回。
- 初次iframe load完成后，壳经Native app:session rotate取得generation，向保存的contentWindow发送init(generation,challenge)；iframe返回hello并回显二者。壳核验source、当前generation、challenge，再经Native app:session issue取得token，只向该contentWindow发送welcome(token,expiresAt)。之后UI才可发送带Authorization的HTTP请求。初始化页面本身不能包含敏感业务数据。
- opaque窗口的targetOrigin只能使用"*"时，仅这条已核验contentWindow的init/welcome通道允许；禁止向任意窗口广播，也不能把origin===null当身份。每条后续消息同时核验source/generation/token。壳从来不把Native Port本身传入iframe。
- 非预期的第二次load/导航立即撤销旧generation并移除iframe，显示已停止/重开；不自动给导航后的文档重新发Token。用户通过壳重开时创建全新iframe、固定初始URL和新的握手。不要复用旧window引用悄悄恢复权限。
- 服务的 CORS 对 sandbox 场景仅允许 Origin: null + 指定 method/header + 有效 bearer；不使用 cookie，不使用 credentials，不把 Origin:null 当鉴权。其他端口应用、普通网站、无 Token、错 Token 和旧 Token 请求全部失败。
- CSP 禁止外联 script/connect/form/base/frame；业务外部 HTTP 由原生应用按固定数据源发起。主题、语言及少量 UI 状态消息使用有界、类型校验的壳消息，不提供通用 Native/文件/SQL/Shell/Secret 桥。
- UI 代码原文/包截图可作为上架审查资料。独立原生程序和可执行 UI 必须来自可审查的官方发布；不能把隔离上下文例外理解成任意代码市场的许可。

## 8. 数据与权限

Core App Store 和应用业务库分别迁移、分别写入。应用不能连接 natives.db 写业务表；Core 不创建 portfolio/transactions/nav 等表。

业务凭据只在应用 Host 内使用并保存在 OS Keychain；扩展壳、iframe、数据包和日志不接触凭据明文。Keychain 锁定/拒绝必须可恢复，不能回退明文文件。

凭据命名固定由app-host-support实现：namespace沿用现有AppMeta::namespace规则，fund为com.natives.app.fund；macOS Keychain service精确等于namespace，account为应用内不透明credentialId；Windows Credential Manager target为namespace+":"+credentialId；Linux Secret Service item属性含service=namespace、account=credentialId。Core清理仅匹配精确service/属性或带冒号分隔的完整target前缀，不用模糊子串。应用不得另建不遵守该命名的Secret存储；支持库和Core删除器共享命名fixture，证明不会删除其他App或Model Host凭据。

首版权限分为：
- 由平台真正控制：是否安装、是否允许运行、是否登记 Native Host、是否清理数据；
- 由官方应用实现并接受测试审计：本应用数据保存、指定数据源联网、Keychain 使用；
- 当前不承诺：恶意原生程序的系统级文件/网络沙箱、任意 Core 文件访问、跨应用数据调用。

应用升级增加权限时，安装前显示变化并取得明确授权。未知权限拒绝安装；不画无法落实的逐项权限开关。

## 9. 错误与验收

至少支持：APP_UNSUPPORTED_PLATFORM、APP_INCOMPATIBLE、APP_SIGNATURE_INVALID、APP_PACKAGE_INVALID、APP_INSTALL_FAILED、APP_INSTALLATION_CHANGED、APP_CLEANUP_PENDING、APP_BUSY、APP_ALREADY_RUNNING、APP_RUNNING_ELSEWHERE、APP_RUNTIME_LIMIT、APP_START_FAILED、APP_PROTOCOL_MISMATCH、APP_SESSION_INVALID、APP_MIGRATION_FAILED、APP_DATA_SCHEMA_INCOMPATIBLE、APP_KEYCHAIN_LOCKED。APP_RUNNING_ELSEWHERE仅表示当前profile无法控制的另一会话，不暴露无法核验的具体profile身份。现有错误可兼容映射，不强制换名重写无关文件。

每个应用必须通过同一组黑盒契约检查：签名安装、激活投影的每个崩溃切点、离线重开、错 origin/协议、并发打开、非法本地请求、停止/EOF/崩溃、隐藏回收、未保存草稿、更新失败恢复、数据迁移失败与显式代码回退、整包更新与修复保留、确认清除、资源增长。结果必须区分 mock、真实 Native Host、真实浏览器、平台签名和线上发布状态。
