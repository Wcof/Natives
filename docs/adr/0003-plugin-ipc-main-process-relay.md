# ADR-0003: 插件间通信经主进程中转

- **状态**: 已接受
- **日期**: 2026-06-14
- **决策者**: 产品方 + AI 架构师
- **关联**: Q12, Q24

## 上下文

插件运行在独立的 sandbox iframe（无 `allow-same-origin`）中，彼此无法直接通信。PRD 定义了 `window.natives.ipc.send/on/broadcast` 接口，但未说明底层通信机制。

## 决策

所有插件间通信经基座主进程中转：

1. `ipc.send(targetModuleId, payload)` — 插件 → postMessage → 基座 → 权限检查 → postMessage → 目标插件
2. `ipc.broadcast(payload)` — 插件 → postMessage → 基座 → 广播到所有活跃 iframe
3. 基座维护消息路由表（moduleId → contentWindow 映射）

## 理由

1. **BroadcastChannel 不可用** — 需要 `allow-same-origin`，sandbox 中被禁用
2. **SharedWorker 不可用** — 同上
3. **主进程中转可做权限检查** — 基座可以验证发送方是否有权向目标发送消息
4. **可审计** — 所有消息经过基座，可记录日志

## 替代方案

| 方案 | 问题 |
|------|------|
| BroadcastChannel | sandbox iframe 中不可用 |
| SharedWorker | sandbox iframe 中不可用 |
| window.open + postMessage | 需要弹出窗口，UX 差 |

## 影响

- **Bridge Host（M4）**：新增 IPC 消息路由逻辑
- **Bridge SDK**：新增 `ipc.send` / `ipc.on` / `ipc.broadcast` 方法
- **权限系统**：新增 `ipc.send` 和 `ipc.receive` 权限
