# ADR-0002: 使用 MessageEvent.source 替代 origin 验证 postMessage 来源

- **状态**: 已接受
- **日期**: 2026-06-14
- **决策者**: 产品方 + AI 架构师
- **关联**: [[ADR-0001-session-token-handshake]], Q24

## 上下文

Natives 使用 iframe sandbox（无 `allow-same-origin`）隔离插件。sandbox iframe 的 origin 固定为 `"null"` 字符串，导致基座无法通过标准的 `event.origin` 验证 postMessage 来源。

这意味着：
- 所有 sandbox iframe 的 origin 都是 `"null"`，基座无法区分消息来自哪个 iframe
- 恶意插件可以在 postMessage 中伪造 `moduleId`，声称自己是别的模块
- 仅依赖 Session Token 不够 — token 下发本身依赖 postMessage，而 postMessage 来源不可信

## 决策

使用 `MessageEvent.source` 窗口引用验证 postMessage 来源，而非依赖 `event.origin`。

工作流程：
1. 基座创建 iframe 时，保存 `iframe.contentWindow` 引用到 `Map<moduleId, Window>`
2. 收到 postMessage 时，遍历映射表，检查 `event.source === savedContentWindow`
3. 匹配 → 处理请求（同时获得可信的 moduleId）；不匹配 → 丢弃

## 理由

1. **source 引用唯一且不可伪造** — 每个 iframe 有独立的 contentWindow 对象，恶意插件无法模拟其他 iframe 的 window 引用
2. **无需依赖 origin** — 完全绕过 sandbox origin 为 `"null"` 的限制
3. **同时解决 moduleId 可信问题** — 匹配到 source 后，基座从映射表中获取 moduleId，无需信任消息体中的 moduleId 字段
4. **业界标准做法** — MDN 文档推荐的 sandbox iframe postMessage 验证方式

## 替代方案

| 方案 | 问题 |
|------|------|
| 信任消息体中的 moduleId | 恶意插件可伪造，越权访问其他模块数据 |
| 使用 `allow-same-origin` + origin 检查 | 破坏沙箱隔离，插件可访问 localStorage/cookies |
| 移除 sandbox | 完全放弃隔离，不可接受 |

## 影响

- **iframe Manager（M7）**：创建 iframe 时需维护 `moduleId → contentWindow` 映射表
- **Bridge Host（M4）**：postMessage 处理逻辑增加 `event.source` 匹配检查
- **安全模型**：postMessage 来源验证从"信任 origin"变为"信任 window 引用"，安全性提升
