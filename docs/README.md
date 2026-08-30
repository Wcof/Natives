# Natives 文档索引

> **当前目标**: Chrome Files Workspace（AI Native Personal Workspace 的文件产品入口）
> **冻结决策**: [ADR-0020](./adr/0020-ai-native-personal-workspace-rearchitecture.md)
> **Chromium 文件管理迁移**: [ADR-0023](./adr/0023-chromium-extension-files-surface.md)
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

## 当前迁移事实（2026-08-25 更新）

> 当前唯一生产代码为 `extension/` + `crates/native-file-host` + `crates/file-manager-core`；旧 AI Workspace/Tauri/Daemon 域已删除，相关 ADR 与审计文档仅保留决策和迁移证据，不属于文件扩展生产入口。

- **ADR-0022（Appearance Preference、Workspace 交互与 Apps Web Surface 统一权威）**已接受：
  - 主题持久权威收敛为 Host `settings:theme`（词表 `dark | light`），由 `AppearanceCoordinator` 单一协调；`workspaces.theme` 降为 v30 迁移存根；
  - Workspace 布局手势严格遵循 `idle → draft → commit | rollback`，移动期间写库为 0，有效 stop 单次原子写入；恢复 Structured Grid 标题栏拖拽与八向缩放，Free Canvas 采用屏幕空间恒定缩放手柄；
  - Apps `AppView` 统一 camelCase 序列化契约，`appId` 保持真实唯一身份；Web Surface 运行时由单例 `BrowserStateHandle` 托管生命周期与 LRU 预算。

- Workspace V2 目标架构 ADR-0021（Multi-Workspace + Grid/Canvas 双布局 + Design System V2）已接受，取代 ADR-0020 §1/§3 冲突范围（其余 ADR-0020 决策继续有效）；冻结契约见 `contracts/workspace-v2-contract.md`，Host SQLite 为 Workspace 唯一权威（v27 迁移 7 表已注册），旧 localStorage 权威判定为 delete（`development/g009-localstorage-conclusion.md`）。

- PWSV2 Personal Workspace V2 完整重设计裁决（2026-08-23）：布局模式词表 `structured | free`（structured 为默认，历史词 `compact` 废弃）；旧 `workspace_tabs` 内容 tab 表降为 legacy，新增 `workspace_open_tabs`（Workspace 会话）与 `workspace_templates`（内置/个人模板）；workspaces 软删 `deleted_at` + `default_layout_mode` + `template_source_id/template_version`；widget `enabled`/`config_version`/`appearance`/`z_index`（`enabled = NOT hidden` 映射）；layout `layout_mode`/`layout_version` + `UNIQUE(workspace_id, layout_mode, breakpoint)`；Browse/Edit 双态 + `Classic Personal Dashboard` 内置模板；增量迁移 **v29**（v28 为应用中心迁移）。见 ADR-0021 修订 §PWSV2 与契约修订头。
- 旧 Tauri/Next/Daemon 生产路径已删除；当前文件产品只保留扩展、Native Host 与文件授权内核。
- 首页 `/` 已是 PersonalWorkspace Home（Grid Widget + 5 个默认 Widget 接真实 Domain）；完整 Usage Dashboard 已迁至数据/用量页（`/usage`）；设置个人概览只保留摘要；Sidebar 折叠为 64px Icon Rail（含数据/用量入口）。
- P0-A 已通过：三协议 fixture/transport、Key Pool、SecretStore、secret scan（PASS）；P0-B 逐文件审计已落档（`provider-adapters-p0b-audit.md`）。
- Legacy 死亡清单保留为删除证据；Tauri/Next/Daemon、Agent、Jobs、Capabilities、Workshop/Plugin Runtime 生产代码已删除，不得重新引入。
- 当前剩余门禁属于发布证据：Chrome Web Store 真实 ID、平台签名/公证、Windows 实机安装验证及跨平台发布检查；不构成文件代码闭环阻塞。

## 按任务速查

| 任务 | 先读 |
|---|---|
| 任何编码 | `standards/README.md` + 相关 1–3 篇 |
| 全局产品/IA/Legacy | ADR-0020 + `standards/product/01-positioning.md` |
| 主题偏好与外观协调 | **ADR-0022** + `contracts/appearance-preference-contract.md` + `standards/ui-ux/01` + `standards/frontend/02` (R-E7.1) |
| 多 Workspace/Grid/Canvas/Widget/Data View | **ADR-0021** + **ADR-0022** + `contracts/workspace-v2-contract.md` + `standards/ui-ux/02` + `standards/technical/04` (R-P11) |
| Apps / Web Surface 运行时 | **ADR-0022** + `contracts/apps-web-surface-contract.md` + `standards/technical/01` (R-T5) + `standards/technical/05` (R-B12) |
| P0 Proxy/OAuth/Secret | `architecture/provider-proxy-architecture.md` + technical 01/02/03/05 |
| P0 provider-adapters 逐文件处置 | `architecture/provider-adapters-p0b-audit.md`（Keep/Extract/Rewrite/Delete 矩阵） |
| Legacy 删除 | `architecture/legacy-death-list.md`（死亡证明 + 待 cutover 路径） |
| Home/Grid/Widget/Sidebar | Home Workspace Patch 的 00/08/13/16/20/24/31/33 + frontend/performance/ui-ux standards |
| Files | `architecture/FILE_MANAGER_AUDIT.md` + data/backend/performance standards |
| Apps / 应用中心 V2 | `pm-context/apps-center-ai-requirements.md` + `creative-app-*` 现状文档 + layering/security/backend standards |
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
