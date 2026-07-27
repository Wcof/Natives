# ADR-0016: 能力库（Capability Hub，能力中心统一管理面）

- **状态**: 已接受
- **日期**: 2026-07-26
- **决策者**: 产品方（用户）
- **关联**: [ADR-0011](./0011-native-engine-production-gaps.md)、[ADR-0012](./0012-product-identity-workshop-scope.md)、[ADR-0014](./0014-creative-app-creator-workbench.md)、`docs/architecture/NATIVE-DAEMON-CAPABILITY-MAP.md`、`docs/superpowers/specs/2026-07-26-native-harness-control-plane-design.md`
- **归类**: Hub 面 + capability 轨

---

## 术语对齐

「**能力库**」是本模块的菜单名与用户可见名称；它就是 ADR-0012 第 4 节冻结的「**能力中心**」（capability 轨的 UI 归属），两名指同一概念。daemon 侧既有的「**能力子系统**」（`NATIVE-DAEMON-CAPABILITY-MAP.md`）是其执行侧实现。三名对齐已登记 `docs/standards/00-glossary.md`。

---

## 上下文

capability 轨是 ADR-0012 双轨中唯一没有落地数据模型与管理界面的一轨。现状核对（2026-07-26，`deploy` 分支）：

| 能力域 | 后端 | 前端 | 缺口 |
|--------|------|------|------|
| MCP | `mcp_runtime.rs`（stdio/HTTP/SSE/OAuth 租约）| `EngineCapabilitiesPanel.tsx` 只读列表 | **无可信配置源**：`rpc.rs` 拒绝内联注册，但全仓库没有任何配置加载器（`register_server` 非测试调用者为 0）——MCP 实质上无法被用户注册。migration 004 的 `mcp_server_config` 表零读写（死表） |
| Skills | daemon `skill_store.rs`（内存扫描）+ Host `agent.rs::scan_skills` | `SkillsPanel.tsx` 启停/卸载 | 无分类、无导入、注入不可按会话选择（全局 `trusted && enabled` 一刀切） |
| 专家/Subagent | **三条互不相通的线**：① `crates/agent-core/profile.rs` AgentProfile（`.claude/agents/*.md`，其 `skills` 字段已解析但零消费者）；② daemon `subagent_store.rs`（路由策略）；③ Host `commands/subagent.rs` 自建 `subagents` 表、绕过 daemon 直连 provider | `/subagents` 孤儿页（无菜单入口，554 行逻辑堆在 page.tsx 违反 R-E2） | 无权威、无专家团、无会话选用 |
| 会话选用 | `CreateRunRequest.agent_profile_id` 已入库并被 `production.rs` 消费 | 前端已发送该字段 | `run_gateway.rs` 不解析直接丢弃，`StartRunRequest` 无此字段——链路断在中间 |

结论：零件都在，缺的是**统一的权威数据模型 + 管理界面 + 会话选用链路**。

---

## 决策

### 1. 新增顶层模块「能力库」：Skills / 连接器 / 专家 三个子域

菜单一级入口（代码命名空间 `capabilities`；`library` 已被素材库占用）。P0 全量交付：三子域 CRUD、专家团、MCP JSON 导入与在线 Hub、会话选用 → 执行引擎注入全链路。

### 2. 权威存储：Daemon `assistant.db` 的 `capability_*` 表族

执行权威与数据权威同进程（MCP 启动、skill 注入、subagent 派生全在 daemon）；符合 R-T2 分库权威与能力域唯一归属原则。死表 `mcp_server_config` 直接 DROP（transport CHECK 词表已与现实现不符，重建成本等于新建）。`capability::bootstrap()` 在 daemon 启动时把 enabled 的 MCP 配置注册进 `global_mcp()`——**即 `rpc.rs` 一直缺失的那个「可信配置源」**。

### 3. 专家统一：DB 为权威，`.md` 是导入/导出格式

`capability_expert` / `capability_expert_team` 表为唯一权威；`.claude/agents/*.md` 经 `profile.rs` 既有解析器导入（记录 source_path + content_hash 做漂移提示），可导出供外部 CLI 消费；`load_agent_profile` 文件查找保留为 fallback。Host `commands/subagent.rs` 旧线数据一次性迁入（source='host_migration'）后退役——**消灭第四条专家线**。专家团 = lead 人格 + 成员白名单 + failure policy（词表复用 `subagents.rs::FailurePolicy`），**不是新调度器**；成员派生走既有 task 工具与 SubAgentManager 限额。

### 4. 会话选用：conversation 默认 + run 覆盖，resolve 快照持久化

选择存 `conversation.capability_selection_json`（对齐 permission_profile 先例）；`run.start` 可带同形字段单次覆盖；daemon 在 run 启动时 resolve 为快照持久化到 run 行（与未来 Harness `ResolvedHarnessSnapshot` 同构）。协议字段全部 `Option` + serde default，**None = 维持现状行为**（向后兼容合同）。

### 5. 失败语义：fail-closed

用户显式勾选 = 对本次执行的合同。resolve 任一项失败（MCP 启动失败 / skill 缺失 / 专家不存在 / 引擎不支持该能力）→ run Failed + 结构化错误码（`CAPABILITY_RESOLVE_FAILED` 系列），provider 零调用。**禁止静默降级**——静默继续等价 ADR-0011 红线所禁的「伪造成功」。唯一合法的「降级」是引擎能力矩阵如实宣告的机制差异（如 claude cli 用 `--append-system-prompt` 而非原生注入），UI 与事件必须如实标注（外部 CLI 自带 harness 时标 `execution_backend`，不得宣称经过原生 Capability Gateway）。

### 6. 联网边界论证（对 ADR-0012 的澄清，非修订）

ADR-0012 后置到 P2 的是「联网商店**上架 / 发现 / 订阅**」——即**把本地数据发布出去**的分发面。本 ADR 澄清：**消费**第三方能力（连接远程 MCP 服务、浏览在线 MCP Registry 并显式添加）属于允许的联网消费，与 Provider API 调用同性质，不触碰 ADR-0012 冻结。边界钉死为：

1. Hub 只读浏览 + 用户显式确认才添加；添加产物 `trusted=false, enabled=false`，需用户二次启用。
2. **不上传任何本地数据**到 Hub / Registry（搜索词除外）。
3. 不自动安装、不自动更新（与 R-P7 第 7 条一致）。
4. Hub 数据源必须真实（官方 MCP Registry API），不可达时诚实报错并引导 JSON 离线导入，禁止假缓存伪装在线。

### 7. 凭证路线

`assistant.db` 永不存明文 secret。MCP env secret / OAuth refresh token → Host `natives.db` 新表 `capability_secrets`（AES-256-GCM，复用 provider_api_keys 信封），daemon 经 NativesDbBroker 只读解密、内存即用即弃；access token 维持内存 `McpCredentialStore` 不落盘；OAuth 浏览器流在 Host 侧（`mcp:oauth:start`）。

### 8. 与 Harness 控制面的关系

能力库 = 能力对象的**权威 CRUD、ID 命名空间与会话/Run 选用权威**。Harness Blueprint 按 global→project→session 管理 Hook 与 Natives Prompt Block；它不复制能力对象，也不建立第二份能力选择。Run 启动时先由 `capability_resolution` 解析能力，再由 Harness 在同一插入缝消费其只读快照引用/哈希与提示词贡献，最终产出一份可执行计划及对应 Run 证据。未来若要让 Blueprint 改写能力选择，必须另行修订本 ADR，不能在 Harness 实施中隐式搬迁权威。

---

## 不变量

1. **单一权威** — 每类能力对象只有 `capability_*` 表一个权威；扫描/文件只是发现器与交换格式。
2. **可信配置源唯一** — MCP 注册只经 `capability::bootstrap()` / `capability.mcp.*`；RPC 内联注册维持拒绝。
3. **选择即合同** — 显式选用的能力 resolve 失败必须 fail-closed，不静默降级。
4. **诚实广告** — 能力广告 ⊆ 可调实现（R-T5）；引擎不支持的能力在矩阵与 UI 如实标注；外部 CLI 执行标记独立后端。
5. **工具执行收敛** — MCP/Skill/子 Agent 不绕过 `Tool Registry → Schema → Path Scope → Permission → Approval → Execution → Audit` 统一链路（CODE_MODULE_GUIDELINES 9.5）。
6. **secret 零落盘（daemon 侧）** — assistant.db、RPC 响应、事件、日志、快照中不出现任何明文凭证。
7. **消费不分发** — 联网仅限只读浏览与显式添加；不上传本地数据（边界见决策 6）。

---

## 明确不做（P0 非目标）

1. Skill 在线市场 / Skill 上架分享（联网分发面维持 ADR-0012 P2 后置）。
2. MCP Hub 之外的第三方 registry 聚合、评分、评论。
3. 专家团静态 DAG / 工作流编排器（团 = lead + 成员白名单，编排由 lead prompt + task 工具驱动）。
4. codex cli 的 MCP/专家注入（能力矩阵标 unsupported，`-c` 覆盖方案仅文档化）。
5. Skills git 导入（P1；P0 支持 zip + 本地目录）。
6. 旧 `subagent_runs` 历史迁移（语义不同，保留只读归档）。

---

## 后果

### 正面

- capability 轨首次获得完整数据模型与管理面，ADR-0012 落地清单「web-module 与 capability 分表」闭环。
- MCP 从「实质不可注册」变为可用；`AgentProfile.skills` 首次获得消费者；三条专家线收敛为一。
- 会话选用链路保持能力库权威；Harness 在 Run 启动单缝消费其解析快照，无双写与数据反迁。

### 负面 / 成本

- 触及 Host / Daemon / 协议 / 前端四层，migration 021 + Host v11 双库变更。
- `production.rs` / `production_tools.rs` / `run_manager.rs` 热路径改动，需 golden 回归（selection 全空 = 现状逐字节等价）保护。
- Host 旧 subagent 线退役有双写窗口，需迁移标记后旧命令改只读报错。

### 中性

- 现有 `mcp.*` / `skill.list` 运行时 RPC 面不变；`EngineCapabilitiesPanel` 只读仪表盘保留。

---

## 落地检查清单

- [ ] Daemon migration 021：`capability_skill` / `capability_mcp_server` / `capability_expert` / `capability_expert_team(_member)` / `capability_mcp_hub_cache` + conversation 加列 + DROP `mcp_server_config`
- [ ] Host v10→v11：`capability_secrets`（AES-256-GCM 信封）
- [ ] `capability.*` RPC 全部登记 methods.rs 两清单 + types.ts（`npm run protocol:check` 绿）
- [ ] `capability::bootstrap()` 可信配置源打通；untrusted 双门拒（resolve + invoke）
- [ ] 缝 A 打通：`StartRunRequest.agent_profile_id` / `capability_selection` 三处透传
- [ ] `capability_resolution.rs` fail-closed + 快照持久化；golden 回归（None = 现状）
- [ ] MCP 按选可用：schema 注入 + server 白名单门（收紧 `mcp__*` 全放行洞）+ acquire/release + idle reaper
- [ ] 专家团：task `agent` 参数 + 成员白名单校验 + SubAgentManager 限额不绕过
- [ ] claude cli：`--mcp-config`（0600 临时文件，三退出路径删除）+ `--strict-mcp-config` + `--agents` + `--append-system-prompt`；事件标 `execution_backend`
- [ ] Host subagent 旧线：Phase A 一次性迁移 + 旧命令只读；Phase B 删除
- [ ] 前端：导航接线、三 Tab（组件 ≤300 行、经 capability-admin 门面）、会话能力选择器（gate 门控）、删 `/subagents`
- [ ] 中英文 i18n 同步；`npm run perf:check` 通过（懒加载、初始 JS 预算）
- [ ] 功能树 K 域回填实现路径；`NATIVE-DAEMON-CAPABILITY-MAP.md` 矩阵更新

---

## 修订

| 日期 | 变更 |
|------|------|
| 2026-07-26 | 初版：冻结能力库三子域、权威存储、会话选用链路、fail-closed 语义、联网消费边界 |
| 2026-07-27 | 对齐 Harness 落地设计：能力库继续拥有会话/Run 选用权威；Harness 只消费只读快照引用并管理 Hook/Prompt Plan |
