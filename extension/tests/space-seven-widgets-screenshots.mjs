// §5.5 视觉验收截图产物：真实 Chrome + 真实 space-dashboard.js Shadow DOM。
// 与 space-seven-widgets-verify.test.mjs 共用加载机制，但不做断言——
// 只把深/浅空间背景下的七组件渲染保存为 PNG，供人工核阅与回归对照。
// 截图注入的是"测试数据"快照（host 不可达走错误/空态文案），不进生产数据。

import { chromium } from 'playwright-core';
import { mkdirSync, writeFileSync, rmSync, readFileSync } from 'node:fs';
import { join, dirname, extname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createServer } from 'node:http';

const __dirname = dirname(fileURLToPath(import.meta.url));
const extRoot = join(__dirname, '..');
const artifactsDir = join(extRoot, 'tests', 'artifacts');
mkdirSync(artifactsDir, { recursive: true });

const SEVEN_KEYS = [
  'widget/aiCost',
  'widget/aiTokens',
  'widget/aiSessions',
  'widget/aiRequests',
  'widget/aiLimits',
  'widget/aiAttention',
  'widget/aiSavings',
];

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
    res.writeHead(404);
    res.end('not found');
  }
});
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
const baseUrl = `http://127.0.0.1:${server.address().port}`;

const snapshotFor = (bgHex) => ({
  revision: 1,
  backgroundJson: { key: 'background/colour', display: { colour: bgHex } },
  widgets: SEVEN_KEYS.map((key, i) => ({
    id: `w-${i}`,
    key,
    enabled: true,
    configJson: {},
    displayJson: { position: 'free', xPercent: 12 + i * 11, yPercent: 40 },
  })),
});

writeFileSync(
  join(extRoot, 'space-verify-boot.html'),
  // 真实 space.html 中宿主是 flex item（space.css .dashboard-host），被 blockified；
  // shadow 的 `:host{all:initial}` 会把裸 div 压成 inline——必须复刻 flex 父级。
  `<!doctype html><html><body style="margin:0"><div style="display:flex;flex-direction:column;width:100vw;height:100vh">
   <main id="dashboard-host" class="dashboard-host" style="flex:1 1 auto;min-width:0;min-height:0;position:relative;overflow:hidden"></main></div>
   <script type="module" src="/space-verify-boot.mjs"></script></body></html>`
);
writeFileSync(
  join(extRoot, 'space-verify-boot.mjs'),
  `import { createSpaceDashboard } from './space-dashboard.js';
   import { widgetPlugins } from './plugins/widgets/index.js';
   import { backgroundPlugins } from './plugins/backgrounds/index.js';
   window.__createDashboard = createSpaceDashboard;
   window.__widgetPlugins = widgetPlugins;
   window.__backgroundPlugins = backgroundPlugins;
   window.__render = async (snapshot, bg) => {
     const host = document.getElementById('dashboard-host');
     host.style.width = '1200px';
     host.style.height = '700px';
     host.style.background = bg;
     const dashboard = window.__createDashboard({
       $: (sel) => document.getElementById(sel) || document.querySelector('#' + sel),
       t: (key, fallback) => fallback || key,
       selectedLanguage: 'zh_CN',
       backgroundPlugins: window.__backgroundPlugins,
       widgetPlugins: window.__widgetPlugins,
       nativeCall: async (method) => { throw new Error('model-host unavailable (' + method + ')'); },
       broadcastRevision: () => {},
       queueWorkspaceMutation: () => {},
       toast: () => {},
     });
     dashboard.render(snapshot, 'ws-test', () => {});
     await new Promise((r) => setTimeout(r, 400));
   };`
);

const browser = await chromium.launch({
  executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  headless: true,
});

try {
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 }, deviceScaleFactor: 2 });
  const page = await context.newPage();
  await page.goto(`${baseUrl}/space-verify-boot.html`);
  await page.waitForFunction(() => window.__createDashboard, null, { timeout: 15000 });

  const saved = [];
  for (const [name, bgHex] of [['space-dark', '#101210'], ['space-light', '#f4f1e8']]) {
    // 每个场景重新加载：宿主元素已 attachShadow，复用会抛 NotSupportedError。
    await page.goto(`${baseUrl}/space-verify-boot.html`);
    await page.waitForFunction(() => window.__createDashboard, null, { timeout: 15000 });
    await page.evaluate(({ snapshot, bg }) => window.__render(snapshot, bg), {
      snapshot: snapshotFor(bgHex),
      bg: bgHex,
    });
    // 等待一帧绘制完成再截图，避免捕到空白帧。
    await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
    const shot = join(artifactsDir, `ai-seven-widgets-${name}.png`);
    const buffer = await page.screenshot({ path: shot, fullPage: false });
    saved.push({ name, bgHex, shot, buffer });
    console.log('saved', shot, `${buffer.length} bytes`);
  }

  // 像素级自证：PNG 经同源 HTTP 回读 canvas，getImageData 统计颜色。
  // 空白/单色画面在这里直接失败，不可能被当成证据交付。
  for (const item of saved) {
    const stats = await page.evaluate(async (url) => {
      const img = new Image();
      img.src = url;
      await new Promise((res, rej) => { img.onload = res; img.onerror = rej; });
      const scale = Math.min(1, 800 / img.naturalWidth);
      const c = document.createElement('canvas');
      c.width = Math.round(img.naturalWidth * scale);
      c.height = Math.round(img.naturalHeight * scale);
      const ctx = c.getContext('2d');
      ctx.drawImage(img, 0, 0, c.width, c.height);
      const data = ctx.getImageData(0, 0, c.width, c.height).data;
      const colors = new Map();
      let nonWhite = 0;
      let total = 0;
      for (let i = 0; i < data.length; i += 40) {
        const key = `${data[i]},${data[i + 1]},${data[i + 2]}`;
        colors.set(key, (colors.get(key) || 0) + 1);
        total++;
        if (data[i] < 240 || data[i + 1] < 240 || data[i + 2] < 240) nonWhite++;
      }
      const top = [...colors.entries()].sort((a, b) => b[1] - a[1]).slice(0, 5);
      const bgPixel = (() => {
        const i = (10 * c.width + 10) * 4;
        return `${data[i]},${data[i + 1]},${data[i + 2]}`;
      })();
      return { distinct: colors.size, nonWhiteRatio: nonWhite / total, top, bgPixel };
    }, `/tests/artifacts/${item.shot.split('/').pop()}`);
    item.stats = stats;
    console.log(`  ${item.name}: distinct=${stats.distinct} nonWhite=${(stats.nonWhiteRatio * 100).toFixed(1)}% bgPixel=(${stats.bgPixel})`);
    if (stats.distinct < 3 || stats.nonWhiteRatio < 0.5) {
      throw new Error(`${item.name}: screenshot looks blank (${stats.distinct} colors, ${(stats.nonWhiteRatio * 100).toFixed(1)}% non-white)`);
    }
  }
  if (saved[0].buffer.equals(saved[1].buffer)) {
    throw new Error('dark and light screenshots are byte-identical — capture is not rendering the scene');
  }
  await context.close();
  console.log('SCREENSHOT ARTIFACTS DONE');
} finally {
  rmSync(join(extRoot, 'space-verify-boot.mjs'), { force: true });
  rmSync(join(extRoot, 'space-verify-boot.html'), { force: true });
  server.close();
  await browser.close();
}
