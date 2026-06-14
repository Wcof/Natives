# ADR-0005: 插件状态保持分层策略

- **状态**: 已接受
- **日期**: 2026-06-14
- **决策者**: 产品方 + AI 架构师
- **关联**: Q19, Q20

## 上下文

User Story 16 要求"切换模块时保留之前模块的状态（不重新加载）"。但 sandbox iframe 无 `allow-same-origin`，无法使用 localStorage/IndexedDB 持久化。同时内存有限，不能无限保留所有 iframe。

## 决策

采用分层状态策略：

| 层级 | 条件 | 行为 | 状态保留 |
|------|------|------|----------|
| 热层 | 当前可见的 iframe | 保持活跃 | ✅ JS 内存 |
| 温层 | 最近 5 个后台 iframe | 保持在 DOM 中但隐藏 | ✅ JS 内存 |
| 冷层 | 超出温层 | iframe 被销毁 | ❌ 丢失 |
| 持久层 | 插件主动保存 | 通过 `natives.db.set()` 保存到基座 | ✅ 持久化 |

## 理由

1. **内存可控** — 最多 5 个后台 iframe（约 250-500MB），加上 1 个前台 iframe
2. **状态不猜测** — 基座不自动持久化插件状态，由插件开发者决定什么需要保存
3. **恢复有路径** — 插件通过 `window.natives.db.set(key, value)` 保存关键数据，重载后通过 `get(key)` 恢复
4. **UX 可接受** — 大多数用户同时使用 < 6 个插件，温层足够覆盖

## 插件开发者指南

```javascript
// 插件在 unload 事件中保存状态
window.natives.lifecycle.onUnload(async () => {
  await window.natives.db.set('editor-content', editor.getValue());
  await window.natives.db.set('scroll-position', window.scrollY);
});

// 插件在 ready 事件中恢复状态
window.natives.lifecycle.ready(async () => {
  const content = await window.natives.db.get('editor-content');
  if (content) editor.setValue(content);
});
```

## 影响

- **iframe Manager（M7）**：实现温层/冷层管理逻辑，LRU 上限 5
- **Bridge API**：`db.set/get` 已有，无需新增接口
- **插件文档**：需提供状态保存最佳实践
