// P2-03 · usePreview 共享 Hook 测试
//
// 验证 sourceKeyOf 稳定键语义：相同 path/mtime 不重复触发加载（避免父组件
// 每次渲染传新 source 对象导致无限重载）；文件 mtime 变化或 memory 内容长度
// 变化时 key 必须变化（触发重载）。hook 的 surface-local controller /
// PREV-006 错误归一逻辑与 PreviewSurface 共用同一实现，集成时统一覆盖。

import assert from 'node:assert/strict';
import test from 'node:test';
import { sourceKeyOf } from './usePreview';

test('file source key is stable for identical path/kind/size/mtime', () => {
  const a = sourceKeyOf({ type: 'file', path: '/tmp/a.ts', kind: 'text', size: 10, mtime: 100 });
  const b = sourceKeyOf({ type: 'file', path: '/tmp/a.ts', kind: 'text', size: 10, mtime: 100 });
  assert.equal(a, b);
  // 对象身份不同但内容相同 → 同一 key（父组件每次渲染新建 source 不会误触发重载）
  assert.equal(a, sourceKeyOf({ type: 'file', path: '/tmp/a.ts', kind: 'text', size: 10, mtime: 100 }));
});

test('file source key changes when path or mtime changes', () => {
  const base = sourceKeyOf({ type: 'file', path: '/tmp/a.ts', kind: 'text', size: 10, mtime: 100 });
  assert.notEqual(base, sourceKeyOf({ type: 'file', path: '/tmp/b.ts', kind: 'text', size: 10, mtime: 100 }));
  assert.notEqual(base, sourceKeyOf({ type: 'file', path: '/tmp/a.ts', kind: 'text', size: 10, mtime: 101 }));
  assert.notEqual(base, sourceKeyOf({ type: 'file', path: '/tmp/a.ts', kind: 'text', size: 20, mtime: 100 }));
});

test('memory source key changes with content length', () => {
  const a = sourceKeyOf({ type: 'memory', name: 'x.html', content: 'abc' });
  const b = sourceKeyOf({ type: 'memory', name: 'x.html', content: 'abcd' });
  assert.notEqual(a, b);
  assert.equal(a, sourceKeyOf({ type: 'memory', name: 'x.html', content: 'abc' }));
});
