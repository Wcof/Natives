# Natives 规范体系（Standards）

> 本目录是 Natives 所有 MUST / SHOULD / MAY 的唯一权威。
> 当前产品与架构冻结见 [ADR-0020](../adr/0020-ai-native-personal-workspace-rearchitecture.md)。
> 冲突顺序：`standards/` > ADR-0020 > 其它 ADR > `architecture/` 现状 > 历史文档。

## 文档地图

```text
docs/standards/
├── README.md
├── 00-glossary.md
├── product/
│   ├── 01-positioning.md        AI Native Personal Workspace / IA / domains / legacy
│   └── 02-feature-spec.md       honest state / target capabilities / hard gates
├── technical/
│   ├── 01-layering.md           Host-default authority / sidecar / migration
│   ├── 02-security.md           process / surface defenses / OS Keychain secrets
│   ├── 03-data.md               SQLite / migration / atomic writes
│   ├── 04-performance.md        budgets / lifecycle / bounded growth
│   └── 05-backend.md            Rust errors / logs / deep shared modules / supervisor
├── frontend/
│   ├── 01-structure.md
│   ├── 02-state-and-data.md
│   └── 03-i18n.md
└── ui-ux/
    ├── 01-design-tokens.md
    ├── 02-interaction.md
    └── 03-feedback.md
```

## 使用规则

1. 任何改动先读本页与相关 1–3 篇。
2. 放宽或废除 MUST 必须先写 ADR，再同步 Standards。
3. Architecture 文档描述当前代码；不得用“当前仍存在”推翻 Target。
4. Legacy Assistant / Agent / Jobs / Capabilities / Daemon / Plugin Runtime 只允许安全、迁移和删除工作。
5. 新建 module/dependency 前必须先审计现有 Files/App/Provider/Usage/UI 资产。

## 任务速查

| 任务 | 必读 |
|---|---|
| 产品 IA / Domain | `product/01` + ADR-0020 |
| 状态/验收/无假数据 | `product/02` |
| Host / Sidecar / IPC | `technical/01` + `technical/05` |
| Secret / Keychain / OAuth | `technical/02` + `technical/03` |
| Proxy / Provider codec | `technical/01` + `technical/02` + `technical/05` + `architecture/provider-proxy-architecture.md` |
| Home / Widget / Sidebar | `product/01` + `frontend/01-03` + `technical/04` + `ui-ux/` |
| Files / Apps lifecycle | `technical/01` + `technical/03-05` |
| Performance | `technical/04`，必须有同设备前后证据 |
| Legacy removal | ADR-0020 + `technical/01` R-T6 + `product/02` death proof |

## RFC 2119

| 关键词 | 含义 | 违反后果 |
|---|---|---|
| **MUST** | 绝对约束 | 禁止合入；偏离需先补 ADR |
| **SHOULD** | 强约定 | 可偏离，但必须记录理由 |
| **MAY** | 建议 | 不强制 |

未标强度的规则按 SHOULD。

## 全局合规自检

- [ ] 新功能属于 Home / Files / Apps / AI / Data & Usage / Settings。
- [ ] Tauri Host 是默认 owner；Sidecar 有真实隔离/生命周期理由且受监督。
- [ ] 无 Renderer 直连 SQLite/文件重 IO/进程/Provider/Secret。
- [ ] Secret 在 OS Keychain；迁移幂等、可恢复、可回滚。
- [ ] 用户可见数据有真实来源，loading/empty/error/unsupported 分离。
- [ ] 中英文文案同步，无硬编码用户可见文本。
- [ ] 无 per-widget query/timer、pointer-move DB write 或无界资源增长。
- [ ] 无新增 Agent/Capability/Job/Plugin runtime；legacy cutover 有 death proof。
- [ ] 文件/函数规模符合 `CODE_MODULE_GUIDELINES` 或有明确例外。
