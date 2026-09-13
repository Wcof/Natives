// §5.5 图形与状态矩阵截图：核心三卡（成本/Token/会话）× 各自 allowedCharts
// 全部兼容形态，ready 态（真实 Host 导出 fixture 经 Native Messaging mock 注入，
// §9.4 契约 fixture 不手写）。截图标注"测试数据"，不进生产数据。
// 产物：tests/artifacts/ai-form-matrix/<card>-<chart>.png + manifest.json。

import { chromium } from 'playwright-core';
import { mkdirSync, writeFileSync, rmSync, readFileSync } from 'node:fs';
import { join, dirname, extname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createServer } from 'node:http';

const __dirname = dirname(fileURLToPath(import.meta.url));
const extRoot = join(__dirname, '..');
const artifactsDir = join(extRoot, 'tests', 'artifacts', 'ai-form-matrix');
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

// 真实 Host 导出的契约 fixture（30d）+ analysis 两行使 rank/table 有数据；
// sessions 走 model_usage_sessions（真实会话聚合协议）。
const hostFixture = JSON.parse(readFileSync(join(extRoot, 'ai-performance', 'host-fixture.json'), 'utf8'));
const fixture = {
  overview: hostFixture.overview,
  analysis: {
    status: 'ready',
    byModel: [
      { key: 'test-model-a', tokens: 1000000, requests: 2, costUsd: 0.28 },
      { key: 'test-model-b', tokens: 5200, requests: 1, costUsd: 0.02 },
    ],
    bySource: [{ key: 'claude-code', tokens: 1005200, requests: 3, costUsd: 0.3 }],
    byProvider: [],
    byHour: hostFixture.overview.trend?.map((t) => ({ hour: t.timestamp, tokens: t.tokens, requests: t.requests, costUsd: t.costUsd })) || [],
  },
  events: { events: [{ id: 'ev1', requestedAt: '2026-09-11T10:00:00Z', source: 'claude-code', tokens: 1000, model: 'test-model-a' }] },
  sessions: {
    status: 'ready', totalSessions: 2, activeDays: 1, unattributed: 0,
    byDay: [{ date: '2026-09-10', sessions: 1 }, { date: '2026-09-12', sessions: 1 }],
    bySource: [{ key: 'claude-code', requests: 3, tokens: 1005200, costUsd: 0.3 }],
  },
};

// 三卡各自 allowedCharts（与 widget.js aiComponentDefinitions 冻结值一致）。
const MATRIX = [
  { card: 'aiCost', metric: 'ai_cost', charts: ['number', 'line', 'bar', 'heatmap', 'table'] },
  { card: 'aiTokens', metric: 'token_usage', charts: ['number', 'line', 'bar', 'heatmap', 'table'] },
  { card: 'aiSessions', metric: 'ai_sessions', charts: ['number', 'heatmap', 'line', 'table', 'timeline'] },
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
writeFileSync(
  join(extRoot, 'space-verify-native-fixture.json'),
  JSON.stringify(fixture)
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
  page.on('pageerror', (e) => console.error('[form-matrix pageerror]', String(e).slice(0, 300)));

  for (const bg of [['space-dark', '#101210'], ['space-light', '#f4f1e8']]) {
    const [bgName, bgHex] = bg;
    for (const entry of MATRIX) {
      for (const chart of entry.charts) {
        // 每次重新加载：宿主元素已 attachShadow，复用会抛 NotSupportedError。
        await page.goto(`${baseUrl}/space-verify-boot.html`);
        await page.waitForFunction(() => window.__createDashboard, null, { timeout: 15000 });
        await page.evaluate(({ key, metric, chart: ch, bgHex: bgc }) =>
          window.__renderOne(key, metric, ch, bgc), { key: `widget/${entry.card}`, metric: entry.metric, chart, bgHex });
        await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
        // 只截卡片元素本身：从 shadow DOM 读 boundingBox。
        const box = await page.evaluate(() => {
          const el = document.getElementById('dashboard-host').shadowRoot.querySelector('[data-widget-id="w-0"]');
          if (!el) return null;
          const r = el.getBoundingClientRect();
          return { x: r.x, y: r.y, width: r.width, height: r.height };
        });
        if (!box) throw new Error(`card element missing for ${entry.card}/${chart}`);
        const shot = join(artifactsDir, `${entry.card}-${chart}-${bgName}.png`);
        const buffer = await page.screenshot({ path: shot, clip: box });
        manifest.push({ card: entry.card, chart, background: bgName, file: `${entry.card}-${chart}-${bgName}.png`, bytes: buffer.length, note: '测试数据 fixture，非生产数据' });
        console.log(`saved ${entry.card}-${chart}-${bgName}.png ${buffer.length}B`);
      }
    }
  }

  writeFileSync(join(artifactsDir, 'manifest.json'), JSON.stringify({ generatedAt: new Date().toISOString(), note: '测试数据基准截图（fixture），不进生产数据', entries: manifest }, null, 2));
  console.log(`FORM MATRIX SCREENSHOTS DONE: ${manifest.length} shots`);
} finally {
  rmSync(join(extRoot, 'space-verify-boot.mjs'), { force: true });
  rmSync(join(extRoot, 'space-verify-boot.html'), { force: true });
  server.close();
  await browser.close();
}
