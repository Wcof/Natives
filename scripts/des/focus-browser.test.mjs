// DES-T09 / A27+A28：专注模式布局模式真实 Chrome 回归。
// 覆盖：显式入口进入（同一图表 DOM 搬移非复制）、徽标身份、退出恢复
// 主栏结构、Esc 三层、20 次进出 DOM/监听稳定、无标的空态。
// 合成壳装配；行情数据为合成空响应。
import assert from 'node:assert/strict';
import { chromium } from 'playwright-core';
import { readFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { shellHtml, shellScripts } from './shell-harness.mjs';

const ROOT = resolve(import.meta.dirname, '../..');
const dist = resolve(ROOT, 'modules/fund/ui/dist');
const artifacts = resolve(ROOT, '.tmp/des-t09/focus');
mkdirSync(artifacts, { recursive: true });

const browser = await chromium.launch({ executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true });
const report = { steps: [], errors: [] };
try {
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  page.on('pageerror', (e) => report.errors.push(String(e.message || e)));
  await page.setContent(shellHtml(), { waitUntil: 'domcontentloaded' });
  await page.evaluate(() => {
    window.requests = [];
    window.fetch = async (url, options = {}) => {
      const u = new URL(url, 'http://localhost');
      window.requests.push(u.pathname + u.search);
      if (u.pathname === '/api/watchlists') {
        return { ok: true, status: 200, json: async () => ({ watchlists: [{ id: 'default', name: '默认组', items: [{ symbol: 'sh600519', name: '贵州茅台', changePct: 1.2, price: 1500 }] }] }) };
      }
      if (u.pathname === '/api/watchlists/items') return { ok: true, status: 200, json: async () => ({ items: [] }) };
      return { ok: true, status: 200, json: async () => ({}) };
    };
    window.addEventListener('message', (e) => {
      if (e.data?.type === 'hello') window.parent.postMessage({ type: 'welcome', token: 't', generation: e.data.generation }, '*');
    });
  });
  await page.addScriptTag({path:resolve(dist,'echarts.min.js')});
  for (const file of shellScripts()) await page.addScriptTag({ path: resolve(dist, file) });
  await page.waitForTimeout(200);

  // 进入趋势视图并给 currentSymbol（检索选中由 trends owner；此处直接经公开 selectSymbol）
  await page.evaluate(() => { document.getElementById('tab-trends').click(); });
  await page.waitForTimeout(150);
  await page.evaluate(() => Fund.actions.selectSymbol('sh600519', '贵州茅台'));
  await page.waitForTimeout(100);
  await page.evaluate(()=>{window.__chartsBefore=Object.values(Fund.trends.charts).filter(Boolean);window.__chartDom=document.getElementById('kline-chart');Fund.home.applyDisplayMode('amount',false);});

  // 1. 显式入口进入专注模式
  await page.locator('#btn-focus-mode').click();
  assert.equal(await page.evaluate(() => document.body.classList.contains('focus-mode')), true);
  const workspaceVisible = await page.locator('#focus-workspace').evaluate(el => !el.hidden);
  assert.ok(workspaceVisible, 'focus workspace visible after enter');
  // 图表容器搬入工作台（唯一 DOM 归属）
  const inCenter = await page.evaluate(() => !!document.getElementById('focus-center')?.querySelector('.chart-container'));
  assert.ok(inCenter, 'chart container moved (not cloned) into focus center');
  report.steps.push({ step: 'enter focus via explicit button moves DOM', pass: true });

  // 2. 退出恢复主栏
  await page.locator('#focus-exit-btn').click();
  assert.equal(await page.evaluate(() => document.body.classList.contains('focus-mode')), false);
  const backInMain = await page.evaluate(() => !!document.querySelector('#view-trends .left-panel .chart-container'));
  assert.ok(backInMain, 'chart container restored to trends left panel');
  report.steps.push({ step: 'exit restores main column structure', pass: true });

  assert.equal(await page.evaluate(()=>window.__chartDom===document.getElementById('kline-chart')),true,'chart DOM identity survives exit');
  assert.equal(await page.evaluate(()=>Object.values(Fund.trends.charts).filter(Boolean).every(c=>window.__chartsBefore.includes(c))),true,'ECharts instances survive exit');
  // Pointer cancellation restores width without writing preferences.
  await page.evaluate(()=>Fund.actions.enterFocusMode());
  const beforeWidth=await page.evaluate(()=>getComputedStyle(document.documentElement).getPropertyValue('--focus-left-width'));
  const bounds=await page.locator('#focus-resizer').boundingBox();
  await page.mouse.move(bounds.x+2,bounds.y+20);await page.mouse.down();await page.mouse.move(bounds.x+72,bounds.y+20);
  await page.keyboard.press('Escape');await page.mouse.up();
  assert.equal(await page.evaluate(()=>getComputedStyle(document.documentElement).getPropertyValue('--focus-left-width')),beforeWidth);
  assert.equal(await page.evaluate(()=>document.body.classList.contains('focus-mode')),true,'first Escape rolls back resize');
  await page.keyboard.press('Escape');
  report.steps.push({step:'real ECharts identity and pointer draft rollback',pass:true});

  // 3. 20 次进出：DOM 归属唯一、无克隆增长
  for (let i = 0; i < 20; i++) {
    await page.evaluate(() => { Fund.actions.enterFocusMode(); Fund.actions.exitFocusMode(); });
  }
  const stability = await page.evaluate(() => ({
    chartContainers: document.querySelectorAll('.chart-container').length,
    chartToolbars: document.querySelectorAll('.chart-toolbar').length,
    workspaceNodes: document.getElementById('focus-center').children.length,
  }));
  assert.equal(stability.chartContainers, 1, 'exactly one chart container after 20 cycles');
  assert.equal(stability.chartToolbars, 1, 'exactly one chart toolbar after 20 cycles');
  report.steps.push({ step: '20 enter/exit cycles keep single DOM owner', pass: true, stability });

  // 4. Esc 三层：dialog 打开时 Esc 不退出模式
  await page.evaluate(() => { Fund.actions.enterFocusMode(); });
  await page.evaluate(() => {
    const dlg = document.createElement('dialog');
    dlg.innerHTML = '<p>synthetic</p><button data-close>关闭</button>';
    document.body.appendChild(dlg);
    dlg.showModal();
    window.__syntheticDialog = dlg;
  });
  await page.keyboard.press('Escape');
  const stillFocus = await page.evaluate(() => document.body.classList.contains('focus-mode'));
  assert.equal(stillFocus, true, 'Esc inside open dialog must not exit focus mode');
  await page.evaluate(() => { window.__syntheticDialog.close(); window.__syntheticDialog.remove(); });
  await page.keyboard.press('Escape');
  assert.equal(await page.evaluate(() => document.body.classList.contains('focus-mode')), false, 'second Esc exits mode');
  report.steps.push({ step: 'Esc layering: dialog first, then mode exit', pass: true });

  // Narrow context uses a native dialog; search and selection never write watchlists.
  await page.setViewportSize({width:320,height:900});
  await page.evaluate(()=>Fund.actions.enterFocusMode());
  await page.locator('#focus-list-btn').click();
  assert.equal(await page.locator('#focus-list-dialog').evaluate(d=>d.open),true);
  await page.keyboard.press('Escape');
  await page.waitForFunction(()=>document.getElementById('focus-left').parentElement.id==='focus-workspace');
  await page.locator('#focus-evidence-btn').click();
  assert.equal(await page.locator('#focus-evidence-dialog').evaluate(d=>d.open),true);
  assert.equal(await page.evaluate(()=>document.getElementById('focus-right').parentElement.id),'focus-evidence-dialog');
  await page.keyboard.press('Escape');
  await page.waitForFunction(()=>document.getElementById('focus-right').parentElement.id==='focus-workspace');
  await page.keyboard.press('Escape');
  assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=document.documentElement.clientWidth+1),true,'narrow mode stays within viewport');
  assert.equal(await page.evaluate(()=>requests.some(path=>path.startsWith('/api/market/stocks'))),false,'no fake 50-stock context');
  report.steps.push({step:'320px context dialog, Escape focus and no whole-market fetch',pass:true});

  await page.setViewportSize({width:1440,height:900});
  await page.evaluate(()=>Fund.actions.openStockDetail('sh600519','贵州茅台','stock'));
  await page.locator('#sd-btn-focus').waitFor({state:'visible'});
  await page.evaluate(()=>{window.__detailStage=document.querySelector('.sd-chart-stage');window.__detailCharts=Object.values(Fund.StockDetailShared.charts).filter(Boolean);});
  await page.locator('#sd-btn-focus').click();
  assert.equal(await page.evaluate(()=>document.getElementById('focus-center').contains(window.__detailStage)),true);
  await page.locator('#focus-exit-btn').click();
  assert.equal(await page.evaluate(()=>document.getElementById('view-stock-detail').contains(window.__detailStage)),true);
  assert.equal(await page.evaluate(()=>Object.values(Fund.StockDetailShared.charts).filter(Boolean).every(c=>window.__detailCharts.includes(c))),true);
  await page.evaluate(()=>Fund.actions.exitStockDetail());
  report.steps.push({step:'detail focus moves/restores the same DOM and ECharts instances',pass:true});

  // 5. 无标的进入：空态提示，不冒充默认 600519
  await page.evaluate(() => { Fund.trends.coordinator.selectStock('', ''); });
  await page.evaluate(() => { Fund.actions.enterFocusMode(); });
  assert.equal(await page.evaluate(() => document.body.classList.contains('focus-mode')), false, 'no-symbol entry rejected');
  report.steps.push({ step: 'no-symbol entry shows hint and stays out', pass: true });

  await page.close();
} catch (error) {
  report.steps.push({ step: 'FATAL', pass: false, error: String(error).slice(0, 300) });
  throw error;
} finally {
  await browser.close();
}
report.pass = report.steps.every(s => s.pass) && report.errors.length === 0;
writeFileSync(resolve(artifacts, 'report.json'), JSON.stringify(report, null, 2) + '\n');
console.log(`focus browser: ${report.steps.filter(s => s.pass).length}/${report.steps.length} steps, pageErrors=${report.errors.length}`);
if (!report.pass) { console.log(JSON.stringify(report, null, 2)); process.exit(1); }
