# ADR-0001: Session Token 两阶段握手机制

- **状态**: 已接受
- **日期**: 2026-06-14
- **决策者**: 产品方 + AI 架构师
- **关联**: Q32, Q33

## 上下文

Natives 使用 iframe sandbox（无 `allow-same-origin`）隔离插件。插件与基座之间的通信需要身份验证，以防止恶意插件冒充其他模块。

原方案（Q32）采用单次推送：基座创建 iframe 后立即通过 postMessage 发送 token。但存在三个致命问题：

1. **竞态条件** — 基座创建 iframe 后立即 postMessage 发送 token，但 iframe 的 SDK 可能还没加载完 `addEventListener`，消息丢失
2. **页面重载丢失** — 插件内部刷新后 JS 内存清空，token 丢失，插件变成"砖头"
3. **无恢复路径** — token "仅下发一次"，没有重试机制

## 决策

采用两阶段握手：由 iframe 主动请求 token，而非基座主动推送。

### 流程

```
[iframe 加载 SDK]
    ↓
iframe → 基座: { type: 'token-request', moduleId }
    ↓
基座: 验证 event.source 引用 + moduleId 合法性，生成 token
    ↓
基座 → iframe: { type: 'token-response', token }
    ↓
SDK: 存入内存变量，标记 ready
```

### 关键设计

- **iframe 主动请求** — 消除竞态，SDK 加载完毕后才请求
- **重载自动恢复** — SDK 初始化时始终先请求 token，不假设内存中有值
- **旧 token 自动失效** — 基座维护 `moduleId → token` 映射，同一 moduleId 重新下发时旧 token 失效
- **token 仅存内存** — sandbox 无 `allow-same-origin`，无法使用 localStorage

## 理由

1. **消除竞态** — 由接收方（iframe）发起请求，确保 SDK 已就绪
2. **支持重载** — 每次 SDK 初始化都重新请求，不依赖内存状态
3. **安全可控** — 基座验证 source 引用后才下发 token，防止恶意请求
4. **简单可靠** — 无需复杂的重试队列或消息确认机制

## 替代方案

| 方案 | 问题 |
|------|------|
| 单次推送（Q32 原方案） | 竞态条件、重载丢失、无恢复路径 |
| URL 参数携带 token | token 暴露在 URL 中，可被日志记录和缓存 |
| Cookie 携带 | sandbox 无 allow-same-origin，Cookie 不可用 |

## 影响

- **Bridge Host（M4）**：处理 `token-request` 消息，验证 source 引用后生成并下发 token
- **Bridge SDK**：初始化时发送 `token-request`，收到 `token-response` 后保存 token
- **安全模型**：token 下发依赖 source 引用验证（ADR-0002）
