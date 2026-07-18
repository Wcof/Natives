# ADR 0011: Native 执行引擎生产闭环缺口与整改状态

> **Status**: Accepted  
> **Date**: 2026-07-17  
> **Related**: `docs/architecture/NATIVE_ENGINE_REMEDIATION_BASELINE.md`  
> **Gap list**: `docs/architecture/NATIVE_ENGINE_GAP_CHECKLIST.md`

## Context

Natives 将 grok-build Agent Runtime 迁入「Agent Daemon + Protocol v2 + multi-provider + tool/hook/subagent」架构。阶段性工作已搭出：

- Fixture 不再伪装成真实 Provider 成功路径
- 真实 HTTP Provider 适配器存在
- `AgentEngine` 具备工具循环骨架
- Permission 事件、cancel 树、Subagent 身份模型有单元测试
- UI/入口多数指向 `RunManager`

全量 `cargo test --workspace --all-targets` 约 501 passed，但覆盖面主要是**内存 / Fixture / 单元逻辑**，未覆盖「真实 Provider + 工具调用 + 独立 sidecar RPC」生产链路。

审计结论：**不得标记为「Native 执行引擎全量整改完成」**。现状是「核心架构原型完成，生产闭环未完成」。

## Decision

1. **状态定义**  
   当前里程碑记为：**核心执行引擎原型可测**。只有完成下方优先序 1–6 且具备可交互 sidecar + 结构化工具闭环后，才可声明生产闭环达标。

2. **阻断问题（必须修复，顺序固定）**

   | # | 阻断 | 说明 |
   |---|---|---|
   | 1 | `EngineMessage → ProviderMessage` 丢失结构 | `tool_calls` / `tool_call_id` 被压成纯文本，第二轮工具结果无法可靠回填 |
   | 2 | sidecar `run.start` 同步阻塞 | 同一连接无法 `permission.respond` / `run.cancel` / 实时订阅 |
   | 3 | Protocol v2 名不副实 | 仍用 v1 Envelope、响应 `0.1.0`，能力声明 `2.0.0` 虚高 |
   | 4 | 双进程双 RunManager | `global_run_manager` 是进程内 `OnceLock`；Tauri 链库与独立 Daemon 不共享状态与 Credential Broker |
   | 5 | 权限未进入 Gateway 强制层 | 仅 `SideEffect`；`PathScope` / `PermissionClass` / 输出与命令限制未统一强制 |
   | 6 | 旧链与死代码未收敛 | `agent_loop` 等仍在，约 230 warnings；双链软切断未真正收敛 |

3. **非阻断但未完成**

   - `run.retry` 在 sidecar 仅建 Run 未启动
   - EventSequencer 仅内存，重启丢失
   - 项目级 Hook 未加载（`.claude/hooks.json`）
   - 子 Agent 简化 Hook / 硬编码 `provider_id: "child"`
   - Subagent 工具 Schema 未暴露 `provider_id` / `key_id` / `model_id`
   - SearchFiles 等工具实现缺口

4. **整改顺序**  
   严格按 1→6 推进；每项关闭须有：代码修改 + 针对该缺口的测试/验收证据 + 更新 `NATIVE_ENGINE_GAP_CHECKLIST.md` 状态。

5. **验收红线**

   - 禁止无 Key 时 offline mock 伪造成功
   - 禁止将「workspace 单测通过」等同于生产闭环完成
   - 独立 Daemon 路径必须走 UDS RPC，不以进程内 `OnceLock` 假装共享
   - 能力声明只能列出已实现方法

## Alternatives Considered

- **先继续堆功能再统一修工具消息**：会放大 Provider 与 Daemon 集成债务，拒绝。
- **保持嵌入式 RunManager 为唯一路径、放弃独立 sidecar**：与「Agent Daemon」目标冲突，拒绝作为终态；嵌入式仅可作过渡。
- **并行大改 1–6**：风险高、难审计，拒绝；采用固定优先序串行主线。

## Consequences

### Positive

- 审计结论与工程状态对齐，避免虚假完成
- 优先序把最严重正确性问题（工具闭环）放在最前
- 缺口清单可追踪关闭，便于回归与外部审计

### Negative

- 短期不能对外宣称引擎整改完成
- 独立 sidecar 交互式体验在 #2/#3/#4 完成前仍不可用

### Neutral

- 既有 Fixture / 单元测试资产保留，作为回归底线
- 用户工作区脏改动继续保护，不 reset

## Implementation Notes

实现进度与逐项验收见：

- `docs/architecture/NATIVE_ENGINE_GAP_CHECKLIST.md`
- `docs/architecture/NATIVE_ENGINE_REMEDIATION_BASELINE.md`（基线同步更新）
