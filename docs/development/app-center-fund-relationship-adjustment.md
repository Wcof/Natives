# Natives 主从关系复核与 Agent 修复执行方案

> 日期：2026-09-11。状态：**Superseded**（2026-09-11 架构整改：本方案的修复清单已全部并入
> [app-center-fund-implementation-plan.md](app-center-fund-implementation-plan.md)（唯一实施入口）；
> 主从关系进一步收敛为“单 Runtime + builtin 托管子应用”，见 [ADR-0029](../adr/0029-unified-suite-preinstalled-apps.md)
> 与 [06-sub-apps](../standards/technical/06-sub-apps.md) 的 runtimeType 模型与 R-SUBAPP-NATIVE-01。
> 本文仅保留历史背景与缺陷清单，不再作为执行依据。）
> 核验基线：Natives HEAD `762f6d84e442247c7ea2c5c1fd66bff2ce98a4a7`，连同当前未提交改动；Fund 按相邻目录实际源码核验。
> 本轮是源码、规范及脚本审查，没有重新运行产品测试、安装程序或发布资产。
> 本文细化[原两阶段方案](app-center-fund-implementation-plan.md)，不替代其业务范围、精度规则和 A-G1—A-G10 / B-G1—B-G6。

## 1. 复核结论：方向已纠正，约束尚未闭环

现在的产品关系已经合适：**一个 Natives 主产品 → 应用中心管理的可选基金扩展包 → 基金包内部模块**。无需推翻现有架构，也无需重新引入 Tauri、通用插件运行时或另一套安装器。

但不能据此宣布调整完成。文件分开、文案改名、增加 activation.json，并不等于主产品已经可靠控制副应用，也不等于内部模块遵守同一套业务规则。

| 优先级 | 当前源码事实 | 后果 | 修复工作包 |
|---|---|---|---|
| P0 | Core `set_enabled` 忽略激活投影写入错误；投影校验允许部分身份字段缺失，generation 未贯通 | 中心显示停用成功，Host 仍可能启动；旧会话与新版本混用 | A2、A3 |
| P0 | `apps:rollback` 用字符串匹配兼容性，检查失败放行；注册/投影与 DB 分步切换 | 未证明数据兼容也可回退；安装权威与实际入口不一致 | A3 |
| P0 | Fund 恢复 journal 不检查新写入标志，`restored` 可再次恢复；恢复路径直接覆盖 DB | 旧备份可能覆盖更新后的数据，故障恢复不幂等 | B3 |
| P1 | CSV 直接插入 transactions，绕过手工账本的超卖校验；持仓回放用 min 截断卖出 | 导入与手工录入规则不同，错误账本被投影掩盖 | B1、B2 |
| P1 | Core 与支持库的部分 appId 锁路径不同；锁文件释放后删除；启动和安装有竞争窗口 | 安装、启停、跨 profile 竞争不能由同一锁可靠约束 | A2、A3 |
| P1 | Fund 手写协议解析及 HTTP 生命周期；支持库仍为相邻目录 path 依赖 | 共享类型实际未贯通，独立交付与有界退出未证明 | A2、B0、B3 |
| P1 | 样例 v1/v2 包复用同一调试程序；真实浏览器、平台签名和完整产物不变证据未闭环 | 当前证据不足以通过 A-Gate，更不能据此通过 B-Gate | A1、A5 |

定位入口（行号可能随后续改动变化，以函数名为准）：

- [Core 管理变更](../../crates/native-file-host/src/app_store/mutation.rs)：`set_enabled`、`rollback`。
- [Core 激活投影](../../crates/native-file-host/src/app_activation.rs)：投影写入/更新、`find_runtime_binary`；[安装事务](../../crates/native-file-host/src/app_store/install.rs)：commit 与 restore。
- [共享激活检查](../../crates/app-host-support/src/activation.rs)、[共享锁](../../crates/app-host-support/src/lock.rs)、[Core 锁](../../crates/native-file-host/src/app_install.rs)。
- [Fund 迁移](../../../Natives-App-Fund/src/migration.rs)：`read_journal`、`recover_pending_journal`、`restore_database`；[Fund 主入口](../../../Natives-App-Fund/src/main.rs)：`data_status`、握手、HTTP 与停止。
- [Fund 导入](../../../Natives-App-Fund/src/import.rs)、[账本](../../../Natives-App-Fund/src/ledger.rs)、[持仓](../../../Natives-App-Fund/src/portfolio.rs)。
- [样例打包](../../scripts/apps/package-demo.mjs)、[集成检查](../../scripts/apps/check-app-integration.mjs)、[真实 Chrome 检查](../../scripts/apps/check-chrome-native.mjs)。

Fund 现在已有实际 Rust 业务、UI、数据库和迁移源码，不再是“只有行为审计文档”。应审计并修复现有实现，不能重新生成占位工程；已有源码也不能等同于业务验收完成。

## 2. 固定关系与不做的事

```text
Natives（唯一主产品）
├─ 应用中心 / Core App Store：安装权威、验签、收据、启停约束、升级与恢复
├─ app.html：通用可信壳、页面会话、隔离呈现
└─ fund 托管扩展包（一个 appId、一条安装记录、一份包版本）
   ├─ UI → ledger / import / nav 等业务接口
   ├─ ledger：交易校验与账本写入；import 复用这一个入口
   ├─ portfolio：从账本和期初快照派生，不另立数据权威
   └─ storage / migration：包内数据库、事务、版本与恢复
```

- Natives 自身可以发布和更新；基金包也可以独立构建、签名和交付，但只能在 Natives 内被管理和使用。
- 保留当前“包内嵌 UI + 受控 Native Host + loopback + sandbox iframe”技术路线。独立进程是包的运行实现，不是第二款桌面产品。
- 基金包升级不得要求重发兼容的 Core/浏览器扩展；包内模块不得有独立安装记录、桌面入口、版本发布流程或 Core 业务分支。
- 不新建模块注册中心、模块依赖注入框架或通用业务 RPC 平台。复用现有 Store、事务、类型和支持库，只收拢重复的公共职责。
- `parentProduct`、`packageRole` 可保留现有固定声明，但不能作为鉴权证明。`moduleManifest` 不应成为强制逐模块版本协议；A0 修订后移除强制要求或保留为非权威描述，迁移兼容已有声明。

### 私有安装目录与系统提示的准确边界

包代码、数据和收据留在 `~/.natives/apps/<appId>/`；不创建独立 `.app`、不写 `/Applications`、不注册独立 Dock / LaunchServices 产品入口。**浏览器发现 Host 所需的最小注册元数据是明确例外**：macOS/Linux 放在相应浏览器指定目录，Windows 注册表指向 manifest JSON；manifest 再指向 Natives 私有目录内的受验签载荷。OS Keychain 继续是持久 Secret 的唯一权威。

Chrome、Chromium、Chrome for Testing 必须分别核验注册位置；Chrome for Testing 从 Chrome 146 起有不同的默认位置，不能靠改目录名猜测。以上注册机制以 [Chrome 官方 Native Messaging 文档](https://developer.chrome.com/docs/extensions/develop/concepts/native-messaging)为依据。

“属于 Natives 的扩展包”不表示 macOS 一定不会提示运行本机代码。记录实际提示来源、签名/公证状态和触发动作；不能把文案换名当作解决，也不能移除 quarantine、关闭 Gatekeeper、隐藏安全警告或自动下载/安装另一款浏览器来制造通过。

## 3. 执行顺序与职责

继续使用原工作包编号，不维护第二套进度：

```text
A0 规范消歧 → A1 真实标准样例 → A2 共享契约/锁/生命周期
                              → A3 安装与恢复 + A4 通用壳
                              → A5 全部 A-Gate
                              → B0 审计接入 → B1/B2 业务统一 → B3 迁移/独立交付
                              → 全部 B-Gate → 可审阅候选包
```

由一个集成负责人拥有协议、共享类型、锁顺序、Core 安装事务和最终验收。A2 接口稳定后，可以委派 A4 UI/测试；A-Gate 通过后，可以委派独立基金领域测试。不能让多个 Agent 同时改共享协议或并发构建同一个 Cargo target。

已有基金代码必须保留；A 阶段可阅读其缺陷和准备测试清单，但不得用基金代替独立样例，也不得绕过 A-Gate 继续基金生产集成。平台/凭据缺失只阻塞依赖它的验收，其他已授权独立任务继续；缺失 Gate 不得写成通过。

## 4. 第一阶段修复工作包

### A0 — 修正规范冲突，冻结唯一契约

修改范围：`AGENTS.md`、ADR-0027、managed-app-contract、Standards product/01 与 technical/01、02、03、06，以及两仓库必要的执行说明。

1. 保留已接受的主从定位，不重复请求用户批准替换 ADR-0026；历史取代关系必须可追踪。
2. 全文区分“浏览器扩展”与“托管扩展包”。修正 R-APP-02 当前“扩展包业务代码禁止进入扩展包”的自相矛盾：禁止进入 Core/浏览器扩展上下文，允许构建期进入自身托管载荷。
3. 将“注册严格只在私有根”修为上一节的受限浏览器注册例外；禁止任意路径、任意 Host 名称、广泛删除其他注册。
4. 固定一种 Host 命名算法、appId 到目录/锁的映射、activationGeneration 数值类型及来源；区分安装 revision 与激活 generation，禁止直接混用。
5. 固定 activation 严格字段、状态、来源校验、维护/失败状态及兼容策略；更新 R-APP-09 的旧 install_package 描述，使其对应现有分块安装事务，而非增加另一套 API。
6. 删除强制模块独立版本设计；修正 Fund 文档的业务规格权威顺序，以现行 Standards/契约和原方案 §7 为约束，行为审计作为输入，不继承过时技术实现。
7. 修正 Fund 本机 Cargo 指令：从 Natives 根通过 manifest-path 使用同一 target；移除 README 中无验收证据的“B0/B1/B2 完成”措辞，区分已写代码与已通过。

出口：规范/契约/AGENTS 不再相互冲突；列出必要迁移字段及错误语义；只保留一份协议定义。不降低原安全、数据和验收 MUST。

### A1 — 用真实独立样例重建最小纵向证据

修改范围：现有 `scripts/apps/fixtures/sample-host/`、样例打包脚本和浏览器 harness。

1. 复用真实 Native Messaging → 受限 loopback → sandbox 链路；样例从编译期确定 appId/版本，不由安装文件夹冒充身份。
2. 制作确有不同 UI 和业务行为的 v1/v2；另有独立 appId 用于新增包验证，不能只给同一二进制换版本目录。
3. 首先用已核验的浏览器路径证明安装、打开、页面交互、停止与 EOF。依赖缺失或安装后未出现“打开”须报告失败原因，不能以超时重试/放宽断言跳过。
4. 对 browser、Core、profile、apps root 和测试资产显式记录。使用隔离 profile/数据根；不得覆盖日常 Natives 安装和用户数据。

出口：真实纵向测试通过，截图、Host PID/退出、注册及请求记录可复查；mock 仅保留为补充单元测试。此时还不宣布 A-Gate。

### A2 — 让主产品控制真正落在共享运行边界

修改范围：`crates/app-host-support`；Core 复用的身份/锁调用点；标准样例适配。

1. 复用已存在的 serde/types、framing、http、session、origin 和 layout，收拢公共启动/会话/停止规则；不要再手写 JSON 子串解析，也不新建通用插件框架。
2. 严格校验投影与编译身份、安装目录、当前载荷、协议版本、合法 origin、收据摘要一致。缺字段、类型错、空允许列表、缺浏览器 caller、错误版本/摘要、非 ready 状态均拒绝启动；只读 inspect 模式单独限权。
3. Core 原子维护单调递增 generation；app.html、Native 握手和运行时使用同一类型和字段。先检查、取得锁后再检查，消除检查后被停用/更新的窗口。
4. Core 与支持库统一锁文件路径和获取顺序 install → runtime。锁文件使用稳定 inode，不在释放时删除；release/Drop 不重复释放。运行时生命周期持锁，更新/回滚/卸载/停用遵守同一互斥规则。
5. 启动与整个安装事务的竞争必须被锁或 durable maintenance 状态覆盖，不能依赖 install_begin 内很快释放的局部 guard。Windows 等条件编译不得无条件引用 Unix API。
6. 公共运行边界实现有界连接/请求/输出、真实 busy 状态、会话失效和停机顺序；停止不再提前报成功。EOF/显式停止按原契约 ≤2 秒退出，事务须完成或取消回滚，不丢已提交写入。

必要回归：缺/坏投影、origin 缺失/错配、旧 generation、载荷身份不符；停用与启动竞争；install/start 交错；短 appId 与历史完整前缀 ID；双页面/跨 profile；释放后第三个进程不能穿过第二个持锁者；忙碌时停止/EOF。

出口：样例通过同一公共边界；共享协议与锁只有一个负责人/实现，A3/A4 可据此集成。

### A3 — 安装、停用、回滚必须可恢复且默认拒绝不确定状态

修改范围：现有 `app_store/{install,mutation,cleanup,query,types}.rs`、`app_activation.rs`、注册和签名模块。

1. 删除安全/一致性路径上的忽略错误：投影更新、注册恢复、删除等失败不得回成功。复用既有安装 journal 保存精确旧收据/入口/投影；跨 DB、文件、注册表步骤支持崩溃恢复，而不是假设一次事务涵盖全部。
2. 从收据取得准确 executable、版本、平台和签名信息；禁止扫描目录选择第一个文件。切换期间禁止启动；提交成功或完整恢复后才暴露 ready，无法恢复则 repair_required/cleanup_pending。
3. 启用、停用、升级、回滚都持正确锁、更新 generation，并对 DB/投影任一步失败提供一致可恢复状态。安装新版不能无条件将已停用包改成 enabled=true。
4. `apps:rollback` 使用有超时、有输出上限、同时排空 stdout/stderr 的现有/共享子进程检查器。检查当前已验签程序的只读 `--inspect-data`；严格反序列化真实 schema/journal/write 状态，并与上版签名清单的读写范围比较。
5. inspect 缺失、超时、非零退出、坏 JSON、类型错误、缺字段、未知 schema 或迁移状态不确定，一律拒绝回退并给可修复错误。禁止用 contains 匹配兼容布尔值。
6. schema 相容且已有新写入时，只回退代码/入口/收据，保留业务库；不相容时保留新版数据和备份，拒绝回退。Core 不打开基金库、不加入基金 schema 判断。
7. 路径、链接/重解析点、权限/ACL、原子写和同步错误必须处理；注册清理只处理收据证明归本产品所有的项。Windows 指向 manifest，不是 executable。
8. 生产候选必须拒绝仓库开发私钥对应的信任根和 fixture 旁路；平台签名检查验证预期身份，签名/公证之后再计算最终 artifact/hash。macOS/Windows 检查采用真实平台工具并有界执行。

必要故障注入：投影不可写、DB commit 失败、注册失败、restore 再失败、每个 commit/recover 切点中断；坏签名/hash/路径/身份；inspect 不同 JSON 空白形式及全部失败分支；回退后新写入仍在；默认卸载保留数据，清理失败可重试。

出口：故障发生后只有“旧版一致可用 / 新版一致可用 / 明确需要修复”三类结果，没有假成功、混合收据或静默数据恢复。

### A4 — 通用壳如实呈现主从状态

修改范围：`extension/app.js`、`apps.js`、native client、lifecycle、两套 locales 及现有测试。

1. 中心只展示包状态；基金和其他包复用 `app.html?app=<appId>`。不增加基金 registry、路由条件、业务字段或本机桌面入口。
2. UI 使用真实 activationGeneration、运行/忙碌/repair 状态，不把管理 revision 当激活序号，不用页面内布尔值代替 Host 状态。
3. 核验 parent/source/challenge/session，覆盖过期/错误 token、页面导航、刷新、bfcache、主题/语言变更和停止确认；坚持无 allow-same-origin、无 Service Worker Native Port/轮询。
4. 将旧 mock-only `apps:browser` 适配新架构但明确其证据级别；真实浏览器检查保留独立门槛。同步中英文包管理文案，错误不得统一显示为空态。

出口：真实浏览器展示 installed / running / stopped / disabled / elsewhere / repair / unsupported；关闭后进程、端口和锁按约定回收。

### A5 — 固定产物，关闭全部 A-Gate

1. 固定一份 Core release 和完整浏览器扩展包，记录不可变 hash；不是只 hash app.js/apps.js/manifest 三个文件。
2. 在同一固定宿主上完成新增样例 appId、真实 v1→v2 UI/业务升级、停用、恢复、回滚、卸载重装与数据保护。v1/v2 载荷不同且业务差异可观察。
3. 清除旧静态业务 registry/资源包生产依赖，保留必要的一次性迁移与历史清理；升级旧安装不得双重注册或丢数据。
4. 以固定 revision/version 交付支持库和契约测试。样例证明在没有 Natives 相邻源码目录的干净环境可构建；不以可变分支或未锁定 path 依赖证明独立交付。需要远端发布时另取授权，可先提供锁定源码快照/本地 Git bundle 候选。
5. 对原方案全部 A-G1—A-G10逐项记录证据，包括签名、公证、跨 profile、真实回收、同设备 Release 资源对比和 30 分钟循环。只声明已实际验证的平台；编译成功不能替代平台验收。

出口：原方案要求的 A-Gate 全通过才交接 B。签名身份、浏览器或平台条件缺失须保留 pending/unsupported，不以调试 fixture 解除门槛。

## 5. 第二阶段修复工作包

### B0 — 接管真实代码并复用已经验收的公共边界

前置：A5 交接的固定契约、支持库、候选宿主和全部 A-Gate 证据。

1. 核验 `/Users/ldh/Downloads/project/AiNative/Natives-App-Fund` 的实际 Git/源码/测试/数据布局，保留未提交实现。按原方案 §7 逐项标记已实现、缺失和未验证，不另立功能范围。
2. 将 main.rs 的公共协议解析、激活、锁、鉴权、HTTP 有界并发及退出复用支持库；保留基金业务路由，不让支持库包含基金语义。
3. 固定不可变支持库依赖及 lockfile，去除发布时对 `../Natives` 的必需依赖；本机验证仍遵守共享 target 规则。
4. 只交付一个 appId=fund；模块依赖通过少量包内函数/类型约束，不给模块增加安装对象或独立版本框架。

出口：基金通过通用接入契约套件；不把“入口能打开”记为业务通过。

### B1 — 账本单一写入口，持仓保持派生

修改范围：Fund `ledger.rs`、`portfolio.rs`、`fixed.rs`、`storage.rs` 和相邻测试。

1. 在既有 ledger 模块提供可复用的事务内校验/写入函数，接受已持有的事务/连接；手工与 CSV 统一调用。不得持 Store mutex 再调用重新加同一锁的公开 snapshot/query。
2. 在同一事务内校验账户/基金、幂等、精度、金额/费用、时序和超卖，再写账本/元数据并生成投影；整批失败不留下部分交易。追溯日期交易须验证完整时间线，不能只看今天余额。
3. 禁止用 min 将超卖“算正常”；发现历史非法账本报告需要处理，不静默改用户数据。期初成本未知在后续买入后仍不能被变成全部成本已知；已清仓已实现收益不能丢。
4. 所有金额/份额计算与汇总在领域层用有溢出检查的定点数完成；页面不得用 JS Number 累计财务结果。

必要回归：原方案全部精度/账本用例；黄金结果买入后成本 161.50、卖出扣成本 64.60、已实现收益 12.90、剩余 90 份成本 96.90；同日多笔、回填日期、超卖、未知成本、清仓、溢出、重开重放一致。

出口：手工和导入具有同一套可测试业务不变量，positions 只是投影。

### B2 — 导入、净值和实际页面完成业务流程

修改范围：Fund `import.rs`、`nav.rs`、`ui.rs`、业务路由及测试；不改 Core 加业务支持。

1. CSV preview/commit 使用同一解析映射和账本入口，复核文件/映射摘要与请求载荷。当前 FNV 摘要不能冒充 SHA-256；复用已安装哈希依赖，保留原始导入文件和来源。
2. 遵守原方案 §7.3 的模板、5 MiB/20,000 行/4 KiB 字段、256 KiB 分块、分页预览；覆盖 BOM、引号、重复行、external_id 内容冲突、同 requestId 不同载荷冲突、预览后文件变更、批次原子性。
3. UI 重试保留同一个 requestId/sourceId，不在每次点击重新生成而破坏幂等；受限 iframe 中验证 storage/postMessage 实际行为，不能通过增加 allow-same-origin 解决。
4. 补齐账户、买卖、期初持仓、导入确认、重开、净值/收益流程及主题/语言/错误状态；财务汇总使用业务层返回的精确值。
5. 对本次选择的净值来源完成至少一次真实请求，保留脱敏响应与字段解析测试；展示来源、交易日、抓取时间、缺失/过期/网络错误，不伪造实时状态。不扩展到未确认的扫码登录、自动交易或投资建议功能。

出口：B-G1/B-G2/B-G3 具备真实业务证据，真实网络失败时明确 pending，不以离线 fixture 冒充成功。

### B3 — 数据恢复安全后再交付独立候选

修改范围：Fund `migration.rs`、`storage.rs`、`main.rs` 的 data_status，以及现有打包/README。

1. `data_status` / `--inspect-data` 共用类型化只读实现：读取实际 DB `PRAGMA user_version`、迁移 journal 和持久化写入元数据；不得用程序常量、默认 false 或字符串匹配代替。
2. journal 缺失、损坏、权限失败和不同迁移状态分别处理；不能把坏 journal 当无迁移。`restored` 是终态，不得每次启动再覆盖；未完成恢复不得进入 ready。
3. 每次业务写入的 lastDataWriterVersion/新写入标记与业务事务共同提交；确定 DB 与 journal 的恢复判定规则，有歧义即保留数据并拒绝回退，不因外部 journal 写失败失去已提交事实。
4. 迁移前用 SQLite 一致性备份包含 WAL 已提交内容；恢复不得在活跃连接打开时删除 WAL/SHM 或直接覆盖库。持独占环境，关闭连接后校验备份并通过可恢复步骤切换；恢复失败保留现场和原数据。
5. 每步迁移使用适用的事务/验证，明确 prepared/migrating/verified/committed/restored/failed；备份统一到契约目录。识别当前开发版 data/backups 等旧布局，迁移不能丢已有备份或冒充干净新装。
6. 有新写入时只允许经 Core 通用兼容检查后的代码回退，不恢复旧备份。没有新写入且迁移失败时才允许安全恢复；错误必须返回中心并保持 repair 状态。
7. 制作真实不同的基金 v1/v2（含可验证 schema/UI/业务变化），在固定 Core/完整扩展 hash 下验证升级、迁移失败、兼容/不兼容回退、默认卸载保留及显式清除。
8. 修正 package.sh：本机从 Natives 根沿用共享 target，独立 CI 使用自身 checkout；先平台签名/公证，再算最终 hash/清单。产出能被通用中心安装的候选，不能只有空 packages 清单或手动可执行文件。

必要故障注入：各 journal 切点断电/中断、坏 journal、备份缺失/损坏、WAL 有已提交写入、恢复再失败、连续重启、恢复后继续写再重启、v2 新写入后兼容与不兼容回退。逐项比较账本/新写入/备份，不能只断言进程返回零。

出口：全部 B-G1—B-G6 通过；Keychain 按实际权限和凭据流程测试。若本期无凭据功能，须在范围/权限中明确，记录该子项不适用及理由，不能据此省掉数据隔离、退出、清除和 Core 的 Keychain 安全验收。

## 6. 检查、证据与交付

### 执行纪律

- 开工记录 HEAD、branch、两仓库工作区差异、工具版本和现有 Cargo/测试进程；保留无关改动，不 reset/clean、不重新安装日常 Natives。
- 所有 shell 按仓库使用 rtk；Cargo 从 Natives 根执行并去掉环境 target 覆盖。不要自行安装其他 Node/浏览器版本，先核验 package.json engines 与已存在依赖。
- 开发期只跑受影响的最小检查；修复失败分支先补能复现的回归。疑似挂起先定位，不叠加重跑，不将活跃测试接 tail。
- 最终命令以原方案 §10 和当前 package.json 为准，复用 perf:check 已包含的检查，不手动重复堆叠。平台/busy/故障场景检查不被这些命令自动替代。

```sh
# Natives 根目录；按影响范围选择，不要求开发期全部反复运行
rtk env -u CARGO_TARGET_DIR cargo test -p app-host-support
rtk env -u CARGO_TARGET_DIR cargo test -p native-file-host app_store
rtk npm run apps:check
rtk npm run apps:integration
rtk npm run apps:browser
rtk npm run apps:chrome-native

# 最终集成：包含的子检查通过日志可复用
rtk env -u CARGO_TARGET_DIR cargo fmt --check
rtk env -u CARGO_TARGET_DIR cargo test --workspace
rtk npm run extension:check
rtk npm run perf:check

# 基金本机验证仍从 Natives 根调用；先确认 target_directory
rtk env -u CARGO_TARGET_DIR cargo metadata --manifest-path ../Natives-App-Fund/Cargo.toml --no-deps --format-version 1
rtk env -u CARGO_TARGET_DIR cargo fmt --manifest-path ../Natives-App-Fund/Cargo.toml --check
rtk env -u CARGO_TARGET_DIR cargo test --manifest-path ../Natives-App-Fund/Cargo.toml
rtk env -u CARGO_TARGET_DIR cargo build --manifest-path ../Natives-App-Fund/Cargo.toml --release
```

### Gate 与证据记录

唯一正式进度仍记录在原方案 §9 或该变更的 PR 证据中，为每个 Gate 增加状态、执行命令/退出码、环境和证据路径。状态使用 passed / failed / pending / unsupported；不把 partial 当通过。

- A-G1/10：完整固定宿主 hash、新 appId、真正不同样例 v1/v2、收据、单一生产入口。
- A-G2/3：真实平台签名/身份与受限注册、坏包拒绝、各安装/恢复故障切点。
- A-G4/5/6/8：真实浏览器隔离与状态、双 profile、停止/EOF/忙碌、PID/端口/锁、中英文截图。
- A-G7/9：数据/Keychain/备份保留、显式清除、同设备 Release 指标和 30 分钟循环。
- B-G1/2/3：真实业务流程、全部确定性数值/幂等用例、真实净值调用及脱敏输入。
- B-G4/5/6：真实基金 v1/v2、WAL/新写入不丢、固定宿主、退出与凭据权限检查。

每包交接最少包含：`变更文件 → 实际行为 → 检查命令/退出码 → 证据 → 失败或缺失条件 → 下一包输入`。历史通过结果要注明对应 hash；相关代码改动后不得直接沿用旧证据。

最终候选包含平台包、签名/清单/hash、固定支持库依赖、安装/升级步骤、旧安装与业务数据迁移说明、三类回滚操作（安装失败、无新写入迁移失败、有新写入代码回退）、数据保留与显式清除说明、全部 Gate 和外部条件。**只生成可审阅候选，不上传公开 Release/Catalog、不触发正式更新、不绕过用户授权。**

## 7. 可直接交给执行 Agent 的指令

```text
请执行 Natives 主从关系修复，不重新设计另一套架构。
主目录：/Users/ldh/Downloads/project/AiNative/Natives
基金目录：/Users/ldh/Downloads/project/AiNative/Natives-App-Fund

先核验当前 HEAD/branch/两仓库未提交改动，保留其他人的工作。
读 AGENTS.md、docs/README.md、docs/standards/README.md 及相关规范，
再读 docs/development/app-center-fund-implementation-plan.md、ADR-0027、
managed-app-contract.md 和 app-center-fund-relationship-adjustment.md。
以现行规范为权威；本补充方案是针对当前缺陷的执行清单，不表示 Gate 已通过。

从 A0 修正规范歧义开始；用户已经授权替换 ADR-0026 冲突决策，不重复询问方向。
保持 Natives 唯一产品、基金为可选托管扩展包、包内模块不独立安装。
先 A0—A5，用独立样例通过全部 A-Gate，随后才能 B0—B3 集成基金。
Fund 已有真实源码，审计修复现有实现，不生成占位页；不在 Core 加基金逻辑。
优先修复激活停用/共享锁、安装恢复/默认拒绝不确定回滚、
账本与导入共用入口、真实 schema 检查和迁移恢复不丢新写入。

共享协议/锁/事务和最终集成由一个负责人负责。复用现有支持库、安装事务、
类型、签名、导航、测试，不维护两条生产链，不增加模块版本管理框架。
使用 rtk；Cargo 从 Natives 根 env -u CARGO_TARGET_DIR，所有本机构建共享 target。
不覆盖日常安装或用户数据，不清 Keychain，不绕过 macOS 安全提示。

每包记录改动、验证、证据和下一步；阻塞时继续独立工作，失败不冒充成功。
按原方案全部 A/B Gate 验收，mock 不替代浏览器/Native Host/平台证据。
交付可审阅候选包及安装、迁移、回滚说明；正式公开发布另行申请授权。
```
