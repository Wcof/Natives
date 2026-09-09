# ADR-0027：官方托管应用独立交付与页面拥有生命周期

- 状态：accepted-target（A0 规范同步提交合入执行分支即生效；生效点与例外范围见 §5。accepted-target 表示目标架构与预算已批准并可据此实施 A1—A5，不代表生产迁移完成——当前生产仍是 ADR-0026 路线，直至 A3—A5 实际替换）。
- 日期：2026-09-09。
- 最近修订：2026-09-10。
- 产品依据：用户确认轻量 Core、应用独立扩展、按需运行与回收；先改造应用中心，再适配基金应用；允许修订冲突的 ADR-0026。
- 技术选择：本 ADR 是本轮提出的落地方案，不把技术细节写成用户已经逐项确认的事实。
- 执行入口：[两阶段实施方案](../development/app-center-fund-implementation-plan.md)。
- 接口唯一来源：[托管应用契约 v1](../contracts/managed-app-contract.md)。

## 1. 决策

Natives 的应用中心与通用 app.html 保留在 Chrome/Chromium 扩展中。官方应用独立发布一个原生可执行程序，程序内包含其业务代码和构建后的 UI 静态资源。应用中心安装、校验、更新该程序，并为其登记受限的 Native Messaging Host。

app.html 先短连接 Core 核验安装、启用、版本与已登记 Host，然后直接连接该应用 Host。Chrome 启动应用进程。应用进程在 127.0.0.1 动态端口提供自身 UI 和自身业务接口，app.html 用受限 sandbox iframe 呈现 UI。业务代码不进入扩展执行上下文。

安装不会启动业务。有效协议范围内的新应用、UI 更新和业务更新不要求重新构建或发布扩展/Core；应用中心只更新签名 Catalog 和安装记录。新增 Core 公共能力仍需要 Core 版本升级。

首版仅交付官方托管应用。第三方 Web URL 是后续接入类型，本次不实现第三方代码下载、任意 URL 嵌入或第三方原生程序安装。

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

沿用 gzip 单载荷 .nap、签名 Catalog、双重 SHA-256、安装事务和数据保留机制；新增 managed_local 类型。每个目标平台的应用包只有一个原生可执行载荷，UI 嵌入该可执行程序，不增加 ZIP/TAR 解包器或在安装时运行包内脚本。

Catalog v3、Core Apps protocol v4、独立 App protocol v1 分别管理目录、安装管理和运行兼容性。协议变更与应用业务版本分离。Native Host 名称、注册路径和安装路径由 Core 计算，不接受目录或页面提供的任意命令、绝对路径或 Host 名称。

升级仅在应用停止后进行。安装程序不接触业务库；候选程序的健康检查不执行生产数据迁移。首次启动业务版本前由应用自己备份并迁移业务库；失败恢复备份。已经接受新写入后不得静默恢复旧备份。代码回退必须同时满足数据 schema 的向后兼容性。

## 4. 性能预算草案

A0 同步 Standards 后生效，不能先更改检查脚本绕过旧门禁：

| 对象 | 预算 |
|---|---|
| Core 扩展估算包 | 保持现行 300 KiB hard gate；不包含业务 UI |
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

- ADR-0026：取代“共享 file Host 承载应用业务、UI/逻辑内置、纯资源包、禁止独立应用 Host”全部目标决策；保留其供应链、原子安装、数据保留和 Secret 原则。标注 superseded by ADR-0027，保留历史原因，不篡改当时结论。
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

## 7. 验证依据

- [Chrome Native Messaging](https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging)：Native Host 按 connectNative 连接启动；不能把共用二进制名称视为跨页面共享一个进程。
- [Chrome Web Store MV3 requirements](https://developer.chrome.com/docs/webstore/program-policies/mv3-requirements/)：隔离上下文的远程代码例外及可审查性要求。
- A1 必须用真实 Chrome/Chromium 验证 Native Messaging、opaque iframe origin、CSP/CORS、资源加载与退出。失败必须修正本 ADR 和契约，不得关闭 sandbox/CSP 或暗改为远程执行。
