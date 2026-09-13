// §5.5「真实工具数据的默认深/浅截图」：数据来自 export-usage-fixture 走产品真实
// CollectSources 链路导出的 real-usage-fixture.json（非手写 fixture，§9.4）。
// 核心三卡默认形态 × 深/浅背景共 6 张；标注"测试数据不进生产数据"。
// 产物：tests/artifacts/ai-real-data/<card>-<bg>.png + manifest.json。

import { chromium } from 'playwright-core';
import { mkdirSync, writeFileSync, rmSync, readFileSync } from 'node:fs';
import { join, dirname, extname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createServer } from 'node:http';

const __dirname = dirname(fileURLToPath(import.meta.url));
const extRoot = join(__dirname, '..');
const artifactsDir = join(extRoot, 'tests', 'artifacts', 'ai-real-data');
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

// 真实导出 fixture：overview/events/sessions 均为 Host 聚合方法原样序列化；
// analysis 从真实 overview.trend 派生（仍是真实采集数据）。
const real = JSON.parse(readFileSync(join(extRoot, 'tests', 'artifacts', 'real-usage-fixture.json'), 'utf8'));
const fixture = {
  overview: real.overview,
  analysis: {
    status: 'ready',
    byModel: [], bySource: [], byProvider: [],
    byHour: (real.overview.trend || []).map((t) => ({ hour: t.timestamp, tokens: t.tokens, requests: t.requests, costUsd: t.costUsd })),
  },
  events: real.events,
  sessions: real.sessions,
};

const CARDS = [
  { card: 'aiCost', metric: 'ai_cost', chart: 'number' },
  { card: 'aiTokens', metric: 'token_usage', chart: 'number' },
  { card: 'aiSessions', metric: 'ai_sessions', chart: 'number' },
];

writeFileSync(
  join(extRoot, 'space-verify-boot.html'),
  `<!doctype html><html><body style="margin:0"><div style="display:flex;flex-direction:column;width:100vw;height:100vh">
   <main id="dashboard-host" class="dashboard-host" style="flex:1 1 auto;min-width:0;min-height:0;position:relative;overflow:hidden"></main></div>
   <script>
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
writeFileSync(join(extRoot, 'space-verify-native-fixture.json'), JSON.stringify(fixture));
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
  page.on('pageerror', (e) => console.error('[real-data pageerror]', String(e).slice(0, 300)));

  for (const bg of [['space-dark', '#101210'], ['space-light', '#f4f1e8']]) {
    const [bgName, bgHex] = bg;
    for (const entry of CARDS) {
      await page.goto(`${baseUrl}/space-verify-boot.html`);
      await page.waitForFunction(() => window.__createDashboard, null, { timeout: 15000 });
      await page.evaluate(({ key, metric, chart: ch, bgHex: bgc }) =>
        window.__renderOne(key, metric, ch, bgc), { key: `widget/${entry.card}`, metric: entry.metric, chart: entry.chart, bgHex });
      await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
      const box = await page.evaluate(() => {
        const el = document.getElementById('dashboard-host').shadowRoot.querySelector('[data-widget-id="w-0"]');
        if (!el) return null;
        const r = el.getBoundingClientRect();
        return { x: r.x, y: r.y, width: r.width, height: r.height };
      });
      if (!box) throw new Error(`card element missing for ${entry.card}/${bgName}`);
      const shot = join(artifactsDir, `${entry.card}-${bgName}.png`);
      const buffer = await page.screenshot({ path: shot, clip: box });
      manifest.push({ card: entry.card, chart: entry.chart, background: bgName, file: `${entry.card}-${bgName}.png`, bytes: buffer.length, note: '真实采集数据截图（测试用途），不进生产数据' });
      console.log(`saved ${entry.card}-${bgName}.png ${buffer.length}B`);
    }
  }

  writeFileSync(join(artifactsDir, 'manifest.json'), JSON.stringify({
    generatedAt: new Date().toISOString(),
    note: '真实工具采集数据默认深/浅截图（§5.5）；数据经产品 CollectSources 链路导出，仅测试用途不进生产数据',
    sourceFixture: 'tests/artifacts/real-usage-fixture.json',
    entries: manifest,
  }, null, 2));
  console.log(`REAL DATA SCREENSHOTS DONE: ${manifest.length} shots`);
} finally {
  rmSync(join(extRoot, 'space-verify-boot.mjs'), { force: true });
  rmSync(join(extRoot, 'space-verify-boot.html'), { force: true });
  rmSync(join(extRoot, 'space-verify-native-fixture.json'), { force: true });
  server.close();
  await browser.close();
}
