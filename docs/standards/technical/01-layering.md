# 技术架构 01 · 进程、分层与通信

> **版本**: 3.0.0 · **日期**: 2026-08-19
> **关联 ADR**: [ADR-0020](../../adr/0020-ai-native-personal-workspace-rearchitecture.md)、[ADR-0008](../../adr/0008-electron-to-tauri-migration.md)
> **适用范围**: 当前生产范围为 Chrome 扩展、Files Native Host/Core，以及 ADR-0020 §6.1 的单用途 Model Host。旧 Tauri/Renderer/Daemon 说明仅适用于历史迁移与删除，不构成恢复许可。

## 一、Authority

#### R-T1 · Native authority 与已接受的生产范围
- **等级**：MUST
- **分类**：分层、进程、安全
- **规则**：当前 Files 主链遵循下方 ADR-0023，Model Host 遵循 ADR-0020 §6.1；扩展页面是唯一产品 Surface。下表及 Tauri 主链记录历史架构，仅在审计、迁移或删除相应历史实现时适用。

| 运行域 | 目标 authority | 允许 | 禁止 |
|---|---|---|---|
| Tauri Host (`src-tauri/`) | 本机 DB、Files、Apps、Provider/Connection/Credential、Proxy、AI Tool Integration、Usage 编排、窗口/进程/PTY | typed commands、领域 module、受监督 sidecar | UI/DOM、无监督进程、第二数据 authority |
| Renderer (`src/`) | UI 投影与交互状态 | typed adapter/IPC、事件订阅、普通 React Widget | SQLite、重文件 IO、业务 spawn、Secret、直接 Provider HTTP |
| 外部 Surface | iframe/WebView/外部工具等独立信任域 | 最小授权通信 | 获得不属于其 trust domain 的 Bridge/Token/DB/Secret |

  历史 Tauri 实现的主链**必须**是 `Renderer → Tauri Host → Domain module`。Sidecar 只有在真实独立生命周期、崩溃隔离或第三方 runtime 要求下才允许，并**必须**由 Host supervisor 管理。

  Agent Daemon 是迁移期 legacy。新功能**禁止**增加对 Daemon、Harness、Agent crates 或 Capability Gateway 的依赖；production cutover **禁止**长期保留 Host/Daemon 双执行或静默 fallback。
- **为什么**：个人桌面应用应让进程复杂度对应真实隔离需求，而不是内部逻辑分层。
- **检查方法**：新增 capability 是否可直接落 Host；新增 sidecar 是否有生命周期 ADR、supervisor、shutdown 与回滚测试。

**Files V1 例外（ADR-0023）**：正式 Chrome 文件 Surface 的主链为
`Chrome Extension Page → Native Messaging → native-file-host → file-manager-core`，不经过 Tauri。
该 Host 必须由 Chrome 按需启动、由文件页面端口拥有生命周期，禁止成为 daemon、开机启动项或
Service Worker 长连接。旧 Tauri Files 调用链是迁移源，不得继续作为生产 fallback。

**Model Host 例外（ADR-0020 §6.1）**：扩展通过独立 Native Messaging interface 调用单用途 `model-host`，不得共享 Files 能力。默认由页面连接拥有生命周期；仅用户显式开启常驻后可跨页面关闭存活，必须单实例、可停止且无登录自启动。Gateway 仅绑定 `127.0.0.1` 并鉴权，禁止 Management API；持久 Secret 由 OS Keychain 持有。该例外不授权通用 Daemon 或新的产品 Surface。

**Extension App 例外（ADR-0026，取代 ADR-0025 独立 Host）**：Extension App 页面直接复用 `native-file-host` 的受限 Apps 领域接口；禁止注册 `com.natives.app.*` Native Messaging Host、下载 `runtime` 包或启动 App 子进程。App UI/业务逻辑随扩展发布，下载包只含只读 `data` / `resource`；新增系统能力必须随 Core Host 更新并重新审查。

#### R-T2 · 数据 authority 单一
- **等级**：MUST
- **分类**：数据、安全
- **规则**：`natives.db` 及目标领域 schema **必须**只由 Host 迁移和写入；Renderer **禁止**直开 SQLite。Legacy Daemon 数据在迁移完成前仍由旧 owner 管理，但**禁止**新增目标产品表或新双写。跨库迁移必须有 checkpoint、幂等、回滚和最终单一 owner。
- **为什么**：双写和迁移版本竞争会破坏可恢复性。

## 二、模块分层

```text
Application  ── Home / Files / Apps / AI / Data / Settings
     ↓
Framework    ── Shell / typed frontend adapters / surface hosting
     ↓
Domain       ── Files / Apps / AI Resources / Proxy / AI Tools / Usage
     ↓
Infrastructure ── Tauri / SQLite / Keychain / PTY / HTTP / supervised process
```

#### R-T3 · 依赖只能向下，领域互调走明确 interface
- **等级**：MUST
- **分类**：分层
- **规则**：下层**禁止** import 上层；Framework **禁止**硬依赖具体页面；Widget **禁止**成为跨领域聚合 backend。共享 codec/算法可放 `crates/`，但不得为了共享重建通用 runtime。
- **为什么**：让变化、测试和删除集中在正确 module。

## 三、通信

#### R-T4 · 跨信任域只走受控通道
- **等级**：MUST
- **分类**：进程、安全
- **规则**：
  - Renderer ↔ Host：Tauri command/event + typed adapter；禁止 `executeJavaScript` 注入业务逻辑。
  - Host ↔ 必要 sidecar：版本化协议、鉴权、大小/超时限制、取消与 shutdown；协议能力必须诚实。
  - Legacy Workshop iframe：删除前继续遵守 postMessage source 验证、Session Token 与 sandbox 红线。
  - Embed/WebView：禁止获得 Workshop Bridge/Session Token。
  - Chrome Surface：`files.html`、`space.html`（ADR-0024 短连接）、`app.html` 与 `apps.html`（ADR-0025）可直接持有 Native Port，生命周期一律为按需连接 + `pagehide` 断开 + 隐藏空闲 60 秒断开；Service Worker 禁止持有、轮询或保活任何 Native Host。
- **为什么**：通信通道同时决定权限、生命周期与可观测性。

#### R-T5 · 命名与协议 Source of Truth
- **等级**：MUST
- **分类**：命名、协议
- **规则**：
  - Host command/channel **必须**使用稳定的 `domain:action` 语义或项目生成绑定约定。
  - Rust 后端结构体是 DTO 单一 Source of Truth；前端 TS 绑定由 `ts-rs` 自动生成，**严禁**手写影子接口。
  - Web Surface 运行时状态（打开、复用、隐藏、预算控制、关闭）**必须**由 Tauri 托管的单一 `BrowserStateHandle` 统一管理，**禁止**命令每次新建局部实例。
- **为什么**：迁移期与多 Surface 管理中，协议与状态实例漂移会导致主键失效和资源泄露。

## 四、迁移 Gate

#### R-T6 · Authority 切换必须一次完成并可回滚
- **等级**：MUST
- **分类**：版本、进程
- **规则**：每个 legacy authority 切换**必须**先完成 parity fixture、真实调用链、资源回收、数据迁移与 rollback；切换同批删除旧 production caller/fallback。不能把新 facade 永久代理到旧 runtime 后宣称完成。
- **为什么**：双 production path 会让故障、Secret 和状态所有权不可判定。

## 五、合规自检

- [ ] 默认落在 Host；Sidecar 有真实隔离/生命周期理由并受监督。
- [ ] Renderer 不直连 SQLite、文件重 IO、进程、Provider 或 Secret。
- [ ] 数据只有一个写 authority，无新增 legacy schema/双写。
- [ ] 跨域通信有 typed contract、鉴权、大小/超时/取消/关闭。
- [ ] legacy cutover 有 parity、rollback 与 death proof。
