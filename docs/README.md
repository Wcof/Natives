# Natives 文档索引

> **当前目标**: AI Native Personal Workspace
> **冻结决策**: [ADR-0020](./adr/0020-ai-native-personal-workspace-rearchitecture.md)
> **原则**: 约束进 `standards/`；决策进 `adr/`；当前实现与迁移证据进既有 `architecture/` 文档。

## 权威顺序

| 优先级 | 路径 | 角色 |
|---|---|---|
| 1 | [`standards/`](./standards/README.md) | 当前 MUST / SHOULD / MAY |
| 2 | [ADR-0020](./adr/0020-ai-native-personal-workspace-rearchitecture.md) | 产品、IA、Host authority、Secret、Legacy 冻结 |
| 3 | [`adr/`](./adr/) 其它 ADR | 仍未被 ADR-0020 取代的决策与历史 |
| 4 | [`architecture/`](./architecture/) | 当前源码现状、迁移设计、测试证据 |
| 5 | Home Workspace Patch（2026-08-19） | Home / Widget / Navigation 专项方案 |
| 6 | 历史讨论与研究 | 仅溯源，不驱动新实现 |

## 目标产品地图

```text
AiNative
├── 首页                 多 Workspace Personal Home（ADR-0021：Workspace 生命周期 + Grid/Canvas 双布局 + Widget Catalog）
├── 文件                 CRUD / Trash / Watch / Search
├── 应用                 App / RuntimeSpec / RuntimeInstance / Surface
├── AI
│   ├── AI Resources     Provider / Connection / Credential
│   ├── Local Proxy      Messages / Chat / Responses / Key Pool
│   └── AI Tools         Claude Code / Codex / Gemini / OpenCode
├── 数据与用量           Usage / Cost / Status
└── 设置                 General / Appearance / AI / Personal summary
```

## 当前迁移事实（2026-08-22 更新）

- Workspace V2 目标架构 ADR-0021（Multi-Workspace + Grid/Canvas 双布局 + Design System V2）已接受，取代 ADR-0020 §1/§3 冲突范围（其余 ADR-0020 决策继续有效）；冻结契约见 `contracts/workspace-v2-contract.md`，Host SQLite 为 Workspace 唯一权威（v27 迁移 7 表已注册），旧 localStorage 权威判定为 delete（`development/g009-localstorage-conclusion.md`）。

- 新 IA 目标边界已落地：`src-tauri/src/apps/`（App/RuntimeSpec/RuntimeInstance/Surface）、`src-tauri/src/ai/`（Provider/Connection/Credential/Model + secret_ref）、`src-tauri/src/proxy/`（Listener/Route/可替换 ProxyEngine trait）、`src-tauri/src/integrations/`（AI Tool 七步契约）、`src-tauri/src/secrets/`（OS Keychain SecretStore + 迁移状态机）、`src-tauri/src/key_pool.rs`（Key Pool/Failover）。
- 首页 `/` 已是 PersonalWorkspace Home（Grid Widget + 5 个默认 Widget 接真实 Domain）；完整 Usage Dashboard 已迁至数据/用量页（`/usage`）；设置个人概览只保留摘要；Sidebar 折叠为 64px Icon Rail（含数据/用量入口）。
- P0-A 已通过：三协议 fixture/transport、Key Pool、SecretStore、secret scan（PASS）；P0-B 逐文件审计已落档（`provider-adapters-p0b-audit.md`）。
- Legacy 死亡清单已建立（`architecture/legacy-death-list.md`）；`examples/minimal-agent` 与 `extension-host` 已删除（无生产引用，编译/类型检查无破坏）。
- 剩余生产切换项（ADR-0020 P0 parity cutover 后执行）：Host ProxyEngine 生产执行、Daemon/Agent crates/前端 legacy 页面删除、`provider_kek`/`env_encryption_key` 迁出 SQLite、packaged Tauri/WebKit 与完整 verify:native-engine 证据链。
- 源码仍包含 Assistant、Jobs、Capabilities、Agent Daemon、Agent crates、Workshop/Plugin Runtime 残留；这些是待迁移 Legacy（见 death-list 第 2 节），不是新产品入口。

## 按任务速查

| 任务 | 先读 |
|---|---|
| 任何编码 | `standards/README.md` + 相关 1–3 篇 |
| 全局产品/IA/Legacy | ADR-0020 + `standards/product/01-positioning.md` |
| 多 Workspace/Grid/Canvas/Widget/Data View | **ADR-0021**（取代 ADR-0020 §1/§3 冲突部分）+ `contracts/workspace-v2-contract.md` + `standards/product/01-02` + `standards/ui-ux/02`（§5-2、§6）+ `architecture/application-visual-experience-remediation.md`（§12 Workspace 画布组件整改）+ `standards/technical/05`（R-B10） |
| P0 Proxy/OAuth/Secret | `architecture/provider-proxy-architecture.md` + technical 01/02/03/05 |
| P0 provider-adapters 逐文件处置 | `architecture/provider-adapters-p0b-audit.md`（Keep/Extract/Rewrite/Delete 矩阵） |
| Legacy 删除 | `architecture/legacy-death-list.md`（死亡证明 + 待 cutover 路径） |
| Home/Grid/Widget/Sidebar | Home Workspace Patch 的 00/08/13/16/20/24/31/33 + frontend/performance/ui-ux standards |
| Files | `architecture/FILE_MANAGER_AUDIT.md` + data/backend/performance standards |
| Apps | `creative-app-*` 现状文档 + layering/security/backend standards |
| Usage/Data | `application-performance-remediation.md` + product/02 + performance standard |
| 旧 Daemon/Agent 删除 | `NATIVE-DAEMON-CAPABILITY-MAP.md`、`NATIVE_ENGINE_FULL_REMEDIATION.md` 仅作引用清单 + ADR-0020 death proof |
| Release Gate | `development/natives-agent-t12-release-gate-report.md`（旧基线）+ ADR-0020 新 Gate；不得把旧绿灯当新架构完成 |

## 目录角色

- `standards/`: 当前可执行约束。
- `adr/`: 决策与取代关系。
- `architecture/`: 当前源码和迁移设计；必须明确 Target 与 Legacy。
- `development/`: 构建、发布与运行策略。
- `harness/`, `superpowers/`: 历史 Agent/Harness 研究，只用于 Legacy 审计和删除，不用于新增能力。
- `img/`: 文档图片。

## 变更纪律

1. 放宽/废除 MUST：先 ADR，再同步 Standards。
2. 产品目标/IA/authority：修订 ADR-0020，并同步 product/layering/security。
3. 迁移证据：更新现有领域 architecture 文档，不新建重复进度快照。
4. 安全防线在旧运行时删除前继续有效，不因“未来会删除”提前放宽。
5. 任何完成声明必须区分 current production、isolated Spike、partial 与 target。
