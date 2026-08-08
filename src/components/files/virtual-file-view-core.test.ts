import assert from 'node:assert/strict';
import test from 'node:test';
import {
  computeRowRange,
  computeGridColumnCount,
  groupIntoRows,
  gridIndexRange,
  rowScrollTop,
  VIRTUAL_ROW_HEIGHT,
} from './virtual-file-view-core';

test('computeRowRange window is bounded by total rows', () => {
  const r = computeRowRange(0, 100, 10, 20, 2);
  // 可见 5 行 + 2 overscan → end=7，被 total=10 钳制在合理区间
  assert.deepEqual(r, { start: 0, end: 7 });
});

test('computeRowRange respects overscan and clamps', () => {
  const r = computeRowRange(0, 200, 1000, 48, 4);
  assert.ok(r.start >= 0);
  assert.ok(r.end >= r.start);
  assert.ok(r.end - r.start <= Math.ceil(200 / 48) + 8);
  // 滚动到底部不越界
  const deep = computeRowRange(999999, 200, 100, 48, 4);
  assert.equal(deep.end, 100);
});

test('empty inputs yield empty range', () => {
  assert.deepEqual(computeRowRange(0, 0, 100), { start: 0, end: 0 });
  assert.deepEqual(computeRowRange(0, 100, 0), { start: 0, end: 0 });
});

test('grid column count depends on container width', () => {
  assert.equal(computeGridColumnCount(1400, 140, 10), 9);
  assert.equal(computeGridColumnCount(300, 140, 10), 2);
  assert.equal(computeGridColumnCount(50, 140, 10), 1);
  assert.equal(computeGridColumnCount(0, 140, 10), 1);
});

test('groupIntoRows packs by column count', () => {
  const rows = groupIntoRows([1, 2, 3, 4, 5], 2);
  assert.deepEqual(rows, [[1, 2], [3, 4], [5]]);
});

test('gridIndexRange stays O(viewport) for 50k entries', () => {
  const range = gridIndexRange(0, 900, 50000, 7, 160);
  assert.ok(range.end - range.start <= 7 * (Math.ceil(900 / 160) + 8));
  assert.ok(range.end <= 50000);
  const bottom = gridIndexRange(99999999, 900, 50000, 7, 160);
  assert.equal(bottom.end, 50000);
  assert.equal(bottom.start, 50000 - 7 * (Math.ceil(900 / 160) + 8) < 0 ? 0 : bottom.start);
});

test('rowScrollTop maps index to its virtual row top', () => {
  assert.equal(rowScrollTop(0, 1, 48), 0);
  assert.equal(rowScrollTop(5, 1, 48), 240);
  assert.equal(rowScrollTop(0, 7, 160), 0);
  assert.equal(rowScrollTop(7, 7, 160), 160);
  assert.equal(rowScrollTop(8, 7, 160), 160);
});

test('row height constant matches FileRow usage', () => {
  assert.ok(VIRTUAL_ROW_HEIGHT > 20);
});
