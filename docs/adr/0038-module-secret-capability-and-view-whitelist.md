# ADR-0038 · 模块 Secret 平台能力与设置视图白名单

- 日期：2026-10-02
- 状态：接受（PanWatch R2 投资模块迁移 T01，用户明确授权实施）
- 依赖：ADR-0020（Keychain 权威）、ADR-0031（统一 App Runtime）、ADR-0036（卡片投影窄命令模式）、ADR-0037（Shell 能力代理模式）

## 背景

内置模块（首个为 fund）需要持久化外部通知渠道凭据等模块 Secret，同时新增「财务设置」视图需要从主应用左下角入口进入。现行平台只有 uninstall 时的
`app_secrets::purge_namespace`（只删不读），没有受控写入／掩码状态能力；深链 `view`
token 仅经通用短 token 正则过滤，模块没有签名声明的可进入视图集合。

## 决策

### 1. 视图白名单进签名元数据

- `module.json` 新增 `views: string[]`：模块接受的全部深链导航 token（纯导航意图，
  不含 URL／数据／HTML）。fund 声明
  `["home","positions","assets","trends","holdings","goal","allocation","settings"]`，
  其中 `holdings/goal/allocation` 是 ADR-0036 兼容映射输入，`settings` 由财务设置实施（T08）消费。
- 该字段沿既有权威链传递：`build.rs` 编译入 `FixedModule.views_json` → Product
  Manifest v2 模块条目 `views` → Core 产品激活收据 v3 模块条目 → `apps:get` 投影
  `app.views`。
- Shell（`extension/app.js`）在取得 `app.views`（非空字符串数组）后，welcome 携带的
  `view` 与后续 `navigate` 一律先查白名单；未声明 token 降级为空（默认视图），不转发。
  未声明 `views`（缺省或空数组）的模块保持既有纯 token 正则行为，直到该模块自行声明。
  iframe 与页面不能扩展白名单；任意 URL、跨 module 跳转仍由 appId 绑定与
  `app:handshake` 单次绑定拒绝。

### 2. Runtime 平台 Secret 能力（`app:secret`）

- App Runtime Protocol v2 新增窄方法 `app:secret`，仅接受参数
  `instanceId`、`generation`、`op ∈ {set,status,delete}`、`accountId`、`value`（仅
  set）、`confirm`（仅 delete）。仿 ADR-0036 卡片投影：参数缺失、generation／
  instanceId 与当前 Ready 会话不匹配、accountId 超模式（`^[a-z0-9][a-z0-9:._-]{0,63}$`）、
  value 超 8192 bytes 或出现未知字段，一律在产生任何副作用前拒绝并返回稳定错误码。
- 能力门：只有已核验产品激活收据中该模块 `capabilities` 含 `platform.secret` 时才允许
  任何 op；否则 `APP_CAPABILITY_DENIED`，无副作用。声明该能力要求 capabilityVersion
  递增（fund 3→4），并同步 registry equality 能力名单与签名清单校验。
- Secret 只进 OS Keychain：service 固定为 Runtime 注入的
  `ModuleContext.keychain_namespace`（`com.natives.app.<appId>`），account 为
  `accountId`；macOS 使用有界、超时、无回显的 `security` 调用，与既有
  `app_secrets::purge_namespace` 同一命名空间。本期实现 macOS 后端；Linux／Windows
  返回可恢复的 `APP_KEYCHAIN_UNSUPPORTED`，不得回退明文文件（R-S4）。
- 明文通路封闭：Secret 只能由 Extension 可信表单经 Native Messaging 端口写入
  （sandbox iframe 无 Native Port 能力，结构上不可达）；`app:secret` 没有读取明文的
  op，`status` 仅返回 `{accountId 是否存在, updatedAt}`；模块业务代码经
  `ModuleContext.secrets`（进程内 Arc）在发送期间短时取用明文，不进日志、不进 DB、
  不回传 iframe。SQLite／JSON／错误／日志只保存引用与掩码。
- 删除 Secret 是独立危险操作：协议要求 `op:"delete"` 且 `confirm:true`；UI 层二次
  确认由消费任务（T08/T09）实现，归档渠道不隐式删除凭据。
- 不新增进程、不扩 Model Host 边界、不新增 Native Messaging Host；能力随
  `natives-app-runtime` 编译交付。

## 后果

- `docs/standards/technical/02-security.md` R-S2／R-S4 与 `06-built-in-modules.md`
  R-A9 的既有 MUST 不变，本 ADR 提供其平台实现路径。
- `managed-app-contract.md` 登记 `app:secret` 请求／响应 schema 与 `views` 字段。
- 后续通知渠道（T09）与财务设置表单（T08）是该能力的唯一生产消费方；在消费前
  能力保持声明但无业务写入。

## 验证

定向回归覆盖：未声明能力、错误 instanceId／generation、未知字段、accountId／value
越限、set→status→delete 幂等链、macOS 后端注入命令执行器与 locked／denied 拒绝路径、
非 macOS `APP_KEYCHAIN_UNSUPPORTED`，以及 Shell 白名单对未声明 view 的降级。Keychain
真实写入只在隔离前缀（测试 service 名）下验证，不触用户既有条目。
