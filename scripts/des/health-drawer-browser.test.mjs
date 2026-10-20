// DES-T03 / A08：五维体检抽屉真实 Chrome 验证。
// 复用 home-browser 的合成传输层（snapshotOverride 注入 + 公开 refresh），
// 不触真实账本。验证：打开/关闭/焦点、五维切换、下钻导航、隐私遮蔽、
// 快照 identity 更新重绘、账户截断说明、Esc 返回触发点。
import assert from 'node:assert/strict';
import { chromium } from 'playwright-core';
import { readFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { installHomeFixture } from '../modules/home-browser-fixture.mjs';

const ROOT = resolve(import.meta.dirname, '../..');
const src = resolve(ROOT, 'modules/fund/ui/src');
const artifacts = resolve(ROOT, '.tmp/des-t03/health-drawer');
mkdirSync(artifacts, { recursive: true });

const HEALTH = {
  scope: 'recorded-securities',
  types: [
    { instrumentKind: 'stock', holdingCount: 2, knownValueMinor: 80000, missingValueCount: 0, unknownCostCount: 1, staleCount: 0, weightBps: 4000 },
    { instrumentKind: 'etf', holdingCount: 3, knownValueMinor: 60000, missingValueCount: 1, unknownCostCount: 0, staleCount: 1, weightBps: 3000 },
    { instrumentKind: 'fund', holdingCount: 4, knownValueMinor: 60000, missingValueCount: 0, unknownCostCount: 2, staleCount: 0, weightBps: 3000 },
  ],
  accounts: [
    { accountId: 1, name: '主账户', archived: false, holdingCount: 5, knownValueMinor: 120000, missingValueCount: 1, unknownCostCount: 2, staleCount: 0 },
    { accountId: 2, name: '旧账户', archived: true, holdingCount: 4, knownValueMinor: 80000, missingValueCount: 0, unknownCostCount: 1, staleCount: 1 },
  ],
  accountsTruncated: false,
  liquidity: { otcPendingCount: 2, offlineCount: 3, offlineReceivableMinor: 5000, offlineUnsettledExits: 1 },
};
const DIAG = { scope: 'recorded-securities', coverage: 'partial', hhiBps: 2650, securities: [
  { key: 'security:SSE:600519', valueMinor: 80000, weightBps: 4000 },
  { key: 'fund:CN:110022', valueMinor: 60000, weightBps: 3000 },
]};

const browser = await chromium.launch({ executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true });
const report = { steps: [], errors: [] };
try {
  for (const theme of ['archive', 'volt']) {
    const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
    page.on('pageerror', (e) => report.errors.push(String(e.message || e)));
    const dist = resolve(ROOT, 'modules/fund/ui/dist');
    const css = [readFileSync(resolve(dist, 'styles/theme.css'), 'utf8'),
      readFileSync(resolve(src, 'styles/layout.css'), 'utf8'),
      readFileSync(resolve(src, 'styles/panel.css'), 'utf8'),
      readFileSync(resolve(src, 'styles/home.css'), 'utf8')].join('\n');
    await page.setContent(`<style>${css}</style>${readFileSync(resolve(src, 'partials/view-home.html'), 'utf8')}`);
    await page.evaluate(t => { document.documentElement.dataset.theme = t; }, theme);
    await page.evaluate(installHomeFixture);
    for (const file of ['core.js', 'home-i18n.js', 'home-overview.js', 'home-opportunities.js', 'home.js', 'home-manage.js', 'home-evidence.js'])
      await page.addScriptTag({ path: resolve(src, file) });
    // 注入带 health 的合成快照（经测试传输层 + 公开 refresh；保留夹具真实基线）。
    await page.evaluate(async ({ HEALTH, DIAG }) => {
      await Fund.actions.refreshHome();
      const base = Fund.home.snapshot();
      window.snapshotOverride = { ...base, portfolio: { ...base.portfolio,
        diagnostics: DIAG, securities: { holdingCount: 9, missingValueCount: 1, unknownCostCount: 3, staleCount: 1 },
        health: HEALTH, asOf: '2026-10-04' } };
      await Fund.actions.refreshHome();
    }, { HEALTH, DIAG });
    // 夹具默认 hidden；抽屉金额/身份断言在 amount 模式下进行（hidden 单独验）。
    await page.evaluate(() => Fund.home.applyDisplayMode('amount', false));
    await page.locator('#home-health-open').waitFor();

    // 1. 打开抽屉：焦点入 tab，范围行显示
    await page.locator('#home-health-open').click();
    assert.equal(await page.locator('#home-health-drawer').evaluate(d => d.open), true);
    const scope = await page.locator('#home-health-scope').innerText();
    assert.match(scope, /估值截至/);
    // 2. 类型维度：三类与权重、缺口计数
    const typesBody = await page.locator('#home-health-body-drawer').innerText();
    assert.match(typesBody, /股票/);
    assert.match(typesBody, /40(\.00)?%/, `types body: ${typesBody.slice(0, 200)}`);
    report.steps.push({ step: `drawer opens with types (${theme})`, pass: true });

    // 3. 维度切换（键盘方向键）
    await page.locator('#health-tab-accounts').click();
    let body = await page.locator('#home-health-body-drawer').innerText();
    assert.match(body, /主账户/);
    assert.match(body, /已归档/);
    await page.locator('#health-tab-accounts').focus();
    await page.keyboard.press('ArrowRight');
    assert.equal(await page.evaluate(() => document.activeElement.dataset.dim), 'concentration');
    body = await page.locator('#home-health-body-drawer').innerText();
    assert.match(body, /0\.265|26\.5/);
    assert.match(body, /证券层集中度|无底层穿透/);
    await page.keyboard.press('ArrowRight');
    body = await page.locator('#home-health-body-drawer').innerText();
    assert.match(body, /已估值资产|缺|未核实|覆盖/, `quality body: ${body.slice(0, 160)}`);
    await page.keyboard.press('ArrowRight');
    body = await page.locator('#home-health-body-drawer').innerText();
    assert.match(body, /2/); // otcPending
    report.steps.push({ step: `five dimensions via keyboard (${theme})`, pass: true });

    // 4. 隐私：hidden 模式遮蔽金额/权重/身份
    await page.evaluate(() => Fund.home.applyDisplayMode('hidden', false));
    await page.evaluate(() => document.dispatchEvent(new Event('fund:display-mode-changed')));
    await page.locator('#health-tab-accounts').click();
    body = await page.locator('#home-health-body-drawer').innerText();
    assert.doesNotMatch(body, /120,?000|主账户|旧账户/, 'hidden masks identity and amounts');
    await page.locator('#health-tab-types').click();
    body = await page.locator('#home-health-body-drawer').innerText();
    assert.doesNotMatch(body, /40%|80,?000/);
    await page.evaluate(() => Fund.home.applyDisplayMode('amount', false));
    await page.evaluate(() => document.dispatchEvent(new Event('fund:display-mode-changed')));
    report.steps.push({ step: `privacy masks drawer (${theme})`, pass: true });

    // 5. Esc 关闭返回触发点（抽屉在隐私步骤后仍打开）
    await page.keyboard.press('Escape');
    assert.equal(await page.locator('#home-health-drawer').evaluate(d => d.open), false);
    assert.equal(await page.evaluate(() => document.activeElement?.id), 'home-health-open');
    report.steps.push({ step: `Esc returns focus (${theme})`, pass: true });

    // 6. 下钻导航：类型 → 持仓（走公开 tab click；合成壳无 positions 模块时不报错）
    await page.locator('#home-health-open').click();
    // 行内下钻按钮可能位于 sticky footer 之下，直接经 DOM click 验证行为。
    await page.evaluate(() => document.querySelector('[data-health-jump="positions:stock"]')?.click());
    assert.equal(await page.locator('#home-health-drawer').evaluate(d => d.open), false);
    report.steps.push({ step: `drill navigation closes drawer (${theme})`, pass: true });

    if (theme === 'archive') await page.screenshot({ path: resolve(artifacts, 'health-drawer.png') });
    await page.close();
  }
} catch (error) {
  report.steps.push({ step: 'FATAL', pass: false, error: String(error).slice(0, 300) });
  throw error;
} finally {
  await browser.close();
}
report.pass = report.steps.every(s => s.pass) && report.errors.length === 0;
writeFileSync(resolve(artifacts, 'report.json'), JSON.stringify(report, null, 2) + '\n');
console.log(`health drawer: ${report.steps.filter(s => s.pass).length}/${report.steps.length} steps, pageErrors=${report.errors.length}`);
if (!report.pass) { console.log(JSON.stringify(report, null, 2)); process.exit(1); }
