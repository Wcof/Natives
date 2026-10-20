// 投资卡视觉验证（用户反馈：投资卡样式须与其他数据卡一致）。
//
// 真实 Chrome + 真实 space-dashboard + 真实 investment widget 模块；
// 仅替换 space-investment-feed.js 为注入固定状态的测试替身（导出同一 API 面），
// 用于把卡片驱动到 ready / incomplete / empty / elsewhere 四种状态。
// 同屏渲染 AI 效能卡（widget/aiCost）作样式基准对比，输出截图到 tests/artifacts/。
//
// 运行：node extension/tests/space-investment-style-screenshots.mjs

import { chromium } from 'playwright-core';
import { readFileSync } from 'node:fs';
import { mkdirSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { join, dirname, extname } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const extRoot = join(__dirname, '..');
const artifacts = join(__dirname, 'artifacts');
mkdirSync(artifacts, { recursive: true });

// 与 space.js 一致的文案解析：先查 zh_CN locale，缺 key 回退 key 本身。
const zhMessages = JSON.parse(readFileSync(join(extRoot, '_locales/zh_CN/messages.json'), 'utf8'));
const localeT = (key, fallback) => zhMessages[key]?.message || fallback || key;

const MIME = {
  '.js': 'text/javascript', '.mjs': 'text/javascript', '.json': 'application/json',
  '.html': 'text/html', '.css': 'text/css', '.svg': 'image/svg+xml',
};
const server = createServer((req, res) => {
  const urlPath = decodeURIComponent(new URL(req.url, 'http://x').pathname);
  const filePath = join(extRoot, urlPath);
  import('node:fs').then(({ readFileSync }) => {
    try {
      const body = readFileSync(filePath);
      res.writeHead(200, { 'content-type': MIME[extname(filePath)] || 'application/octet-stream' });
      res.end(body);
    } catch {
      res.writeHead(404);
      res.end('not found');
    }
  });
});
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
const baseUrl = `http://127.0.0.1:${server.address().port}`;

// feed 测试替身：导出面与 space-investment-feed.js 完全一致。
const feedDouble = (state) => `
const __state = ${JSON.stringify(state)};
export const INVESTMENT_WIDGET_KEYS = ['widget/investmentHoldings','widget/investmentAssets','widget/investmentGoal','widget/investmentAllocation','widget/investmentAllocationCheck'];
export function initInvestmentFeed() {}
export function setInvestmentCardsPresent() {}
export function suspendInvestmentFeed() {}
export function resumeInvestmentFeed() {}
export function refreshInvestmentCards() {}
export function setInvestmentNavigationHandler() {}
export function openInvestmentDetail() {}
export function setMoneyVisible(v) { __state.moneyVisible = Boolean(v); }
export function isMoneyVisible() { return Boolean(__state.moneyVisible); }
export function subscribeInvestmentCards(listener) { listener({ ...__state }); return () => {}; }
export function investmentFeedDiagnostics() { return __state; }
export function resetInvestmentFeedForTests() {}
`;

const READY_SNAPSHOT = {
  registeredFundHoldings: {
    positionCount: 3, accountCount: 2,
    cost: { complete: true, total: { amount: '404.45', currency: 'CNY' } },
    holdingDates: { earliest: '2026-09-01', latest: '2026-09-28' },
    quality: 'ready',
  },
  recordedInvestmentAssets: {
    quality: 'ready', hasEntries: true,
    pricedCount: 4, missingPriceCount: 0, currencyUnconfirmedCount: 0, currencyUnsupportedCount: 0,
    oldValuationCount: 0, overlapReviewCount: 0, unknownClassCount: 0,
    fundValue: { amount: '354.45', currency: 'CNY' },
    manualValue: { amount: '50.00', currency: 'CNY' },
    knownValue: { amount: '404.45', currency: 'CNY' },
    completeValue: { amount: '404.45', currency: 'CNY' },
    valuationDates: { earliest: '2026-09-27', latest: '2026-09-29' },
  },
  goal: {
    configured: true,
    targetAmount: { amount: '539.27', currency: 'CNY' },
    targetDate: '2026-12-31', expired: false,
    progress: { computable: true, ratioPercent: '75.0', currentValue: { amount: '404.45', currency: 'CNY' }, reasonCodes: [] },
  },
  allocation: {
    configured: true, computable: true, reasonCodes: [], unknownClassCount: 0,
    classes: [
      { riskClass: 'equity', targetTenths: 500, lowerTenths: 400, upperTenths: 600, actualTenths: 667, amount: '354.45', status: 'above' },
      { riskClass: 'gold', targetTenths: 200, lowerTenths: 100, upperTenths: 300, actualTenths: 133, amount: '50.00', status: 'within' },
      { riskClass: 'fixed_income', targetTenths: 100, lowerTenths: 0, upperTenths: 200, actualTenths: 0, amount: '0.00', status: 'within' },
      { riskClass: 'cash', targetTenths: 200, lowerTenths: 100, upperTenths: 300, actualTenths: 0, amount: '0.00', status: 'within' },
    ],
  },
  allocationCheck: { configured: true, computable: true, reasonCodes: [], unknownClassCount: 0, outOfRange: [{ riskClass: 'equity', direction: 'above', actualTenths: 667 }] },
};

const INCOMPLETE_SNAPSHOT = {
  registeredFundHoldings: {
    positionCount: 3, accountCount: 2,
    cost: { complete: false, known: { amount: '404.45', currency: 'CNY' }, unknownCount: 1 },
    holdingDates: { earliest: '2026-09-01', latest: '2026-09-28' },
    quality: 'incomplete',
  },
  recordedInvestmentAssets: {
    quality: 'incomplete', hasEntries: true,
    pricedCount: 2, missingPriceCount: 1, currencyUnconfirmedCount: 1, currencyUnsupportedCount: 0,
    oldValuationCount: 1, overlapReviewCount: 1, unknownClassCount: 2,
    fundValue: { amount: '100.00', currency: 'CNY' },
    manualValue: { amount: '50.00', currency: 'CNY' },
    knownValue: { amount: '150.00', currency: 'CNY' },
    completeValue: null,
    valuationDates: { earliest: '2026-09-18', latest: '2026-09-29' },
  },
  goal: {
    configured: true, targetAmount: { amount: '539.27', currency: 'CNY' }, targetDate: '2026-12-31', expired: false,
    progress: { computable: false, reasonCodes: ['incomplete'], ratioPercent: null, currentValue: null },
  },
  allocation: { configured: true, computable: false, reasonCodes: ['missing_nav', 'unknown_class'], unknownClassCount: 2, classes: [], outOfRange: [] },
  allocationCheck: { configured: true, computable: false, reasonCodes: ['missing_nav', 'unknown_class'], unknownClassCount: 2, outOfRange: [] },
};

const SCENARIOS = [
  { name: 'investment-ready', state: { status: 'ready', errorCode: null, readiness: 'ready', snapshot: READY_SNAPSHOT, generatedAt: '2026-09-29T12:00:00+08:00', fetchedAt: Date.now(), moneyVisible: false, loading: false } },
  { name: 'investment-ready-money-visible', state: { status: 'ready', errorCode: null, readiness: 'ready', snapshot: READY_SNAPSHOT, generatedAt: '2026-09-29T12:00:00+08:00', fetchedAt: Date.now(), moneyVisible: true, loading: false } },
  { name: 'investment-incomplete', state: { status: 'ready', errorCode: null, readiness: 'ready', snapshot: INCOMPLETE_SNAPSHOT, generatedAt: '2026-09-29T12:00:00+08:00', fetchedAt: Date.now(), moneyVisible: false, loading: false } },
  { name: 'investment-elsewhere', state: { status: 'unavailable', errorCode: 'APP_RUNNING_ELSEWHERE', readiness: 'ready', snapshot: null, generatedAt: null, fetchedAt: 0, moneyVisible: false, loading: false } },
  { name: 'investment-empty', state: { status: 'ready', errorCode: null, readiness: 'ready', snapshot: { registeredFundHoldings: { positionCount: 0, accountCount: 0, cost: null, holdingDates: null, quality: 'empty' }, recordedInvestmentAssets: { quality: 'empty', hasEntries: false, pricedCount: 0, missingPriceCount: 0, currencyUnconfirmedCount: 0, currencyUnsupportedCount: 0, oldValuationCount: 0, overlapReviewCount: 0, unknownClassCount: 0, fundValue: null, manualValue: null, knownValue: null, completeValue: null, valuationDates: null }, goal: { configured: false }, allocation: { configured: false }, allocationCheck: { configured: false } }, generatedAt: '2026-09-29T12:00:00+08:00', fetchedAt: Date.now(), moneyVisible: false, loading: false } },
];

const keys = [
  'widget/aiCost',
  'widget/investmentHoldings',
  'widget/investmentAssets',
  'widget/investmentGoal',
  'widget/investmentAllocation',
  'widget/investmentAllocationCheck',
];

const browser = await chromium.launch({
  executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  headless: true,
});
const results = [];
try {
  for (const scenario of SCENARIOS) {
    const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
    const page = await context.newPage();
    page.on('pageerror', (e) => console.error(`[${scenario.name} pageerror]`, String(e).slice(0, 200)));
    // 只替换 feed 模块为固定状态替身；其余走真实扩展模块。
    await page.route('**/space-investment-feed.js', (route) =>
      route.fulfill({ contentType: 'text/javascript', body: feedDouble(scenario.state) }));
    writeFileSync(
      join(extRoot, 'space-verify-boot.html'),
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
       window.__backgroundPlugins = backgroundPlugins;`
    );
    await page.goto(`${baseUrl}/space-verify-boot.html`);
    await page.waitForFunction(() => window.__createDashboard, null, { timeout: 15000 });

    const snapshot = {
      revision: 1,
      backgroundJson: { key: 'background/colour', display: { colour: '#101210', brightness: 1 } },
      widgets: keys.map((key, i) => ({
        id: `w-${i}`,
        key,
        enabled: true,
        configJson: {},
        displayJson: { position: 'free', xPercent: 12 + (i % 3) * 30, yPercent: i < 3 ? 26 : 72, scale: 1 },
      })),
    };
    await page.evaluate(async ({ snap, messages }) => {
      const localeT = (key, fallback) => messages[key]?.message || fallback || key;
      const host = document.getElementById('dashboard-host');
      host.style.width = '1200px';
      host.style.height = '700px';
      host.style.background = '#101210';
      const dashboard = window.__createDashboard({
        $: (sel) => document.getElementById(sel) || document.querySelector('#' + sel),
        t: localeT,
        selectedLanguage: 'zh_CN',
        backgroundPlugins: window.__backgroundPlugins,
        widgetPlugins: window.__widgetPlugins,
        nativeCall: async (method) => { throw new Error('unavailable (' + method + ')'); },
        broadcastRevision: () => {},
        queueWorkspaceMutation: () => {},
        toast: () => {},
      });
      dashboard.render(snap, 'ws-style-check', () => {});
      await new Promise((r) => setTimeout(r, 400));
    }, { snap: snapshot, messages: zhMessages });
    const shot = join(artifacts, `${scenario.name}.png`);
    await page.screenshot({ path: shot, clip: { x: 0, y: 0, width: 1280, height: 800 } });
    // 记录投资卡的实际计算样式，核对与 AI 卡同语言（玻璃底/字号/圆角）。
    const styleProbe = await page.evaluate(() => {
      const shadow = document.getElementById('dashboard-host').shadowRoot;
      const pick = (selector) => {
        const el = shadow.querySelector(selector);
        if (!el) return null;
        const cs = getComputedStyle(el);
        return {
          borderRadius: cs.borderRadius,
          background: cs.backgroundColor,
          backdropFilter: cs.backdropFilter,
          fontSize: cs.fontSize,
          fontWeight: cs.fontWeight,
          fontVariantNumeric: cs.fontVariantNumeric,
          padding: cs.padding,
        };
      };
      return {
        aiCard: pick('.ai-perf-number'),
        investmentCard: pick('.InvestmentCard'),
        investmentMetric: pick('.inv-card-main'),
      };
    });
    results.push({ scenario: scenario.name, shot: shot.replace(extRoot + '/', ''), styleProbe });
    await context.close();
  }
} finally {
  await browser.close();
  server.close();
}
console.log(JSON.stringify(results, null, 1));
