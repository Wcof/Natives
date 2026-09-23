# ADR-0027：官方托管扩展包独立交付与页面拥有生命周期

- 状态：accepted-target（A0 规范同步提交合入执行分支即生效；生效点与例外范围见 §5。accepted-target 表示目标架构与预算已批准并可据此实施 A1—A5，不代表生产迁移完成——当前生产仍是 ADR-0026 路线，直至 A3—A5 实际替换）。
- 日期：2026-09-09。
- 2026-09-11 补充：[ADR-0029](0029-unified-suite-preinstalled-apps.md) 决定统一套件预装、受限系统安装源与本地/发布分级验收，取代本文冲突的逐包获取含义和无条件私有根表述；独立运行/更新、Core 单一安装权威及其余安全规则不变。本文保留原决策背景，具体新增规则以 0029 与当前 Standards/契约为准。
- 2026-09-11 架构整改明确（Managed Multiple Hosts）：重新定义“独立交付”（Independent Delivery）为五维独立：独立源码（Independent Source）、独立构建（Independent Build）、独立版本（Independent Version）、独立更新（Independent Update）、独立回滚（Independent Rollback）。同时明确：**Independent Delivery != Independent User Installation**。官方托管子应用的首发版本（Initial Release）随 Natives Suite Seed 统一交付，无需用户进行远端二次下载；受控独立 Native Host（如 fund-host）保留作为托管子应用运行载荷，禁止将业务编译进 Core（禁止 Fund builtin 化），Core 统一管控其生命周期。
- 2026-09-12 路线收敛（用户最终决定，取代本文冲突的独立交付语义）：Natives 是唯一产品，基金等均为**内置模块**，全部随 Natives 完整安装包一起构建、安装、更新和修复。**取消模块独立下载、独立安装、独立升级、独立回滚、独立 Release、在线 Catalog 驱动新 appId 与 Fund 独立 nap 发布**；应用中心不再有"可添加/应用商店"语义。新增模块只能通过新的完整 Natives 版本交付。本文其余安全、数据、生命周期与沙箱约束继续有效；凡与本次收敛冲突的"独立发布/独立更新/不重发 Core/不重发扩展"表述一律以本注记为准，配套 MUST 同步见 [实施方案 §5 P0](../development/app-center-fund-implementation-plan.md)。
- 最近修订：2026-09-14。
- **2026-09-14 统一内置应用 Runtime 架构取代（ADR-0031）**：[ADR-0031](0031-unified-builtin-app-runtime-and-monorepo-modules.md) 已正式取代本文关于“每模块一个 App Host executable（如 fund-host）”、“每模块 Native Messaging Host（如 com.natives.app.a<hash>）”、“runtime payload 可执行文件落地”以及“独立 executable 激活”的决策。整个 Natives 收敛为唯一产品级 `natives-app-runtime`，由单一 Native Host `com.natives.app_runtime` 登记；各内置应用编译进该 Runtime，但运行时仍通过 Chrome Native Messaging 机制按需拉起独立的 Runtime 进程实例（Single Runtime Binary / Multi Process Instance），生命周期以进程为隔离边界。本文关于 `app.html` owner、sandbox iframe、127.0.0.1 动态 loopback、Bearer Token、数据物理隔离、按需启动与 stdin EOF 2秒彻底回收的约束继续保留有效。
- 产品依据：用户确认轻量 Core、应用独立扩展、按需运行与回收；先改造应用中心，再适配基金扩展包；明确 Natives 是唯一产品，托管扩展包是 Natives 的组成部分；允许修订冲突的 ADR-0026。
- 技术选择：本 ADR 是本轮提出的落地方案，不把技术细节写成用户已经逐项确认的事实。
- 执行入口：[两阶段实施方案](../development/app-center-fund-implementation-plan.md)。
- 接口唯一来源：[托管应用契约 v1](../contracts/managed-app-contract.md)。

## 1. 决策

Natives 是唯一的用户产品。托管扩展包（如基金扩展包）是 Natives 应用中心管理的可选扩展能力，属于 Natives 的组成部分，不是独立桌面产品；扩展包内部的持仓、账本、净值、导入和数据迁移等是包的内部模块，不应分别变成可安装应用或拥有独立产品身份。

官方托管扩展包的源码可以按清晰领域边界独立构建与维护，但**不再独立发布**：基金等内置模块代码随 Natives 完整安装包一起构建、安装、更新和修复，用户版本、安装包和 Release 与 Natives 联动。应用中心只保留内置功能打开、导航显示/隐藏、偏好设置与数据管理；不存在模块安装/下载/升级/卸载语义，也不再更新签名 Catalog 来交付新扩展包。新增 Core 公共能力仍需要 Core 版本升级。

硬约束：
1. 安装目录只属于 Natives 私有数据根（`~/.natives/apps/<appId>/`）；严禁写入 `/Applications`、独立 `.app` bundle、独立 Dock 图标或 LaunchServices 系统产品注册。
2. 若要做到“下载后在扩展上下文执行新的 JS”，必须先另立安全 ADR；本项目当前不允许以此绕过浏览器远程代码限制。
3. 应用中心只做内置功能入口与偏好管理（打开/显示隐藏/设置/数据管理），不包含任何业务判断、应用商店语义或特定扩展包专用代码。
4. 2026-09-12 收敛：不保留任何“模块独立下载/安装/更新/卸载”的生产入口或配置开关；旧动态注册不得作为静默 fallback 继续运行。

### 运行实现机制

运行时是否使用本机进程属于实现细节，不出现在产品名称、导航、安装文案或数据模型的产品层：
- Natives 的应用中心与通用 `app.html` 保留在 Chrome/Chromium 扩展中。
- 官方托管扩展包的运行载荷由受控本机实现承载，载荷内包含其业务代码和构建后的 UI 静态资源。模块文件随完整产品安装/更新交付，由产品安装链路验签、健康检查并登记受限的 Native Messaging Host；首用只核验当前产品版本并初始化业务数据，无代码下载。
- `app.html` 作为每个扩展包的通用呈现会话（Surface），先短连接 Core 核验安装、启用、版本与已登记 Host，然后直接连接该扩展包 Host。Chrome 按需启动应用进程。
- 应用进程在 127.0.0.1 动态端口提供自身 UI 和自身业务接口，`app.html` 用受限 sandbox iframe 呈现 UI。业务代码不进入扩展执行上下文。
- 首版仅交付官方托管扩展包。第三方 Web URL 是后续接入类型，本次不实现第三方代码下载、任意 URL 嵌入或第三方原生程序安装。

## 2. 职责与安全

- Core 的 App Store 仅拥有应用登记、安装事务、包收据、启用/导航设置和 Native Host 注册；不写基金业务库，不代理任意业务指令。
- 每个 app.html 是其应用 Native Port 的唯一 owner；应用中心、Service Worker 和其他页面不保活该应用。
- 应用 Host 拥有业务计算、本应用数据和本应用 Keychain 凭据；首版单进程，无开机启动、无子进程、无后台任务调度。
- 每个 appId 在一个 OS 用户的数据命名空间内至多一个运行实例；OS 文件锁跨 Chrome profile 和浏览器兜底。其他 profile 不能停止 owner 时明确报告正在别处运行。
- 安全沙箱仅隔离浏览器应用 UI。签名原生程序仍以用户权限执行，本方案不宣称对恶意原生程序具有 OS 沙箱或可强制的全局网络/文件权限隔离。因此仅接受受信官方发布。
- iframe 为 allow-scripts allow-forms，不添加 allow-same-origin/top-navigation/popups；不交付 Native Port、Core 通用文件/SQL/Shell/Secret 接口。
- 应用本地接口鉴权、CSP、Origin/Host 校验、会话撤销与限额见契约。持久 Secret 仍只进 OS Keychain。
- 关闭或停止回收应用；隐藏且无业务操作 60 秒后回收。未保存编辑与后台操作必须按契约处理，不能仅靠 beforeunload 保存。

## 3. 交付与更新

沿用 gzip 单载荷 .nap、签名 Catalog、双重 SHA-256、安装事务和数据保留机制；新增 managed_local 类型。每个目标平台的应用包只有一个原生可执行载荷，UI 嵌入该可执行程序，不增加 ZIP/TAR 解包器或在安装时运行包内脚本。此为 legacy executable 路线描述；单 Runtime 整改完成后，普通 Managed App Package（.nap）为资源包，不含 Mach-O executable、*.dylib、*.app、app-exec 或 fund-host。

2026-09-12 收敛：上述验证与事务能力**只在完整产品安装/更新层使用**，用于核验安装包内固定模块文件；不再作为运行时 Catalog 分发链路。不存在独立的应用包 Release 或在线 Catalog 更新。

Catalog v3、Core Apps protocol v4、独立 App protocol v1 分别管理目录、安装管理和运行兼容性。协议变更与应用业务版本分离。Native Host 名称、注册路径和安装路径由 Core 计算，不接受目录或页面提供的任意命令、绝对路径或 Host 名称。运行时不再出现"更新签名 Catalog 交付新扩展包"语义。

升级仅在应用停止后进行。安装程序不接触业务库；候选程序的健康检查不执行生产数据迁移。首次启动业务版本前由应用自己备份并迁移业务库；失败恢复备份。已经接受新写入后不得静默恢复旧备份。代码回退必须同时满足数据 schema 的向后兼容性。

## 4. 性能预算草案

A0 同步 Standards 后生效，不能先更改检查脚本绕过旧门禁：

| 对象 | 预算 |
|---|---|
| Core 扩展估算包 | 保持现行 360 KiB hard gate（2026-09-13 用户决定自 300 KiB 上调 20%，ADR-0024 修订）；不包含业务 UI |
| native-file-host | 保持现行过渡 4 MiB hard gate、12 MB 空闲 RSS、0.5% 空闲 CPU |
| 单个平台应用下载包 | wire ≤ 32 MiB；payload ≤ 128 MiB；精确长度和 hash 校验 |
| 应用代码占用 | 活跃版本 + 上一版本；有界 staging，峰值至多三份载荷；个人数据另计 |
| 未启动或已停止应用 | 应用进程、监听端口、业务定时器为 0；不承诺 Chrome 壳页面 RSS 为 0 |
| EOF/停止至应用退出 | ≤ 2 秒，含取消、连接关闭和锁释放；不存在未落盘成功操作 |
| 标准样例启动 ready | 同机 Release 5 次 p75 ≤ 2.5 秒 |
| 并发 | 每 appId 一个运行实例；每 OS 用户的数据命名空间最多四个活动应用实例，由共享运行槽文件锁保证；到上限明确提示 |
| 请求 | Native 单帧 < 1 MiB；HTTP 请求/响应默认 ≤ 1 MiB，需大批量时分页/分块 |

32/128 MiB 是本方案对原生载荷的新增目标上限，不是实测值；修改原因是交付物从数据变为可执行应用。样例与基金分别提交实际尺寸、RSS、CPU 和打开/关闭循环证据，禁止用新上限掩盖 Core 回归。

## 5. 取代关系

A0 必须在同一个文档提交内同步下列内容，之后才修改生产路径：

- ADR-0026：取代“共享 file Host 承载应用业务、UI/逻辑内置、纯资源包、禁止独立应用 Host”全部目标决策；保留其供应链、原子安装、数据保留和 Secret 原则。旧 ADR-0026 只保留历史原因，不恢复其旧生产链路；旧“Extension App UI 必须 build-time 进入扩展包”的 MUST 规则不能与新的独立发布目标并存，在此明确被 ADR-0027 取代。标注 superseded by ADR-0027，保留历史原因，不篡改当时结论。
- ADR-0025：取代构建期 UI 白名单、旧 Catalog/协议、旧包预算与旧运行呈现方式；可复用独立 Host 注册与事务思路，不恢复旧代码整包。
- ADR-0020 §4/§5 与生产范围：添加“官方托管应用”这个有界例外；继续禁止 Agent/Harness/Planner/Jobs、通用 Plugin Runtime 和旧 Tauri 产品路径。
- ADR-0023：Files 保持原约束。扩展页面仍是管理壳；仅 app.html 内的官方应用 sandbox 与应用自有 loopback 被本 ADR 授权，不给 Files/Core 增加通用 HTTP 服务。
- ADR-0022 与旧 Apps Web Surface contract：Tauri BrowserStateHandle/child WebView 的 Apps 部分标为历史；Workspace/Appearance 部分不变。
- product/01、product/02；technical/01/02/03/04/05/06；AGENTS.md、docs/README.md 和引用基金项目的说明按实施方案矩阵同步。

本草案本身不静默覆盖仍生效的 Standards。执行代理从 A0 开始完成取代关系与规范同步；不得只改 ADR 标题、漏改 MUST 或把历史代码重新置为 production。

生效点为 A0 文档提交合入执行分支：同一提交把本 ADR 状态改为 accepted-target、契约改为 active-target，并同步 Standards/AGENTS 的限定例外。此前现行 MUST 仍约束生产；A1 也必须在该例外生效后才创建新架构测试夹具。该提交不代表生产迁移完成。

## 6. 为什么选择这条路线

- 内置 UI + 外部资源不能兑现独立发布 UI 的目标，因此弃用。
- 通用 app.html 保持页面直属 Port；普通外部浏览器 Tab 不能直接拥有 Native Port，会额外引入生命周期协调，本期不选。
- 每个应用独立 Native Host 直接复用 Chrome 的进程启动/连接机制，避免在 Core 内再建设一层应用子进程守护与跨进程路由。
- UI 使用隔离上下文，不在扩展页动态执行下载代码。Chrome Web Store 对隔离上下文有政策例外；是否通过真实审核仍属于发布门禁，不能凭本 ADR 宣称已获准。
- 应用 Host 的通用协议、鉴权、限额、退出和锁逻辑作为小型编译期支持库，由标准样例与基金共用；不提供动态插件加载、任意方法代理或通用业务框架。
- 承认独立 Native Host 进程路线的 macOS 信任成本：未签名或按包分发的原生载荷会被 syspolicyd/Gatekeeper 评估，并可能被 SIGKILL 终止；这一信任与运维成本是转向单 Runtime builtin 模式的直接动因之一。

## 7. 验证依据

- [Chrome Native Messaging](https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging)：Native Host 按 connectNative 连接启动；不能把共用二进制名称视为跨页面共享一个进程。
- [Chrome Web Store MV3 requirements](https://developer.chrome.com/docs/webstore/program-policies/mv3-requirements/)：隔离上下文的远程代码例外及可审查性要求。
- A1 必须用真实 Chrome/Chromium 验证 Native Messaging、opaque iframe origin、CSP/CORS、资源加载与退出。失败必须修正本 ADR 和契约，不得关闭 sandbox/CSP 或暗改为远程执行。
