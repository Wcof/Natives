// 基金持仓详情与首页 AI 决策回归测试
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..', '..');
const chartSrc = readFileSync(join(root, 'modules/fund/ui/src/fund-detail-chart.js'), 'utf8');

test('fund-detail-chart: exposes ensureEchartsReady, renderMinuteChart, renderPerfChart, renderProfitChart', () => {
  globalThis.Fund = { t: s => s, home: { escape: s => String(s ?? "") }, themeColor: () => "#345780" };
  new Function(chartSrc)();
  const C = globalThis.Fund.holdingDetailCharts;
  assert.ok(C, 'holdingDetailCharts missing');
  assert.equal(typeof C.ensureEchartsReady, 'function');
  assert.equal(typeof C.renderMinuteChart, 'function');
  assert.equal(typeof C.renderPerfChart, 'function');
  assert.equal(typeof C.renderProfitChart, 'function');
});

test('fund-detail-chart: renderMinuteChart accepts points and sets ECharts options', async () => {
  let optionCalled = false;
  globalThis.echarts = {
    getInstanceByDom: () => null,
    init: () => ({
      setOption: (opt) => {
        optionCalled = true;
        assert.equal(opt.series[0].data.length, 2);
        assert.deepEqual(opt.series[0].data, [10.5, 11.2]);
      },
      clear: () => {},
      dispose: () => {},
      resize: () => {},
    }),
  };

  const chartBox = {};
  await globalThis.Fund.holdingDetailCharts.renderMinuteChart(chartBox, [
    { time: '09:30', price: 10.5 },
    { time: '10:00', price: 11.2 },
  ]);
  assert.ok(optionCalled, 'setOption was not called on minute chart');
});

test('fund-detail-chart: renderProfitChart in hidden mode shows placeholder and does not plot zero line', async () => {
  let setOptionCalled = false;
  let box = { innerHTML: '' };
  globalThis.echarts = {
    getInstanceByDom: () => null,
    init: () => ({
      setOption: () => { setOptionCalled = true; },
      clear: () => {},
      dispose: () => {},
      resize: () => {},
    }),
  };

  await globalThis.Fund.holdingDetailCharts.renderProfitChart(box, [{ date: '2026-10-09', amount: '10' }], true);
  assert.equal(setOptionCalled, false, 'hidden mode must not draw a curve/line');
  assert.ok(box.innerHTML.includes('fundHomeOverviewHistoryHidden'), 'must display privacy hidden text placeholder');
});

test('fund-detail-chart: renderPerfChart sets series values and dates', async () => {
  let perfOption = null;
  globalThis.echarts = {
    getInstanceByDom: () => null,
    init: () => ({
      setOption: (opt) => { perfOption = opt; },
      clear: () => {},
      dispose: () => {},
      resize: () => {},
    }),
  };

  const box = {};
  await globalThis.Fund.holdingDetailCharts.renderPerfChart(box, [
    { navDate: '2026-10-08', unitNav: '1.0500' },
    { navDate: '2026-10-09', unitNav: '1.0600' },
  ]);
  assert.ok(perfOption, 'perfOption must be set');
  assert.deepEqual(perfOption.xAxis.data, ['2026-10-08', '2026-10-09']);
  assert.deepEqual(perfOption.series[0].data, [1.05, 1.06]);
});

test('private history plots ledger amounts and preserves missing-price gaps', async () => {
  let option;
  globalThis.echarts = {getInstanceByDom:()=>null,init:()=>({setOption:value=>{option=value;},dispose(){}})};
  await Fund.holdingDetailCharts.renderProfitChart({},[
    {date:'2026-01-05',amount:'9.90'},
    {date:'2026-01-06',amount:null},
    {date:'2026-01-07',amount:'-12.00'},
  ],false);
  assert.deepEqual(option.series[0].data,[9.9,null,-12]);
  assert.deepEqual(option.xAxis.data,['2026-01-05','2026-01-06','2026-01-07']);
});

test('leaving or hiding a chart invalidates a delayed ECharts load', async () => {
  delete globalThis.echarts;
  let ready;
  Fund.ensureEcharts=()=>new Promise(resolve=>{ready=resolve;});
  const pending=Fund.holdingDetailCharts.renderProfitChart({},[{date:'2026-01-07',amount:'12'}],false);
  Fund.holdingDetailCharts.disposeAll();
  let initialized=0;
  globalThis.echarts={getInstanceByDom:()=>null,init:()=>{initialized++;return {setOption(){}};}};
  ready();await pending;assert.equal(initialized,0);
  delete globalThis.echarts;
  const box={innerHTML:''};
  const pendingHidden=Fund.holdingDetailCharts.renderProfitChart(box,[{date:'2026-01-07',amount:'12'}],false);
  await Fund.holdingDetailCharts.renderProfitChart(box,[],true);
  globalThis.echarts={getInstanceByDom:()=>null,init:()=>{initialized++;return {setOption(){}};}};
  ready();await pendingHidden;assert.equal(initialized,0);
  assert.ok(box.innerHTML.includes('fundHomeOverviewHistoryHidden'));
});
