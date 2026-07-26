# 00 · 术语与关键词

> 本篇定义**规范体系内部**使用的术语与关键词语义。  
> 产品身份与面/轨以 [ADR-0012](../adr/0012-product-identity-workshop-scope.md) 与 [`product/01-positioning.md`](./product/01-positioning.md) 为准。

---

## 一、规则强度关键词（RFC 2119 精简版）

本规范体系借用 RFC 2119 的关键词语义，但**只保留三档**，避免「SHOULD NOT」「RECOMMENDED」等近义词造成 AI 解析歧义。

| 关键词 | 中文 | 含义 | 违反的后果 |
|--------|------|------|-----------|
| **MUST** | 必须 / 必须不 | 绝对约束。不可妥协的红线。 | 禁止合入；视为缺陷；若确需偏离，必须先补 ADR。 |
| **SHOULD** | 应该 | 强约定。承认存在合理例外。 | 可以偏离，但**必须**在 PR 描述或 ADR 中写明理由。 |
| **MAY** | 可以 | 建议、最佳实践、风格偏好。 | 鼓励遵守，不强制，不作合入门槛。 |

**约定**：

- 关键词在规范正文中以**加粗**出现（**MUST** / **必须**）时，按上表语义解读。
- 一条规则若未标注强度，默认按 **SHOULD** 处理。
- 当「应该」与「可以」同时出现描述同一做法时，取**更严**的一档。

---

## 二、规则分类标签

每条规则除强度外，还会打一个或多个**分类标签**，便于按维度检索（例如「这次改动涉及数据，把所有 `数据` 分类的规则过一遍」）。

| 标签 | 覆盖范围 |
|------|---------|
| `安全` | 沙箱、权限、凭证、CSP、Session Token、来源验证 |
| `数据` | SQLite、迁移、命名空间隔离、原子写入 |
| `无假数据` | 用户可见字段必须有真实来源 |
| `i18n` | 双语同步、文案键管理 |
| `命名` | 文件、目录、变量、IPC channel、CSS 变量 |
| `分层` | 模块依赖方向、跨层调用限制 |
| `状态` | 前端状态管理、IPC、广播同步 |
| `性能` | 渲染、内存、启动、LRU |
| `可访问性` | 键盘焦点、ARIA、对比度 |
| `主题` | 设计令牌、三皮肤、字体绑定 |
| `交互` | toast/通知/模态的选用、空/加载态、快捷键 |
| `反馈` | 动效、声音、聚焦环 |
| `进程` | Host Main / Agent Daemon / Renderer / 租户（iframe·Embed）边界 |
| `版本` | SemVer、minNativesVersion、插件更新 |

---

## 三、规范体系内的核心术语

以下术语**仅在规范语境**中使用；产品业务术语以 ADR-0012 与 `product/01-positioning.md` 为准。

### 规范（Standard / 本目录）
对后续所有更新的**约束**。本目录文档的总称。区别于「描述」（陈述当前怎么实现）和「决策」（ADR，记录为什么这么定）。

### 红线（Red Line）
**MUST** 级规则的俗称。例如「无假数据」「凭证必须加密」「iframe 禁止 allow-same-origin」都是红线。

### 现状描述（Description）
`docs/architecture/` 等文档的角色。它们陈述「当前系统是如何实现的」，但不构成约束。当描述与规范冲突，规范胜出，描述应被更新。

### 三面 / 双轨（Surface / Track）
- **Hub / Workshop / Embed**：产品入口与安全模型分面（ADR-0012）。  
- **web-module / capability**：物理实现双轨；禁止单 manifest 硬揉。

### 能力库 / 能力中心 / 能力子系统（三名对齐，ADR-0016）
- **能力库（Capability Hub）**：菜单名与用户可见名称，Skills / 连接器 / 专家 三子域的统一管理面。
- **能力中心**：ADR-0012 第 4 节为 capability 轨冻结的 UI 归属名。与「能力库」是**同一概念**。
- **能力子系统**：daemon 侧执行实现（`NATIVE-DAEMON-CAPABILITY-MAP.md`：mcp_runtime / skill_store / subagent_store 等）。能力库是它的配置权威与管理表面。

### 决策记录（ADR, Architecture Decision Record）
记录「**为什么**在某个时间点做了某个架构选择」。ADR 不直接是约束，但规范中的 MUST/SHOULD 常常**源自**某个 ADR。规范篇会在「关联 ADR」处双向链接。

### 四件套（规则的四个组成部分）
指一条完整规则的四段：**规则 + 正例 + 反例 + 为什么**（外加可选的「检查方法」）。详见 `README.md`。

### 合规自检清单（Compliance Checklist）
每篇规范文末的小清单，以及 `README.md` 底部的全局清单。提交前过一遍。

---

## 四、缩写与代号

| 缩写 | 全称 | 说明 |
|------|------|------|
| FOUC | Flash of Unstyled Content | 主题未就绪时窗口先显示无样式内容 |
| PTY | Pseudo-Terminal | portable-pty 提供的完整终端能力 |
| CSP | Content-Security-Policy | 插件 HTTP 响应注入的安全策略头 |
| LRU | Least Recently Used | 后台 iframe 的淘汰策略 |
| KV | Key-Value | `module_data` 表的存储模型 |
| WAL | Write-Ahead Logging | SQLite 的并发写入模式 |
| SemVer | Semantic Versioning | 主.次.修订 版本号规范 |
| UDS | Unix Domain Socket | Host ↔ Agent Daemon 生产通信 |
| KI | Kernel Invariant | Workshop 微内核不变量 KI-1…5 |

---

## 五、文档约定

- 规范篇正文用中文为主、代码/标识符保留英文，与 `ARCHITECTURE.md` 风格一致。
- 跨篇引用用相对链接，如从 `00-glossary.md` 引用 `[分层规则](technical/01-layering.md)`（同目录下用相对路径，跨级用 `../`）。
- 引用 ADR 用 `ADR-00XX` 代号，指向 `docs/adr/00XX-*.md`。
- 每篇规范顶部固定四块元信息：版本、日期、关联 ADR、关联源文件。
