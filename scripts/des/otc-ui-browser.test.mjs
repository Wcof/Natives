// N09 场外操作 UI 流程探针：真实 Chrome + 生产 partials/src + 合成 API。
// 验证：菜单项、加仓申请 payload、减仓快捷填充与可用份额、订单确认/到账
// 操作、编辑弹层校准模式、定投计划列表/新增/期次。业务规则由 Rust 单元
// 测试权威覆盖；此处不冒充业务验收。
import assert from 'node:assert/strict';
import { chromium } from 'playwright-core';
import { readFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { shellHtml, shellScripts } from './shell-harness.mjs';

const ROOT = resolve(import.meta.dirname, '../..');
const dist = resolve(ROOT, 'modules/fund/ui/dist');
const artifacts = resolve(ROOT, '.tmp/n09-otc/ui-probe');
mkdirSync(artifacts, { recursive: true });

const report = { steps: [], payloads: [], errors: [] };
const browser = await chromium.launch({ executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true });
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
page.on('pageerror', (e) => report.errors.push(String(e.message || e)));

await page.setContent(shellHtml(), { waitUntil: 'domcontentloaded' });
await page.evaluate(() => {
  window.requests = [];
  window.fetch = async (url, options = {}) => {
    const u = new URL(url, 'http://localhost');
    const path = u.pathname + u.search;
    const body = options.body ? JSON.parse(options.body) : null;
    window.requests.push({ method: options.method || 'GET', path, body });
    const ok = (data) => ({ ok: true, status: 200, json: async () => data, text: async () => JSON.stringify(data) });
    if (path.startsWith('/api/otc/orders')) {
      if (options.method === 'GET') {
        return ok({
          orders: [
            { id: 7, op: 'buy', status: 'pending', appliedDate: '2026-10-06', cutoff: 'before_1500', amount: '1000.00', fee: null, feeKnown: false, shares: null, expectedNavDate: '2026-10-06', version: 1, cashArrival: null, confirmedShares: null, confirmedNavDate: null, confirmedAmount: null, feeDerived: null },
            { id: 5, op: 'sell', status: 'confirmed', appliedDate: '2026-10-05', cutoff: 'after_1500', amount: '330.00', fee: '3.30', feeKnown: true, shares: '300.0000', confirmedShares: '300.0000', confirmedAmount: '330.00', confirmedFee: '3.30', confirmedNavDate: '2026-10-05', confirmedNav: '1.1000', version: 2, cashArrival: null },
          ],
          availableShares: '700.0000',
          pendingBuyMinor: 100000,
          inTransitMinor: 32670,
        });
      }
      return ok({ ok: true, id: 9, expectedNavDate: '2026-10-06', cashReserved: true, tradingDayVerified: false });
    }
    if (path === '/api/otc/orders/confirm') return ok({ ok: true, txId: 42 });
    if (path === '/api/otc/orders/cancel') return ok({ ok: true });
    if (path === '/api/otc/orders/arrival') return ok({ ok: true });
    if (path.startsWith('/api/otc/plans/runs')) return ok({ planId: 1, runs: [{ periodKey: '2026-09-30', status: 'registered', orderId: 9, orderStatus: 'pending', createdAt: 'x' }] });
    if (path.startsWith('/api/otc/plans')) {
      if (options.method === 'GET') {
        return ok({ plans: [{ id: 1, account: 'A', fundCode: '110022', fundName: '消费', amount: '500.00', freq: 'weekly', weekday: 3, monthday: null, startDate: '2026-09-09', endDate: null, nonTrading: 'next', status: 'active', nextDate: '2026-10-07', tradingDayVerified: false, registeredRuns: 1, version: 1 }] });
      }
      return ok({ ok: true, id: 2 });
    }
    if (path === '/api/otc/holding/preview') {
      return ok({ mode: 'new', value: '1234.56', gain: '0.00', cost: '1234.56', shares: '1000.0486', sharesDerived: true, navRaw: '1.2345', navDate: '2026-10-06', navIsExact: true, tradingDayVerified: false });
    }
    if (path === '/api/otc/holding/edit/preview') {
      return ok({ mode: 'calibration', value: '2000.00', gain: '100.00', cost: '1900.00', shares: '1100.0000', sharesDerived: false, navRaw: '1.2345', navDate: '2026-10-06', navIsExact: true, tradingDayVerified: false,
        before: { quantity: '1000.0000', cost: '1000.00', asOf: '2026-10-01' }, after: { quantity: '1100.0000', cost: '1900.00', asOf: '2026-10-06' }, opening: { quantity: '900.0000', totalCost: '900.00', asOf: '2026-09-01', version: 3 } });
    }
    if (path.startsWith('/api/otc/holding')) return ok({ ok: true, mode: 'calibration', deltaShares: '100.0000', deltaCost: '900.00', sharesDerived: false });
    return ok({});
  };
  window.addEventListener('message', (e) => {
    if (e.data?.type === 'hello') window.parent.postMessage({ type: 'welcome', token: 't', generation: e.data.generation }, '*');
  });
});
await page.addScriptTag({ path: resolve(dist, 'echarts.min.js') });
for (const file of shellScripts()) await page.addScriptTag({ path: resolve(dist, file) });
await page.waitForTimeout(400);
// 默认隐私模式 hidden 会挡住弹层（与 transferDialog 同门槛）；切到 amount。
await page.evaluate(() => globalThis.Fund.home.applyDisplayMode('amount', false));
await page.waitForTimeout(100);

const holding = { accountId: 1, fundId: 2, fundCode: '110022', fundName: '消费行业', account: '主账户', venue: 'otc', quantity: '1000.0000', otcPendingBuyMinor: 100000, otcInTransitMinor: '326.70' };

// 1. 加仓：表单提交 payload 与申请记录渲染。
await page.evaluate((h) => globalThis.Fund.otcOps.buy(h), holding);
await page.waitForSelector('dialog[open] [data-orders] .otc-order');
await page.fill('dialog[open] [name=amount]', '1000.00');
await page.click('dialog[open] .home-manager-body > form button[type=submit]');
await page.waitForTimeout(200);
const createPayload = await page.evaluate(() => window.requests.filter(r => r.path === '/api/otc/orders').map(r => r.body));
assert.equal(createPayload.at(-1)?.op, 'buy');
assert.equal(createPayload.at(-1)?.amount, '1000.00');
assert.ok(createPayload.at(-1)?.requestId, 'requestId 必填');
assert.ok(await page.locator('.otc-badge-pending').first().isVisible());
report.steps.push({ step: 'buy dialog create payload + order list', ok: true });
await page.keyboard.press('Escape');

// 2. 减仓：快捷 1/2 填值（不提交）+ 全部。
await page.evaluate((h) => globalThis.Fund.otcOps.sell(h), holding);
await page.waitForSelector('dialog[open] [data-available]');
assert.equal(await page.locator('dialog[open] [data-available]').textContent(), '700.0000');
await page.click('dialog[open] [data-quick="1/2"]');
assert.equal(await page.locator('dialog[open] [name=shares]').inputValue(), '350');
await page.click('dialog[open] [data-quick="all"]');
assert.equal(await page.locator('dialog[open] [name=shares]').inputValue(), '700');
report.steps.push({ step: 'sell quick fill 1/2 + all', ok: true });
await page.keyboard.press('Escape');

// 3. 订单确认弹层（申购订单）+ 到账按钮（在减仓弹层）。
await page.evaluate((h) => globalThis.Fund.otcOps.buy(h), holding);
await page.waitForSelector('dialog[open] [data-orders] .otc-order');
await page.click('dialog[open] [data-confirm="7"]');
await page.fill('dialog[open] [name=nav]', '1.2300');
await page.click('dialog[open] .home-manager-body > form button[type=submit]');
await page.waitForTimeout(200);
const confirmPayload = await page.evaluate(() => window.requests.filter(r => r.path === '/api/otc/orders/confirm').map(r => r.body));
assert.equal(confirmPayload.at(-1)?.id, 7);
assert.equal(confirmPayload.at(-1)?.confirmedNav, '1.2300');
await page.keyboard.press('Escape');
await page.evaluate((h) => globalThis.Fund.otcOps.sell(h), holding);
await page.waitForSelector('dialog[open] [data-arrive="5"]');
await page.click('dialog[open] [data-arrive="5"]');
await page.waitForTimeout(200);
const arrivals = await page.evaluate(() => window.requests.filter(r => r.path === '/api/otc/orders/arrival').map(r => r.body));
assert.equal(arrivals.at(-1)?.id, 5);
report.steps.push({ step: 'confirm dialog payload + arrival button', ok: true });
await page.keyboard.press('Escape');

// 4. 转换：目标基金 + 快捷比例。
await page.evaluate((h) => globalThis.Fund.otcOps.convert(h), holding);
await page.waitForSelector('dialog[open] [name=target]');
await page.fill('dialog[open] [name=target]', '005827');
await page.click('dialog[open] [data-quick="all"]');
assert.equal(await page.locator('dialog[open] [name=shares]').inputValue(), '700');
await page.click('dialog[open] .home-manager-body > form button[type=submit]');
await page.waitForTimeout(200);
const conv = await page.evaluate(() => window.requests.filter(r => r.path === '/api/otc/orders').map(r => r.body));
assert.equal(conv.at(-1)?.op, 'convert');
assert.equal(conv.at(-1)?.targetCode, '005827');
report.steps.push({ step: 'convert payload', ok: true });
await page.keyboard.press('Escape');

// 5. 编辑持仓：校准模式预览（前后份额/成本）。
await page.evaluate((h) => globalThis.Fund.otcOps.holdingEdit(h), holding);
await page.fill('dialog[open] [name=value]', '2000.00');
await page.fill('dialog[open] [name=gain]', '100.00');
await page.click('dialog[open] .home-manager-body > form button[type=submit]');
await page.waitForSelector('dialog[open] [data-edit-ext]:not([hidden])');
assert.match(await page.locator('dialog[open] [data-mode-hint]').textContent(), /校准/);
assert.ok(await page.locator('dialog[open] .otc-preview-row').count() >= 3);
report.steps.push({ step: 'edit calibration preview', ok: true });
await page.keyboard.press('Escape');

// 6. 定投：计划列表、新增表单周期联动、执行记录。
await page.evaluate((h) => globalThis.Fund.otcOps.plans(h), holding);
await page.waitForSelector('dialog[open] [data-plans] .otc-plan');
assert.match(await page.locator('dialog[open] [data-plans]').textContent(), /每周/);
assert.match(await page.locator('dialog[open] [data-plans]').textContent(), /节假日未判定/);
await page.click('dialog[open] [data-new-plan]');
await page.fill('dialog[open] [data-plan-form] [name=amount]', '500.00');
await page.selectOption('dialog[open] [data-plan-form] [name=freq]', 'monthly');
assert.ok(await page.locator('dialog[open] [data-monthday]').isVisible());
await page.click('dialog[open] [data-plan-form] button[type=submit]');
await page.waitForTimeout(200);
const planPayload = await page.evaluate(() => window.requests.filter(r => r.path === '/api/otc/plans').map(r => r.body));
assert.equal(planPayload.at(-1)?.freq, 'monthly');
await page.click('dialog[open] [data-runs="1"]');
assert.match(await page.locator('dialog[open] .otc-runs').textContent(), /2026-09-30/);
report.steps.push({ step: 'plans list + create + runs', ok: true });
await page.keyboard.press('Escape');

// 7. 新增持仓：预览（含 navIsExact）+ 保存 payload。
await page.evaluate((h) => globalThis.Fund.otcOps.holdingNew({ account: '主账户', fundCode: '012414', fundName: '新基金' }), holding);
await page.fill('dialog[open] [name=value]', '1234.56');
await page.fill('dialog[open] [name=gain]', '0');
await page.click('dialog[open] .home-manager-body > form button[type=submit]');
await page.waitForSelector('dialog[open] .otc-preview b');
await page.click('dialog[open] [data-save]');
await page.waitForTimeout(200);
const holdPayloads = await page.evaluate(() => window.requests.filter(r => r.path === '/api/otc/holding').map(r => r.body));
assert.equal(holdPayloads.at(-1)?.fundCode, '012414');
assert.equal(holdPayloads.at(-1)?.value, '1234.56');
report.steps.push({ step: 'new holding preview + save', ok: true });

assert.equal(report.errors.length, 0, '页面错误: ' + report.errors.join('; '));
await browser.close();
writeFileSync(resolve(artifacts, 'ui-probe.json'), JSON.stringify(report, null, 2));
console.log(`N09 UI probe: ${report.steps.length}/7 steps passed`);
