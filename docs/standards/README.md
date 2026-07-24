# Natives 规范体系（Standards）

> **本目录是 Natives 项目所有「约束」的唯一权威来源。**  
> 当本目录与其它文档（`architecture/` 现状描述、历史讨论、根 README 等）冲突时，**以本目录为准**。  
> 产品身份与工坊边界的决策冻结见 [ADR-0012](../adr/0012-product-identity-workshop-scope.md)；本目录将其落成可执行规则。  
> 完整文档地图见 [docs/README.md](../README.md)。

---

## 为什么需要这套规范

约束与「现在碰巧怎么实现」必须分开。本目录只收 **MUST / SHOULD / MAY**；架构现状、进度、历史 Q&A 放在 `architecture/` 与 `adr/`。

- **对人**：一眼看清红线。  
- **对 AI**：每条规则可解析、可审查，降低协作偏航。

---

## 文档地图

```text
docs/standards/
├── README.md                          ← 你在这里
├── 00-glossary.md                     ← 术语 + RFC 2119 关键词
├── product/                           ← 产品架构规范
│   ├── 01-positioning.md                身份、三面/双轨、阶段、内置封闭集合
│   └── 02-feature-spec.md               优先级、无假数据、错误展示
├── technical/                         ← 技术架构规范
│   ├── 01-layering.md                   四类边界、分层依赖、IPC/协议
│   ├── 02-security.md                   五大防线 + 安全红线
│   └── 03-data.md                       SQLite / 命名空间 / 迁移 / 凭证
│   └── 04-performance.md                性能预算、主线程与增长边界
├── frontend/                          ← 前端架构规范
│   ├── 01-structure.md                  目录 / 命名 / 组件分层
│   ├── 02-state-and-data.md             状态 / IPC / 无假数据
│   └── 03-i18n.md                       双语同步
└── ui-ux/                             ← UI / UE
    ├── 01-design-tokens.md
    ├── 02-interaction.md
    └── 03-feedback.md
```

**关联架构文档（非规范；功能域任务须同步阅读）：**

| 领域 | 文档 | 关联规范 |
|------|------|----------|
| 模块管理 / 创意工坊 | [`module-workshop-kernel-runtime.md`](../architecture/module-workshop-kernel-runtime.md) | technical/02 · 03 · frontend/02 |
| 代码模块 / 文件规模 | [`CODE_MODULE_GUIDELINES.md`](../architecture/CODE_MODULE_GUIDELINES.md) | technical/01 · frontend/01 |
| Daemon 能力域 | [`NATIVE-DAEMON-CAPABILITY-MAP.md`](../architecture/NATIVE-DAEMON-CAPABILITY-MAP.md) | technical/01 |
| 引擎整改契约 | [`NATIVE_ENGINE_FULL_REMEDIATION.md`](../architecture/NATIVE_ENGINE_FULL_REMEDIATION.md) | technical/01 · product/02 |
| 外部容器应用 | [ADR-0013](../adr/0013-creative-app-dual-source.md) + design 文 | product/01 · technical/02 |

共 14 篇规范 + 关联架构指南。**新增功能或重构必须先检索相关规范篇。**

---

## 如何使用

### 人类协作者

1. 动手前：文档地图定位 1–3 篇通读。  
2. 动手中：「能不能这么写」查规范，不凭直觉。  
3. 提交前：过文末与本页「合规自检清单」。

### AI 协作者

- 任务开始时加载本 README + 相关规范篇。  
- 规则结构见 `00-glossary.md`。  
- 必然违反 **MUST** 时：**停止并向用户确认**，或先补 ADR 再实施。

---

## 规则强度（RFC 2119 精简）

| 标签 | 含义 | 违反后果 |
|------|------|----------|
| **MUST** / 必须不 | 绝对约束 | 禁止合入；视为缺陷 |
| **SHOULD** / 应该 | 强约定，可例外 | PR/ADR 说明偏离 |
| **MAY** / 可以 | 建议 | 不强制 |

---

## 规则书写格式

```text
#### R-[序号] [规则标题]
- **等级**：MUST | SHOULD | MAY
- **分类**：安全 | 数据 | i18n | 无假数据 | …
- **规则**：…
- **正例** / **反例** / **为什么** / **检查方法**
```

最低要求：**等级 + 规则 + 为什么**。

---

## 变更流程

1. **小改**（措辞、正反例、SHOULD/MAY）：随业务 PR，`docs(standards): …`。  
2. **大改**（新增/废除 MUST、改架构边界）：先 ADR，再改规范「关联 ADR」与正文。  
3. **禁止**无 ADR 静默放宽 MUST。

---

## 功能域映射

| 功能域 | 核心架构文档 | 关联规范 | 关键不变量 |
|--------|-------------|---------|-----------|
| 模块管理 / 创意工坊 | `module-workshop-kernel-runtime.md` | technical/02·03 · frontend/02 | KI-1…KI-5 |
| 助理 / Native 引擎 | `NATIVE_ENGINE_FULL_REMEDIATION.md` · capability map | technical/01 · product/02 | 广告 ⊆ 可调；Host/Daemon 权威分立 |
| 个人创意双来源 | ADR-0013 · creative-app design | product/01 · technical/02 | 管理面统一、运行时分轨 |
| 全应用性能 | `application-performance-remediation.md` | technical/04 | 预算可测、增长有界、可见性门控 |

---

## 与现有文档的对应

| 文档 | 角色 | 对应 |
|------|------|------|
| `docs/architecture/ARCHITECTURE.md` | 架构现状描述 | technical / frontend / product 概述对齐 |
| `docs/architecture/DESIGN_DISCUSSION.md` | 历史 Q&A | 仅溯源；被 ADR-0012 等修订 |
| `docs/adr/*` | 决策记录 | 各篇「关联 ADR」 |
| 根 `CLAUDE.md` / `AGENTS.md` | AI 协作入口 | 顶部指向本目录 |
| ~~PRD / FanBox / atoms / G 系列~~ | 已清理 | 不再引用 |

---

## 合规自检清单（提交前）

- [ ] 读过与本次改动相关的规范篇（≥1）。  
- [ ] 无假数据 / 占位零值 / 无来源字段（`product/02`）。  
- [ ] 用户可见文案中英文同步（`frontend/03`）。  
- [ ] 未破坏五大防线（`technical/02`）。  
- [ ] 已声明 Hub / Workshop / Embed 与（若适用）web-module / capability（`product/01`）。  
- [ ] 若违反 MUST，已写 ADR。  
- [ ] 涉及工坊时校验 KI-1…KI-5。  
- [ ] 涉及引擎时能力广告 ⊆ 可调实现。
- [ ] 性能改动提供同设备前后证据，并通过 `npm run perf:check`（`technical/04`）。
