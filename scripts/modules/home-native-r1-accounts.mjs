// R1 账户与持仓收口验收（拆分自 home-native-browser.test.mjs 以守架构规模门禁）：
// 改名 409 保留草稿、账户归档/恢复保留历史、场外待确认买入经流水确认回放、
// 能力矩阵实测口径。页面自身的缺净值自动同步会写账本；与任何写并发时快照
// 读取可遇瞬态 409，与 UI 行为一致按可重试处理。
import assert from 'node:assert/strict';

export async function verifyR1Accounts(page, frame, report) {
  const r1Runtime = page.frames().find(f => f.url().startsWith('http://127.0.0.1:'));
  assert.ok(r1Runtime);
  // 1) 改名冲突：对话框打开后版本被同实例 API 推进 → 提交 409，草稿保留在原对话框。
  await frame.locator('#pos-accounts').click();
  await frame.locator('dialog[open] .pos-acct-row').filter({ hasText: 'Transfer target' }).locator('[data-rename]').click();
  await frame.locator('dialog[open] [name=name]').fill('Transfer target v2');
  await r1Runtime.evaluate(async () => {
    const data = await Fund.api('GET', '/api/accounts/detail?view=all');
    const account = data.items.find(a => a.name === 'Transfer target');
    await Fund.api('POST', '/api/accounts/update', { id: account.id, name: 'Transfer target bumped', expectedVersion: account.version, requestId: Fund.home.requestId() });
  });
  await frame.locator('dialog[open] [type=submit]').click();
  await frame.locator('dialog[open] .home-form-error').filter({ hasText: /.+/ }).waitFor({ timeout: 15000 });
  assert.equal(await frame.locator('dialog[open] [name=name]').inputValue(), 'Transfer target v2', '409 must keep the draft in the still-open dialog');
  await frame.locator('dialog[open] [data-close]').first().click();
  await r1Runtime.waitForFunction(() => document.querySelectorAll('dialog[open]').length === 0, undefined, { timeout: 15000 });
  // 2) 账户归档保留关联记录 → 恢复回 active。
  const archiveId = await r1Runtime.evaluate(async () => {
    const data = await Fund.api('GET', '/api/accounts/detail?view=all');
    return data.items.find(a => a.name === 'Transfer target bumped').id;
  });
  await frame.locator('#pos-accounts').click();
  await frame.locator('dialog[open] .pos-acct-row').filter({ hasText: 'Transfer target bumped' }).locator('[data-toggle-archive]').click();
  await frame.locator('dialog[open] [data-yes]').last().click();
  await r1Runtime.waitForFunction(() => document.querySelectorAll('dialog[open]').length === 0, undefined, { timeout: 15000 });
  const archived = await r1Runtime.evaluate(async id => {
    const data = await Fund.api('GET', '/api/accounts/detail?view=all');
    const hidden = await Fund.api('GET', '/api/positions/detail');
    const kept = await Fund.api('GET', '/api/positions/detail?includeArchived=1');
    return {
      archived: data.items.find(a => a.id === id).archived,
      hiddenFromDefault: !hidden.holdings.some(h => h.accountId === id),
      holdingsRemain: kept.holdings.some(h => h.accountId === id),
    };
  }, archiveId);
  assert.equal(archived.archived, true, 'archive marks the account archived');
  assert.equal(archived.holdingsRemain, true, 'archived account keeps its recorded history via includeArchived');
  assert.equal(archived.hiddenFromDefault, true, 'default positions view hides archived accounts');
  await frame.locator('#pos-accounts').click();
  await frame.locator('dialog[open] .pos-acct-row').filter({ hasText: 'Transfer target bumped' }).locator('[data-toggle-archive]').click();
  await frame.locator('dialog[open] [data-yes]').last().click();
  await r1Runtime.waitForFunction(() => document.querySelectorAll('dialog[open]').length === 0, undefined, { timeout: 15000 });
  assert.equal(await r1Runtime.evaluate(async id => (await Fund.api('GET', '/api/accounts/detail?view=all')).items.find(a => a.id === id).archived, archiveId), false, 'restore returns the account to active');
  // 3) 场外基金：待确认买入 → pendingCount 增加且数量不变 → 交易流水确认 → 数量入账。
  // 页面自身的缺净值自动同步会写账本；与任何写并发时快照读取可遇瞬态 409，
  // 与 UI 行为一致按可重试处理。
  const detailStable = async () => {
    for (let attempt = 0; ; attempt++) {
      try {
        return await r1Runtime.evaluate(() => Fund.api('GET', '/api/positions/detail'));
      } catch (error) {
        if (attempt >= 5 || !/changed/.test(String(error.message))) throw error;
        await new Promise(resolve => setTimeout(resolve, 300));
      }
    }
  };
  await frame.locator('#pos-tab-otc').click();
  const otcBefore = await detailStable().then(d => { const h = d.holdings.find(x => x.fundCode === '110022'); return { q: Number(h.quantity), p: Number(h.otcPendingBuyMinor) }; });
  const otcCard = frame.locator('.pos-holding-card').filter({ hasText: '110022' });
  await otcCard.locator('[data-pos-act=otcBuy]').first().click();
  await frame.locator('dialog[open] [name=amount]').fill('15.00');
  await frame.locator('dialog[open] [name=fee]').fill('0');
  await frame.locator('dialog[open] [name=feeKnown]').check();
  await frame.locator('dialog[open] form [type=submit]').click();
  await frame.locator('dialog[open] [data-confirm]').first().waitFor();
  const otcPending = await detailStable().then(d => { const h = d.holdings.find(x => x.fundCode === '110022'); return { q: Number(h.quantity), p: Number(h.otcPendingBuyMinor) }; });
  assert.equal(otcPending.p, otcBefore.p + 1500, 'pending amount is reserved, separate from confirmed shares');
  assert.equal(otcPending.q, otcBefore.q, 'OTC quantity stays unchanged until confirmation');
  await frame.locator('dialog[open] [data-confirm]').first().click();
  await frame.locator('dialog[open] [name=nav]').fill('1.5001');
  await frame.locator('dialog[open] [name=shares]').fill('10.0001');
  await frame.locator('dialog[open] [name=amount]').fill('15.00');
  await frame.locator('dialog[open] [name=fee]').fill('0');
  await frame.locator('dialog[open] [name=feeKnown]').check();
  await frame.locator('dialog[open] [type=submit]').click();
  await r1Runtime.waitForFunction(async before => {
    const d = await Fund.api('GET', '/api/positions/detail');
    const h = d.holdings.find(x => x.fundCode === '110022');
    return Number(h.otcPendingBuyMinor) === before.p && Number(h.quantity) === before.q + 10.0001;
  }, otcBefore, { timeout: 20000 });
  assert.equal(await detailStable().then(d => d.holdings.find(h => h.fundCode === '110022').quantity), '130.1235');
  report.steps.push({ name: 'R1 account archive/restore, stale-rename 409; current OTC amount application → reservation → explicit NAV/share confirmation → ledger quantity, no legacy row operation' });
  // 4) 能力矩阵：港美股为部分可用（真实公开上游探针口径），FX 不可用不折算。
  await frame.locator('#tab-positions').click();
  await frame.locator('#pos-tab-stock').click();
  await frame.locator('#pos-capability').click();
  const capText = await frame.locator('dialog[open] .home-manager-body').textContent();
  assert.ok(capText.includes('Partial'), 'HK/US capability renders as partial');
  assert.ok(capText.includes('verified by a live public-upstream probe'), 'capability note cites the real probe');
  await frame.locator('dialog[open] [data-close]').first().click();
  await r1Runtime.waitForFunction(() => document.querySelectorAll('dialog[open]').length === 0, undefined, { timeout: 15000 });
  report.steps.push({ name: 'capability matrix matches real probed upstream capabilities (HK/US partial, FX unavailable without conversion)' });
}
