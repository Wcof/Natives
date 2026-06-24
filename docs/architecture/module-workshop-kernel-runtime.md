# 🧬 Natives V3.2 Final：AI-Native 微内核运行时可行性报告

> **领域**: 模块管理 / 创意工坊 (Module Management / Creative Workshop)
> **版本**: V3.2
> **状态**: APPROVED FOR V3.2 FREEZE
> **基石源码**: `src-tauri/src/module_manager.rs` · `src/lib/iframe-manager.ts`
> **审计判定**: 通过 — V3.2 内核化收敛阶段唯一至高技术冻结依据

---

## 一、 背景 (Background)

Natives 作为一款原生的 **macOS 客户端软件**，其核心地基采用 **Tauri v2 + Next.js 15 + SQLite** 技术栈构建。
在此前的架构演进中，系统已经完成了两个具备工业级强度的物理防线基础设施：

1. **底座事务级热拉起机制 (`module_manager.rs`)**：支持强类型 `Manifest` 规约，提供了成熟的磁盘静态资产扫描、SQLite 数据库增量热同步、动态权限重刷，以及基于 `atomic_write`（`.tmp` 临时文件落盘 ➔ `fsync` ➔ 原子覆盖重命名）的物理防断电落盘机制。
2. **唯一源绝对隔离沙箱 (`iframe-manager.ts`)**：实现了一套精密的安全隔离运行时，通过强制切断同源策略的沙箱容器（`sandbox="allow-scripts allow-forms"`，严格去掉了 `allow-same-origin`），强行将加载页面锁死在 Unique Origin（无源空域）中，并配备了后台内存 LRU 自动置换与心跳崩溃检测逻辑。

---

## 二、 需求 (Requirements)

面向 **"一句话输入意图 ➔ AI 自动化组装生成 ➔ 底座零重启上架 ➔ 物理沙箱安全运行"** 的纯 AI-Native 核心场景，微内核底座必须解决以下高阶生产级技术规约：

1. **小时抛/日抛代码的短寿性**：AI 生成的模块和页面本身是高度动态且短寿命的，可能存活几小时即被无情覆盖或废弃。
2. **数据资产的长青连续性**：用户通过这些临时应用沉淀下来的业务数据核心资产必须具备长寿性，决不能因前端代码的频繁销毁而断层。
3. **运行态行为的绝对确定性**：高频更替的异构模块在共享本地持久化存储和通信总线时，行为必须百分之百可预测、可收敛，从源头杜绝状态崩塌。
4. **复杂度红线圈定**：为了维持 macOS 宿主的原生高性能特权，底座内部禁止引入本地 `npm install` 与二次打包构建，AI 生成的编译产物必须死锁在免编译的单页面应用（SPA 静态沙箱）范围内。
5. **绝对主权与网络供应链闭环**：生成的模块界面样式需服从系统的 Liquid Glass 视觉约束，同时底座对外部请求、数据连接拥有绝对的审查主权，防止任何针对敏感凭证的黑客外泄。

---

## 三、 现状与核心架构风险 (Current Status & Risks)

在通过解耦 `moduleId` 引入共享 `domain_namespace` 空间后，由于系统此前缺乏一套不可违背的"内核信任织体（Kernel Trust Fabric）"，在极端高频抛弃迭代的极客场景下，暴露出了三类严重的系统性架构风险：

### 3.1 Kernel Identity Ownership 缺失引发运行态语义漂移 (Semantic Drift)

身份主权未收归内核。目前的 `contract_id` 和配置允许由 AI 任意生成，当多个不同版本的临时应用（如 `todo-v1` 的旧 Schema 和 `todo-v2` 的新 Schema）共享同一个数据域存储空间时，新旧 Schema 的突变交替会导致底座 SQLite 中的 Blob 数据在运行态被新旧应用发生"语义误读"。

### 3.2 Kernel Execution Boundary 缺失引发非原子级脏回滚

现有的备份机制仅发生在物理文件层面（`.bak` 目录重命名）。当 AI 生成的代码在运行中崩溃触发回滚时，代码虽然回滚，但由于持久化状态、并发写操作和数据迁移（Migration）边界不明确，会导致数据库、代码资产与契约层状态脱节，造成严重的系统级跨系统状态脏污。

### 3.3 Trust Fabric 缺失引发总线失控与供应链外泄

- **总线建模盲区**：此前将跨沙箱的事件通信错误建模为了强一致系统。实际上，`iframe` 加上异步的 `postMessage` 通信，在物理世界中**天然只是一个最终一致性系统（Eventually Consistent System）**，缺乏时序与版本控制会导致多模块联动时 UI 状态、异步外部事件出现严重竞态条件死锁。
- **供应链攻击面外露**：沙箱内部由于允许通过外部远程 CDN（如 esm.sh、cdnjs）动态载入第三方 JavaScript 依赖，使底座暴露在极其危险的远程脚本供应链劫持和敏感数据外泄风险之下。

---

## 四、 方案：V3.2 微内核确定性收敛方案 (Feasibility Solution)

为彻底消灭上述风险，本方案决定停止任何在应用层松耦合的破坏性大重构（继续沿用现有的 `manifest.json` 配置骨架），通过**在既有代码管道上增量焊死"5 大内核级系统不变量（Kernel Invariants）"**，全面铺设起绝对可信的微内核信任织体：

### 🚨 不变量 1：Kernel-Owned Identity System（内核主权身份系统）

| 属性 | 说明 |
|------|------|
| **标签** | `KI-1` |
| **控制强度** | MUST |
| **控制点** | `module_manager.rs` — `write_generated_module` 入口 |

**控制要求**：大模型绝对不拥有身份和标识符的生成特权。所有 AI 生成模块的 `contract_id` 与 `module_id` 必须强制由 Rust 内核读取其领域、版本与代码指纹后在底层计算得出：

$$\text{contract\_id} = \text{SHA256}(\text{manifest.domain} + \text{manifest.schema\_version} + \text{SHA256}(\text{html\_content}))$$

**作用**：将标识符空间全量收归 Rust 内核主权，彻底杜绝身份伪造和三元组路由欺骗。

---

### 🚨 不变量 2：Serialized WAL Execution Layer（串行化预写日志事务层）

| 属性 | 说明 |
|------|------|
| **标签** | `KI-2` |
| **控制强度** | MUST |
| **控制点** | `module_manager.rs` — 全局 WAL Journal |

**控制要求**：将物理文件原子写入、SQLite 数据库状态更新、契约版本变更三者，统一打包泵入微内核的全局 `WAL Journal`（预写日志状态机）。

**串行化保证**：微内核引入单线程 `Mutex` 独占队列，强制所有的写操作锁定在同一个独立的 SQLite 事务边界（SQLite TX Boundary）内，绝对禁止出现并行的 `APPLYING` 状态。状态机遵循 `PENDING ➔ APPLYING ➔ COMMITTED ➔ ROLLBACK` 严格串行流转，一旦 AI 新生成的代码在运行时崩溃，三层状态同步原子复位，实现全状态级影子回滚，拒绝残留系统脏污。

---

### 🚨 不变量 3：Contract Enforcement Gate（契约门禁内核校验）

| 属性 | 说明 |
|------|------|
| **标签** | `KI-3` |
| **控制强度** | MUST |
| **控制点** | `module_manager.rs` — Contract Linter 门禁入口 |

**控制要求**：在 Rust 内核大门前部署 `Contract Linter` 门禁。所有 AI 生成的配置在落盘前，必须通过门禁的强类型与 `schema_version` 注册表校验，成功后增量记入底座全局的 `module_contracts` 审计树中，非法语义禁止进入运行时。

**数据迁移限制**：迁移契约（Migration Contract）内部**严禁包含可执行代码**，仅允许声明声明式的 JSON Mapping DSL 映射，且执行主体被完全死锁在 Rust 内核独占的 `Migration Runner` 中。

---

### 🚨 不变量 4：Eventual Consistency Event Model（事件总线降级最终一致性）

| 属性 | 说明 |
|------|------|
| **标签** | `KI-4` |
| **控制强度** | MUST |
| **控制点** | `iframe-manager.ts` — `handleBridgeProxy` 桥接层 |

**控制要求**：全面放弃强一致性 UI 假设。跨沙箱事件总线的事件载荷必须被强制挂载 `version + sequence_id` 时序追踪标签。

**作用**：承认并接受异步 `postMessage` 的弱一致现实，迫使沙箱内部的消费端（Consumer）主动采用状态调和（Reconcile State）模型，彻底免疫高频异步通信带来的竞争时序干扰。

---

### 🚨 不变量 5：Closed Supply Chain Runtime（供应链隔离安全收敛）

| 属性 | 说明 |
|------|------|
| **标签** | `KI-5` |
| **控制强度** | MUST |
| **控制点** | `iframe-manager.ts` — CSP 注入 + 路由拦截 |

**控制要求**：彻底封闭网络脚本供应链边界，全面禁止沙箱动态引用任何外部公网 CDN 运行时链接。生成页面所需的所有轻量级基础第三方响应式框架（如 Alpine.js 或无构建版前端库），必须全量在本地 **Vendored（本地化打包离线化托管）**。

**安全锁死**：配合 `IframeManager` 路由拦截机制，强行将内容安全策略（CSP）死死收敛至 `script-src 'self' tauri://assets`，从根本上斩断动态代码向公网泄露用户敏感凭证或资产的通道。

---

### 🎨 关键基础概念对齐

**数据域三元组公式化**：将 `domain_namespace` 从单纯的"数据切片"升华为饱含"数据域 + 行为约束 + 语义版本"的三元组矩阵公式：

$$\text{domain\_namespace} = \{\text{domain: "todo"}, \text{schema\_version: 2}, \text{contract\_id: "todo-contract-v2"}\}$$

`IframeManager` 的 `handleBridgeProxy` 拦截网桥将根据此三元组进行强路由加锁控制。

**UI 视觉规约解耦**：确认 Liquid Glass 视觉规约属于设计约束（Design Constraint），不属于内核强一致性不变量（Kernel Invariant）。底座仅通过容器级 CSS 注入最大程度强制重绘磨砂茉莉或暗黑伏特绿皮肤，提供视觉吸附感，但不让 UI 状态干预系统微内核的正确性判定。

---

## 五、 目标效果 (Target Effects)

1. **微内核高度确定性**：底座全面转型为具备硬性不变量约束的确定性核心系统（Kernel-Enforced Deterministic System），系统运行态行为实现**可证明的正确性**。
2. **小时抛应用，长青资产**：生成的单页面应用（SPA）代码可以像一次性工具一样高频换代、覆盖和丢弃，但用户沉淀的领域数据资产在三元组命名空间内持续平滑续写，实现**语义零漂移**。
3. **零重启极速响应**：后端 `write_generated_module` 写入到热上架左侧菜单栏的整个生命周期无需重启客户端，Sidebar 动态感应弹出、秒级可用，主进程**零闪烁、热回流**。
4. **绝对主权，极致防泄露**：沙箱 Unique Origin 物理防线、低权限单会话 `SessionToken` 派发、与 Vendored 供应链 CSP 产生三层强强合围，主系统的全局配置表、多环境加密凭证实现绝对物理级防外泄。

---

## ⚖️ 架构师最终审计结论 (Sign-off)

| 评估维度 | 最终状态评价 |
|----------|-------------|
| **架构正确性** | 🟢 极高（已补齐运行态中枢断层） |
| **工程可落地性** | 🟢 高（基于既有 `module_manager.rs` 与 `iframe-manager.ts` 增量重构） |
| **一致性保障** | 🟢 强约束（依赖 5 大内核系统不变量强制 Enforcement） |
| **安全性** | 🟢 已完全闭环（公网脚本供应链边界已关闭） |
| **OS化潜力** | 🟢 具备完整的 AI Native 微内核核心属性 |

**审计判定：通过（APPROVED FOR V3.2 FREEZE）。** 本报告中关于【内核主权身份空间 ➔ 串行单线程 WAL 事务 ➔ 最终一致性事件总线 ➔ 离线化 Vendored CSP 供应链封闭】的微内核治理路径完全成立，无任何逻辑与体验偏差，可以作为项目推进 V3.2 内核化收敛阶段的唯一至高技术冻结依据！
