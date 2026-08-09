import assert from 'node:assert/strict';
import { describe, it } from 'node:test';
import { computeMultiSelect } from './useFileSelection';

/**
 * F3-04 · computeMultiSelect 特性测试（ARCH-002 拆分后的纯函数）。
 * 语义与原 FileBrowser handleSelect 多选分支完全一致：
 * - shift：从锚点 lastClickedIndex 到点击 idx 的闭区间全部加入（锚点不变）
 * - cmd/meta：切换 entryPath（锚点更新为 idx）
 * - 其它：集合原样返回
 */

const PATHS = ['/a', '/b', '/c', '/d', '/e'];

function run(input: Partial<Parameters<typeof computeMultiSelect>[0]>) {
  return computeMultiSelect({
    prev: new Set<string>(),
    entryPath: '/b',
    idx: 1,
    lastClickedIndex: -1,
    isShift: false,
    isCmd: false,
    orderedPaths: PATHS,
    ...input,
  });
}

describe('computeMultiSelect', () => {
  it('no modifiers: returns previous set unchanged and anchor unchanged', () => {
    const prev = new Set(['/x']);
    const { next, lastClickedIndex } = run({ prev, lastClickedIndex: 3 });
    assert.deepEqual([...next], ['/x']);
    assert.equal(lastClickedIndex, 3);
  });

  it('shift range: adds closed interval [anchor..idx] to empty set', () => {
    const { next, lastClickedIndex } = run({ isShift: true, lastClickedIndex: 0, idx: 2 });
    assert.deepEqual([...next].sort(), ['/a', '/b', '/c']);
    // 锚点不变
    assert.equal(lastClickedIndex, 0);
  });

  it('shift range: merges into existing multi-selection', () => {
    const { next } = run({
      prev: new Set(['/e']),
      isShift: true,
      lastClickedIndex: 1,
      idx: 3,
    });
    assert.deepEqual([...next].sort(), ['/b', '/c', '/d', '/e']);
  });

  it('shift range: reversed interval works (idx < anchor)', () => {
    const { next } = run({ isShift: true, lastClickedIndex: 3, idx: 1 });
    assert.deepEqual([...next].sort(), ['/b', '/c', '/d']);
  });

  it('shift without anchor: no-op', () => {
    const prev = new Set(['/a']);
    const { next, lastClickedIndex } = run({ prev, isShift: true, lastClickedIndex: -1 });
    assert.deepEqual([...next], ['/a']);
    assert.equal(lastClickedIndex, -1);
  });

  it('cmd toggle: adds entry path and updates anchor to idx', () => {
    const { next, lastClickedIndex } = run({ isCmd: true, idx: 2, entryPath: '/c' });
    assert.deepEqual([...next], ['/c']);
    assert.equal(lastClickedIndex, 2);
  });

  it('cmd toggle: removes existing entry path and still updates anchor', () => {
    const { next, lastClickedIndex } = run({
      prev: new Set(['/b']),
      isCmd: true,
      idx: 1,
      entryPath: '/b',
    });
    assert.deepEqual([...next], []);
    assert.equal(lastClickedIndex, 1);
  });

  it('shift then cmd: cmd toggles within the extended set', () => {
    const shifted = run({ isShift: true, lastClickedIndex: 0, idx: 2 });
    const { next } = run({ prev: shifted.next, isCmd: true, idx: 1, entryPath: '/b' });
    // /b was in the shifted set → toggled off
    assert.deepEqual([...next].sort(), ['/a', '/c']);
  });

  it('idx out of bounds with shift: only valid paths added', () => {
    const { next } = run({ isShift: true, lastClickedIndex: 2, idx: 99 });
    assert.deepEqual([...next].sort(), ['/c', '/d', '/e']);
  });
});
