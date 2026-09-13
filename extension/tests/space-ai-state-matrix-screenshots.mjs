// §5.5 图形与状态矩阵：六形态外的状态维度（ready/empty/error/loading/缺价）
// 真实 Chrome + 真实 space-dashboard.js Shadow DOM；Native Messaging mock 按
// 状态分发（ready 用真实 Host 导出 fixture，§9.4 契约 fixture 不手写）。
// 产物：tests/artifacts/ai-state-matrix/<card>-<chart>-<state>.png + manifest.json。

import { chromium } from 'playwright-core';
import { mkdirSync, writeFileSync, rmSync, readFileSync } from 'node:fs';
import { join, dirname, extname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createServer } from 'node:http';

const __dirname = dirname(fileURLToPath(import.meta.url));
const extRoot = join(__dirname, '..');
const artifactsDir = join(extRoot, 'tests', 'artifacts', 'ai-state-matrix');
mkdirSync(artifactsDir, { recursive: true });

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

// 状态分发：ready=真实 Host fixture；empty=Host 明确空；error=Host 调用失败；
// loading=延迟应答（截图时仍未返回）；unpriced=有用量但价格缺失（estimatedCostUsd=null）。
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
const EMPTY = { overview: { status: 'empty' }, analysis: { status: 'empty' }, events: { events: [] }, sessions: { status: 'empty' } };
const UNPRICED = JSON.parse(JSON.stringify(READY));
UNPRICED.overview.metrics.estimatedCostUsd = null; // 缺价：金额 null，不得伪装 0

// 状态矩阵：三卡 × number 形态 × 5 状态 + 成本/Token line 形态 × empty/error。
const MATRIX = [];
for (const card of ['aiCost', 'aiTokens', 'aiSessions']) {
  for (const state of ['ready', 'empty', 'error', 'loading', 'unpriced']) {
    MATRIX.push({ card, chart: 'number', state });
  }
}
for (const state of ['empty', 'error', 'loading']) {
  MATRIX.push({ card: 'aiCost', chart: 'line', state });
  MATRIX.push({ card: 'aiTokens', chart: 'line', state });
}

writeFileSync(
  join(extRoot, 'space-verify-boot.html'),
  `<!doctype html><html><body style="margin:0"><div style="display:flex;flex-direction:column;width:100vw;height:100vh">
   <main id="dashboard-host" class="dashboard-host" style="flex:1 1 auto;min-width:0;min-height:0;position:relative;overflow:hidden"></main></div>
   <script>
     window.__nativeFixture = null;
     window.__nativeDelayMs = 0;
     fetch('/space-verify-native-fixture.json').then((r) => r.json()).then((d) => { window.__nativeFixture = d; });
     window.chrome = {
       runtime: {
         connectNative: () => {
           const listeners = new Set();
           return {
             onMessage: { addListener: (fn) => listeners.add(fn) },
             onDisconnect: { addListener: () => {} },
             postMessage: (msg) => {
               const respond = () => {
                 if (!window.__nativeFixture) return;
                 const f = window.__nativeFixture;
                 if (f.__mode === 'error') {
                   for (const fn of [...listeners]) fn({ id: msg.id, ok: false, error: { message: 'host unavailable (test)' } });
                   return;
                 }
                 const result = msg.method === 'model_usage_overview' ? f.overview
                   : msg.method === 'model_usage_analysis' ? f.analysis
                   : msg.method === 'model_usage_events' ? f.events
                   : msg.method === 'model_usage_sessions' ? f.sessions
                   : { status: 'ready' };
                 for (const fn of [...listeners]) fn({ id: msg.id, ok: true, result });
               };
               if (window.__nativeDelayMs > 0) setTimeout(respond, window.__nativeDelayMs);
               else if (window.__nativeFixture) respond();
               else setTimeout(respond, 50);
             },
             disconnect: () => {},
           };
         },
       },
     };
   </script>
   <script type="module" src="/space-verify-boot.mjs"></script></body></html>`
);
writeFileSync(
  join(extRoot, 'space-verify-native-fixture.json'),
  JSON.stringify({ __mode: 'ready', ...READY })
);
writeFileSync(
  join(extRoot, 'space-verify-boot.mjs'),
  `import { createSpaceDashboard } from './space-dashboard.js';
   import { widgetPlugins } from './plugins/widgets/index.js';
   import { backgroundPlugins } from './plugins/backgrounds/index.js';
   window.__createDashboard = createSpaceDashboard;
   window.__widgetPlugins = widgetPlugins;
   window.__backgroundPlugins = backgroundPlugins;
   window.__renderOne = async (key, metric, chart, bg) => {
     const host = document.getElementById('dashboard-host');
     host.style.width = '420px';
     host.style.height = '300px';
     host.style.background = bg;
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
     dashboard.render({
       revision: 1,
       backgroundJson: { key: 'background/colour', display: { colour: bg, brightness: 1 } },
       widgets: [{ id: 'w-0', key, enabled: true, configJson: { metric, view: 'usage', chart, range: '30d', dimension: 'model', subMetric: 'total' }, displayJson: { position: 'free', xPercent: 5, yPercent: 5 } }],
     }, 'ws-test', () => {});
     await new Promise((r) => setTimeout(r, 700));
   };`
);

const browser = await chromium.launch({ executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true });
const manifest = [];
try {
  const context = await browser.newContext({ viewport: { width: 460, height: 340 }, deviceScaleFactor: 2 });
  const page = await context.newPage();
  page.on('pageerror', (e) => console.error('[state-matrix pageerror]', String(e).slice(0, 300)));

  const bgHex = '#101210'; // 深色空间背景（§5.5 状态维度默认深/浅之一，form-matrix 已覆盖浅）
  for (const entry of MATRIX) {
    // 每次重新加载：宿主元素已 attachShadow，复用会抛 NotSupportedError。
    await page.goto(`${baseUrl}/space-verify-boot.html`);
    await page.waitForFunction(() => window.__createDashboard, null, { timeout: 15000 });

    // 按状态配置 mock。
    if (entry.state === 'error') {
      await page.evaluate(() => { window.__nativeFixture = { __mode: 'error' }; window.__nativeDelayMs = 0; });
    } else if (entry.state === 'loading') {
      await page.evaluate(() => { window.__nativeFixture = null; window.__nativeDelayMs = 10000; });
    } else if (entry.state === 'empty') {
      await page.evaluate((empty) => { window.__nativeFixture = empty; window.__nativeDelayMs = 0; }, EMPTY);
    } else if (entry.state === 'unpriced') {
      await page.evaluate((u) => { window.__nativeFixture = u; window.__nativeDelayMs = 0; }, UNPRICED);
    } else {
      await page.evaluate((ready) => { window.__nativeFixture = ready; window.__nativeDelayMs = 0; }, READY);
    }

    await page.evaluate(({ key, metric, chart: ch, bgHex: bgc }) =>
      window.__renderOne(key, metric, ch, bgc), { key: `widget/${entry.card}`, metric: entry.metric || (entry.card === 'aiSessions' ? 'ai_sessions' : entry.card === 'aiTokens' ? 'token_usage' : 'ai_cost'), chart: entry.chart, bgHex });

    // loading 态在应答到达前截图；其余等应答 + 双帧渲染。
    if (entry.state === 'loading') {
      await page.waitForTimeout(400);
    } else {
      await page.waitForFunction(() => window.__nativeFixture !== null, null, { timeout: 5000 }).catch(() => {});
      await page.waitForTimeout(entry.state === 'error' ? 300 : 500);
    }
    await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));

    const box = await page.evaluate(() => {
      const el = document.getElementById('dashboard-host').shadowRoot.querySelector('[data-widget-id="w-0"]');
      if (!el) return null;
      const r = el.getBoundingClientRect();
      return { x: r.x, y: r.y, width: r.width, height: r.height };
    });
    if (!box) throw new Error(`card element missing for ${entry.card}/${entry.chart}/${entry.state}`);
    const shot = join(artifactsDir, `${entry.card}-${entry.chart}-${entry.state}.png`);
    const buffer = await page.screenshot({ path: shot, clip: box });
    manifest.push({ card: entry.card, chart: entry.chart, state: entry.state, file: `${entry.card}-${entry.chart}-${entry.state}.png`, bytes: buffer.length, note: '测试数据 fixture，非生产数据' });
    console.log(`saved ${entry.card}-${entry.chart}-${entry.state}.png ${buffer.length}B`);
  }

  writeFileSync(join(artifactsDir, 'manifest.json'), JSON.stringify({ generatedAt: new Date().toISOString(), note: '测试数据基准截图（fixture），不进生产数据', entries: manifest }, null, 2));
  console.log(`STATE MATRIX SCREENSHOTS DONE: ${manifest.length} shots`);
} finally {
  rmSync(join(extRoot, 'space-verify-boot.mjs'), { force: true });
  rmSync(join(extRoot, 'space-verify-boot.html'), { force: true });
  rmSync(join(extRoot, 'space-verify-native-fixture.json'), { force: true });
  server.close();
  await browser.close();
}
