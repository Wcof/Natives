// Real Chrome, synthetic transport: detail visibility and truthful missing-data states.
import assert from 'node:assert/strict';
import { chromium } from 'playwright-core';
import { readFileSync, mkdirSync } from 'node:fs';
import { resolve } from 'node:path';
import { installHomeFixture } from './home-browser-fixture.mjs';

const root = resolve(import.meta.dirname, '../../modules/fund/ui/src');
const out = resolve(import.meta.dirname, '../../.tmp/home-wealth-details-20261026');
mkdirSync(out, { recursive: true });
const browser = await chromium.launch({ executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true });
try {
  for (const theme of ['archive', 'volt']) for (const language of ['zh-CN', 'en']) for (const width of [1280, 390]) {
    const page = await browser.newPage({ viewport: { width, height: 900 } });
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    const css = ['theme', 'layout', 'panel', 'home', 'investment'].map(name => readFileSync(resolve(root, `../dist/styles/${name}.css`), 'utf8')).join('\n');
    await page.setContent(`<style>${css}</style>${readFileSync(resolve(root, 'partials/view-home.html'), 'utf8')}`);
    await page.evaluate(installHomeFixture);
    for (const file of ['core.js', 'home-i18n.js', 'home-trend-chart.js', 'home-overview.js', 'home.js', 'home-range.js', 'home-manage.js']) await page.addScriptTag({ path: resolve(root, file) });
    await page.evaluate(async ({ theme, language }) => {
      document.documentElement.dataset.theme = theme;
      Fund.applyLanguage(language);
      Fund.state.token = 'test';
      await Fund.actions.loadHome();
      Fund.home.applyDisplayMode('amount', false);
    }, { theme, language });
    const total = page.locator('[data-detail="total"]');
    await total.focus();
    await page.keyboard.press('Enter');
    assert.equal(await page.locator('#home-detail-panel').evaluate(el => {
      const rect = el.getBoundingClientRect();
      return rect.top >= 0 && rect.top < innerHeight && el.contains(document.activeElement);
    }), true, 'clicking total must bring asset details into view and move keyboard focus');
    await page.locator('#home-detail-close').click();
    assert.equal(await total.evaluate(el => el === document.activeElement), true, 'closing restores the original trigger');

    await page.evaluate(async () => {
      window.snapshotOverride = structuredClone(Fund.home.snapshot());
      const cash = snapshotOverride.wealth.cash;
      cash.reserveState = 'unconfigured'; cash.reservedMinor = null; cash.availableMinor = null;
      cash.accounts[0].reserveState = 'unconfigured'; cash.accounts[0].reservedMinor = null; cash.accounts[0].availableMinor = null;
      snapshotOverride.portfolio.ledger.byType.stock.holdingCount = 0;
      snapshotOverride.portfolio.ledger.byType.stock.day.amountMinor = null;
      await Fund.home.refresh();
      Fund.home.applyDisplayMode('amount', false);
    });
    assert.match(await page.locator('#hc-cash-value').innerText(), /23,456\.78/, 'known cash remains visible without an allocation reserve');
    await page.locator('[data-detail="cash"]').click();
    assert.match(await page.locator('#home-detail-body').innerText(), /Private Account/);
    assert.match(await page.locator('#home-detail-body').innerText(), /23,456\.78/);
    assert.equal(await page.locator('#home-detail-panel').evaluate(el => el.contains(document.activeElement)), true);
    assert.doesNotMatch(await page.locator('#home-stock-income').innerText(), /待核实|Needs verification/i, 'no position is an empty state, not an unverified position');
    assert.doesNotMatch(await page.locator('#view-home').innerText(), /待核实|Needs verification/i, 'missing values use specific reasons instead of a generic verification demand');
    await page.evaluate(() => Fund.home.applyDisplayMode('hidden', false));
    assert.doesNotMatch(await page.locator('#home-detail-body').innerText(), /Private Account|23,456/);
    await page.evaluate(() => Fund.home.applyDisplayMode('amount', false));
    await page.screenshot({ path: resolve(out, `cash-${theme}-${language}-${width}.png`), fullPage: false });
    await page.locator('#home-detail-title').focus();
    await page.keyboard.press('Escape');
    assert.equal(await page.locator('[data-detail="cash"]').evaluate(el => el === document.activeElement), true);
    await page.evaluate(async () => { snapshotOverride.wealth.cash.accounts = []; snapshotOverride.wealth.cash.cashMinor = null; await Fund.home.refresh(); });
    assert.equal(await page.locator('[data-detail="cash"]').isVisible(), false, 'no empty cash detail entry');
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), true, 'details fit narrow windows');
    assert.deepEqual(errors, []);
    await page.close();
  }
  console.log('Home wealth: 8 Chrome layouts, visible details, focus restoration, known cash, empty/missing states and privacy passed.');
} finally {
  await browser.close();
}
