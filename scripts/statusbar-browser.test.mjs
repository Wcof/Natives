// Production panel HTML with isolated usage projections; no user database or provider calls.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdirSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { chromium } from 'playwright-core';

const root = resolve(import.meta.dirname, '..');
const scratch = resolve(root, '.tmp/statusbar-fix');
mkdirSync(scratch, { recursive: true });
const binary = resolve(scratch, 'data-test');
const htmlPath = resolve(scratch, 'panel.html');
execFileSync('clang', ['-fobjc-arc', '-framework', 'Foundation', '-framework', 'WebKit', '-lsqlite3',
  '-I', resolve(root, 'installers/macos/resources'), resolve(root, 'scripts/statusbar-data.test.m'), '-o', binary]);
execFileSync(binary, [htmlPath]);
const browser = await chromium.launch({ executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true });
try {
  const page = await browser.newPage({ viewport: { width: 380, height: 620 } });
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.setContent(readFileSync(htmlPath, 'utf8'));
  await page.evaluate(() => {
    const empty = { totalTokens: 0, costUsd: 0, priced: false };
    const history = { totalTokens: 1200, costUsd: 0, priced: false };
    dispatchEvent(new MessageEvent('message', { data: { type: 'panel', panel: {
      today: empty, thisWeek: empty, thisMonth: history, allTime: history,
      toolsByPeriod: { today: [], thisWeek: [], thisMonth: [{ name: 'gpt-6.1-sol', tokens: 1200, priced: false }], allTime: [{ name: 'gpt-6.1-sol', tokens: 1200, priced: false }] },
      sessionsByPeriod: { today: [], thisWeek: [], thisMonth: [{ tool: 'codex', id: 'Historical session', totalTokens: 1200, priced: false }], allTime: [{ tool: 'codex', id: 'Historical session', totalTokens: 1200, priced: false }] },
    } } }));
  });
  assert.equal(await page.locator('#tNum').textContent(), '0');
  assert.equal(await page.locator('#tSub').textContent(), '--');
  assert.match(await page.locator('#tools').textContent(), /暂无使用数据/);
  assert.match(await page.locator('#sessions').textContent(), /暂无会话/);
  await page.locator('[data-p="month"]').focus();
  await page.keyboard.press('Enter');
  assert.equal(await page.locator('#tCap').textContent(), '近 30 天消耗');
  assert.match(await page.locator('#tools').textContent(), /gpt-6\.1-sol/);
  assert.match(await page.locator('#sessions').textContent(), /Historical session/);
  assert.equal(await page.locator('#tSub').textContent(), '--');
  await page.locator('[data-p="today"]').click();
  assert.match(await page.locator('#sessions').textContent(), /暂无会话/);
  await page.evaluate(() => dispatchEvent(new MessageEvent('message', { data: { type: 'panel', usageOffline: true, panel: {} } })));
  assert.equal(await page.locator('#tNum').textContent(), '--');
  assert.match(await page.locator('#tSub').textContent(), /用量数据暂不可用/);
  await page.setContent(readFileSync(htmlPath, 'utf8'));
  await page.evaluate(() => {
    dispatchEvent(new MessageEvent('message', { data: { type: 'limits', quotaSource: '8766', items: [{ provider: 'Codex', windows: [{ label: '5H', pct: 40 }] }] } }));
    dispatchEvent(new MessageEvent('message', { data: { type: 'panel', panel: { today: { totalTokens: 100, costUsd: 2, priced: true }, allTime: { totalTokens: 100, costUsd: 2, priced: true }, tools: [], sessions: [] } } }));
  });
  assert.equal(await page.locator('.cost-val').textContent(), '≈$2.00', 'usage arriving after quotas updates the estimate');
  await page.evaluate(() => {
    PANEL.trends = [{ date: '2026-10-11', totalTokens: 1 }, { date: '2026-10-12', totalTokens: 2 }];
    renderTrend();
  });
  assert.deepEqual(await page.locator('#trend .col span').allTextContents(), ['10-11', '10-12']);
  await page.evaluate(() => { window.webkit = { messageHandlers: { native: { postMessage() {} } } }; });
  await page.locator('#refreshBtn').click();
  assert.equal(await page.locator('#refreshBtn').evaluate(button => button.classList.contains('spinning')), true);
  assert.equal(await page.evaluate(() => window._spinGuard), undefined, 'UI timer cannot report completion before native results');
  await page.evaluate(() => dispatchEvent(new MessageEvent('message', { data: { type: 'refreshDone' } })));
  assert.equal(await page.locator('#refreshBtn').evaluate(button => button.classList.contains('spinning')), false);
  assert.deepEqual(errors, []);
  console.log('statusbar Chrome: empty periods, model, unknown cost, keyboard and unavailable state passed');
} finally {
  await browser.close();
}
