// R7（§5.1 第 5 条 / §9.0.1）钻取闭环测试：点击金额/排行行/表格行 →
// openDrilldown 收到与当前卡片一致的筛选（range + 维度 key 映射到
// model/provider/source 槽位）。无回调（旧环境）时不得绑定点击。
import assert from 'node:assert/strict';
import { CHARTS } from '../ai-performance/charts.js';

// charts.js 渲染路径使用全局 document.createElement；提供最小 shim。
if (!globalThis.document) {
  globalThis.document = { createElement: (tag) => makeEl(tag), createElementNS: (ns, tag) => makeEl(tag) };
}

// 最小 DOM stub：覆盖 charts.js 用到的 createElement/append/addEventListener/
// querySelectorAll/classList/dataset/innerHTML。
function makeEl(tag = 'div') {
  const el = {
    tag,
    className: '',
    innerHTML: '',
    tabIndex: 0,
    dataset: {},
    style: {},
    children: [],
    listeners: {},
    replaceChildren(...nodes) { el.children = [...nodes]; },
    classList: {
      add(...cls) { for (const c of cls) el.dataset['cls_' + c] = '1'; },
      remove() {},
    },
    setAttribute(k, v) { el.dataset['attr_' + k] = String(v); },
    append(...nodes) { el.children.push(...nodes); },
    addEventListener(type, fn) { (el.listeners[type] ||= []).push(fn); },
    click() { for (const fn of el.listeners.click || []) fn(); },
    keydown(key) { for (const fn of el.listeners.keydown || []) fn({ key, preventDefault() {} }); },
    querySelectorAll(sel) {
      if (sel !== '[data-drill-key]') return [];
      // 同一 innerHTML 复用同一组行 stub：charts.js 绑定监听与测试点击
      // 必须作用在同一对象上（真实 DOM 中 querySelectorAll 返回同一节点）。
      if (el.__drillRows && el.__drillHTML === el.innerHTML) return el.__drillRows;
      const keys = [];
      for (const m of String(el.innerHTML).matchAll(/data-drill-key="([^"]*)"/g)) keys.push(m[1]);
      el.__drillHTML = el.innerHTML;
      el.__drillRows = keys.map((k) => {
        const row = makeEl('row');
        row.dataset.drillKey = k;
        return row;
      });
      return el.__drillRows;
    },
  };
  return el;
}

function containerWithRows(rowKeys) {
  // makeEl.querySelectorAll 已按 innerHTML 解析并缓存 [data-drill-key] 行，
  // 无需再覆盖 append（避免绑定与点击作用在不同对象上）。
  return makeEl('container');
}

const baseUi = {
  t: (k, f) => f || k,
  label: 'AI 成本',
  metricId: 'ai_cost',
  fmtValue: (v) => `$${v}`,
  fmtRank: (v) => `$${v}`,
  range: '30d',
  dimension: 'model',
  openDrilldown: null,
};

function collectDrilldowns() {
  const calls = [];
  return { calls, openDrilldown: (f) => calls.push(f) };
}

// --- number 卡：点击主值 → 仅 range（无维度 key）---
{
  const { calls, openDrilldown } = collectDrilldowns();
  const container = makeEl('container');
  CHARTS.number.render(container, { number: { value: 1.23, isPercent: false } }, { ...baseUi, openDrilldown });
  const card = container.children[0];
  assert.ok(card, 'number 卡已渲染');
  card.click();
  card.keydown('Enter');
  assert.equal(calls.length, 2, 'click 与 Enter 各触发一次');
  assert.deepEqual(calls[0], { range: '30d' }, 'number 钻取只带 range');
}

// --- bar 行：点击第 2 行 → model=key2（dimension=model）---
{
  const { calls, openDrilldown } = collectDrilldowns();
  const container = containerWithRows(['key1', 'key2']);
  CHARTS.bar.render(container, { rank: [{ key: 'key1', name: 'A', value: 5 }, { key: 'key2', name: 'B', value: 3 }] }, { ...baseUi, openDrilldown });
  const wrap = container.children[0];
  const rows = wrap.querySelectorAll('[data-drill-key]');
  assert.equal(rows.length, 2);
  rows[1].click();
  assert.equal(calls.length, 1);
  assert.deepEqual(calls[0], { range: '30d', model: 'key2' }, 'bar 行按 model 槽位钻取');
}

// --- table 行：dimension=source → source 槽位 ---
{
  const { calls, openDrilldown } = collectDrilldowns();
  const container = containerWithRows(['claude-code']);
  CHARTS.table.render(container, { tableRows: [{ key: 'claude-code', name: 'Claude Code', requests: 2, tokens: 1000, cost: 0.28 }] }, { ...baseUi, dimension: 'source', openDrilldown });
  const rows = container.children[0].querySelectorAll('[data-drill-key]');
  rows[0].click();
  assert.deepEqual(calls[0], { range: '30d', source: 'claude-code' }, 'table 行按 source 槽位钻取');
}

// --- table 行：dimension=provider → provider 槽位 ---
{
  const { calls, openDrilldown } = collectDrilldowns();
  const container = containerWithRows(['openai']);
  CHARTS.table.render(container, { tableRows: [{ key: 'openai', name: 'OpenAI', requests: 1, tokens: 10, cost: 0.01 }] }, { ...baseUi, dimension: 'provider', openDrilldown });
  container.children[0].querySelectorAll('[data-drill-key]')[0].click();
  assert.deepEqual(calls[0], { range: '30d', provider: 'openai' }, 'table 行按 provider 槽位钻取');
}

// --- 无回调环境：不得绑定点击（旧 env 兼容，无 ai-perf-drillable 类）---
{
  const container = makeEl('container');
  CHARTS.number.render(container, { number: { value: 1 } }, baseUi);
  const card = container.children[0];
  assert.ok(!card.listeners.click, '无 openDrilldown 时不绑定 click');
  assert.ok(!card.dataset['cls_ai-perf-drillable'], '无 openDrilldown 时不加 drillable 类');
  const barContainer = containerWithRows(['k']);
  CHARTS.bar.render(barContainer, { rank: [{ key: 'k', name: 'K', value: 1 }] }, baseUi);
  assert.ok(!barContainer.children[0].dataset['cls_ai-perf-drillable'], 'bar 无回调不加 drillable');
  const tableContainer = containerWithRows(['k']);
  CHARTS.table.render(tableContainer, { tableRows: [{ key: 'k', name: 'K' }] }, baseUi);
  assert.ok(!tableContainer.children[0].dataset['cls_ai-perf-drillable'], 'table 无回调不加 drillable');
}

console.log('AI drilldown tests: 6 passed');
