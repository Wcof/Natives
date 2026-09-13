// §5.5 对比度证据：真实 Chrome + 真实 space-dashboard.js Shadow DOM，
// 采样核心三卡在深/浅空间背景下的合成文字颜色与背景，计算 WCAG 对比度。
// 标准（§5.5）：正文/必要辅助 ≥4.5:1；大号文字（主指标 ≥18pt/14pt bold）与必要图形 ≥3:1。
// 产物：tests/artifacts/space-ai-contrast.json。

import { chromium } from 'playwright-core';
import { mkdirSync, writeFileSync, rmSync, readFileSync } from 'node:fs';
import { join, dirname, extname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createServer } from 'node:http';

const __dirname = dirname(fileURLToPath(import.meta.url));
const extRoot = join(__dirname, '..');
const artifactsDir = join(extRoot, 'tests', 'artifacts');
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
writeFileSync(join(extRoot, 'space-verify-native-fixture.json'), JSON.stringify(READY));
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

// WCAG 相对亮度与对比度（在页面上下文内执行）。
const CONTRAST_FN = `
  (() => {
    const lum = (rgb) => {
      const [r, g, b] = rgb.map((v) => {
        v /= 255;
        return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
      });
      return 0.2126 * r + 0.7152 * g + 0.0722 * b;
    };
    const parse = (c) => {
      const m = c.match(/rgba?\\(([\\d.]+),\\s*([\\d.]+),\\s*([\\d.]+)(?:,\\s*([\\d.]+))?\\)/);
      if (!m) return null;
      return { rgb: [+m[1], +m[2], +m[3]], alpha: m[4] === undefined ? 1 : +m[4] };
    };
    // 合成 alpha 通道到底色（半透明前景必须按实际合成色评估，§5.5"合成后的实际空间背景"）。
    const composite = (fg, bg) => {
      if (fg.alpha >= 1) return fg.rgb;
      return fg.rgb.map((c, i) => Math.round(c * fg.alpha + bg.rgb[i] * (1 - fg.alpha)));
    };
    return (fgStr, bgStr) => {
      const fg = parse(fgStr);
      const bg = parse(bgStr);
      if (!fg || !bg) return null;
      const f = composite(fg, bg);
      const l1 = lum(f);
      const l2 = lum(bg.rgb);
      const [hi, lo] = l1 >= l2 ? [l1, l2] : [l2, l1];
      return (hi + 0.05) / (lo + 0.05);
    };
  })()
`;

// 采样选择器与角色（读取计算样式；逐卡向上找合成底色）。
const SAMPLES = [
  { sel: '.ai-perf-number-value', role: 'metric-large', min: 3, note: '主指标（≥20px/600 视为大号文字）' },
  { sel: '.ai-perf-title, .ai-perf-head', role: 'body-text', min: 4.5, note: '卡头/标题' },
  { sel: '.ai-perf-sub, .ai-perf-status, .ai-perf-meta', role: 'secondary-text', min: 4.5, note: '次文字/单位/状态' },
  { sel: '.ai-perf-table-row:not(.ai-perf-table-head) .ai-perf-table-cell, .ai-perf-table-row:not(.ai-perf-table-head)', role: 'table-text', min: 4.5, note: '表格正文' },
];

const CARDS = [
  { card: 'aiCost', metric: 'ai_cost', chart: 'number' },
  { card: 'aiTokens', metric: 'token_usage', chart: 'number' },
  { card: 'aiSessions', metric: 'ai_sessions', chart: 'number' },
  // 表格形态：覆盖正文/表元 4.5:1 档（§5.5 正文与必要辅助 ≥4.5:1）。
  { card: 'aiCost', metric: 'ai_cost', chart: 'table' },
];
const BACKGROUNDS = [['space-dark', '#101210'], ['space-light', '#f4f1e8']];

const browser = await chromium.launch({ executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true });
const results = [];
let failures = 0;
try {
  const context = await browser.newContext({ viewport: { width: 460, height: 340 }, deviceScaleFactor: 2 });
  const page = await context.newPage();
  page.on('pageerror', (e) => console.error('[contrast pageerror]', String(e).slice(0, 300)));

  for (const [bgName, bgHex] of BACKGROUNDS) {
    for (const entry of CARDS) {
      await page.goto(`${baseUrl}/space-verify-boot.html`);
      await page.waitForFunction(() => window.__createDashboard, null, { timeout: 15000 });
      await page.evaluate(({ key, metric, chart: ch, bgHex: bgc }) =>
        window.__renderOne(key, metric, ch, bgc), { key: `widget/${entry.card}`, metric: entry.metric, chart: entry.chart, bgHex });
      await page.waitForTimeout(400);
      await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));

      const samples = await page.evaluate(({ sels, contrastSrc, bgHex: bgc }) => {
        const contrast = eval(contrastSrc); // 页面内编译 WCAG 函数
        const shadow = document.getElementById('dashboard-host').shadowRoot;
        const out = [];
        for (const s of sels) {
          const el = shadow.querySelector(s.sel);
          if (!el || !el.textContent.trim()) continue;
          const cs = getComputedStyle(el);
          const fg = cs.color;
          // 实际底色：逐层向上找到第一个非透明背景，最终落到空间背景。
          let node = el;
          let bg = null;
          while (node && node !== document.documentElement) {
            const owner = node.getRootNode?.()?.host || node;
            const style = (owner === node ? getComputedStyle(node) : getComputedStyle(owner));
            const c = style.backgroundColor;
            if (c && c !== 'rgba(0, 0, 0, 0)' && c !== 'transparent') { bg = c; break; }
            node = node.getRootNode?.()?.host || node.parentElement;
          }
          if (!bg) bg = bgc;
          const ratio = contrast(fg, bg);
          out.push({ sel: s.sel, role: s.role, min: s.min, note: s.note, fg, bg, ratio: ratio === null ? null : Math.round(ratio * 100) / 100, fontSize: cs.fontSize, fontWeight: cs.fontWeight });
        }
        return out;
      }, { sels: SAMPLES, contrastSrc: CONTRAST_FN, bgHex });

      for (const s of samples) {
        const ok = s.ratio !== null && s.ratio >= s.min;
        if (!ok) failures++;
        results.push({ card: entry.card, background: bgName, ...s, pass: ok });
        console.log(`${ok ? 'PASS' : 'FAIL'} ${entry.card}/${bgName} ${s.sel} ratio=${s.ratio} (min ${s.min}) fg=${s.fg} bg=${s.bg}`);
      }
    }
  }

  const summary = {
    generatedAt: new Date().toISOString(),
    standard: 'WCAG 2.1：正文/辅助 ≥4.5:1，大号文字/图形 ≥3:1（§5.5，合成后实际底色）',
    note: '测试数据 fixture，非生产数据',
    total: results.length,
    failures,
    results,
  };
  writeFileSync(join(artifactsDir, 'space-ai-contrast.json'), JSON.stringify(summary, null, 2));
  console.log(`CONTRAST VERIFY DONE: ${results.length} samples, ${failures} failures`);
  if (failures > 0) process.exitCode = 1;
} finally {
  rmSync(join(extRoot, 'space-verify-boot.mjs'), { force: true });
  rmSync(join(extRoot, 'space-verify-boot.html'), { force: true });
  rmSync(join(extRoot, 'space-verify-native-fixture.json'), { force: true });
  server.close();
  await browser.close();
}
