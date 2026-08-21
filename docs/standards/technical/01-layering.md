# 技术架构 01 · 进程、分层与通信

> **版本**: 3.0.0 · **日期**: 2026-08-19
> **关联 ADR**: [ADR-0020](../../adr/0020-ai-native-personal-workspace-rearchitecture.md)、[ADR-0008](../../adr/0008-electron-to-tauri-migration.md)
> **迁移说明**: Agent Daemon/UDS 仍存在于当前源码，但不再是目标 authority；只允许迁移与删除所需变更。

## 一、Authority

#### R-T1 · Tauri Host 是默认 Native Backend
- **等级**：MUST
- **分类**：分层、进程、安全
- **规则**：

| 运行域 | 目标 authority | 允许 | 禁止 |
|---|---|---|---|
| Tauri Host (`src-tauri/`) | 本机 DB、Files、Apps、Provider/Connection/Credential、Proxy、AI Tool Integration、Usage 编排、窗口/进程/PTY | typed commands、领域 module、受监督 sidecar | UI/DOM、无监督进程、第二数据 authority |
| Renderer (`src/`) | UI 投影与交互状态 | typed adapter/IPC、事件订阅、普通 React Widget | SQLite、重文件 IO、业务 spawn、Secret、直接 Provider HTTP |
| 外部 Surface | iframe/WebView/外部工具等独立信任域 | 最小授权通信 | 获得不属于其 trust domain 的 Bridge/Token/DB/Secret |

  目标生产主链**必须**是 `Renderer → Tauri Host → Domain module`。Sidecar 只有在真实独立生命周期、崩溃隔离或第三方 runtime 要求下才允许，并**必须**由 Host supervisor 管理。

  Agent Daemon 是迁移期 legacy。新功能**禁止**增加对 Daemon、Harness、Agent crates 或 Capability Gateway 的依赖；production cutover **禁止**长期保留 Host/Daemon 双执行或静默 fallback。
- **为什么**：个人桌面应用应让进程复杂度对应真实隔离需求，而不是内部逻辑分层。
- **检查方法**：新增 capability 是否可直接落 Host；新增 sidecar 是否有生命周期 ADR、supervisor、shutdown 与回滚测试。

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
- **为什么**：通信通道同时决定权限、生命周期与可观测性。

#### R-T5 · 命名与协议 Source of Truth
- **等级**：MUST
- **分类**：命名、协议
- **规则**：Host command/channel **必须**使用稳定的 `domain:action` 语义或项目生成绑定约定。仍存在的 Host↔sidecar wire type **必须**只有一个 Rust Source of Truth，前端 binding 由生成/同步检查维护，禁止手写影子类型。
- **为什么**：迁移期更需要避免新旧契约漂移。

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
