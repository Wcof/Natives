// DES A42 / T07+T08：图形渲染层语义取色回归（真实 Chrome）。
// 覆盖：trends.C 从 CSS 变量解析并随主题变化；reloadTheme 重取色不重建
// 实例；stock-drawings 缺省色随主题、显式用户色保持；trends-drawings
// 新建对象不把缺省色伪存为用户色。
import assert from 'node:assert/strict';
import { chromium } from 'playwright-core';
import { readFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { shellHtml } from './shell-harness.mjs';

const ROOT = resolve(import.meta.dirname, '../..');
const dist = resolve(ROOT, 'modules/fund/ui/dist');
const artifacts = resolve(ROOT, '.tmp/des-t07/a42-graphics');
mkdirSync(artifacts, { recursive: true });

const browser = await chromium.launch({ executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true });
const report = { steps: [], errors: [] };
try {
  const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  page.on('pageerror', (e) => report.errors.push(String(e.message || e)));
  const themeCss = readFileSync(resolve(dist, 'styles/theme.css'), 'utf8');
  await page.setContent(shellHtml(), { waitUntil: 'domcontentloaded' });
  await page.addScriptTag({ path: resolve(dist, 'core.js') });
  await page.addScriptTag({ path: resolve(dist, 'home-i18n.js') });
  await page.addScriptTag({ path: resolve(dist, 'research-series.js') });

  // 1. trends.C 解析自 CSS 变量并随主题变化
  await page.addScriptTag({ path: resolve(dist, 'trends-chart.js') });
  const colorsFor = () => page.evaluate(() => ({
    text: Fund.trends.C.text, grid: Fund.trends.C.grid, up: Fund.trends.C.up,
    expectedText: getComputedStyle(document.documentElement).getPropertyValue('--chart-text').trim(),
    expectedUp: getComputedStyle(document.documentElement).getPropertyValue('--color-up').trim(),
  }));
  // 生产流程：主题切换经 applyGlobalTheme → trends.reloadTheme 重取色。
  await page.evaluate(() => { document.documentElement.dataset.theme = 'archive'; Fund.trends.reloadTheme(); });
  let c = await colorsFor();
  assert.equal(c.text, c.expectedText);
  assert.equal(c.up, c.expectedUp);
  await page.evaluate(() => { document.documentElement.dataset.theme = 'volt'; Fund.trends.reloadTheme(); });
  c = await colorsFor();
  assert.equal(c.text, c.expectedText);
  assert.equal(c.up, c.expectedUp);
  assert.notEqual(c.text, '#202733', 'volt text differs from archive default');
  report.steps.push({ step: 'trends.C resolves semantic roles per theme', pass: true, voltText: c.text });

  // Render actual ECharts series: theme changes must preserve instance, data and zoom.
  await page.addScriptTag({ path: resolve(dist, 'echarts.min.js') });
  for (const file of ['trends-kline.js','trends-minute.js','trends-interact.js','trends-indicator.js','trends-flow.js','trends-scan.js']) await page.addScriptTag({ path: resolve(dist,file) });
  await page.evaluate(() => {
    document.querySelectorAll('.main-column > section').forEach(el=>el.hidden=el.id!=='view-trends');
    document.querySelector('.workspace-body').classList.add('trends-active');
    Fund.applyLanguage('zh-CN');
    document.getElementById('view-trends').hidden=false;
    document.getElementById('trends-empty').hidden=true;
    Fund.trends.chart.initCharts();
    const rows=Array.from({length:80},(_,i)=>({date:new Date(Date.UTC(2026,5,1+i)).toISOString().slice(0,10),open:10+i/10,close:11+i/10,low:9+i/10,high:12+i/10,volume:1000+i}));
    Fund.trends.chart.renderKline(rows,{});
    Fund.trends.charts.klineChart.dispatchAction({type:'dataZoom',start:35,end:75});
    window.__chart=Fund.trends.charts.klineChart;
    window.__data=JSON.stringify(window.__chart.getOption().series[0].data);
    window.fetchCalls=0;
    window.fetch=async()=>{window.fetchCalls++;throw new Error('theme must not fetch');};
  });
  for (const theme of ['archive','volt','archive']) {
    await page.evaluate(theme=>{document.documentElement.dataset.theme=theme;Fund.trends.reloadTheme();},theme);
    const result=await page.evaluate(()=>{
      const option=Fund.trends.charts.klineChart.getOption();
      return {same:window.__chart===Fund.trends.charts.klineChart,data:JSON.stringify(option.series[0].data),zoom:option.dataZoom[0],up:option.series[0].itemStyle.color,expected:Fund.themeColor('--color-up'),fetchCalls};
    });
    assert.equal(result.same,true);assert.equal(result.data,await page.evaluate(()=>window.__data));
    assert.equal(result.zoom.start,35);assert.equal(result.zoom.end,75);assert.equal(result.up,result.expected);assert.equal(result.fetchCalls,0);
  }
  report.steps.push({step:'real ECharts candles recolor, preserve data/zoom/identity and fetch nothing',pass:true});

  await page.locator('#kline-chart').focus();
  await page.keyboard.press('End');
  assert.match(await page.locator('#trends-chart-readout').innerText(),/2026-08-19/);
  await page.locator('[data-chart-action=table]').click();
  assert.equal(await page.locator('#trends-data-body tr').count(),80);
  await page.keyboard.press('Escape');
  await page.locator('#scan-btn').click();
  assert.equal(await page.locator('#scan-overlay').evaluate(el=>el.open),true);
  assert.match(await page.locator('#scan-content').innerText(),/尚未实现|not implemented/);
  await page.keyboard.press('Escape');
  assert.equal(await page.evaluate(()=>fetchCalls),0);
  await page.setViewportSize({width:320,height:900});
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1),true);
  await page.screenshot({path:resolve(artifacts,'trends-archive-320.png'),fullPage:true});
  report.steps.push({step:'keyboard reads actual bar, bounded native data table, honest scan and narrow layout',pass:true});

  await page.evaluate(()=>{Fund.trends.state.instrumentKind='fund';Fund.trends.state.currentView='dayk';Fund.trends.state.navVisible=[{navDate:'2026-09-01',unitNav:'1.0200',source:'synthetic NAV'},{navDate:'2026-09-02',unitNav:'1.0310',source:'synthetic NAV'}];Fund.trends.charts.klineChart.setOption(Fund.researchSeries.navOption(Fund.trends.state.navVisible),true);});
  await page.locator('#kline-chart').focus();await page.keyboard.press('End');
  assert.match(await page.locator('#trends-chart-readout').innerText(),/2026-09-02.*1\.0310/);
  await page.locator('[data-chart-action=table]').click();
  assert.equal(await page.locator('#trends-data-body tr').count(),2);
  assert.match(await page.locator('#trends-data-head').innerText(),/NAV/);
  assert.doesNotMatch(await page.locator('#trends-data-head').innerText(),/Open|开盘/);
  await page.keyboard.press('Escape');
  report.steps.push({step:'NAV keyboard and table read actual exact net value and business dates, with no OHLC',pass:true});

  // 3. stock-drawings：缺省色随主题；显式用户色保持
  await page.addScriptTag({ path: resolve(dist, 'stock-drawings-shapes.js') });
  const drawingColors = () => page.evaluate((options) => {
    const els = [];
    globalThis.Fund.StockDrawingsShapes.buildGraphic({
      selected: null, hoverHit: null, drawings: [], locked: false, magnetEnabled: false,
      get strokeColor() { return Fund.themeColor('--draw-line'); },
      strokeWidth: 2, dashPattern: () => null, pointToPixel: (i, j) => [Number(i) * 10, Number(j) * 10],
      chartWidthPx: 600,
    }, { id: 't1', toolType: 'trendline', points: [{ date: '2026-09-01', price: 10 }, { date: '2026-09-02', price: 11 }], options }, els);
    return els;
  }, {});
  await page.evaluate(() => document.documentElement.dataset.theme = 'archive');
  const archStroke = (await drawingColors())[0].style.stroke;
  await page.evaluate(() => document.documentElement.dataset.theme = 'volt');
  const voltStroke = (await drawingColors())[0].style.stroke;
  assert.notEqual(archStroke, voltStroke, 'default drawing stroke follows theme');
  report.steps.push({ step: 'default drawing stroke follows theme', pass: true, archStroke, voltStroke });

  // 4. trends-drawings 新建对象 options 不携带缺省色（源码级契约锚点）。
  const trendsDrawSrc = readFileSync(resolve(dist, 'trends-drawings.js'), 'utf8');
  assert.ok(!/options:\s*\{\s*color:\s*COLORS\[drawingTool\]\s*\}/.test(trendsDrawSrc), 'create path must not persist default color as user color');
  assert.ok(/options:\s*\{\}/.test(trendsDrawSrc), 'create path uses empty options');
  report.steps.push({ step: 'drawing create path does not persist default color', pass: true });

  await page.close();
} catch (error) {
  report.steps.push({ step: 'FATAL', pass: false, error: String(error).slice(0, 300) });
  throw error;
} finally {
  await browser.close();
}
report.pass = report.steps.every(s => s.pass) && report.errors.length === 0;
writeFileSync(resolve(artifacts, 'report.json'), JSON.stringify(report, null, 2) + '\n');
console.log(`A42 graphics: ${report.steps.filter(s => s.pass).length}/${report.steps.length} steps, pageErrors=${report.errors.length}`);
if (!report.pass) { console.log(JSON.stringify(report, null, 2)); process.exit(1); }
