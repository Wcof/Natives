# 官方托管应用契约 v1

> 状态：active-target；随 [ADR-0027](../adr/0027-managed-apps-independent-delivery.md) 在实施方案 A0 提交生效（accepted-target 同批；active-target 表示可据此实施并修改生产路径，不代表实现已完成）。
> 验收前不豁免现行 MUST；实现状态与 A-Gate/B-Gate 证据见实施方案，不写在本文。
> 本文是应用中心与应用开发者共享的接口唯一来源；实现状态和任务进度不写在本文。
> 范围：官方 managed_local 应用。外部 Web URL 仅保留类型语义，本期不得注册或执行。
> 版本：Catalog v3 / Core Apps protocol v4 / App protocol v1。实现前检查当前 HEAD，若号码已占用则顺延并同步全部 fixture，禁止覆盖已发布协议。

## 1. 产品对象与权威

| 对象 | 含义 | 唯一权威 |
|---|---|---|
| AppDefinition | 应用身份、发布者、用途、兼容要求与交付声明 | 已验证签名 Catalog |
| Installation | 已安装版本、包收据、启用状态、侧栏设置 | Core App Store |
| RuntimeInstance | 一次真实应用进程、锁、会话与当前工作状态 | 持有运行锁的应用 Host |
| Surface | 通用 app.html 中一个应用呈现会话 | 该 app.html 页面 |
| UserData | 用户业务记录与导入原件 | 对应应用的业务库/数据目录 |
| Credential | 外部服务凭据 | OS Keychain，业务库仅保存 opaque reference |

安装状态与运行状态分离；Core DB 不通过持久化 running 布尔值判断进程存活。
显示 running 必须取得当前 owner 的真实响应；无响应先显示 unknown/checking，锁被占用且当前 profile 找不到 owner 时显示“应用正在另一会话中使用”。仅凭锁和 Native origin 无法识别具体 Chrome profile；不得声称准确检测到哪个 profile，也不能猜测 stopped。

## 2. 应用声明

Catalog 使用已有 app_id 命名风格，不引入第二 App Registry。schema 字段由协议代码生成校验资料，禁止扩展页面和 Host 各维护不同枚举。

| 字段 | 要求 |
|---|---|
| app_id | 稳定且全局唯一；沿用现有合法 ID，fund 不改名 |
| kind | 当前只接受 managed_local；web_link 是未来语义，当前返回 unsupported |
| name / description | 真实名称及 zh_CN/en 文案 |
| publisher | 官方发布身份；绑定本地信任根，不仅依赖一个 official 标签 |
| version | 应用 SemVer，与 Core/App 协议版本分离 |
| minExtensionVersion / minHostVersion | 安装管理端的最低版本 |
| appProtocolVersion | 1；独立运行协议 |
| dataSchema | 当前版本及可读/可写版本范围；用于决定代码回退是否安全 |
| permissions | 实际使用的能力声明；只展示或约束确实实现的能力 |
| packages | 每个目标 platform/arch 恰好一个 required runtime 载荷 |
| publishedAt / changelog | 有来源的发布时间和变更说明 |
| runtime budgets | 声明内存/请求/缓存限额，不能提升 Core 的绝对上限 |

不允许声明任意系统路径、shell 命令、启动参数、安装脚本、任意 Native Host 名称或扩展脚本 URL。

runtimeHost 由 Core 计算：com.natives.app.a + app_id 的完整 SHA-256 小写十六进制。避免 app_id 中的连字符与 Native Host 命名限制产生冲突；Keychain namespace 仍沿用 com.natives.app.<appId> 语义，fund 保持 com.natives.app.fund。映射结果持久化在安装收据中，不由页面猜测或 Catalog 覆盖。

## 3. 包与目录

首版一个平台包就是 gzip 单载荷 .nap，载荷为平台原生可执行程序；UI 的 HTML/CSS/JS/图片在构建时嵌入该程序。没有压缩目录树，没有安装时 npm/pip/cargo，也不要求用户预装语言运行时。

每个包必须包含版本、平台、架构、wire_size、payload_size、artifact_sha256、payload_sha256。先做平台代码签名/公证，再计算最终载荷 hash 和压缩包 hash；签名后修改二进制会使发布失败。

首版统一从现有 Wcof/Natives 的 GitHub Releases 交付 Catalog 和应用资产；基金可以独立构建，再把候选交给该发布仓库，不要求 Core 重新构建。浏览器仅接受签名条目中的 github.com/Wcof/Natives/releases/download/... 初始 URL，最终 response.url 仅接受 HTTPS 的 github.com、release-assets.githubusercontent.com、objects.githubusercontent.com 或 releases.githubusercontent.com。请求 credentials=omit、referrerPolicy=no-referrer，禁止未经签名的镜像替换与 HTTP 降级；页面 CSP/host_permissions 同步限定来源。

浏览器负责网络请求和可观测的初始/最终 URL 校验；普通 Fetch 不能报告所有中间重定向时，不宣称已逐跳审计。A1 必须验证跨来源、HTTP 降级、循环重定向、超时和响应上限；Core 不发起该 HTTP 请求，只验证签名选择与实际 artifact 字节，不能假称它观察了网络链。新增下载域或更强网络策略属于显式兼容变化，本期不承诺任意仓库接入。

Host 在可信边界重新验证签名 Catalog 原文、选择的条目与所有元数据，并对实际字节计算摘要。不能只信任前端 alreadyVerified 或前端传来的 hash。签名私钥只在发布凭据中，禁止进源码、安装包或测试证据。

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
      data/                          # 应用独占；升级与默认卸载保留
      imports/                       # 用户导入原件；默认保留
      cache/                         # 有界、可清理
      staging/<installId>/           # 失败可恢复，最多一个安装事务
      backups/<migrationId>/         # 业务 schema 迁移备份，有界保留
~~~

Native Host manifest 与 Windows 注册表属于 Core 的安装收据，不允许应用自己随意登记。Chrome/Chromium、实际扩展 origin 和 OS 平台差异由现有安装器机制统一处理，不接受请求体自报 origin 授权。

原生程序按用户权限运行。namespace 是数据组织约定，不能宣称阻止恶意原生程序访问其他用户文件。官方发布审查是当前信任前提，第三方 native 包明确拒绝。

### 3.1 激活投影的读取契约

Core App Store 是安装权威；activation.json 是 Core 在安装/启用/停用时生成的原子投影。App Host 只读该投影，不猜测或直连 natives.db 的内部 schema。

投影使用严格 JSON，字段固定为 receiptVersion=1、appId、runtimeHost、activeVersion、generation、activationState（ready/maintenance/removed）、enabled、appProtocolVersion、payloadSha256、allowedOrigins、catalogEvidenceSha256。不得包含用户数据、凭据、任意启动参数或可覆盖标准路径的字段。

路径固定在标准 apps/<appId>/activation.json；Core 创建父目录为仅当前用户可写，文件权限 POSIX 0600 或对应 Windows 用户 ACL，拒绝符号链接/重解析点逃逸，写入采用 temp+fsync+rename。App ID 来自程序编译期身份，与安装目录、投影和 Catalog 四方一致；不能从网页参数决定读取路径。

catalogEvidenceSha256 指向标准 apps/.catalog-evidence/<sha256>.json 与同名 .sig：Core 保存已验证的原始签名目录，支持库用锁定的官方信任根验证后，核对本程序身份、版本、平台和载荷摘要。启动时校验 current_exe 的实际摘要与投影、签名条目一致。allowedOrigins 来自 Core 核验过的 Chrome 启动 origin，应用再与自己的真实 argv origin 比对。

签名保护发布身份与不可变包信息；enabled/generation 是 Core 的本地控制状态，由受限写入与 OS ACL 保护。这不是抵御同用户恶意原生程序篡改的认证系统，不额外引入一个把密钥放同盘的伪安全签名器。

每次变更先写持久 journal，再将投影原子改为 maintenance；随后修改注册与 DB，最后写与已提交收据一致的 ready 投影。整个过程持有 install→runtime 锁。崩溃时 App Host 遇到缺失/maintenance/不一致即拒绝启动，由 Core recover 恢复；不能推测启用或自行修复投影。停用写 enabled=false；卸载写 removed/删除投影，均先阻断新启动。catalog-evidence 仅在没有当前/上版收据与 journal 引用时清理。

## 4. 安装、更新与清理

管理方法沿用 apps:install_begin / install_chunk / install_finish / install_commit / install_abort / recover / uninstall / clear_data / set_enabled 等现有语义，修改现有协议类型，不创建 parallel v2 store。

> 实现注记：v4 传输协议落地后，`install_begin` 只接受签名 Catalog（`catalogBase64` + `signature`，Ed25519 固定信任根验签，Core 自行选包）；旧的无签名整包 `install_package` 方法已删除，不再保留两条生产链路。分块方法实现为 `install_chunk`。

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
9. 默认卸载移除程序、注册和入口，保留 data/imports/业务备份；清缓存可独立执行。清除用户数据和 Keychain 是另一个显式危险操作，沿用二次确认。
10. Windows 被占用文件、锁住的 Keychain 或无法删除注册表：显示 retryable cleanup_pending，保留清理收据；禁止返回成功再默默留下残留。

平台安全提示正常展示。禁止通过移除 quarantine、关闭签名验证或使用任意 shell 安装器实现“无感安装”。

平台检查必须区分：macOS 为 Developer ID 签名、固定发布者 Team ID、notarytool 的已接受公证产物及实际 Gatekeeper 启动证据；安装时校验代码完整性和身份，不把 Catalog 自报 notarized=true 当公证验证。裸 CLI 的公证/离线票据支持在 A1 核验，不把不适用的 spctl/stapler 输出当成功；首次离线无法验证则明确不可用，不移除 quarantine。Windows 为 WinVerifyTrust/Authenticode 与固定发布者身份校验，签名/吊销状态不确定不静默接受。Linux 不要求不存在的统一 OS 签名机制，以官方 Ed25519 发布签名、载荷 hash、ELF 架构和受限文件权限作为验收。平台验证调用限时 30 秒并回收；无签名仅允许隔离开发 fixture，不能加入正式 Catalog。

### 4.1 业务迁移失败与代码回退

应用支持库在 data/.migration.json 记录 migrationId、from/toSchema、from/toAppVersion、state、backupId、hasCommittedNewWrites；状态为 prepared→migrating→verified→committed，失败为 restored/failed。字段不含业务内容或 Secret，原子写入。业务 DB 的 schema version 是实际数据版本权威，journal 负责记录跨步骤恢复；二者不一致时先恢复/修复，禁止 ready。

第一次迁移在接收任何用户写入前完成；备份在 backups/<migrationId>/，使用一致性快照。升级成功后最多保留两份已验证迁移备份；失败中/恢复未完成的备份禁止自动清除，也不再发起另一迁移堆积备份，要求先修复。备份和其他个人数据一样，默认卸载保留。

App Host 必须实现固定只读模式 --inspect-data：Core 在持 install→runtime 锁时执行，限时五秒、输出不超过 64 KiB；该模式不再获取 runtime 锁、不迁移、不联网、不访问 Keychain，只返回实际 schema、journal 状态、最后写入版本、已提交新写入标记和可验证的兼容性。禁止页面提供可执行路径或任意参数。

运行中失败通过 app:data_status/app:failed 返回同样的有界状态，供壳展示；Core 回退不能只信任页面转述，必须自行执行已安装且验签版本的 --inspect-data 核验。

新增 apps:rollback 为显式管理操作：仅当应用已停止、业务库与上版的读写 schema 范围相容，且 migration journal 一致时，按安装 journal 原子恢复上版 manifest/activation/收据。已经接受新写入但 schema 仍相容时只回退代码，不恢复备份。schema 不兼容、inspect 不可用或状态不确定时，标记 repair_required 并拒绝回退；不删除新版数据。

迁移失败且新写入尚未开放时，由候选应用恢复其备份，报告 APP_MIGRATION_FAILED 并退出；Core 保持候选版本为需要修复，用户可以重试修复或执行通过上述核验的“回退上一版本”。不能只恢复 DB 后仍把安装卡片显示正常。崩溃发生在迁移/恢复中则先以当前版本重开执行 journal 恢复，成功前禁用业务写入和不安全代码回退。

## 5. 运行协议

### 5.1 启动顺序

1. app.html 从 URL 只读取 appId。
2. 短连 Core apps:get，核验已安装/启用/兼容性，返回appId、activeVersion、runtimeHost、activationGeneration、enabled及安装状态；随后关闭 Core Port。
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

每个应用必须通过同一组黑盒契约检查：签名安装、激活投影的每个崩溃切点、离线重开、错 origin/协议、并发打开、非法本地请求、停止/EOF/崩溃、隐藏回收、未保存草稿、更新失败恢复、数据迁移失败与显式代码回退、卸载保留、确认清除、资源增长与独立发布。结果必须区分 mock、真实 Native Host、真实浏览器、平台签名和线上发布状态。
