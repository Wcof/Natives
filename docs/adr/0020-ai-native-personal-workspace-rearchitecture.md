# ADR-0020: AI Native Personal Workspace 产品与 Native Backend 重构

- **状态**: 已接受（目标与迁移约束）；生产切换按 Gate 逐项验收
- **取代范围注记**: §1「首页就是 V1 唯一 Personal Workspace Home；不新增 Workspace 一级菜单、Workspace CRUD 或多工作台数据域」与 §3「Home 布局复用 settings K/V 单份版本化 JSON」及「禁止 Infinite Canvas 与嵌套容器」中与 [ADR-0021](./0021-multi-workspace-design-system-v2.md)（含 2026-08-23 PWSV2 修订）冲突的部分由后者取代；其余决策（产品身份、停止建设、领域边界、Host authority、Secret、迁移纪律）继续有效。
- **日期**: 2026-08-19
- **决策者**: 产品方（用户）
- **取代**: ADR-0012 的产品面/双轨与 Workshop 优先级、ADR-0015 的 Jobs 生产目标、ADR-0016 的 Capability Hub 产品目标、ADR-0019 的 Daemon Provider 执行归属
- **保留**: ADR-0001/0002/0006 的现存 iframe 安全防线，直到对应旧运行时被完整删除
- **关联**: `docs/standards/`、`docs/architecture/provider-proxy-architecture.md`、Home Workspace Patch（2026-08-19）

## 上下文

Natives 已叠加 Assistant、Agent Runtime、Harness、Jobs、Capability Gateway、Plugin Runtime、Provider routing 与固定 Usage Dashboard。现有源码在文件 CRUD、应用进程管理、Provider 协议编解码、OAuth、Usage 归一和 Tauri 生命周期方面有可复用资产，但产品入口、进程 authority 与领域命名已不再匹配目标。

2026-08-19 P0 审计确认：

- `provider-adapters` 的高价值资产是纯 request codec、SSE parser、usage/error normalization；`ProviderAdapter` 本身是浅 interface，混合旧 stream、静态模型、发现、测试和明文 Credential。
- Messages、Chat Completions、Responses、tools、reasoning、usage 与 stream lifecycle 均只有部分贯通；异常 EOF 仍可能被包装成完成。
- API Key 仍是显式/primary 选择，不是完整 Key Pool；OAuth pool 已有优先级/并发基础，但 refresh/expiry 语义未闭环。
- `provider_kek` 与 `env_encryption_key` 存在 SQLite `settings`，不满足 OS Keychain secret ownership。
- Home Grid 浏览器 Spike 证明 `react-grid-layout@2.2.4` 可进入下一 Gate，但 packaged Tauri/WebKit、Retina/zoom 和 soak 尚未验收。

## 决策

### 1. 产品身份与一级结构

产品唯一身份为：

> **AiNative = AI Native Personal Workspace。**

一级结构固定为：首页、文件、应用、AI、数据与用量、设置。AI 下分 AI Resources、Local Proxy、AI Tool Integration。

首页就是 V1 唯一 Personal Workspace Home；不新增 Workspace 一级菜单、Workspace CRUD 或多工作台数据域。

### 2. 明确停止建设

不再建设 Agent Runtime、Assistant、Harness、Planner、Subagent Runtime、Capability Gateway、Jobs 自主任务系统和 Plugin Runtime。迁移期旧代码只允许修复安全、数据迁移与删除阻断，不得新增产品能力。

### 3. Home 与 Widget

Widget 是内置普通 React renderer + definition/config + versioned grid layout。禁止 Widget Plugin Framework、Runtime、Worker、Event Bus、Package Manager、Marketplace、Infinite Canvas 与嵌套容器。

Home 布局复用 `settings` K/V 的单份版本化 JSON；拖动/缩放过程中不得写 SQLite，只有 stop/debounce/flush 持久化。Widget 只消费领域 query/facade，不直接碰 SQLite、文件扫描、Provider 或 secret。

### 4. 领域边界

- Files 保留成熟 CRUD、Trash、Watch、Search；资源能力与内容能力分离。
- Apps 使用 App / RuntimeSpec / RuntimeInstance / Surface，运行与呈现分离。
- Provider 是厂商；Connection 是真实 upstream；Credential 独立且支持多 Key；Secret 由 OS Keychain 持有。
- Proxy 是个人本地轻量代理，不是企业 AI Gateway；协议转换由可替换的深 `ProxyEngine` module 执行。
- Claude Code、Codex、Gemini CLI、OpenCode 属于 AI Tool Integration，配置流程必须支持 Detect / Inspect / Backup / Plan / Apply / Verify / Rollback。
- Usage/Analytics 复用现有采集、归一、聚合资产，不建设统一 Event Platform。

### 5. Native Backend authority

Tauri Rust Host 是默认 Native Backend，拥有本机 DB、文件、进程、Provider/Connection/Credential、Local Proxy、AI Tool Integration 与 Usage 编排。Renderer 只能经 typed adapter/IPC 调用 Host。

Sidecar 只在真实独立生命周期、崩溃隔离或第三方运行时要求下使用，并必须由 Host supervisor 管理。不得为逻辑分层新建 sidecar。

Agent Daemon 是待迁移旧生产路径，不再是目标 Provider/Run authority。迁移期间保持单一 production path；Host ProxyEngine 通过 fixture parity、secret migration 与 lifecycle Gate 后一次切换，禁止长期 Host/Daemon 双执行。

### 6. Secret ownership

持久 Secret 必须进入 OS Keychain。SQLite 只保存非敏感元数据与 opaque secret reference；不得把解密主密钥与密文放在同一数据库作为完成态设计。

迁移必须幂等、可恢复、可回滚：读取旧密文、写 Keychain、回读验证、切换引用、再清理旧密文。Keychain locked/unavailable 必须显式报错并保持旧数据可恢复；任何阶段都不得把明文写入日志、事件、Renderer 或临时文件。

### 6.1 Model Host exception（2026-08-30）

模型设置与 Local Proxy 使用隔离的单用途 Go `model-host`，复用固定提交
`CLIProxyAPI v7@f0de1d008fe8881dcb7431cf97b147295874c2b2` 的 SDK implementation。
这是第三方运行时与可选独立生命周期所需的受限 sidecar，不恢复旧 Agent Daemon：

- Extension 只通过独立 Native Messaging interface 调用 Model Host；Files Host 与
  Model Host 不共享文件、进程或 Secret interface。
- Model Host 是 Provider、OAuth account、model catalog、Secret reference 与 loopback
  gateway runtime 的单一 authority；CLIProxyAPI 默认配置文件和 token 文件不得成为
  第二 authority。
- Secret 只进入 OS Keychain 和必要的短时进程内存；CLIProxyAPI 使用自定义 auth Store，
  禁止把 access token、refresh token、API Key 或 gateway key 写入配置文件、日志、
  argv 或环境变量。
- Gateway 只绑定 `127.0.0.1`，必须鉴权，不暴露 CLIProxyAPI Management API。
- 默认生命周期由 Native Messaging 页面连接拥有。用户显式开启「常驻」后，Model
  Host 可以跨页面关闭继续运行，但必须单实例、可显式停止、无系统登录自启动；电脑重启
  后仅在再次打开 Natives 页面时按设置恢复。
- OAuth browser callback 流必须校验随机 state，并在供应商协议支持时使用 PKCE；Kimi、
  xAI 的供应商原生 Device Authorization Grant 以短时 device/user code、过期与轮询约束
  替代 callback state/PKCE，不得伪造不存在的 callback 安全步骤。

复用 Gate：外部 Go module 必须能锁定该提交；CLIProxyAPI 的 `sdk/config`、
`sdk/auth`、`sdk/cliproxy` 必须可由外部 module 编译；自定义 `auth.Store` 必须替换
FileTokenStore 并接管 refresh 回写。任一条件不满足时停止集成，不得以明文 token 文件
或完整 Rust 协议重写绕过。

### 7. 迁移与删除

实施顺序：P0 Gate → ADR/Standards → Shell/IA/Home Foundation → Files → Apps → AI Resources → Proxy → AI Tool Integration → Usage/Data → Home Widgets → Legacy Removal → Stability/Release Gate。

Legacy 删除必须有引用清单、迁移/回滚证据与 death proof。不得用新 facade 永久包住旧 Agent/Daemon 体系；只有完成 parity 的目标 module 才能接管 production path。

## P0 Gate

进入 Host ProxyEngine 生产切换前必须满足：

1. Messages、Chat Completions、Responses 有真实 request/stream/non-stream/error fixture。
2. tools、tool history、reasoning controls、usage、stop reason 端到端不丢失。
3. 异常 EOF 返回结构化 Error；取消/断连释放 socket/task；慢 consumer 有界。
4. Antigravity 使用专用 adapter 与 project header；OAuth refresh/expiry/client secret 语义闭环。
5. API Key Pool 有公平、并发、冷却、失败切换与可观测测试。
6. Keychain migration 的 write/read/verify/rollback/locked 场景通过，secret disk/log scan 通过。
7. production cutover 后不存在 Daemon Provider fallback。

Home Grid 采用与否另过 packaged Tauri/WebKit、缩放/Retina、循环压力、RSS/observer/listener Gate。

## 后果

### 正面

- 产品 IA 与长期维护目标一致，旧 Agent 概念不再驱动新代码。
- Host 成为本机能力单一 authority，减少 UDS/sidecar/双数据库复杂度。
- Provider codec 与 Proxy policy 分开，保留成熟协议资产并删除浅 wrapper。
- Secret 从“同盘加密”升级为 OS Keychain ownership。

### 成本与约束

- 迁移期必须维护明确的旧/新引用矩阵，但不得双写或双执行。
- Keychain 与三协议 fidelity 是生产切换硬 Gate，不能用 UI 完成度替代。
- 现存 Workshop iframe 防线在旧代码删除前继续生效，不能因产品下线计划提前放宽。

## 修订旧 ADR

- ADR-0012：保留历史背景；产品身份、一级结构、三面/双轨与 Workshop 优先级由本 ADR 取代。
- ADR-0015：Jobs 不再是目标产品域，只允许迁移与删除工作。
- ADR-0016：Capability Hub 不再是目标产品域；可复用连接/配置能力分别归 AI Resources 或 AI Tool Integration。
- ADR-0019：保留 P0 源码审计；Daemon 作为完成态 Model Gateway authority 的决策被取代。
