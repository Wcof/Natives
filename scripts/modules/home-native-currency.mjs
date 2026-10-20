// Real isolated Runtime + metadata API + homepage currency confirmation.
import assert from 'node:assert/strict';

export async function verifyNativeCurrency(page, frame, report) {
  const runtime = page.frames().find(f => f.url().startsWith('http://127.0.0.1:'));
  const original = await runtime.evaluate(async () => {
    const assets = await Fund.api('GET', '/api/assets');
    const fund = assets.funds.find(f => f.code === '110022');
    if (!fund) throw Error('isolated fund fixture missing');
    await Fund.api('POST', '/api/fund-asset-metadata', {
      fundId: fund.fundId, navCurrency: 'unknown', riskClass: fund.riskClass, dataRevision: assets.dataRevision,
    });
    return fund;
  });
  await frame.locator('#tab-home').click();
  await runtime.evaluate(async () => { Fund.home.applyDisplayMode('amount', false); await Fund.home.refresh(); });
  const before = await runtime.evaluate(() => Fund.home.snapshot());
  assert.ok(before.wealth.valuation.unconfirmedCurrencies > 0);
  assert.ok(before.portfolio.securities.knownCostMinor != null);
  assert.match(await frame.locator('#home-recorded-basics').innerText(), /持仓|holding/);
  await frame.locator('#home-currency-manage').click();
  await frame.locator('dialog[open] [name=fundId]').selectOption(String(original.fundId));
  await frame.locator('dialog[open] [name=navCurrency]').selectOption('CNY');
  await frame.locator('dialog[open] [type=submit]').click();
  await frame.locator('dialog[open]').waitFor({ state: 'detached' });
  await runtime.waitForFunction(() => Fund.home.snapshot().wealth.valuation.unconfirmedCurrencies === 0);
  const after = await runtime.evaluate(async () => ({
    fund: (await Fund.api('GET', '/api/assets')).funds.find(f => f.code === '110022'),
    snapshot: Fund.home.snapshot(),
  }));
  for (const field of ['quantity', 'cost', 'nav', 'navDate', 'navSource', 'riskClass']) {
    assert.equal(after.fund[field], original[field], `currency confirmation preserves ${field}`);
  }
  assert.equal(after.fund.navCurrency, 'CNY');
  assert.ok(after.snapshot.wealth.totalA != null);
  assert.equal(await frame.locator('#home-currency-manage').isVisible(), false);
  report.steps.push({ name: 'legacy currency gap: recorded basics remain visible; real homepage metadata-only confirmation restores aggregate value and preserves NAV/quantity/cost', currencyBefore: before.wealth.valuation.unconfirmedCurrencies, currencyAfter: 0 });
  await frame.locator('#tab-positions').click();
}
