// §5.5 动态切换维度：外观（colour/fontSize/fontWeight）切换不得触发数据查询；
// 样式即时生效且无旧色快照。真实 Chrome + 真实 space-dashboard.js Shadow DOM。
// 产物：tests/artifacts/space-ai-appearance-switch.json。

import { chromium } from 'playwright-core';
import { writeFileSync, rmSync, readFileSync } from 'node:fs';
import { join, dirname, extname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createServer } from 'node:http';

const __dirname = dirname(fileURLToPath(import.meta.url));
const extRoot = join(__dirname, '..');

const MIME = {
  '.js': 'text/javascript', '.mjs': 'text/javascript', '.json': 'application/json',
  '.html': 'text/html', '.css': 'text/css',
};
const server = createServer((req, res) => {
  const urlPath = decodeURIComponent(new URL(req.url, 'http://x').pathname);
  try {
    const body = readFileSync(join(extRoot, urlPath));
    res.writeHead(200, { 'content-type': MIME[extname(urlPath)] || 'application/octet-stream' });
    res.end(body);
  } catch {
    res.writeHead(404); res.end('not found');
  }
});
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
const baseUrl = `http://127.0.0.1:${server.address().port}`;

const hostFixture = JSON.parse(readFileSync(join(extRoot, 'ai-performance', 'host-fixture.json'), 'utf8'));
const READY = {
  overview: hostFixture.overview,
  analysis: {
    status: 'ready',
    byModel: [{ key: 'test-model-a', tokens: 1000000, requests: 2, costUsd: 0.28 }],
    bySource: [{ key: 'claude-code', tokens: 1000000, requests: 2, costUsd: 0.28 }],
    byProvider: [],
    byHour: hostFixture.overview.trend?.map((t) => ({ hour: t.timestamp, tokens: t.tokens, requests: t.requests, costUsd: t.costUsd })) || [],
  },
  events: { events: [{ id: 'ev1', requestedAt: '2026-09-11T10:00:00Z', source: 'claude-code', tokens: 1000, model: 'test-model-a' }] },
  sessions: {
    status: 'ready', totalSessions: 2, activeDays: 1, unattributed: 0,
    byDay: [{ date: '2026-09-10', sessions: 1 }, { date: '2026-09-12', sessions: 1 }],
    bySource: [{ key: 'claude-code', requests: 2, tokens: 1000000, costUsd: 0.28 }],
  },
};

writeFileSync(
  join(extRoot, 'space-verify-boot.html'),
  `<!doctype html><html><body style="margin:0"><main id="dashboard-host" style="width:440px;height:320px;background:#101210"></main>
   <script>
     window.__nativeCalls = 0;
     window.__nativeFixture = null;
     fetch('/space-verify-native-fixture.json').then((r) => r.json()).then((d) => { window.__nativeFixture = d; });
     window.chrome = {
       runtime: {
         connectNative: () => {
           const listeners = new Set();
           return {
             onMessage: { addListener: (fn) => listeners.add(fn) },
             onDisconnect: { addListener: () => {} },
             postMessage: (msg) => {
               window.__nativeCalls += 1; // 计数每次聚合查询
               const respond = () => {
                 if (!window.__nativeFixture) return;
                 const f = window.__nativeFixture;
                 const result = msg.method === 'model_usage_overview' ? f.overview
                   : msg.method === 'model_usage_analysis' ? f.analysis
                   : msg.method === 'model_usage_events' ? f.events
                   : msg.method === 'model_usage_sessions' ? f.sessions
                   : { status: 'ready' };
                 for (const fn of [...listeners]) fn({ id: msg.id, ok: true, result });
               };
               if (window.__nativeFixture) respond(); else setTimeout(respond, 50);
             },
             disconnect: () => {},
           };
         },
       },
     };
   </script>
   <script type="module" src="/space-verify-boot.mjs"></script></body></html>`
);
writeFileSync(join(extRoot, 'space-verify-native-fixture.json'), JSON.stringify(READY));
writeFileSync(
  join(extRoot, 'space-verify-boot.mjs'),
  `import { createSpaceDashboard } from './space-dashboard.js';
   import { widgetPlugins } from './plugins/widgets/index.js';
   import { backgroundPlugins } from './plugins/backgrounds/index.js';
   window.__createDashboard = createSpaceDashboard;
   window.__widgetPlugins = widgetPlugins;
   window.__backgroundPlugins = backgroundPlugins;
   window.__render = async () => {
     const host = document.getElementById('dashboard-host');
     const dashboard = window.__createDashboard({
       $: (sel) => document.getElementById(sel) || document.querySelector('#' + sel),
       t: (key2, fallback) => fallback || key2,
       selectedLanguage: 'zh_CN',
       backgroundPlugins: window.__backgroundPlugins,
       widgetPlugins: window.__widgetPlugins,
       nativeCall: async (method) => { throw new Error('unexpected nativeCall ' + method); },
       broadcastRevision: () => {},
       queueWorkspaceMutation: () => {},
       toast: () => {},
     });
     window.__dashboard = dashboard;
     const widget = { id: 'w-0', key: 'widget/aiCost', enabled: true, configJson: { metric: 'ai_cost', view: 'usage', chart: 'number', range: '30d', dimension: 'model', subMetric: 'total' }, displayJson: { position: 'free', xPercent: 50, yPercent: 50 } };
     window.__widget = widget;
     dashboard.render({
       revision: 1,
       backgroundJson: { key: 'background/colour', display: { colour: '#101210', brightness: 1 } },
       widgets: [widget],
     }, 'ws-test', () => {});
     await new Promise((r) => setTimeout(r, 700));
   };
   // 外观更新走产品真实路径：同一 configJson + 新 displayJson 再次 render
   //（dashboard 内部经 patchWidget → applyWidgetDisplayStyles，不重挂载数据）。
   window.__reapplyAppearance = async (disp) => {
     window.__widget.displayJson = { ...window.__widget.displayJson, ...disp };
     window.__dashboard.render({
       revision: 2,
       backgroundJson: { key: 'background/colour', display: { colour: '#101210', brightness: 1 } },
       widgets: [window.__widget],
     }, 'ws-test', () => {});
   };`
);

const browser = await chromium.launch({ executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true });
let result;
try {
  const context = await browser.newContext({ viewport: { width: 480, height: 360 }, deviceScaleFactor: 2 });
  const page = await context.newPage();
  page.on('pageerror', (e) => console.error('[appearance-switch pageerror]', String(e).slice(0, 300)));

  await page.goto(`${baseUrl}/space-verify-boot.html`);
  await page.waitForFunction(() => window.__createDashboard, null, { timeout: 15000 });
  await page.evaluate(() => window.__render());
  await page.waitForFunction(() => window.__nativeCalls > 0, null, { timeout: 10000 });

  const reads = async () => {
    return page.evaluate(() => {
      const shadow = document.getElementById('dashboard-host').shadowRoot;
      const el = shadow.querySelector('[data-widget-id="w-0"]');
      const value = shadow.querySelector('.ai-perf-number-value');
      const cs = value ? getComputedStyle(value) : null;
      return {
        nativeCalls: window.__nativeCalls,
        cardColour: el ? el.style.getPropertyValue('--space-widget-text') : null,
        fontSize: cs ? cs.fontSize : null,
        fontWeight: cs ? cs.fontWeight : null,
        valueText: value ? value.textContent : null,
      };
    });
  };

  const before = await reads();

  // 外观切换 1：文字色 + 字重（不重新 render 数据）。
  await page.evaluate(() => window.__reapplyAppearance({ colour: '#ffd54a', fontWeight: '700', useAccentColor: true }));
  await page.waitForTimeout(300);
  const after1 = await reads();

  // 外观切换 2：主指标字号放大。
  await page.evaluate(() => window.__reapplyAppearance({ colour: '#ffd54a', fontWeight: '700', fontSize: 32, useAccentColor: true }));
  await page.waitForTimeout(300);
  const after2 = await reads();

  // 断言：查询次数不变（外观变化不发数据请求）。
  const queriesUnchanged = before.nativeCalls === after1.nativeCalls && after1.nativeCalls === after2.nativeCalls;
  // 断言：样式生效（colour 变量或字号改变；统计值文本不变）。
  const styleChanged = after2.fontSize !== before.fontSize || after2.fontWeight !== before.fontWeight || after2.cardColour !== before.cardColour;
  const valueStable = before.valueText === after2.valueText;

  result = {
    generatedAt: new Date().toISOString(),
    note: '测试数据 fixture，非生产数据',
    before, after1, after2,
    assertions: { queriesUnchanged, styleChanged, valueStable },
    pass: queriesUnchanged && styleChanged && valueStable,
  };
  writeFileSync(join(extRoot, 'tests', 'artifacts', 'space-ai-appearance-switch.json'), JSON.stringify(result, null, 2));
  console.log(`appearance-switch: queries ${before.nativeCalls}->${after2.nativeCalls} unchanged=${queriesUnchanged} styleChanged=${styleChanged} valueStable=${valueStable}`);
  if (!result.pass) process.exitCode = 1;
} finally {
  rmSync(join(extRoot, 'space-verify-boot.mjs'), { force: true });
  rmSync(join(extRoot, 'space-verify-boot.html'), { force: true });
  rmSync(join(extRoot, 'space-verify-native-fixture.json'), { force: true });
  server.close();
  await browser.close();
}
