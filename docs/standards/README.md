# Natives 现行规范体系

> 版本：4.1.0 · 维护日期：2026-10-04
> 本目录是 Natives 当前所有 MUST / SHOULD / MAY 的唯一规范权威。
> 架构主干以 ADR-0031 为准；ADR-0035～0038 分别补充动态端口、空间投资卡、通用模块推理代理和模块 Secret／深链能力。其他 ADR 按其有效范围适用，已被替代的正文只供历史追溯；ADR-0032 的固定 App Runtime 端口已退役，仅保留端口登记要求。
> ADR-0039 补充包内模块边界、公共契约与验收，不改变现有产品、进程及安全边界；稳定规则见技术 08。

## 当前产品边界

Natives 是一个完整产品。当前生产代码只包括：

- Chrome/Chromium Extension 页面；
- ADR-0042 的认证本机 Web 页面，与扩展共用前端、配置和领域 writer；
- `native-file-host` + `file-manager-core`；
- 单用途 `model-host`；
- 静态编译进统一 `natives-app-runtime`（ADR-0031）、随完整安装包交付的官方内置模块（`modules/`）；
- 只负责选择扩展／本机网页、打开 Chrome、扩展引导和简短诊断的薄 Launcher。

Tauri/Next/React 工作台、通用 Daemon、Assistant、Agent、Jobs、Capabilities、Harness、
Workshop 和 Plugin Runtime 已退出当前架构。它们的历史设计只保留在 superseded ADR、
Git 历史和 `docs/architecture/legacy-death-list.md`，不得作为实现依据。

## 文档地图

```text
docs/standards/
├── README.md
├── 00-glossary.md
├── product/
│   ├── 01-positioning.md
│   └── 02-feature-spec.md
├── technical/
│   ├── 01-layering.md
│   ├── 02-security.md
│   ├── 03-data.md
│   ├── 04-performance.md
│   ├── 05-backend.md
│   ├── 05-port-registry.md
│   ├── 06-built-in-modules.md
│   ├── 07-built-in-app-development.md
│   └── 08-module-boundaries-and-contracts.md
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

1. 修改前先读本页，再按项目 AGENTS.md 的任务表读取所有涉及领域的规范正文与现行契约／ADR，不以篇数限制跳过跨领域要求。
2. 放宽或废除当前 MUST 必须先写 ADR；清除已被接受 ADR 取代的历史规则不算放宽。
3. `docs/standards/` 只放现行规则，不保存历史副本。
4. ADR 记录决策原因；`docs/architecture/` 记录当前实现和验证；`docs/archive/` 只保存历史材料。
5. 新 module/dependency 前先审计现有 Files、Apps、Provider、Usage 和 UI 资产。
6. 不允许新旧生产链双执行、双写或静默 fallback。

## 任务速查

| 任务 | 必读 |
|---|---|
| 产品、IA、状态 | `product/01-positioning.md` + `product/02-feature-spec.md` |
| Extension 结构、状态、i18n | `frontend/01-03` |
| Files / Workspace | `technical/01` + `technical/03-05` |
| Model / Usage / Keychain | `technical/01-05` + ADR-0030 |
| 应用中心 / 内置模块 | `technical/06-built-in-modules.md` + `technical/07-built-in-app-development.md` + `technical/08-module-boundaries-and-contracts.md` + `technical/02-05` |
| 模块拆分、内部 API、SDK 适配与依赖门禁 | `technical/08-module-boundaries-and-contracts.md` + ADR-0039；再读涉及的分层、数据、性能与前端正文 |
| 本地端口申请与登记 | `technical/05-port-registry.md` + ADR-0032/0033/0035 |
| 内置模块 UI 开发 | `technical/07-built-in-app-development.md` |
| 性能与内存 | `technical/04-performance.md` |
| UI / 空间组件 | `ui-ux/01-03`；空间内组件永远消费空间局部样式角色 |
| 安装与完整产品 | ADR-0029 + `technical/02` + `technical/06-built-in-modules.md` |

## RFC 2119

| 关键词 | 含义 | 违反后果 |
|---|---|---|
| **MUST** | 绝对约束 | 禁止合入；偏离须先补 ADR |
| **SHOULD** | 强约定 | 可偏离，但必须记录理由 |
| **MAY** | 建议 | 不强制 |

未标强度的规则按 SHOULD。

## 全局合规自检

- [ ] 新功能属于 Home / Files / Apps / AI / Data & Usage / Settings。
- [ ] UI 只经领域 client 与所属 Native Host 通信，无第二数据权威。
- [ ] Service Worker 无 Native Port、轮询、keepalive 或本地服务。
- [ ] Secret 只进 OS Keychain；用户数据迁移幂等、可恢复。
- [ ] 内置模块随完整 Natives 安装，不独立下载、安装、更新、卸载或发布。
- [ ] 用户可见数据有真实来源，loading/empty/error/unsupported 分离。
- [ ] Extension 中英文 locale 同步。
- [ ] 订阅、timer、observer、Native Port、子进程和缓存有界且可回收。
- [ ] 没有恢复已淘汰的工作台、Daemon、Agent 或 Plugin Runtime。
