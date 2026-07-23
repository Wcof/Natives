# 技术架构 01 · 进程模型与分层依赖

> **版本**: 2.0.0 · **日期**: 2026-07-23  
> **关联 ADR**: [ADR-0008](../../adr/0008-electron-to-tauri-migration.md)、[ADR-0011](../../adr/0011-native-engine-production-gaps.md)、[ADR-0012](../../adr/0012-product-identity-workshop-scope.md)  
> **关联源文件**: `src-tauri/`、`src-agent-daemon/`、`src/app/`、`src/components/`、`crates/`  
> **关联指南**: [`docs/architecture/CODE_MODULE_GUIDELINES.md`](../../architecture/CODE_MODULE_GUIDELINES.md)、[`NATIVE-DAEMON-CAPABILITY-MAP.md`](../../architecture/NATIVE-DAEMON-CAPABILITY-MAP.md)

---

## 一、本篇要约束什么

Natives 是 Tauri Host + Next.js 壳 + Agent Daemon + 沙箱租户的多进程系统。最大的技术风险是**跨层越权**（前端直连 SQLite、插件直达 Node、UI 绕过 Daemon 调 Provider）。本篇钉死进程边界、分层依赖方向与 IPC 约定。

---

## 二、进程模型（四类边界，职责不可互换）

#### R-T1 · 四类进程/信任域的职责边界
- **等级**：MUST
- **分类**：进程、分层
- **规则**：系统**必须**按下列边界划分职责；**禁止**交叉：

| 边界 | 角色 | 允许 | 禁止 |
|------|------|------|------|
| **Host Main**（`src-tauri/`） | 桌面权威：窗口、本机 DB（`natives.db` 等）、PTY、模块安装、凭证 broker、Daemon 监督 | SQLite（宿主库）、子进程生命周期、文件系统重操作、IPC command 注册、UDS 客户端编排 | 渲染 DOM；直接跑 Agent Loop / Provider 流（生产路径） |
| **Agent Daemon**（`src-agent-daemon/` + `crates/*`） | 执行引擎权威：Run、Provider、工具 Gateway、事件持久化（`assistant.db` 等） | 会话/Run 状态机、工具执行门禁、协议 v2 RPC | 渲染 UI；绕过 Gateway 的任意本机破坏性操作；长存用户明文 Key（经 Host broker 租约） |
| **Renderer**（`src/app/` + `src/components/`） | UI 投影与编排 | 渲染、经 `tauri-adapter` invoke、订阅事件、管理 iframe / 子 WebView 宿主 UI | 直接打开 SQLite；直接 spawn 业务子进程；直接调 Provider HTTP |
| **租户**（Workshop iframe / Embed 子 WebView） | 不信任或弱信任代码 | 经 Bridge（Workshop）或受限导航（Embed） | 访问 Node / preload / 宿主 DB / Daemon UDS |

生产执行主链（诚实能力）：

```text
UI (Renderer) → Tauri Host (ExecutionAuthority façade) → UDS → Agent Daemon
```

- **禁止**：UI / 任意 Tauri command / 旧 Runtime 在生产路径上直接调 Provider、Stream 或绕过 Daemon 的 Agent Loop。  
- **Embedded Daemon**：仅单测与开发诊断；**禁止**生产静默降级到嵌入式假绿路径（见引擎整改契约）。
- **为什么**：进程边界即安全与「广告 ⊆ 可调」的基础。
- **检查方法**：
  - Renderer **禁止** `better-sqlite3` / `rusqlite` / `fs` 重操作 / `child_process` 直连。
  - Host Main **禁止** React/DOM。
  - 生产 `runtime=native` 路径必须落到 Daemon RPC，而非 UI 内嵌假完成。

#### R-T2 · SQLite 读写按库分权威
- **等级**：MUST
- **分类**：数据、安全
- **规则**：
  - 宿主配置/模块等 **`natives.db`（及同类宿主库）**：**必须**仅由 Host Main 读写。
  - 助理/引擎权威表（Daemon 侧，如 conversation/message/run）：**必须**由 Daemon 迁移与写入权威；Host 可镜像/投影，但**禁止**双写打架或共用一套 schema 版本表导致迁移互跳（见既有 `_daemon_schema_version` 分立约定）。
  - Renderer **禁止**直接打开任何 SQLite 连接；**必须**经 IPC / 协议。
- **为什么**：多连接乱写破坏 WAL；双权威破坏「无假绿」。
- **检查方法**：`grep` Renderer 是否 import 数据库驱动；Host/Daemon 迁移版本表是否分立。

---

## 三、模块分层与依赖方向

基座代码内部按四层组织，依赖**只能向下**。

```text
应用层 (Application)   ← 内置功能页、AI 工作台 UI、工具页
        ↓
框架层 (Framework)     ← Shell、iframe 管理、Bridge、主题、tauri-adapter
        ↓
服务层 (Service)       ← 宿主侧服务编排、模块管理 façade、搜索命令
        ↓
基础设施层 (Infrastructure) ← Tauri Host、Daemon、SQLite、PTY、HTTP、UDS
```

#### R-T3 · 依赖只能向下
- **等级**：MUST
- **分类**：分层
- **规则**：上层可依赖下层；下层**必须不**依赖上层；同层**应该**避免环。
  - 基础设施层**禁止** import `src/app/` / `src/components/`。
  - 框架层**禁止** import 具体业务页组件作为硬依赖。
- **为什么**：可测试、可演进；底层不被 UI 绑架。
- **检查方法**：底层出现 `@/components` / `@/app` 即不合规。

#### R-T4 · 跨进程通信只走 IPC / 协议 / Bridge
- **等级**：MUST
- **分类**：进程、安全
- **规则**：
  - Host ↔ Renderer：Tauri command / event；**禁止** `executeJavaScript` 注入业务逻辑。
  - Host ↔ Daemon：协议 v2（UDS）；方法三态诚实（implemented / unsupported / invalid_request）。
  - Renderer ↔ Workshop 租户：Bridge（postMessage + 本地 HTTP）+ Session Token；**禁止**放开 `allow-same-origin`。
  - Embed 子 WebView：**禁止**接入 Workshop Session Token / Bridge SDK。
- **为什么**：可审计通道；沙箱与引擎权威才成立。
- **检查方法**：`grep executeJavaScript`；Embed 路径是否误用 Bridge。

---

## 四、IPC 与协议命名

#### R-T5 · Host IPC 用 `domain:action`；Daemon 方法用协议清单
- **等级**：MUST
- **分类**：命名
- **规则**：
  - Host IPC channel **必须** `domain:action`（如 `db:get`、`terminal:create`、`module:install`）。
  - Daemon RPC 方法**必须**登记在 `assistant-protocol` 方法表；能力广告 ⊆ 可调实现。
- **为什么**：权限审计与诚实能力表面依赖稳定命名。
- **检查方法**：新增 command/方法时对照 adapter 与 `ALL_METHODS` / `IMPLEMENTED_METHODS`。

---

## 五、本篇合规自检清单

- [ ] 代码落在正确边界（Host / Daemon / Renderer / 租户），无越权。
- [ ] 未在 Renderer 直连 SQLite / 重 fs / 业务 spawn（R-T1、R-T2）。
- [ ] import 方向上层 → 下层（R-T3）。
- [ ] 跨进程只用 IPC / UDS 协议 / Bridge（R-T4）。
- [ ] 命名符合 R-T5；Daemon 能力无假广告。

---

## 六、关联指南

- 文件/函数规模、状态唯一归属、引擎拆分阈值：[`CODE_MODULE_GUIDELINES.md`](../../architecture/CODE_MODULE_GUIDELINES.md)
- Daemon 域划分：[`NATIVE-DAEMON-CAPABILITY-MAP.md`](../../architecture/NATIVE-DAEMON-CAPABILITY-MAP.md)
- 引擎整改契约与进度：[`NATIVE_ENGINE_FULL_REMEDIATION.md`](../../architecture/NATIVE_ENGINE_FULL_REMEDIATION.md)

冲突时以本 standards 篇的 MUST 为准。
