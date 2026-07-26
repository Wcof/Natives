# Natives 文档索引

> **最后整理**: 2026-07-26  
> **原则**: 约束进 `standards/`；决策进 `adr/`；现状与领域设计进 `architecture/`。冲突时 **standards > ADR（产品冻结类）> architecture 现状描述 > 历史讨论**。

---

## 权威优先级

| 优先级 | 路径 | 角色 |
|--------|------|------|
| 1 | [`standards/`](./standards/README.md) | **约束唯一权威**（MUST/SHOULD/MAY） |
| 2 | [`adr/0012-product-identity-workshop-scope.md`](./adr/0012-product-identity-workshop-scope.md) | 产品身份、三面、双轨、P0–P2 冻结 |
| 3 | [`adr/`](./adr/) 其余 | 架构决策记录 |
| 4 | [`architecture/`](./architecture/) | 现状描述、领域冻结设计、引擎契约 |
| 5 | 根 `CLAUDE.md` / `AGENTS.md` | AI 协作入口（指向本目录） |

历史 PRD、FanBox 迁移动子蓝图、一次性 G 系列审计、过期整改快照已于 2026-07-23 清理，不再作为依据。

---

## 目录地图

```text
docs/
├── README.md                 ← 你在这里
├── standards/                ← 约束（必须遵守）
│   ├── README.md
│   ├── 00-glossary.md
│   ├── product/              定位、无假数据、功能治理
│   ├── technical/            分层、安全、数据、性能、后端
│   ├── frontend/             结构、状态、i18n
│   └── ui-ux/                令牌、交互、反馈
├── adr/                      ← 决策（为什么）
│   ├── 0001 … 0011           安全/主题/引擎/迁移等
│   ├── 0012-…                产品身份冻结 ⭐
│   ├── 0013-…                创意双来源（内部+GitHub 容器）
│   └── 0014-…                创作台 P0 主流程冻结（AI 生成主流程）
├── architecture/             ← 现状与领域设计（描述，非红线）
│   ├── ARCHITECTURE.md       总览（已对齐 ADR-0012）
│   ├── DESIGN_DISCUSSION.md  历史 Q&A（被 ADR 修订处见文首）
│   ├── module-workshop-kernel-runtime.md   Workshop KI-1…5
│   ├── CODE_MODULE_GUIDELINES.md           模块边界与规模
│   ├── creative-app-github-container-install.md
│   ├── creative-app-creator-workbench.md   创作台落地设计（决策见 ADR-0014）
│   ├── creative-app-local-remediation.md   本地项目 gap 整改设计（9 项，B1–B4/F5–F9）
│   ├── creative-app-local-project-remediation.md  同域收敛现状与验收（承接上行 gap 设计，基线 deploy@4f130256）
│   ├── provider-routing-sub2api.md         供应商路由与 Sub2API 账号池
│   ├── application-performance-remediation.md     全应用性能整改记录
│   ├── FILE_MANAGER_AUDIT.md               文件管理器审计（对照 fanbox，Hub 面）
│   ├── NATIVE-DAEMON-CAPABILITY-MAP.md
│   ├── NATIVE_ENGINE_FULL_REMEDIATION.md   引擎契约与进度（唯一进度源）
│   └── NATIVE_ENGINE_ENV.md
├── superpowers/              ← harness 控制面设计稿
│   └── specs/2026-07-26-native-harness-control-plane-design.md
└── img/                      说明性截图
```

---

## 按任务速查

| 你在做什么 | 先读 |
|------------|------|
| 任何编码 | `standards/README.md` + 相关 1–3 篇 |
| 产品边界 / 商店 / 工坊 | ADR-0012 + `standards/product/01-positioning.md` |
| 安全 / iframe / Token | `standards/technical/02-security.md` + ADR-0001/0002/0006 |
| 分层 / Daemon / IPC | `standards/technical/01-layering.md` + `CODE_MODULE_GUIDELINES.md` |
| Workshop 内核 | `module-workshop-kernel-runtime.md` |
| 外部 GitHub 容器应用 | ADR-0013 + `creative-app-github-container-install.md` |
| 创作台（AI 生成主流程） | ADR-0014 + `creative-app-creator-workbench.md` |
| 后端 Rust 编码 | `standards/technical/05-backend.md` |
| 性能改动 | `standards/technical/04-performance.md` + `application-performance-remediation.md` |
| Provider 路由 / 账号池 | `provider-routing-sub2api.md` |
| Agent 引擎能力与整改 | `NATIVE_ENGINE_FULL_REMEDIATION.md` + `NATIVE-DAEMON-CAPABILITY-MAP.md` |
| 历史决策溯源 | `DESIGN_DISCUSSION.md`（以 ADR 修订为准） |

---

## 变更纪律

1. **改约束** → 改 `standards/`；放宽 MUST 必须先 ADR。  
2. **改产品身份/面轨** → 修订或新增 ADR，再同步 `product/01-positioning.md` 与 `ARCHITECTURE.md` 概述。  
3. **改引擎进度** → 只更新 `NATIVE_ENGINE_FULL_REMEDIATION.md`（及必要的 capability map），**禁止**再复制多份进度快照。  
4. **禁止**再往 `docs/` 根目录堆 FanBox 遗留、一次性审计报告、agent task pack、atoms 蓝图；一次性材料用完即删或勿入库。
