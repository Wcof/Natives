// Goal/privacy edge cases use synthetic snapshots; real ledger coverage stays in the Native harness.
import assert from 'node:assert/strict';
import { resolve } from 'node:path';

export async function verifyHomeUsability(page, { width, theme, shots }) {
  const baseline = await page.evaluate(() => ({
    goals: Fund.home.snapshot().goals,
    wealth: Fund.home.snapshot().wealth,
    mode: Fund.home.displayMode(),
  }));
  // 快照只读（R-M5）：合成变体注入测试 stub 的传输层（snapshotOverride），
  // 经公开 refresh 入口重载；生产公开接口没有任何发布/替换入口。
  const inject = async overrides => page.evaluate(async overrides => {
    window.snapshotOverride = { ...Fund.home.snapshot(), ...overrides };
    await Fund.actions.refreshHome();
  }, overrides);
  const clearOverride = async () => page.evaluate(async () => {
    window.snapshotOverride = null;
    await Fund.actions.refreshHome();
  });
  const show = async (goals, mode = 'amount') => { await inject({ goals }); await page.evaluate(mode => Fund.home.applyDisplayMode(mode, false), mode); };
  const orb = page.locator('#home-goal-body .goal-water-content');
  await show(baseline.goals, 'hidden');
  assert.equal(await orb.getAttribute('aria-valuenow'), null, 'private goal cannot expose progress');
  assert.equal(await page.locator('#home-goal-body .goal-water-fill').isVisible(), false, 'private water level is neutral');
  for (const [ratioTenths, value] of [[0, '0'], [625, '62.5'], [1000, '100'], [1250, '100']]) {
    await show({ ...baseline.goals, progress: { ...baseline.goals.progress, ratioTenths } }, 'percent');
    assert.equal(await orb.getAttribute('aria-valuenow'), value);
    // 动态小数契约：≤4 位小数并去尾零（0 显示 0% 而非 0.0%）。
    assert.equal(await orb.getAttribute('aria-valuetext'), `${ratioTenths / 10}`.replace(/(\.\d*?)0+$/, '$1').replace(/\.$/, '') + '%');
    assert.equal(await page.locator('#home-goal-body .goal-water-fill').isVisible(), ratioTenths > 0);
    assert.match(await page.locator('#home-goal-progress').innerText(), new RegExp(`${ratioTenths / 10}\\.?(0)?%`));
  }
  // ppm 精度源（0.0001% 粒度）：低于 0.1% 的真实进度不塌缩为 0。
  await show({ ...baseline.goals, progress: { ...baseline.goals.progress, ratioPpm: 10, ratioTenths: 0 } }, 'percent');
  assert.equal(await orb.getAttribute('aria-valuetext'), '0.001%', 'sub-tenth progress must stay visible via ppm');
  assert.equal(await page.locator('#home-goal-progress strong').innerText(), '0.001%');
  for (const progress of [
    { computable: false, reasonCodes: ['missing_price'] },
    { computable: true, ratioTenths: null },
    { computable: true, ratioTenths: -1 },
  ]) {
    await show({ ...baseline.goals, progress });
    assert.equal(await orb.getAttribute('aria-valuenow'), null, 'unavailable is not confirmed zero');
    assert.equal(await page.locator('#home-goal-body .goal-water-fill').isVisible(), false);
  }
  await show({ status: 'empty' });
  assert.equal(await page.locator('#home-goal-create').isVisible(), true);
  assert.equal(await orb.getAttribute('aria-valuenow'), null);
  await inject({ wealth: { ...baseline.wealth, cumulative: { totalMinor: '1234500', complete: true, coverageFrom: '2026-09-01' } } });
  await show(baseline.goals);
  assert.notEqual(await page.locator('#home-cumulative-value').innerText(), '–');
  const order = await page.evaluate(() => {
    const ids = ['hc-total', 'home-goal-body', 'home-distribution-body', 'home-combination-body', 'home-ai-body', 'home-all-income', 'home-history-chart', 'home-market-body', 'home-today-body'];
    return ids.every((id, i) => !i || !!(document.getElementById(ids[i - 1])
      .compareDocumentPosition(document.getElementById(id)) & Node.DOCUMENT_POSITION_FOLLOWING));
  });
  assert.equal(order, true, 'reading order matches the confirmed prototype: assets, full-width goal, distribution, AI, returns, folded history');
  if (width === 1280) {
    const left = await page.locator('.home-ledger-strip').boundingBox();
    const right = await page.locator('.home-goal').boundingBox();
    assert.ok(right.y >= left.y + left.height && Math.abs(right.width-left.width)<2, 'goal is a full-width card below assets');
  }
  assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1));
  await page.emulateMedia({ reducedMotion: 'reduce' });
  assert.equal(await orb.evaluate(node => node.getAnimations({ subtree: true }).length), 0);
  if (width === 1280 || width === 320) {
    await page.screenshot({ path: resolve(shots, `usability-${theme}-${width}-amount.png`), fullPage: true });
  }
  await inject({ wealth: { ...baseline.wealth, totalA: '9223372036854775807', cumulative: { ...baseline.wealth.cumulative, totalMinor: '-9223372036854775807' } } });
  await show({ ...baseline.goals, goal: { ...baseline.goals.goal, targetMinor: '9223372036854775807' },
    progress: { ...baseline.goals.progress, ratioTenths: 1000, scopeValueMinor: '9223372036854775807', gapMinor: 0 } });
  assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth + 1), 'long amounts fit narrow layout');
  await inject({ wealth: { ...baseline.wealth, totalA: null, knownA: 12345678, valuation: { ...baseline.wealth.valuation, missingQuotes: 1 } } });
  await page.evaluate(() => Fund.home.applyDisplayMode('amount', false));
  assert.equal(await page.locator('#hc-total .hc-label').innerText(), '已估值资产');
  assert.notEqual(await page.locator('#hc-total-value').innerText(), '待核实', 'known amount stays visible');
  await clearOverride();
  await page.evaluate(mode => Fund.home.applyDisplayMode(mode, false), baseline.mode);
  await page.emulateMedia({ reducedMotion: 'no-preference' });
}

// ==== 复核 T05：公开 selector 只读封装回归（合成快照；生产公开接口核对） ====
export async function verifyHomeEncapsulation(page) {
  const probes = await page.evaluate(async () => {
    const findings = { mutationAttempts: [], publishAbsent: false, refreshOk: false, displayModeValue: Fund.home.displayMode() };
    // 1) 每个公开状态查询：尝试修改返回值（含嵌套对象/数组），确认内部状态不变。
    const original = Fund.home.snapshot();
    const originalJson = JSON.stringify(original);
    const mutations = [
      snap => { snap.dataRevision = 'tampered'; },
      snap => { snap.wealth.totalA = 1; },
      snap => { snap.wealth.cash.accounts.push('tampered'); },
      snap => { snap.opportunities.items.splice(0, 1); },
      snap => { snap.goals.goal.name = 'tampered'; },
    ];
    for (const attempt of mutations) {
      let threw = false;
      const copy = structuredClone(original);
      try { attempt(copy); } catch { threw = true; }
      findings.mutationAttempts.push({ threw, ignoredOrThrew: threw || JSON.stringify(copy) !== JSON.stringify(original) });
      // 关键：对 Fund.home.snapshot() 返回值本身执行同样修改，owner 内部必须不变。
      const live = Fund.home.snapshot();
      let liveThrew = false;
      try { attempt(live); } catch { liveThrew = true; }
      findings.mutationAttempts.at(-1).ownerUnchanged =
        JSON.stringify(Fund.home.snapshot()) === originalJson && (liveThrew || !liveThrew && JSON.stringify(Fund.home.snapshot()) === originalJson);
    }
    // benchmark 同样处理（可能为 null）。
    const bench = Fund.home.benchmark();
    if (bench && typeof bench === 'object') {
      const before = JSON.stringify(Fund.home.benchmark());
      try { bench.status = 'tampered'; } catch {}
      try { bench.points[0].portfolioPpm = 'tampered'; } catch {}
      try { bench.points.push({date:'tampered'}); } catch {}
      findings.benchmarkUnchanged = JSON.stringify(Fund.home.benchmark()) === before;
    } else findings.benchmarkUnchanged = true;
    // 2) 生产公开接口不得包含发布/替换入口。
    findings.publishAbsent = typeof Fund.home.publishTestSnapshot !== 'function';
    // 3) 公开 refresh 入口仍工作（真实账本来源快照可重载）。
    await Fund.actions.refreshHome();
    findings.refreshOk = !!Fund.home.snapshot()?.snapshotId;
    return findings;
  });
  for (const [index, attempt] of (probes.mutationAttempts ?? []).entries()) {
    assert.ok(attempt.ownerUnchanged, `snapshot mutation #${index} reached owner state`);
  }
  assert.ok(probes.benchmarkUnchanged, 'benchmark selector exposes mutable internal state');
  assert.ok(probes.publishAbsent, 'production surface must not provide synthetic snapshot publisher');
  assert.ok(probes.refreshOk, 'public refresh entry must still reload the authoritative snapshot');
  assert.ok(['amount', 'percent', 'hidden'].includes(probes.displayModeValue), 'displayMode selector must return a tri-state string');
}
