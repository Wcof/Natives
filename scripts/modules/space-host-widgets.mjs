// Extension widgets on the real isolated Chrome/Native Host chain. No business
// responses are replaced; the parent harness owns the isolated ledger/config.
import assert from 'node:assert/strict';
import { renameSync } from 'node:fs';
import { join } from 'node:path';

export async function seedSpaceWidgetLedger(page) {
  await page.frames().find(f => f.url().startsWith('http://127.0.0.1:')).evaluate(async () => {
    const valuedAt = new Date(Date.now() + 8 * 3600000).toISOString().slice(0, 10);
    await Fund.api('POST', '/api/assets', { account: 'Isolated cash', name: 'Isolated cash', productType: 'cash', riskClass: 'cash', marketValue: '10000.01', valuedAt });
    const assets = await Fund.api('GET', '/api/assets');
    const plan = await Fund.api('GET', '/api/home/group-plan');
    await Fund.api('PUT', '/api/home/group-plan', { expectedRevision: plan.revision, groups: [{ groupId: '', name: 'Cash group', description: 'Recorded CNY cash in the isolated account', ratioTenths: 1000, sortOrder: 0, cashBindings: [{ accountId: assets.manual[0].accountId }] }] });
  });
}

async function cardState(page) {
  return page.evaluate(async () => {
    const { subscribeInvestmentCards } = await import('./space-investment-feed.js');
    let state;
    const dispose = subscribeInvestmentCards(value => { state = value; });
    dispose();
    return state;
  });
}

async function ready(page) {
  await page.evaluate(async () => {
    window.investmentDiagnostics = (await import('./space-investment-feed.js')).investmentFeedDiagnostics;
  });
  await page.waitForFunction(() => window.investmentDiagnostics().status === 'ready', null, { timeout: 30000 });
}

async function limitVisibleWidgets(space, name, offset, count) {
  await space.evaluate(async ({ name, offset, count }) => {
    const { createNativeClient } = await import('./native-client.js');
    const client = createNativeClient({ host: 'com.natives.file_manager', handshake: { method: 'version', params: { extensionVersion: chrome.runtime.getManifest().version } }, writeMethods: ['workspace_widget_upsert'] });
    try {
      const session = await client.call('workspace_session');
      const workspace = session.workspaces.find(w => w.name === name);
      let snapshot = await client.call('workspace_snapshot', { workspaceId: workspace.id });
      for (const widget of snapshot.widgets.filter((_, i) => i < offset || i >= offset + count)) snapshot = await client.call('workspace_widget_upsert', { workspaceId: workspace.id, expectedRevision: snapshot.revision, widget: { ...widget, enabled: false } });
    } finally { client.disconnect(); }
  }, { name, offset, count });
}

export async function verifySpaceHostWidgets(space, report) {
  // The parent already verified 20 rendered cards; restore the visible layout.
  await limitVisibleWidgets(space, 'Isolated investment acceptance', 0, 5);
  await space.reload();
  await ready(space);
  console.log('Space widgets: restored layout and real snapshot ready');
  const lastControl = space.locator('.InvestmentAllocationCheck .inv-card-refresh').first();
  await lastControl.focus();
  assert.equal(await lastControl.evaluate(el => { const r = el.getBoundingClientRect(), h = el.getRootNode().host.getBoundingClientRect(); return r.top >= h.top && r.bottom <= h.bottom; }), true, 'keyboard focus scrolls the last card control into the bounded slot');
  const before = (await cardState(space)).snapshot;
  assert.equal(typeof before.dataRevision, 'string');
  assert.ok(before.cash && before.groupAllocation && before.portfolio);
  assert.equal(before.groupAllocation.configured, true);
  for (const forbidden of ['accountId', 'accountName', 'fundId', 'assetId', 'groupId', 'bindings']) {
    assert.ok(!JSON.stringify(before).includes(forbidden), `card payload leaked ${forbidden}`);
  }
  await space.locator('.InvestmentHoldings .inv-card-open').first().click();
  const frame = space.frameLocator('iframe');
  await frame.locator('#view-positions:not([hidden])').waitFor({ timeout: 30000 });
  const home = await space.frames().find(f => f.url().startsWith('http://127.0.0.1:')).evaluate(() => Fund.api('GET', '/api/home/snapshot'));
  // N04：卡片与首页可消费不同估值批次（各自显示来源与时间）；账本派生的
  // 聚合（范围/持仓数/已实现/未知成本）必须一致，价格口径按批次判断。
  const cardSec = before.portfolio.securities;
  const homeSec = home.portfolio.securities;
  for (const key of ['scope', 'realizedScope', 'holdingCount', 'realizedMinor', 'knownRealizedMinor', 'unknownCostCount', 'excludedOverlapCount']) {
    assert.deepEqual(cardSec[key], homeSec[key], `batch-independent aggregate ${key} must match`);
  }
  assert.ok(before.recordedInvestmentAssets?.valuation?.batch != null, 'card payload carries its own valuation batch metadata');
  if (before.recordedInvestmentAssets?.valuation?.batch === home.wealth.valuation?.batch) {
    assert.deepEqual(cardSec, homeSec, 'same valuation batch must reconcile exactly');
  }
  assert.equal(before.groupAllocation.groups[0].actualTenths, home.groupAllocation.groups[0].currentTenths);
  await space.locator('.ws-select').filter({ hasText: 'Isolated investment acceptance' }).click();
  await ready(space);
  report.steps.push({ name: 'Space card and Fund detail share domain aggregates; snapshot version and sanitized payload verified' });

  console.log('Space widgets: Fund detail parity passed');
  for (const [theme, language, width] of [['archive', 'zh_CN', 1280], ['volt', 'en', 1280], ['archive', 'en', 320], ['volt', 'zh_CN', 640]]) {
    await space.evaluate(async ({ theme, language }) => chrome.storage.local.set({ 'natives-theme': theme, 'natives-language': language }), { theme, language });
    await space.setViewportSize({ width, height: 900 });
    await space.reload();
    await ready(space);
    assert.equal(await space.locator('.InvestmentHoldings .inv-card-title').first().textContent(), language === 'en' ? 'Investment holdings summary' : '投资持仓摘要');
    // Hidden amounts remain masked in DOM, including new cash/portfolio fields.
    const state = await cardState(space);
    assert.equal(state.moneyVisible, false);
    const button = space.locator('.InvestmentHoldings .inv-card-refresh').first();
    await button.focus();
    assert.equal(await button.evaluate(el => el.getRootNode().activeElement === el), true);
    await button.press('Enter');
    await ready(space);
    assert.equal(await space.evaluate(() => window.snapshotQueries.filter(q => q.method === 'app:card_snapshot').length), 2, 'keyboard refresh issues one shared query');
    try { await button.click({ timeout: 5000 }); } catch (error) {
      report.widgetHitRegions = await button.evaluate(el => {
        const rect = node => { const r = node.getBoundingClientRect(); return { x: r.x, y: r.y, width: r.width, height: r.height }; };
        const b = rect(el), host = el.getRootNode().host;
        return { button: b, host: rect(host), shell: rect(host.parentElement), toolbar: rect(document.getElementById('dashboard-toolbar')), hit: document.elementFromPoint(b.x + b.width / 2, b.y + b.height / 2)?.className, viewport: innerWidth };
      });
      throw error;
    }
    await ready(space);
    assert.equal(await space.evaluate(() => window.snapshotQueries.filter(q => q.method === 'app:card_snapshot').length), 3, 'mouse refresh issues one shared query');
    report.steps.push({ name: `Space investment ${theme}/${language}/${width}px: real snapshot, privacy, mouse and keyboard refresh` });
  }

  // Remove only this test profile's registration: exercise a real disconnect,
  // keep the previous snapshot visibly stale, then recover with one user refresh.
  const manifest = join(report.testDirectory, 'chrome-profile/NativeMessagingHosts/com.natives.local.app_runtime.json');
  renameSync(manifest, `${manifest}.disabled`);
  try {
    await space.locator('.InvestmentHoldings .inv-card-refresh').first().focus();
    await space.locator('.InvestmentHoldings .inv-card-refresh').first().press('Enter');
    await space.waitForFunction(() => window.investmentDiagnostics().status === 'unavailable');
    assert.ok(await space.locator('.InvestmentHoldings .inv-badge-unavailable').first().isVisible());
    assert.equal((await cardState(space)).snapshot.dataRevision, before.dataRevision);
  } finally { renameSync(`${manifest}.disabled`, manifest); }
  await space.locator('.InvestmentHoldings .inv-card-refresh').first().focus();
  await space.locator('.InvestmentHoldings .inv-card-refresh').first().press('Enter');
  await ready(space);
  report.steps.push({ name: 'Real missing Runtime registration: explicit unavailable state, retained stale snapshot and manual recovery' });

  await space.setViewportSize({ width: 1280, height: 900 });
  const resizer = space.locator('#sidebar-resizer');
  await resizer.focus();
  await resizer.press('Home');
  assert.equal(await resizer.getAttribute('aria-valuenow'), '190');
  const createBox = await space.locator('#workspace-create').boundingBox(), resizeBox = await resizer.boundingBox();
  assert.ok(createBox.x + createBox.width <= resizeBox.x, 'long names keep create inside the sidebar');
  await space.mouse.move(resizeBox.x + resizeBox.width / 2, 300);
  await space.mouse.down(); await space.mouse.move(240, 300); await space.mouse.up();
  assert.equal(await resizer.getAttribute('aria-valuenow'), '240');
  await resizer.press('ArrowRight');
  assert.equal(await resizer.getAttribute('aria-valuenow'), '260');
  await space.locator('#workspace-create').click({ timeout: 5000 });
  report.steps.push({ name: 'Long workspace name at 190 px sidebar: mouse create, resize drag and keyboard width adjustment' });
  await space.locator('#ws-name-input').fill('AI Host regression');
  await space.locator('#ws-template-select').selectOption('blank');
  await space.locator('#ws-name-submit').click();
  await space.locator('.ws-select').filter({ hasText: 'AI Host regression' }).waitFor();
  const aiWorkspaceId = await space.evaluate(async () => {
    const { createNativeClient } = await import('./native-client.js');
    const client = createNativeClient({ host: 'com.natives.file_manager', handshake: { method: 'version', params: { extensionVersion: chrome.runtime.getManifest().version } }, writeMethods: ['workspace_widget_upsert'] });
    try {
      const session = await client.call('workspace_session');
      const workspace = session.workspaces.find(w => w.name === 'AI Host regression');
      let snapshot = await client.call('workspace_snapshot', { workspaceId: workspace.id });
      for (let i = 0; i < 20; i++) {
        snapshot = await client.call('workspace_widget_upsert', { workspaceId: workspace.id, expectedRevision: snapshot.revision,
          widget: { id: '', key: i === 19 ? 'widget/aiLimits' : 'widget/aiSessions', order: i, enabled: true, configJson: { chart: 'number', range: '30d' }, displayJson: { position: 'middleCentre' } } });
      }
      return workspace.id;
    } finally { client.disconnect(); }
  });
  const aiUrl = new URL(space.url()); aiUrl.searchParams.set('workspace', aiWorkspaceId);
  await space.goto(aiUrl.href);
  await space.locator('.AiPerformance').nth(19).waitFor({ timeout: 30000 });
  await space.waitForFunction(() => window.snapshotQueries.some(q => q.method === 'model_quota_cached'));
  assert.equal(await space.evaluate(() => window.snapshotQueries.filter(q => q.method === 'model_usage_sessions').length), 1, '19 session widgets share one real Model Host query');
  assert.equal(await space.evaluate(() => window.snapshotQueries.filter(q => q.method === 'model_quota_query').length), 0);
  await space.evaluate(async () => {
    const { acquireModelApi, releaseModelApi } = await import('./ai-usage-client/client.js');
    const api = acquireModelApi();
    try {
      await api.snapshot();
      await api.upsertUsageBudget({ id: 'isolated-live-budget', scope: 'api_estimate', currency: 'USD', amountMicro: 1000000, period: 'daily', enabled: true });
    } finally { releaseModelApi(); }
  });
  await space.locator('.ai-perf-budget-head').filter({ hasText: 'isolated-live-budget' }).waitFor();
  assert.equal(await space.evaluate(() => window.snapshotQueries.filter(q => q.method === 'model_usage_sessions').length), 2, 'Host write event refreshes all session widgets with one query');
  report.steps.push({ name: '20 real AI cards: shared Native Model Host query, cached quota without upstream probe, live budget event updates fixed view' });

  await limitVisibleWidgets(space, 'AI Host regression', 19, 1);
  for (const [theme, language, width] of [['archive', 'en', 320], ['volt', 'zh_CN', 640]]) {
    await space.evaluate(async ({ theme, language }) => chrome.storage.local.set({ 'natives-theme': theme, 'natives-language': language }), { theme, language });
    await space.setViewportSize({ width, height: 900 });
    await space.reload();
    await space.locator('.ai-perf-budget-head').filter({ hasText: 'isolated-live-budget' }).waitFor();
    const text = await space.locator('.ai-perf-limits').textContent();
    assert.ok(!/aiPerf[A-Z]/.test(text), 'quota and budget text must be localized');
    assert.ok(text.includes(language === 'en' ? 'Daily' : '每日'));
    report.steps.push({ name: `Space AI ${theme}/${language}/${width}px: real budget and explicit missing quota cache` });
  }
}
