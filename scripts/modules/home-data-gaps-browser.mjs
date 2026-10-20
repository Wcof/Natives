// Synthetic UI gap/confirmation regression; domain arithmetic is covered by Rust/Native tests.
import assert from 'node:assert/strict';

export async function verifyHomeDataGaps(page) {
  await page.evaluate(async () => {
    window.currencyBaseline = structuredClone(Fund.home.snapshot());
    window.currencyFetch = window.fetch;
    window.currencyWrites = [];
    window.fetch = async (url, options = {}) => {
      const path = new URL(url, 'http://localhost').pathname;
      if (path === '/api/assets') return new Response(JSON.stringify({ dataRevision: '7', funds: [
        { fundId: 1, name: '<img src=x onerror=window.injected=true>', code: '110022', navCurrency: 'unknown', riskClass: 'equity' },
        { fundId: 1, name: 'Second account', code: '110022', navCurrency: 'unknown', riskClass: 'equity' },
      ] }), { status: 200 });
      if (path === '/api/fund-asset-metadata') {
        window.currencyWrites.push(JSON.parse(options.body));
        return new Response(JSON.stringify({ error: 'FINANCE_CONFLICT', message: 'Refresh required' }), { status: 409 });
      }
      // 快照只读（R-M5）：缺口样例由本隔离测试 stub 的传输层服务（生产页面
      // 无任何发布/替换入口），经公开 refresh 入口重载。
      if (window.gapSnapshot && path.startsWith('/api/home/snapshot')) {
        return new Response(JSON.stringify(window.gapSnapshot), { status: 200, headers: { 'Content-Type': 'application/json' } });
      }
      return window.currencyFetch(url, options);
    };
    const base = Fund.home.snapshot();
    window.gapSnapshot = { ...base,
      wealth: { ...base.wealth, quality: 'incomplete', totalA: null, knownA: null,
        issues: [{ code: 'currency_unconfirmed', count: 1 }], valuation: { ...base.wealth.valuation, unconfirmedCurrencies: 1 } },
      portfolio: { ...base.portfolio, securities: { ...base.portfolio.securities, holdingCount: 1, knownCostMinor: '12000' } },
    };
    await Fund.actions.refreshHome();
    Fund.home.applyDisplayMode('amount', false);
  });
  assert.match(await page.locator('#home-recorded-basics').innerText(), /1.*120/);
  assert.doesNotMatch(await page.locator('#hc-total-sub').innerText(), /currency_unconfirmed/);
  await page.locator('#home-currency-manage').click();
  const dialog = page.locator('dialog.home-manager');
  await dialog.locator('[name=fundId]').waitFor();
  assert.equal(await dialog.locator('[name=fundId] option').count(), 1, 'fund shared across accounts is confirmed once');
  assert.equal(await dialog.locator('[name=navCurrency]').inputValue(), 'unknown');
  assert.equal(await dialog.locator('img').count(), 0, 'identity is escaped');
  await dialog.locator('[type=submit]').click();
  await page.waitForFunction(() => document.querySelector('.home-form-error').textContent.length > 0);
  assert.equal(await page.evaluate(() => currencyWrites.length), 0, 'unconfirmed currency cannot submit');
  await dialog.locator('[name=navCurrency]').selectOption('CNY');
  await dialog.locator('[type=submit]').click();
  await page.waitForFunction(() => currencyWrites.length === 1 && !document.querySelector('[type=submit]').disabled);
  assert.deepEqual(await page.evaluate(() => currencyWrites[0]), { fundId: 1, navCurrency: 'CNY', riskClass: 'equity', dataRevision: '7' });
  assert.equal(await dialog.locator('[name=navCurrency]').inputValue(), 'CNY', 'conflict retains draft');
  await page.evaluate(() => Fund.applyLanguage('en'));
  assert.equal(await dialog.locator('[name=navCurrency]').inputValue(), 'CNY');
  assert.match(await dialog.locator('h2').innerText(), /currency/i);
  assert.match(await page.locator('#home-recorded-basics').innerText(), /holding records/);
  await page.evaluate(() => Fund.applyLanguage('zh-CN'));
  await dialog.locator('[data-close]').click();
  assert.equal(await page.locator('#home-currency-manage').evaluate(el => el === document.activeElement), true);
  for (const mode of ['percent', 'hidden']) {
    await page.evaluate(mode => Fund.home.applyDisplayMode(mode, false), mode);
    assert.equal(await page.locator('#home-currency-manage').isVisible(), false);
    assert.doesNotMatch(await page.locator('#home-recorded-basics').innerText(), /120/);
  }
  await page.evaluate(async () => {
    Fund.home.applyDisplayMode('amount', false);
    const fetch = window.fetch;
    window.fetch = (url, options) => new Promise(resolve => { window.releaseCurrencyRead = () => resolve(fetch(url, options)); });
    const opening = Fund.home.manageCurrency();
    await Promise.resolve();
    Fund.home.closePanel();
    window.releaseCurrencyRead();
    await opening;
    window.fetch = fetch;
  });
  assert.equal(await dialog.count(), 0, 'cancelled read cannot open a late dialog');
  await page.evaluate(async () => {
    window.gapSnapshot = null;
    window.fetch = window.currencyFetch;
    await Fund.actions.refreshHome();
    Fund.home.applyDisplayMode('hidden', false);
  });
}
