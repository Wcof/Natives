// §5.5 布局维度验收：1440×900 / 960×600 最小支持尺寸，浏览器缩放 100%/125%/200%
// （headless 以 CSS zoom 模拟浏览器缩放，作用于内容重排，与 §5.5 目的一致）。
// 通过标准：无页级横滚（documentElement scrollWidth <= clientWidth）；
// 主指标金额不被截断（number 元素 scrollWidth <= clientWidth）。
// 数据层复用真实 Host 导出 fixture + chrome.runtime mock（与 seven-widgets-verify 一致）。
import { chromium } from 'playwright-core';
import { createServer } from 'node:http';
import { readFile, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { join, extname } from 'node:path';
import assert from 'node:assert';

const extRoot = join(fileURLToPath(new URL('.', import.meta.url)), '..');
const artifactsDir = join(extRoot, 'tests', 'artifacts');

const MIME = {
  '.html': 'text/html', '.js': 'text/javascript', '.mjs': 'text/javascript',
  '.css': 'text/css', '.json': 'application/json', '.svg': 'image/svg+xml', '.png': 'image/png',
};
const server = createServer(async (req, res) => {
  try {
    const url = new URL(req.url, 'http://localhost');
    const path = url.pathname === '/' ? '/space-verify-boot.html' : url.pathname;
    const data = await readFile(join(extRoot, path.slice(1)));
    res.writeHead(200, { 'content-type': MIME[extname(path)] || 'application/octet-stream' });
    res.end(data);
  } catch {
    res.writeHead(404); res.end();
  }
});
await new Promise((r) => server.listen(0, r));
const baseUrl = `http://localhost:${server.address().port}`;

const hostFixture = JSON.parse(await readFile(join(extRoot, 'ai-performance', 'host-fixture.json'), 'utf8'));
const analysis = {
  status: 'ready',
  byModel: [{ key: 'test-model-a', tokens: 1000000, requests: 2, costUsd: 0.28 }],
  bySource: [{ key: 'claude-code', tokens: 1005200, requests: 3, costUsd: 0.3 }],
  byProvider: [], byHour: [],
};
await writeFile(
  join(extRoot, 'space-verify-native-fixture.json'),
  JSON.stringify({ overview30: hostFixture.overview, overview7: hostFixture.overview, analysis, events: { events: [] } })
);
await writeFile(
  join(extRoot, 'space-verify-boot.html'),
  `<!doctype html><html><body style="margin:0"><div style="display:flex;flex-direction:column;width:100vw;height:100vh">
   <main id="dashboard-host" class="dashboard-host" style="flex:1 1 auto;min-width:0;min-height:0;position:relative;overflow:hidden"></main></div>
   <script>
     window.__nativeFixture = null;
     fetch('/space-verify-native-fixture.json').then((r) => r.json()).then((d) => { window.__nativeFixture = d; });
   </script>
   <script type="module" src="/space-verify-boot.mjs"></script></body></html>`
);
await writeFile(
  join(extRoot, 'space-verify-boot.mjs'),
  `import { createSpaceDashboard } from './space-dashboard.js';
   import { widgetPlugins } from './plugins/widgets/index.js';
   import { backgroundPlugins } from './plugins/backgrounds/index.js';
   window.__createDashboard = createSpaceDashboard;
   window.__widgetPlugins = widgetPlugins;
   window.__backgroundPlugins = backgroundPlugins;`
);

const SEVEN_KEYS = ['widget/aiCost','widget/aiTokens','widget/aiSessions','widget/aiRequests','widget/aiLimits','widget/aiAttention','widget/aiSavings'];
const snapshot = () => ({
  revision: 1,
  backgroundJson: { key: 'background/colour', display: { colour: '#101210', brightness: 1 } },
  widgets: SEVEN_KEYS.map((key, i) => ({
    id: `w-${i}`, key, enabled: true, configJson: {},
    displayJson: { position: 'free', xPercent: 12 + i * 11, yPercent: 30 + (i % 2) * 30 },
  })),
});

const browser = await chromium.launch({ executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true });
const COMBOS = [
  { label: '1440x900@100%', width: 1440, height: 900, zoom: 1 },
  { label: '960x600@100%', width: 960, height: 600, zoom: 1 },
  { label: '960x600@125%', width: 960, height: 600, zoom: 1.25 },
  { label: '960x600@200%', width: 960, height: 600, zoom: 2 },
];
const results = [];
try {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  const page = await context.newPage();
  page.on('pageerror', (e) => console.error('[layout pageerror]', String(e).slice(0, 300)));
  await page.goto(`${baseUrl}/space-verify-boot.html`);
  await page.waitForFunction(() => window.__createDashboard, null, { timeout: 15000 });
  await page.evaluate(() => {
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
                const result = msg.method === 'model_usage_overview' ? f.overview30
                  : msg.method === 'model_usage_analysis' ? f.analysis
                  : msg.method === 'model_usage_events' ? f.events
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
    const host = document.getElementById('dashboard-host');
    host.style.background = '#101210';
    window.__dashboard = window.__createDashboard({
      $: (sel) => document.getElementById(sel) || document.querySelector('#' + sel),
      t: (key, fallback) => fallback || key,
      selectedLanguage: 'zh_CN',
      backgroundPlugins: window.__backgroundPlugins,
      widgetPlugins: window.__widgetPlugins,
      nativeCall: async (method) => { throw new Error('unexpected nativeCall ' + method); },
      broadcastRevision: () => {},
      queueWorkspaceMutation: () => {},
      toast: () => {},
    });
  });

  for (const combo of COMBOS) {
    // 浏览器缩放的本质：CSS 视口按 1/zoom 缩小、内容按 zoom 放大渲染。
    // 用"缩小视口 + 等比放大页面"组合模拟，documentElement.zoom 叠加 100vw 会产生假横滚。
    const cssW = Math.round(combo.width / combo.zoom);
    const cssH = Math.round(combo.height / combo.zoom);
    await page.setViewportSize({ width: cssW, height: cssH });
    await page.evaluate(({ snap }) => {
      window.__dashboard.render(snap, 'ws-test', () => {});
    }, { snap: snapshot() });
    await new Promise((r) => setTimeout(r, 700));
    const m = await page.evaluate(() => {
      const de = document.documentElement;
      const numbers = [];
      for (const el of document.getElementById('dashboard-host').shadowRoot.querySelectorAll('.ai-perf-number-value')) {
        numbers.push({ text: el.textContent, scrollW: el.scrollWidth, clientW: el.clientWidth });
      }
      return {
        pageScrollW: de.scrollWidth, pageClientW: de.clientWidth,
        numberCount: numbers.length, numbers,
      };
    });
    assert.ok(m.pageScrollW <= m.pageClientW + 1, `${combo.label}: page-level horizontal scroll ${m.pageScrollW} > ${m.pageClientW}`);
    for (const n of m.numbers) {
      assert.ok(n.scrollW <= n.clientW + 1, `${combo.label}: amount "${n.text}" truncated (scrollW ${n.scrollW} > clientW ${n.clientW})`);
    }
    results.push({ combo: combo.label, pageScrollW: m.pageScrollW, pageClientW: m.pageClientW, numbers: m.numberCount });
    console.log(`  ${combo.label}: no page h-scroll (${m.pageScrollW}<=${m.pageClientW}), ${m.numberCount} amounts untruncated`);
  }

  await writeFile(
    join(artifactsDir, 'space-ai-layout-verify.json'),
    JSON.stringify({ generatedAt: new Date().toISOString(), note: 'zoom emulated via CSS zoom in headless Chrome', results }, null, 2)
  );
  console.log('AI WIDGET LAYOUT VERIFY PASSED');
  await context.close();
} finally {
  const { rmSync } = await import('node:fs');
  rmSync(join(extRoot, 'space-verify-boot.mjs'), { force: true });
  rmSync(join(extRoot, 'space-verify-boot.html'), { force: true });
  server.close();
  await browser.close();
}
