// 画线纯计算层回归测试（stock-drawings-shapes.js）：
// shapesCtx 的 selected/hoverHit/drawings/locked 是属性 getter，
// 拆分层必须以属性访问读取；若回退为 ctx.selected() 函数调用，
// 本测试稳定复现 TypeError（guard: get 触发即视为正确取值）。
// 覆盖 buildGraphic / hitTest / moveAnchor / moveWholeDrawing。
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const shapesSrc = readFileSync(join(root, 'modules/fund/ui/src/stock-drawings-shapes.js'), 'utf8');

// 加载源文件 IIFE，得到 globalThis.Fund.StockDrawingsShapes
// 语义取色 mock：Shapes 在选中/锚点渲染时回退 Fund.themeColor（DES 02）。
globalThis.Fund = { themeColor: (name) => ({ '--draw-selected': '#b13642', '--natives-surface': '#ffffff' })[name] || '#345780', themeRgba: (name, a) => `rgba(52,87,128,${a})` };
new Function(shapesSrc)();
const Shapes = globalThis.Fund.StockDrawingsShapes;
assert.ok(Shapes && typeof Shapes.buildGraphic === 'function', 'shapes layer failed to load');

// 构造 mock ctx：getter 属性（修复前被错误地当函数调用 → TypeError）
function makeCtx(overrides = {}) {
  const ctx = {
    chart: null,
    getKlineData: () => [
      { date: '2026-01-01', open: 1, close: 2, high: 3, low: 0, time: '2026-01-01' },
      { date: '2026-01-02', open: 2, close: 3, high: 4, low: 1, time: '2026-01-02' },
    ],
    get drawings() { return overrides.drawings ?? []; },
    get selected() { return overrides.selected ?? null; },
    get hoverHit() { return overrides.hoverHit ?? null; },
    get magnetEnabled() { return false; },
    get locked() { return overrides.locked ?? false; },
    get strokeColor() { return '#38bdf8'; },
    get strokeWidth() { return 2; },
    dashPattern: (type) => (type === 'dashed' ? [5, 4] : null),
    pointToPixel: (pt) => (pt && pt.date != null && pt.price != null ? [pt.date === '2026-01-01' ? 10 : 20, 50 - pt.price * 10] : null),
    chartWidthPx: () => 600,
    snapPoint: (date, price) => ({ date, price }),
    ...overrides.extra,
  };
  return ctx;
}

test('buildGraphic reads selected/hoverHit as properties and renders trendline', () => {
  const els = [];
  const d = {
    id: 't1', toolType: 'trendline',
    points: [{ date: '2026-01-01', price: 1 }, { date: '2026-01-02', price: 2 }],
    options: { name: '趋势' },
  };
  const ctx = makeCtx({ selected: { id: 't1', anchor: -1 } });
  Shapes.buildGraphic(ctx, d, els);
  assert.ok(els.length >= 2, 'trendline + name label expected');
  assert.ok(els.some(e => e.type === 'line'));
  assert.ok(els.some(e => e.type === 'text' && e.style.text === '趋势'));
});

test('buildGraphic renders anchors for the selected drawing', () => {
  const els = [];
  const d = {
    id: 'h1', toolType: 'horizontal',
    points: [{ date: '2026-01-01', price: 2.5 }],
    options: {},
  };
  const ctx = makeCtx({ selected: { id: 'h1', anchor: 0 } });
  Shapes.buildGraphic(ctx, d, els);
  assert.ok(els.some(e => e.type === 'circle'), 'anchor handle circle expected');
});

test('hitTest detects anchor hit via property access on drawings', () => {
  const d = {
    id: 'a1', toolType: 'horizontal',
    points: [{ date: '2026-01-01', price: 2.5 }],
    options: {},
  };
  const ctx = makeCtx({ drawings: [d] });
  // pointToPixel 把 price=2.5 映射为 y=25；锚点 18px 磁性半径内
  const hit = Shapes.hitTest(ctx, 10, 25);
  assert.ok(hit, 'expected anchor hit');
  assert.equal(hit.id, 'a1');
  assert.equal(hit.anchor, 0);
  assert.equal(Shapes.hitTest(ctx, 500, 500), null);
});

test('moveAnchor snaps point and respects locked as a property', () => {
  const d = { id: 'm1', toolType: 'horizontal', points: [{ date: '2026-01-01', price: 1 }], options: {} };
  Shapes.moveAnchor(makeCtx(), d, 0, '2026-01-01', 9.5);
  assert.equal(d.points[0].price, 9.5);

  const locked = { id: 'm2', toolType: 'horizontal', points: [{ date: '2026-01-01', price: 1 }], options: {} };
  Shapes.moveAnchor(makeCtx({ locked: true }), locked, 0, '2026-01-01', 9.5);
  assert.equal(locked.points[0].price, 1, 'locked ctx must not mutate points');
});

test('moveWholeDrawing shifts points by bar/price delta from the chart', () => {
  const chart = {
    convertFromPixel(axis, v) {
      // x 轴: 10 -> 索引 0, 30 -> 索引 1；y 轴: 50 -> 价 0, 40 -> 价 1
      if (axis && axis.yAxisIndex === 0) return (50 - v) / 10;
      return Math.round((v - 10) / 20);
    },
  };
  const orig = [{ date: '2026-01-01', price: 1 }];
  const d = { id: 'w1', toolType: 'trendline', points: [{ date: '2026-01-01', price: 1 }], options: {} };
  const ctx = makeCtx({ extra: { chart } });
  // startPix.x=10(索引0) curPix.x=30(索引+1)；startPix.y=50(price0) curPix.y=40(price+1)
  Shapes.moveWholeDrawing(ctx, d, orig, { x: 10, y: 50 }, { x: 30, y: 40 });
  assert.equal(d.points[0].date, '2026-01-02', 'date should shift by bar delta');
  assert.equal(d.points[0].price, 2, 'price should shift by price delta');
});

test('moveWholeDrawing is a no-op when locked', () => {
  const d = { id: 'w2', toolType: 'trendline', points: [{ date: '2026-01-01', price: 1 }], options: {} };
  const ctx = makeCtx({ locked: true, extra: { chart: { convertFromPixel: () => 0 } } });
  Shapes.moveWholeDrawing(ctx, d, [{ date: '2026-01-01', price: 1 }], { x: 10, y: 50 }, { x: 30, y: 40 });
  assert.equal(d.points[0].price, 1, 'locked ctx must not move drawing');
});
