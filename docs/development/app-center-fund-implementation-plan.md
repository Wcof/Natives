# 应用中心与基金应用：两阶段可执行改造方案

> 日期：2026-09-09。
> 最近修订：2026-09-10。
> 状态：ready-for-execution；已选定默认技术路线，执行从 A0 开始，尚未实施或通过新架构验收。
> 交付用途：其他工程代理按工作包实施。先完成应用中心验收，再进行基金应用接入与业务交付。
> 当前核验基线：Natives commit db0e03df09ed84e2b0c98182d2c15e02a3c6b5cf；仅源码和文档核验，未运行新架构测试。
> 目标决策：[ADR-0027 草案](../adr/0027-managed-apps-independent-delivery.md)。
> 唯一接入规范：[官方托管应用契约 v1](../contracts/managed-app-contract.md)。
> 用户已授权讨论中冲突的 ADR-0026 可以改；实施必须同步下述 ADR/Standards，不能只删检查绕过规范。

## 1. 本轮必须交付的效果

1. Core 继续作为轻量个人 AI 工作台；应用中心管理应用，不包含基金计算、数据源或业务界面。
2. 官方应用的 UI、业务程序和资源独立打包、独立发布；协议兼容时，新增/升级应用不重新发布 Natives 扩展和 Core Host。
3. 应用安装后不自动运行；打开才启动；停止、关闭、闲置可回收，数据按明确规则保留。
4. 应用中心支持真实的安装、打开、停止、更新、停用、卸载、修复和数据清理；状态与错误有来源。
5. 基金应用使用与标准样例相同的接入机制；禁止基金专用安装器、Host 管理分支或扩展内置业务页面。
6. 本次只实施官方托管应用。第三方 Web URL 的类型语义保留在契约中，不实现 URL 添加界面、任意网站嵌入、第三方原生市场。
7. ADR、Standards、协议契约、现状文档和检查脚本相互一致；不恢复旧 Tauri/Agent/Jobs/通用 Plugin Runtime。

决定“可以独立发布”不能只看文件是否分目录。最终必须证明：锁定同一份 Core/扩展二进制，发布一个新应用或基金新版后，用户可以发现、安装并使用新 UI/新业务。

## 2. 已核验的资产与实际缺口

| 资产 | 当前事实 | 本次处置 |
|---|---|---|
| extension/apps.js、apps.html、apps.css | 已有 Catalog 列表、安装/更新/启用/卸载/清数据与错误界面 | 复用，扩展详情和真实运行操作 |
| extension/app.js、native-app-client.js、app-lifecycle.js | 静态应用模块承载，页面直属 Port，已有停止通知/闲置清理 | 改为通用 App Host + sandbox 承载 |
| extension/app-module-registry.js | 构建期白名单，包含 Demo/fund | 删除生产依赖；不再要求新增应用修改扩展映射 |
| extension/apps/demo-ui.js、fund-ui.js | Demo 为资源展示，Fund 为空态占位 | Demo 用新标准样例替换；Fund 占位在安全迁移后删除 |
| extension/app-download.js、catalog-client.js、app-catalog-policy.js | 签名 Catalog、受限下载和资源包规则 | 保留防线，更新 Catalog v3/runtime 包契约 |
| crates/native-file-host/src/app_store/ | App Store、事务、锁、迁移、收据、清理 | 作为唯一安装权威扩展，禁止第二套 Registry |
| app_install.rs、app_host.rs、app_host_manifest.rs | 当前拒绝 runtime；后两者主要为旧版本/旧注册清理 | 审计现有及历史可复用原子写/注册代码，按新契约恢复受限原生包安装；不整体回退旧提交 |
| scripts/apps/、app-release.yml | 包/签名/安全检查与浏览器 harness；当前偏纯资源包 | 修改成新契约；旧的“永不注册 App Host”断言须替换为受限注册验证 |
| ../Natives-App-Fund/ | 只有 docs/reference/fundval-local-audit.md，无 Cargo、UI、业务、测试、Release | 第二阶段包含真实应用落地；不得声称是已有完整程序的迁移 |
| 旧 Apps V2/Web Surface 文档 | 仍描述已删除的 Tauri/macOS 窗口体系 | 加历史标记与新入口，禁止作为新实现的默认方案 |

基金行为输入：[基金审计文档](../../../Natives-App-Fund/docs/reference/fundval-local-audit.md)。它包含历史实现设想和数据源调查结论；本次只继承经审查的业务语义。其“基金 UI 编译进扩展”等旧技术落点必须更新。

## 3. 选定的落地架构

~~~mermaid
flowchart LR
    C["应用中心<br/>扩展内管理页面"] --> H["Core Native Host<br/>安装、登记、更新、清理"]
    H --> R["App Store 与安装收据"]
    S["通用 app.html<br/>每应用一个 owner 页面"] -->|"短连核验安装记录"| H
    S -->|"直属 Native Port"| A["官方 App Host<br/>Chrome 按需启动"]
    S -->|"受限 sandbox iframe"| U["应用自带 UI<br/>127.0.0.1 动态端口"]
    U -->|"短期会话鉴权"| A
    A --> D["本应用业务库 / OS Keychain"]
~~~

这里的 App Host 是下载的应用程序本身，不是 Core 再启动的一层守护进程。应用程序内嵌 UI，既处理 Native Messaging 生命周期，也提供自己的 loopback 业务接口。Chrome 按连接启动/退出它。

Core 不给业务程序做通用 RPC 转发，不持有基金 DB，不实现“基金专用能力”。app.html 只提供可信壳、主题/语言、生命周期与安全握手，不执行下载的业务 JS。

独立 Native Messaging 注册和 sandbox 是新 ADR 的精确例外。Service Worker 仍无状态、无 Port、无轮询。文件页和 Model Host 保持现有边界。

这是技术方案的默认路线，不是声称新安全边界已验证。A1 必须先证明真实 Chrome/Chromium 的 sandbox、Native Messaging、CSP/CORS 和回收成立，不能跳过它直接开发基金。

## 4. 工作顺序与交接

~~~text
A0 规范同步与基线
  → A1 独立应用纵向验证
  → A2 固化协议和通用支持库
  → A3 安装、升级、注册、恢复
  → A4 应用中心与通用呈现
  → A5 迁移、独立发布与总验收
  → B0 基金接入与范围基线
  → B1 基金持仓业务
  → B2 数据导入与净值
  → B3 基金更新、迁移、发布验收
~~~

A2 完成后，A3 和 A4 可以由不同工程代理并行；共享协议只由 A2 owner 修改。A1 是测试夹具/纵向验证，不产生第二条生产运行链路。B0 可以在 A 阶段阅读需求，但 B 阶段生产集成以 A5 通过为前置。

每个工作包提交：改动文件、真实行为、所跑检查及结果、已知限制、下一包输入。只维护本方案中的验收状态或 PR 证据，不再生成多份“最终完成报告”。

## 5. 第一阶段：应用中心改造

### A0 — 同步 ADR、规范与改造基线

输入：当前仓库、ADR-0027 草案、managed-app-contract 草案。

动作：
- 核对当前 HEAD、工作区、最高 ADR/协议/schema 号；不覆盖其他代理的新变更。
- 阅读 docs/README.md、Standards 与 AGENTS；执行本节后的规范修订矩阵。
- 以用户已经确定的目标为范围，在同一个A0文档提交中把ADR-0027转为accepted-target、契约转为active-target，并同步Standards/AGENTS限定例外；该提交合入执行分支才生效，生产仍未迁移。生效前不进入A1，也不把草案当作放宽MUST的许可。实际取代关系必须可追踪。
- 记录改造前相同机器/构建类型/数据路径的包体、启动、进程、RSS/CPU、关闭回收和重复开关表现。复用既有 perf 脚本，不先新造监控平台。
- 核验独立基金目录，不把占位或审计文本计作业务实现。
- 本文预算是提议的目标；在此步骤同步规则与检查口径。Core 原有预算不放宽，运行载荷新增 32/128 MiB 上限须记录原因。

交付：一个文档/规范提交，基线证据附于已有性能文档或 PR；明确 current 与 target。
验收：对改造范围进行冲突扫描，无“新方案允许、现行 MUST 仍禁止”的未处理冲突；未恢复任何历史业务代码。

### A1 — 独立应用端到端纵向验证

创建最小标准样例，只有版本展示、输入保存/读取、一个可取消操作；不使用基金业务。

动作：
- 使用独立原生 App Host 和内嵌静态 UI，构建一次即可由通用 app.html 打开。
- 测试用户目录安装与 Native Host 注册，真实 connectNative，获取随机 loopback 端口，加载 sandbox iframe。
- 验证 opaque origin 下静态资源、HTTP preflight、鉴权请求、父子握手、主题与语言传递。
- 验证 UI 无 chrome.runtime Native 能力、不能导航顶层、访问 Core、其他应用和外部网络。
- 验证页面关闭、显式停止、Host 崩溃、浏览器退出、隐藏闲置回收与数据重开。
- 同时打开两个相同应用页与两个 profile，证明只有一个业务实例，其他页有真实错误或复用行为。
- 样例使用隔离用户数据/浏览器 profile；测试结束清理注册和进程，禁止修改真实基金数据。

主要落点：scripts/apps/check-chrome-native.mjs、check-app-browser.mjs、native-fixture.mjs；测试用样例放 scripts/apps/fixtures/，不新增正式侧栏应用。
验收：真实 Chrome 证据通过；至少一次实际 Chromium 测试作为声明支持 Chromium 的依据；记录精确版本和平台。
失败处理：修复契约/ADR和验证，不通过取消 sandbox、允许 arbitrary JS 或常驻 Service Worker 继续。不能只用浏览器 mock 宣称可行。

### A2 — 协议、类型、通用支持库

动作：
- 固化 Catalog v3 / Core Apps v4 / App v1 类型和 fixtures；增量演进现有 app_store/types.rs，错误映射与 zh_CN/en 同步。
- 创建小型 app-host-support 编译期库，吸收 A1 证实必要的 framing、origin、运行锁、会话鉴权、HTTP 限额与 EOF 关闭逻辑。标准样例和基金共用，避免两次手写安全协议。
- 协议类型/校验资料统一生成；不让支持库依赖 native-file-host 二进制或 file-manager-core 文件能力。
- 支持库提供稳定版本引用与接入范例。基金独立 CI 能获取固定版本，不依赖开发机 ../ 路径。
- 拒绝未支持的 kind/权限/协议；权限展示与真正可执行的限制对应。
- 健康检查与实际运行分开；不让健康检查获取安装器已经持有的 runtime 锁。

主要落点：crates/native-file-host/src/app_store/types.rs；新增 crates/app-host-support/；scripts/apps/fixtures/；现有 check-app-manifest.mjs。
验收：至少两个独立入口使用同一套支持库测试；超长帧、错 origin、错协议、错会话、重载撤销、运行锁竞争和 EOF 均有可运行检查。
不做：插件 SDK、动态路由注册平台、通用数据源接口、业务数据库 ORM、自动重启守护。

### A3 — 安装、注册、更新和恢复

动作：
- 在既有事务中增加 managed_local 单可执行包，Host 重新核验签名目录与实际 hash。
- 前端只下载原始gzip，以契约定义的256KiB顺序chunk上传；Core接收有界签名目录、核验原始artifact、流式解压并核对payload长度/hash。实现install_finish、重复chunk幂等、乱序拒绝、取消、断连与半chunk恢复；每帧≤512KiB，不能把128MiB载荷塞进Native消息。
- 原生格式/架构/平台签名验证；不执行安装脚本，不信任 Catalog 的磁盘路径或启动参数。
- 根据 appId 计算 runtimeHost，写入受限 manifest；注册 Chrome/Chromium 及 Windows HKCU。实际扩展 origin 取自可信启动上下文。
- 从当前实现/历史提交复用路径校验、原子写、旧状态快照、Windows 注册和回滚算法，不恢复历史 Tauri、demo-host 产品或 runtime 通用入口。
- 统一 install→runtime 锁顺序，运行中不能更新、卸载、停用或清数据。
- 实现 journal 崩溃恢复：包写入、候选健康检查、注册切换、DB 提交、清理均有故障切点。
- 安装“修复”只处理代码、注册和收据，不重置业务 DB；候选 --health 不进行数据迁移。
- 卸载和清数据沿用 retained-data/cleanup-pending 收据；清数据与 Keychain 删除需二次确认，失败可重试。

主要落点：app_install.rs、app_host.rs、app_host_manifest.rs、app_store/install.rs/schema.rs/cleanup.rs/mutation.rs、app_dispatch.rs、app_secrets.rs。
验收：坏签名/hash/平台/大小/路径/文件占用全部拒绝；每个崩溃切点能恢复到旧版本或明确待恢复；没有“成功但注册不可用”的半安装；升级和默认卸载保留数据。

### A4 — 应用中心与通用 app.html

动作：
- 应用中心保留“已安装/可获取”的使用方式，补齐用途、发布者、来源、兼容性、更新说明、权限及代码/用户数据占用详情。
- 每张卡片只展示真实可用操作：安装、打开、停止、更新、启用/停用、修复、卸载；数据清理放独立危险操作。
- 应用中心/侧栏/已打开 app.html 复用已有导航投影和生命周期通知；新增版本或启用状态变化不靠长期轮询同步。
- 删除静态业务 UI 白名单依赖：app.html 从 Core 权威安装快照获取 runtimeHost，再短断 Core、直连 App Host。
- app.html 实现契约规定的 sandbox、两阶段握手、会话撤销和本地 URL 校验；不调用 new Function/eval/import 下载代码。
- 处理当前 profile 页面复用、运行/停止/失败/别处运行。每 OS 用户命名空间最多四个运行实例，由支持库的运行槽文件锁保证，不能只用 tabs.query 做有竞态的数量检查。停止通过 runtime.sendMessage 协调 owner，并由 runtime 锁/退出证据确认。
- 卸载、停用、更新先停止拥有 Port 的页面；其他 profile 仍运行则返回 APP_BUSY，用户可在对应会话停止后重试。
- 显示用户能理解的状态：未安装、安装中、已安装、启动中、使用中、已停止、更新中、需要修复；内部 busy/dirty 不直接暴露成工程术语。
- 补齐键盘路径、焦点环、dialog 交互、空态/加载/错误/不支持和 zh_CN/en；复用现有 tokens 与组件模式。

主要落点：extension/apps.*、app.*、native-app-client.js、app-lifecycle.js、app-navigation-projection.js、catalog-client.js、manifest.json、两套 locales。
验收：标准样例全操作真实可用；静态映射中不存在该 appId 也能安装和打开；隐藏/关闭后无业务资源遗留；前后台任务处理符合契约。

### A5 — 迁移、独立发布和第一阶段总验收

动作：
- Catalog v2 与 v3 显式隔离；新客户端只激活 v3 managed_local，旧客户端不能误装新载荷。
- 现有 extension_app 记录转换为保留安装历史/数据、需要迁移或重装的状态；不能把旧 resource 记录直接标记为新应用可运行。
- 保留原 appId、sidebar_order、show_in_sidebar、enabled、data/imports。已安装且曾启用但没有新包时显示需要升级/重装，不自动运行旧二进制。
- 旧 legacy 清理迁移必须增加版本/收据门槛，不能按 com.natives.app.* 广泛删除新登记的 Host。
- 删除 extension/apps/*-ui.js 的旧生产加载、registry 及相关纯资源运行断言；保留必要的迁移 fixture，不保留双生产 fallback。
- 扩展发布物确认无 Demo/Fund 业务代码；普通 App 发布 pipeline 不调用 extension:release，不构建 Core。
- 发布流水线产出各平台候选包、签名目录、hash 清单、接入检查和回滚资料。Core Catalog trust root 管理保持单一。
- 使用固定的扩展/Core 构建产物：安装样例 v1，再只发布样例 v2，验证 UI 和行为真实变化；再加入一个全新 appId，验证无需修改扩展代码。
- 检查安装器/卸载器能清理 App Native 注册，同时不误删用户数据与 Model/Files Host。

验收：第 9 节 A-Gate 全部通过；交付协议版本、固定支持库版本、样例包和证据。未通过的平台标记 unsupported/pending 并阻止该平台发布，不显示安装成功。

## 6. 第一阶段必须同步的规范矩阵

| 文件/范围 | 必须修改的内容 | 必须保留的约束 |
|---|---|---|
| ADR-0027 | 接受目标方案、记录选择原因/预算/例外与验收状态 | 不宣称当前已迁移 |
| ADR-0026 | 标记由 0027 取代其 Apps 目标，保留历史正文与原因 | 签名、原子更新、默认留数据、Keychain |
| ADR-0025 | 注明构建期 UI、旧协议/包预算等冲突由 0027 取代 | 可复用的供给链与安装安全思想 |
| ADR-0020 §4/§5/生产范围 | 允许官方 App Host 与应用自有业务数据；保持领域独立 | 禁止 Agent/Harness/Jobs/通用 Plugin Runtime |
| ADR-0023 | 为 app.html sandbox/应用 loopback 增加明确例外，Files 不扩权 | Files EOF、无常驻 SW、无通用路径/进程接口 |
| ADR-0022 Apps 部分、contracts/apps-web-surface-contract.md | Tauri child WebView/BrowserStateHandle 标历史，新目标指向本契约 | Appearance/Workspace 无关决策不变 |
| standards/product/01 | App 独立交付、入口和来源/托管分离；删 Apps 静态 UI MUST | AI Personal Workspace 定位与单一 Registry |
| standards/product/02 | 新管理行为、诚实状态、独立发布验收 | 无假数据、失败不可伪完成 |
| standards/technical/01 | App Native authority、app.html owner、允许范围；注明 Native 每 Port 独立进程 | Files/Model 隔离、无 SW 保活 |
| standards/technical/02 R-S1～6、R-S14 | 单进程 App EOF、官方包、sandbox/Token/CSP/CORS/Host 验证；旧通用路径前缀改为每 App 独立端口与资源域 | Secret 不入 UI、签名/hash、路径与权限校验 |
| standards/technical/03 R-D1/D3/D6 | App Store 与业务库分别写；独立 schema/备份/回退；不依赖 beforeunload 保存 | 增量迁移、WAL/FK、原子写、数据保留 |
| standards/technical/04 R-P11～13 | Apps 不用旧 WebView LRU预算；Core/App 分账、32/128 MiB载荷与实测回收 | Core 原有预算、同机前后证据、无门禁豁免 |
| standards/technical/05 | App 运行支持库、HTTP/取消/结构化错误、锁顺序与受限健康检查 | 无 panic/裸 spawn、日志脱敏与共享协议 |
| standards/technical/06-sub-apps.md | 改写为本契约的规范入口；删除 R-APP-01/02/05/08 等冲突规定并逐条给替代规则 | 稳定 ID、权限、数据与 Secret 规则 |
| AGENTS.md / docs/README.md / standards/README.md | 更新生产目标例外与任务地图，不保留“所有 iframe/App Host 都禁止”的无条件措辞 | Cargo/测试/复用/迁移纪律 |
| pm-context/apps-center-ai-requirements.md | 旧 Tauri Web/macOS 需求标历史，指向本方案 | 不把旧承诺悄悄冒充现状 |
| REFERENCED_PROJECTS.md、基金审计文档技术映射 | 准确记录独立基金仓库和新接入落点；历史事实保留日期 | Clean Implementation，行为参考与源码区分 |
| scripts/perf 与 scripts/apps 检查 | 逐条改为新规则的可验证检查；新增失败用例 | 不删测试让其变绿，不在 CI 加 skip/budget waiver |

规范正文与测试不必机械保留旧名称；但每个放宽的 MUST 必须能追溯到新 ADR。不能只更新 ADR-0026 却让 product/technical/AGENTS 继续互相否定。

## 7. 第二阶段：基金应用按新规范落地

### 7.1 本次范围

本阶段目标为一个真正可安装、可独立更新的个人基金持仓应用：账户、基金、持仓、确认交易、手工/文件导入、净值和基础市值/盈亏展示，以及完整的数据与生命周期保护。

现有审计报告中的养基宝登录、跨市场行情、穿透估值、FundScore、风险分析与 AI 解释，不自动全部纳入本次接入改造。不得做成假入口。B0 用“本次交付/后续候选”矩阵记录；若用户已有其他已确认基金需求，补入矩阵并实现相应工作包，不能被本方案静默删减。

当前本机没有完整基金实现，所以这里明确包含首次业务落地。后续执行时若发现新增真实源码，先按 B0 重新核验并复用，禁止无条件重建。

### 7.2 首版资金与记账口径

- 只处理人民币场外基金的已确认交易；币种固定CNY。股票/外汇/多币种、场内买卖、自动交易不在本次范围。
- amount、fee、cost_basis、realized_pnl以元为单位，输入最多两位小数；quantity与基金NAV/price最多四位。多余有效精度不静默截断，返回校验错误。
- 内部采用有界十进制数及溢出检查；截取到指定精度统一向零截断（ROUND_DOWN），不是二进制浮点或负数floor。中间乘除不提前量化，落账的金额/费用/成本分配量化到两位，持仓份额四位。
- amount表示不含费用的已确认成交金额；BUY计入成本amount+fee，SELL净收入amount-fee。未提供amount时由quantity×price截取两位生成；提供时与该值差异超过0.01元即作为冲突进入导入预览/人工更正，不能悄悄改账。金额和份额必须正，费用非负，卖出费用不得大于成交金额。
- 卖出使用移动平均成本：扣除成本=当前成本×卖出份额/当前份额，最终截取两位；已实现收益=卖出净收入-扣除成本。全部清仓时扣完剩余全部成本，避免分钱残留；任何时间点按稳定顺序（交易日期、确认时间、内部ID）重放结果一致。
- 平均持仓成本价与展示NAV可显示四位，但已四位显示的均价不能回写成本计算。缺成本的期初记录保持cost_basis=null，相关收益显示不可计算；不得补零后产生假利润。
- 交易日期按Asia/Shanghai的YYYY-MM-DD解释；净值日期按数据源原值保存，节假日/缓存不改写成今天。卖出超过重放所得可用份额直接拒绝。
- B1黄金用例必须作为精确断言；另覆盖分币成本、负收益、全部清仓、溢出和同日多笔顺序。

### 7.3 CSV模板与导入身份

首版只支持UTF-8 CSV（可有BOM，标准双引号转义），不承诺XLSX。文件≤5MiB、数据行≤20,000、单字段≤4KiB；界面选择模板版本1，不通过任意脚本/表达式映射列。传输复用应用业务HTTP的分块上传，每块≤256KiB，预览分页，不突破通用请求上限。

| 模板 | 必填列 | 可选列/规则 |
|---|---|---|
| 已确认交易 | account,fund_code,trade_date,type,quantity,price | amount,fee,external_id；type为BUY/SELL；fee缺省为0，amount按7.2计算 |
| 期初持仓快照 | account,fund_code,as_of,quantity | cost_amount,market_value,earnings,nav,nav_date,external_id；成本缺失时允许以市值减收益推算但必须标记，结果为负需人工更正；无法推算则保留未知成本 |

account映射到本地唯一账户，缺失账户先在预览中确认创建；fund_code为六位数字字符串，保留前导零。快照仅用于尚无该账户/基金记账历史的期初建账；已有流水时返回冲突，不覆写positions或编造反向交易。

持久化import_runs：sourceId、templateVersion、原文件SHA-256、导入参数/账户映射摘要、确认requestId、状态和结果；行记录保留原行序号、规范化字段摘要和结果。精确重复（同source/template/rawHash/参数）返回原结果，不再次入账；同requestId不同payload摘要返回conflict。

有可信external_id时以(sourceId,accountId,external_id)去重；相同ID内容变化进入冲突预览，禁止静默覆盖。没有external_id时以导入批次身份+行序号+内容摘要去重，只保证同一确认批次的重试幂等；不同文件/批次中“看起来相同”的交易提示可能重复，由用户明确处理，不自动合并合法的同日双买入。手工记账同样持久化requestId与payload摘要。

解析/校验/预览发生在提交事务之外；确认时再次核验数据版本并原子提交业务与导入收据。提交取消/失败回滚整批，重复确认返回持久结果；成功响应必须在事务提交后。导入原文件作为用户数据保留在imports/，可由独立清理动作删除。

### B0 — 基金接入与业务基线

仓库：/Users/ldh/Downloads/project/AiNative/Natives-App-Fund；目录当前尚无实现，先核验是否为 Git 仓库，必要时初始化独立项目。

动作：
- 读 fundval-local-audit.md，只使用行为规格；不复制 FundVal-Live 源码、组件或运行依赖。
- 修订其技术映射：Fund UI/业务独立交付，fund-host 自身为 Native Messaging Host，使用 app-host-support；不再写 extension/apps/fund-ui.js。
- 保持 appId=fund 和 Keychain namespace；安装 runtimeHost 使用 Core 统一算法，不手工登记。
- 建立最小 Rust fund-host、原生静态 UI 源文件、静态打包脚本、锁定依赖、单独 Release workflow；不引入 Docker/Django/Redis/Tauri/独立后台服务。
- 按7.2/7.3固化资金精度、交易类型、已确认/待确认区别、CSV身份和净值日期，并生成范例与测试。现有审计文本不能被当作最新数据源可用性证明。
- 建立业务范围与验收清单：首版限人民币基金资产；股票/外汇/多币种资产与高级投资分析不默默附带。
- 用 A5 发布的支持库版本和黑盒契约套件完成空应用接入；此空壳仅算接入通过，不算第二阶段完成。

建议文件落点：Cargo.toml、src/main.rs、src/portfolio/、src/storage/、src/sources/、ui/、app.json、scripts/、.github/workflows/release.yml。按真实复杂度建文件，不生成空目录/空接口。

验收：从应用中心安装、打开、停止、卸载新基金包；不修改 Natives 中任何基金专用逻辑；错误和空态真实。

### B1 — 持仓与交易的可用业务

动作：
- fund.db 仅由 fund-host 写；基于标准 data 根路径，SQLite WAL/FK、版本化增量迁移、原子备份。
- 按7.2实现账户与基金基本资料、已确认买入/卖出、期初持仓、交易列表与持仓汇总。
- positions 是可重建投影；交易/明确标记的期初快照是来源。禁止既直接改 positions 又用 transactions 重算造成双权威。
- 同一天同账户同基金可以存在两笔合法买入；不能把 (account, fund, date, type) 当所有交易的唯一键。手工提交以 requestId 幂等；导入以真实外部交易 ID 或已确认导入批次/行身份幂等。
- 已确认交易才改变持仓；待确认记录单独展示，不能以估值净值伪造已确认成交。
- 金额/净值/份额使用十进制定点语义，定义精度和舍入，禁用二进制浮点累计成本；超卖在写入时拒绝。卖出后剩余成本及已实现收益可重放。
- 用户保存返回成功前完成事务；输入草稿与已确认交易分开保存，关闭不会把草稿误作交易。
- 零资产显示空态；缺净值显示缺失/过期，不把成本价当最新市值。

至少保留一组黄金用例：买入 100 份×1.00，费用 1.00；再买入 50 份×1.20，费用 0.50；总成本 161.50、份额 150；卖出 60 份×1.30、费用 0.50，按移动平均成本法扣成本 64.60，已实现收益 12.90，剩余份额 90、成本 96.90。此为测试 fixture，禁止写入用户初始数据。

验收：开户→录入→查看→卖出→重开可用；同日多笔交易、重复提交、超卖、重算、崩溃恢复、精度、未确认交易有有效测试。

### B2 — 导入与净值

动作：
- 按7.3提供两种明确CSV模板和手工补录；只处理用户主动选择/提交的内容，不开放任意路径扫描。
- 导入流程为预览→字段/格式检查→差异与重复项→确认→单事务提交→结果。失败保留原数据，重复导入不倍增持仓。
- 仅有持仓快照时记录带来源/日期的期初快照；不捏造真实买入交易。成本由市值减收益推导时显式标记推算，缺失不默认为零。
- 接入一个验证可用的基金净值 HTTPS 来源；首选审计文档中的 EastMoney Mobile 作为调查起点，实际接口/使用条件和字段以实施时核验为准。不 eval 外部脚本/JSONP。
- 外部来源超时、无数据、限流、字段变化分别处理；有界请求、增量历史净值、缓存/数据库去重；同步由前台或用户动作触发，无后台常驻刷新。
- 每条净值/汇总显示 source、navDate、fetchedAt、是否过期；只有来源可比且数据完整时计算总市值/盈亏。跨日期或缺失项显示覆盖情况，不伪造完整总额。
- 净值不可用仍可使用本地账本和已明确日期的缓存；数据源失败不能吞成空持仓或 0 市值。
- 无公开可用来源时，B2 的在线净值验收保持 pending；不能用 fixture 冒充线上可用或把来源缺口藏进“改造完成”。

若另有已确认要求接入养基宝：单独追加认证工作包，先验证 HTTPS/合规可用接口；Token 只进 Keychain，失效为 auth_required，QR 轮询仅可见+有界且可取消。不把审计报告中的历史 HTTP 明文端点直接作为生产默认。

验收：手工与导入均产生真实可持久化资产；重复导入幂等；净值源有真实调用证据与脱敏 fixture；缺失/过期/失败显示正确；业务联网和凭据不经过扩展页面。

### B3 — 独立更新、数据迁移与基金总验收

动作：
- 用同一 Core/扩展安装基金 v1，创建真实测试数据；只发布基金 v2，验证 UI/业务更新和数据保留。
- v2 修改业务 schema 时，独占运行锁，关闭写入、SQLite 在线备份或其他一致性快照、执行迁移、验证后再进入 ready。备份包括 WAL 一致性，禁止只复制正在使用的 db 文件。
- 按接入契约4.1记录迁移journal、真实schema和备份。迁移失败恢复备份后退出并报告APP_MIGRATION_FAILED，中心显示需要修复；显式apps:rollback由Core持锁执行当前已安装版本的只读--inspect-data核验，兼容时原子恢复上版manifest/activation/收据，不兼容或状态不确定则拒绝。不能只恢复DB却把候选入口显示正常，也不能静默恢复旧备份丢掉新写入。
- 系统 Keychain 可用、锁定、用户拒绝、退出登录、卸载留数据、确认清数据和重装均测试。
- 基金包每个平台先代码签名/公证，再计算 hash 和生成 .nap，进入官方签名 Catalog；公开发布由执行任务的实际授权决定，本方案只要求产出可审阅候选和验证证据。
- 删除/停用原 fund placeholder 的生产引用；保留 fund 身份、侧栏顺序、已有 data/imports。未知开发期数据不能直接 DROP/覆盖，先备份与识别。
- 提交真实 UI 截图与业务/性能/迁移证据。空壳、Demo 成功、只显示“暂无资产”均不能作为基金交付完成。

验收：第 9 节 B-Gate 全部通过；没有为基金修改 Core 业务代码或增加扩展业务体积。

## 8. 发布与平台纪律

当前代码已有 macOS 真实 Native Messaging harness，其他平台不能因为编译成功就标“已支持”。

首个本机验收平台为 macOS arm64；macOS x64、Windows x64、Linux x64 按 Catalog 宣告的平台逐一产出安装/启动/退出/更新/注册清理证据。未宣告的平台显示 unsupported。Chrome 和 Chromium 的注册路径分别核验，Chrome for Testing 与正式 Chrome 路径差异也须核验。

仓库职责：
- Natives：Core 安装管理、通用壳、协议/支持库、官方 Catalog 信任根、通用发布校验。
- Natives-App-Fund：基金业务、UI、业务数据迁移、依赖锁、平台构建与应用候选 Release。
- 官方 Catalog 发布：从应用候选读取最终签名后的 artifact/hash；只更新目录和应用资产，不重新发布扩展/Core。

升级回滚边界：
- 下载/验签/注册/安装失败：恢复原代码、注册和收据。
- 业务 migration 失败且尚未接受新写入：应用恢复一致性备份，保持原数据可用。
- 新版本已写入且 schema 不兼容：禁止自动恢复旧数据；显示不可安全回退，保留新数据和备份。
- 清理失败：保留 cleanup_pending，明确重试。
- 不兼容老 Core：显示客户端升级提示，不尝试下载动态补丁到扩展。

## 9. 验收矩阵

| Gate | 必须证明 | 证据 |
|---|---|---|
| A-G1 独立交付 | 固定 Core/扩展可安装新 appId、升级样例 UI 和业务 | 构建 hash 不变、应用 v1/v2 实际截图及安装收据 |
| A-G2 供应链 | 坏签名、hash、平台、超限、非法来源/路径不能安装 | Host 侧失败 fixture，不只前端单测 |
| A-G3 注册与恢复 | Chrome/Chromium/Windows 注册受限，崩溃切点可恢复 | 平台测试与 journal 故障注入 |
| A-G4 承载隔离 | UI 无扩展能力；错/旧 Token、别的 origin/app 请求失败 | 真实浏览器 + 本地 HTTP 集成测试 |
| A-G5 生命周期 | 安装不运行；停止/EOF≤2秒退出；闲置回收；无孤儿 | PID/端口/锁/资源检查 |
| A-G6 竞争 | 双开、跨profile、运行中更新/卸载不破坏实例和数据 | 两页面/两profile/Host并发测试 |
| A-G7 数据保护 | 升级不覆盖数据；默认卸载保留；清除二次确认 | 实际文件/Keychain/收据检查 |
| A-G8 诚实UI | loading/empty/error/unsupported/运行在别处可区分 | 浏览器操作与中英文截图 |
| A-G9 资源 | Core预算不放宽；应用包/并发/缓存有界；循环不持续增长 | 同设备Release前后数据、30分钟循环 |
| A-G10 单一生产链 | 无静态业务registry或旧Tauri fallback | 构建产物/引用/入口检查 |
| B-G1 真实业务 | 完成账户、买卖、持仓、重开、导入、净值流程 | 端到端业务记录与截图 |
| B-G2 数值正确 | 黄金用例、超卖、同日多笔、幂等/重放一致 | 确定性领域测试 |
| B-G3 数据来源 | 真实净值调用，来源/日期/缺失/失败可见 | 脱敏响应fixture+一次真实验证 |
| B-G4 升级恢复 | v1数据→v2迁移；失败/不兼容回退不丢数据 | 版本化DB fixture和候选包 |
| B-G5 独立性 | 基金新增UI/业务无需Core代码/构建变更 | 跨版本集成，Core/扩展hash不变 |
| B-G6 退出与凭据 | 本应用数据/Secret归属、退出、锁定、清除正确 | 通用契约套件+平台Keychain测试 |

不允许用以下结果代替完成：只写 ADR、只编译、mock 返回成功、Demo通过代替基金、页面隐藏但进程仍运行、源码没有业务但入口能打开、在线来源未验证却显示正常。

## 10. 检查命令与执行纪律

开发中只跑覆盖改动的最小检查；Rust 先精确测试/受影响包，相关代码没改不重复已经通过的检查。

现有可用命令（从 Natives 根目录）：

~~~sh
rtk npm run apps:check
rtk npm run apps:integration
rtk npm run apps:browser
rtk npm run apps:chrome-native
rtk env -u CARGO_TARGET_DIR cargo test -p native-file-host app_store
~~~

其中 apps:chrome-native 依赖真实 Chrome for Testing 和 Playwright 模块路径；脚本需输出缺少的依赖，不得静默 skip。当前只覆盖 macOS，新增平台检查是 A5 的工作。

最终集成必跑并记录，复用 scripts 中相互包含的检查结果，避免手动重复叠跑：
- rtk npm run extension:check
- rtk npm run perf:check
- rtk npm run perf:files（当前已包含在 perf:check 内，保留其完整通过结果）
- rtk env -u CARGO_TARGET_DIR cargo fmt --check
- rtk env -u CARGO_TARGET_DIR cargo test --workspace
- 更新后的 Apps 集成与真实浏览器/Native 验证。
- 基金独立项目的 format/test/build/业务与迁移检查、通用接入契约检查和平台候选包测试。

本机基金 Rust 检查也从 Natives 根调用 --manifest-path ../Natives-App-Fund/Cargo.toml，并通过 cargo metadata 核验 target_directory 沿用此 checkout 的 target/。不得新增 --target-dir、CARGO_TARGET_DIR 或第二 worktree target；CI 独立构建按其 checkout 自带配置执行。

不得堆叠疑似挂起的测试；先 --no-run 区分编译，再 time-bound 精确测试、查锁和进程。不得将活跃测试管到 tail。最终只有文档变化时不运行整套产品测试，但必须核验文档链接、路径、diff 和 current/target 声明。

## 11. 给执行代理的起始指令

### 执行代理 A：应用中心

~~~text
在 Natives 仓库执行 docs/development/app-center-fund-implementation-plan.md 的 A0—A5。
先读 ADR-0027 草案和 managed-app-contract 草案，按 A0 同步冲突规范，再实施。
目标是官方应用 UI/业务独立交付、通用应用中心管理和页面直属生命周期。
复用现有 App Store/安装事务/路径校验/导航/测试，不恢复 Tauri/Agent/Jobs。
A1 必须用真实浏览器证明新承载链路；A5必须固定Core和扩展产物证明独立新增/更新。
不开发基金业务。交付协议/支持库版本、样例包、迁移说明与A-Gate证据后再交接B。
~~~

### 执行代理 B：基金应用

~~~text
以应用中心 A5 已通过的契约与支持库为输入，在 Natives-App-Fund 执行 B0—B3。
先核验现状；当前基线只有行为审计文档，不能将占位页当既有完整应用。
保持 appId=fund；实现本方案限定的真实持仓业务及独立签名发布。
不在 Natives 扩展/Core 中新增基金 UI、基金数据表或专用协议。
使用标准 App Host 支持库；业务库和凭据归基金，停止/更新/卸载遵守通用契约。
交付真实业务、数据迁移、独立更新、生命周期和B-Gate证据。
~~~

执行任务完成报告应区分已实现、已测试、已生成候选、已发布、平台未验证。发布授权和外部平台审核不能由本方案文字替代。

## 12. 依据

- [现行子应用规范](../standards/technical/06-sub-apps.md)：本次被替换的主要约束。
- [当前 ADR-0026](../adr/0026-sub-apps-shared-host-and-resource-distribution.md)：历史纯资源路线及安全保留项。
- [Chrome Native Messaging](https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging)：每个 connectNative 的独立 Host 和消息限额。
- [Chrome MV3 政策](https://developer.chrome.com/docs/webstore/program-policies/mv3-requirements/)：隔离上下文例外、代码可审查性与发布审核边界。
