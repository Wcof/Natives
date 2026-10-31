import assert from 'node:assert/strict';

export async function verifyNativeWealth(page, frame, report) {
  const runtime = page.frames().find(f => f.parentFrame() && f.url().startsWith('http://127.0.0.1:'));
  await runtime.waitForFunction(() => window.Fund?.home?.snapshot() != null);
  await runtime.evaluate(async () => {
    const date = new Date(Date.now() + 8 * 3600000).toISOString().slice(0, 10);
    await Fund.api('POST', '/api/assets', { account: 'Cash fixture', name: 'Wallet fixture', productType: 'cash', riskClass: 'cash', marketValue: '23456.78', valuedAt: date });
    await Fund.actions.refreshHomeValuation();
    Fund.home.applyDisplayMode('amount', false);
  });
  const facts = await runtime.evaluate(() => Fund.home.snapshot().wealth.cash);
  assert.equal(facts.cashMinor, '2345678');
  assert.equal(facts.availableMinor, null, 'unknown reserve must not be silently treated as zero');
  assert.match(await frame.locator('#hc-cash-value').innerText(), /23,456.78/);
  await frame.locator('[data-detail=cash]').click();
  assert.match(await frame.locator('#home-detail-body').innerText(), /Cash fixture/);
  assert.equal(await frame.locator('#home-detail-title').evaluate(el => el === document.activeElement), true);
  await page.keyboard.press('Escape');
  assert.equal(await frame.locator('[data-detail=cash]').evaluate(el => el === document.activeElement), true);
  await frame.locator('[data-detail=total]').click();
  assert.match(await frame.locator('#home-detail-body').innerText(), /23,456.78/);
  await frame.locator('[data-wealth-view=assets]').click();
  assert.equal(await frame.locator('#view-assets').isVisible(), true, 'asset entry opens the real asset page');
  await frame.locator('#tab-home').click();
  await frame.locator('[data-detail=total]').click();
  await frame.locator('[data-wealth-view=positions]').click();
  assert.equal(await frame.locator('#view-positions').isVisible(), true, 'holdings entry opens the real holdings page');
  report.steps.push({ name: 'Real isolated ledger: known cash without reserves, visible details, Escape focus and asset/holding navigation' });
}
