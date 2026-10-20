// 持仓菜单的真实隔离 Chrome→Extension→Native→Runtime 用户路径。
import assert from 'node:assert/strict';
import { resolve } from 'node:path';

export async function verifyNativePositionsMenu(page, frame, report) {
  const runtime = page.frames().find(f => f.url().startsWith('http://127.0.0.1:'));
  await runtime.evaluate(async () => { Fund.home.applyDisplayMode('amount', false); await Fund.actions.loadPositions(); });
  const summary = frame.locator('.pos-more > summary').first();
  await summary.waitFor();
  const ledgerHoldings = () => runtime.evaluate(async () => JSON.stringify((await Fund.api('GET', '/api/positions/detail')).holdings.map(h =>
    ({ accountId: h.accountId, fundId: h.fundId, quantity: h.quantity, cost: h.cost, realized: h.realized, pendingCount: h.pendingCount }))));
  const before = await ledgerHoldings();
  const row = summary.locator('xpath=ancestor::article');
  const height = (await row.boundingBox()).height;
  for (const act of ['buy', 'sell']) {
    await summary.click();
    const menu = frame.locator('.pos-action-menu:popover-open');
    await menu.waitFor();
    assert.equal((await row.boundingBox()).height, height);
    assert.equal(await menu.locator('[role=group]').count(), 3);
    if (act === 'buy') await page.screenshot({ path: resolve(import.meta.dirname, '../../.tmp/positions-menu-ux/menu-native.png') });
    const id = await summary.getAttribute('id');
    await menu.locator(`[data-pos-act=${act}]`).click();
    await frame.locator('dialog[open] [name=quantity]').waitFor();
    assert.equal(await frame.locator('.pos-more[open]').count(), 0);
    assert.match(await frame.locator('dialog[open] h2').innerText(), act === 'buy' ? /买入|Buy/ : /卖出|Sell/);
    await frame.locator('dialog[open] [data-close]').click();
    await frame.locator('dialog[open]').waitFor({ state: 'detached' });
    assert.equal(await runtime.evaluate(() => document.activeElement.id), id, 'cancel returns to corresponding row trigger');
  }
  await summary.press('ArrowDown');
  await page.keyboard.press('Escape');
  assert.equal(await frame.locator('.pos-more[open]').count(), 0);
  await summary.click();
  await runtime.evaluate(async () => { await Fund.actions.refreshPositionsQuotes(); });
  assert.equal(await frame.locator('.pos-action-menu:popover-open').count(), 1);
  assert.equal(await runtime.evaluate(() => document.activeElement.dataset.posAct), 'buy');
  await frame.locator('#pos-display-toggle').click();
  assert.equal(await frame.locator('.pos-action-menu:popover-open').count(), 0);
  await summary.click();
  assert.doesNotMatch(await frame.locator('.pos-action-menu:popover-open').innerText(), /600519|110022|Acceptance/);
  await page.keyboard.press('Escape');
  await frame.locator('#pos-display-toggle').click();
  await summary.click();
  await frame.locator('#tab-home').click();
  assert.equal(await frame.locator('.pos-action-menu:popover-open').count(), 0);
  await frame.locator('#tab-positions').click();
  const after = await ledgerHoldings();
  assert.equal(after, before, 'opening, canceling and dismissing menus preserves ledger holdings');
  report.steps.push({ name: 'N06 positions menu: no row expansion; grouped actions; real buy/sell forms and cancel focus; quote refresh; privacy; page leave; no ledger mutation' });
}
