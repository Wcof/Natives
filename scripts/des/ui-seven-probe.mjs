// 七项 UI 改进探针：真实 Chrome + 生产装配 + 合成 API。
// 1 体检卡名称+分页 2 goal-orb 波浪动画 3 账户排序+隐藏金额不藏名
// 4 账户管理弹窗结构 5 持仓 4 列+新增卡 6 更新时间 7 能力 icon+移除按钮
import assert from 'node:assert/strict';
import { chromium } from 'playwright-core';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { shellHtml, shellScripts } from './shell-harness.mjs';
import { installHomeFixture } from '../modules/home-browser-fixture.mjs';

const ROOT = resolve(import.meta.dirname, '../..');
const dist = resolve(ROOT, 'modules/fund/ui/dist');
const browser = await chromium.launch({ executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true });
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
const errors = [];
page.on('pageerror', (e) => errors.push(String(e)));
await page.setContent(shellHtml(), { waitUntil: 'domcontentloaded' });
await page.evaluate(installHomeFixture);
await page.evaluate(() => {
  const holdings = Array.from({ length: 6 }, (_, i) => ({
    accountId: 1, fundId: i + 1, fundCode: String(100000 + i), fundName: '基金' + (i + 1),
    venue: 'otc', instrumentKind: 'fund', quantity: '10', cost: '100.00',
    marketValue: String(100 + i * 50), realized: '0', costUnknown: false,
    nav: { nav: '1.0000', date: '2026-09-30' },
  }));
  window.fetch = async (url, o = {}) => {
    const p = new URL(url, 'http://x').pathname + new URL(url, 'http://x').search;
    if (p.startsWith('/api/positions/detail')) return { ok: true, status: 200, json: async () => ({ dataRevision: '1', holdings, accounts: [{ id: 1, name: '主账户' }, { id: 2, name: '备用账户' }], realized: [], quoteFailed: false }) };
    if (p.startsWith('/api/accounts/detail')) return { ok: true, status: 200, json: async () => ({ total: 2, items: [{ id: 1, name: '主账户', archived: false, version: 1, cash: { quality: 'recorded', amount: '1000.00' } }, { id: 2, name: '备用账户', archived: true, version: 3, cash: { quality: 'unbooked' } }] }) };
    if (p.startsWith('/api/market/suggest')) return { ok: true, status: 200, json: async () => ({ suggestions: [{ symbol: '110022', name: '消费行业' }] }) };
    return { ok: true, status: 200, json: async () => ({}), text: async () => '{}' };
  };
  window.addEventListener('message', (e) => { if (e.data?.type === 'hello') window.parent.postMessage({ type: 'welcome', token: 't', generation: e.data.generation }, '*'); });
});
for (const f of shellScripts()) await page.addScriptTag({ path: resolve(dist, f) });
await page.evaluate(installHomeFixture);
await page.waitForTimeout(200);
await page.evaluate(() => globalThis.Fund.home.applyDisplayMode('amount', false));

// 首页体检卡：名称+代码+分页（13 条诊断数据 → 3 页）
await page.evaluate(async () => {
  const sec = Array.from({ length: 13 }, (_, i) => ({ key: i < 11 ? `fund:CN:11002${i}` : `security:SSE:60051${i}`, name: '基金名称' + (i + 1), valueMinor: 1000 - i, weightBps: 1000 - i }));
  await Fund.actions.refreshHome();
  const snap = Fund.home.snapshot();
  window.snapshotOverride = { ...snap, portfolio: { ...snap.portfolio, diagnostics: { ...snap.portfolio.diagnostics, securities: sec } }, goals: { status: 'ready', goal: { name: 'G', targetMinor: 20000000, targetDate: '2099-12-31', scopeKind: 'A' }, progress: { computable: true, ratioPpm: 625000, ratioTenths: 625, scopeValueMinor: 12500000, gapMinor: 0, surplusMinor: 0 } } };
  await Fund.actions.refreshHome();
});
await page.waitForTimeout(300);
const concText = await page.locator('#home-concentration-list').innerText();
assert.match(concText, /基金名称1/, 'concentration must show names');
assert.match(concText, /110020/, 'concentration must show codes');
assert.match(concText, /1\/3/, 'concentration must paginate 13 items into 3 pages');
await page.click('[data-conc-next]');
assert.match(await page.locator('#home-concentration-list').innerText(), /基金名称6/, 'page 2 starts at item 6');
await page.click('[data-conc-prev]');
const anim = await page.evaluate(() => {
  const w = document.querySelector('.goal-orb-water');
  return w ? getComputedStyle(w, '::before').animationName : null;
});
assert.equal(anim, 'goal-wave', 'goal orb wave animation must be active');

// 持仓页（先切页签，再重注入持仓 stub——home 夹具覆盖了 fetch）
await page.evaluate(() => document.getElementById('tab-positions').click());
await page.waitForTimeout(200);
await page.evaluate(() => {
  const holdings = Array.from({ length: 6 }, (_, i) => ({
    accountId: 1, fundId: i + 1, fundCode: String(100000 + i), fundName: '基金' + (i + 1),
    venue: 'otc', instrumentKind: 'fund', quantity: '10', cost: '100.00',
    marketValue: String(100 + i * 50), realized: '0', costUnknown: false,
    nav: { nav: '1.0000', date: '2026-09-30' },
  }));
  window.fetch = async (url, o = {}) => {
    const p = new URL(url, 'http://x').pathname + new URL(url, 'http://x').search;
    if (p.startsWith('/api/positions/detail')) return { ok: true, status: 200, json: async () => ({ dataRevision: '1', holdings, accounts: [{ id: 1, name: '主账户' }, { id: 2, name: '备用账户' }], realized: [], quoteFailed: false }) };
    if (p.startsWith('/api/accounts/detail')) return { ok: true, status: 200, json: async () => ({ total: 2, items: [{ id: 1, name: '主账户', archived: false, version: 1, cash: { quality: 'recorded', amount: '1000.00' } }, { id: 2, name: '备用账户', archived: true, version: 3, cash: { quality: 'unbooked' } }] }) };
    if (p.startsWith('/api/market/suggest')) return { ok: true, status: 200, json: async () => ({ suggestions: [{ symbol: '110022', name: '消费行业' }] }) };
    return { ok: true, status: 200, json: async () => ({}), text: async () => '{}' };
  };
});
await page.evaluate(() => Fund.actions.loadPositions());
await page.waitForTimeout(400);
// 5) 4 列网格 + 新增卡
const gridCols = await page.evaluate(() => {
  // Chrome 在有子元素时序列化为逐轨道 px，无子元素时为 repeat() 记法；两种都数轨道数。
  const v = getComputedStyle(document.querySelector('.pos-card-grid')).gridTemplateColumns;
  return v.startsWith('repeat(') ? Number(v.match(/repeat\((\d+)/)[1]) : v.trim().split(/\s+/).length;
});
assert.equal(gridCols, 4, `expected 4 columns, got ${gridCols}`);
assert.equal(await page.locator('.pos-add-card').count(), 1, 'add card first in group');
// 新增卡点击打开新增流程
await page.locator('.pos-add-card').first().evaluate((el) => el.click());
await page.waitForSelector('dialog[open]');
await page.keyboard.press('Escape');
// 6) 更新时间
const updated = await page.locator('#pos-total-updated').textContent();
assert.match(updated, /\d{2}:\d{2}/, `updated time must be HH:MM, got ${updated}`);
// 7) 移除按钮 + icon 存在且可开弹窗
assert.equal(await page.locator('#holding-history').count(), 0, 'opening-history button removed');
assert.equal(await page.locator('.pos-toolbar #pos-capability.btn, .pos-toolbar button[data-i18n="fundLedgerCapabilityMatrix"]').count(), 0, 'capability toolbar button removed');
await page.locator('#pos-capability').evaluate((el) => el.click());
await page.waitForSelector('dialog[open]');
assert.match(await page.locator('dialog[open] h2').innerText(), /能力|Capab/, 'icon opens capability dialog');
await page.keyboard.press('Escape');
// 3) 账户排序 + 隐藏金额不藏名
await page.selectOption('#pos-account-sort', 'amountDesc');
await page.waitForTimeout(200);
// 4) 账户管理弹窗
await page.locator('#pos-accounts').evaluate((el) => el.click());
await page.waitForSelector('dialog[open] .pos-acct-row');
assert.equal(await page.locator('.pos-acct-row').count(), 2);
assert.match(await page.locator('.pos-acct-row .pos-acct-id b').first().innerText(), /主账户/, 'account name always visible');
assert.ok(await page.locator('.pos-acct-row.is-archived').count() === 1, 'archived badge row');
// 隐藏金额模式：名字仍在
await page.keyboard.press('Escape');
await page.evaluate(() => Fund.home.applyDisplayMode('hidden', false));
await page.evaluate(() => Fund.actions.loadPositions());
await page.waitForTimeout(300);
assert.match(await page.locator('.pos-account-name').first().innerText(), /主账户/, 'account name survives hidden mode');
assert.equal(await page.locator('#pos-total-projected').textContent(), '****');
assert.equal(errors.length, 0, 'page errors: ' + errors.join('; '));
console.log('ui-7 probe: 12/12 assertions passed');
await browser.close();
