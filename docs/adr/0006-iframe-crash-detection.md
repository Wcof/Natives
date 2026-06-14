# ADR-0006: iframe 插件崩溃双层检测

- **状态**: 已接受
- **日期**: 2026-06-14
- **决策者**: 产品方 + AI 架构师
- **关联**: Q20, M7

## 上下文

iframe 中运行的插件可能因 JS 错误、内存溢出等原因崩溃。主进程无法收到 `child_process` 事件（iframe 不是子进程），需要其他方式检测崩溃。

## 决策

双层检测机制：

### 第一层：心跳检测
- 插件每 5s 发送 heartbeat postMessage：`{ type: 'heartbeat', moduleId }`
- 基座维护每个插件的最后心跳时间
- 连续 3 次缺失（15s 无心跳）→ 标记为"无响应"
- 无响应后 10s 仍无恢复 → 标记为"已崩溃"

### 第二层：主动错误报告
- 插件可通过 `window.natives.lifecycle.error(errorInfo)` 主动通知基座
- 插件可通过 Bridge API 的 `error` 事件发送错误详情
- 基座记录到通知中心

### 第三层：iframe load/error 事件
- 监听 iframe 的 `load` 事件（页面加载完成）
- 监听 iframe 的 `error` 事件（资源加载失败）
- 导航到不存在的页面 → 检测为错误

## 崩溃恢复流程

```
检测到崩溃 → 通知中心记录错误 → 显示友好错误页面 + "重新加载"按钮
                                    ↓
                              用户点击重新加载 → 重新创建 iframe → 恢复持久层状态
```

## 理由

1. **心跳是基础** — 无论插件如何崩溃，心跳缺失都能检测到
2. **主动报告更精确** — 插件自己知道出了什么问题，可以提供详细的错误信息
3. **load/error 补充** — 检测页面级加载失败
4. **不自动重载** — 避免崩溃循环（插件持续崩溃 → 持续重载 → 资源耗尽）

## 影响

- **iframe Manager（M7）**：实现心跳计时器、状态机（active → unresponsive → crashed）
- **Bridge SDK**：实现 heartbeat 定时器、`lifecycle.error()` 方法
- **Error Classifier（M10）**：新增 `PLUGIN_CRASH` 和 `PLUGIN_TIMEOUT` 错误类型
- **通知中心**：显示崩溃详情
