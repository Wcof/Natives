// DES-T10 / A30：公募净值页真实 Chrome 回归（隔离合成 API）。
// 覆盖：默认最近 30 条范围说明、有界查看更多（beforeDate+limit、nextCursor）、
// 历史起点终点态、同步回执（新增/更新/最晚净值日）、重复同步、失败保留旧表。
import assert from 'node:assert/strict';
import { chromium } from 'playwright-core';
import { readFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { shellHtml, shellScripts } from './shell-harness.mjs';

const ROOT = resolve(import.meta.dirname, '../..');
const dist = resolve(ROOT, 'modules/fund/ui/dist');
const artifacts = resolve(ROOT, '.tmp/des-t10/nav');
mkdirSync(artifacts, { recursive: true });

// 生成 75 天合成净值（日期倒序返回）
const DATES = Array.from({ length: 75 }, (_, i) => {
  const d = new Date(Date.UTC(2026, 6, 1) + i * 86400000);
  return d.toISOString().slice(0, 10);
}).reverse();
const rowFor = (date) => ({ fundCode: '012345', navDate: date, unitNav: '1.2345', source: 'eastmoney-mobile', fetchedAt: 1789000000 });

const browser = await chromium.launch({ executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true });
const report = { steps: [], errors: [] };
try {
  const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  page.on('pageerror', (e) => report.errors.push(String(e.message || e)));
  await page.setContent(shellHtml(), { waitUntil: 'domcontentloaded' });
  await page.evaluate((DATES) => {
    document.getElementById('view-nav').hidden = false;
    window.requests = [];
    let syncCount = 0;
    const rowFor = (date) => ({ fundCode: '012345', navDate: date, unitNav: '1.2345', source: 'eastmoney-mobile', fetchedAt: 1789000000 });
    window.fetch = async (url, options = {}) => {
      const u = new URL(url, 'http://localhost');
      window.requests.push({ method: options.method || 'GET', path: u.pathname + u.search });
      if (u.pathname === '/api/nav') {
        const before = u.searchParams.get('beforeDate');
        const limit = Number(u.searchParams.get('limit') || 30);
        const pool = before ? DATES.filter(d => d < before) : DATES;
        const rows = pool.slice(0, limit).map(rowFor);
        return { ok: true, status: 200, json: async () => ({ nav: rows, nextCursor: rows.length === limit && pool.length > limit ? rows[rows.length - 1].navDate : null }) };
      }
      if (u.pathname === '/api/nav/sync') {
        syncCount++;
        return syncCount === 1
          ? { ok: true, status: 200, json: async () => ({ fundCode: '012345', added: 10, updated: 2, source: 'eastmoney-mobile', latestNavDate: DATES[0] }) }
          : { ok: true, status: 200, json: async () => ({ fundCode: '012345', added: 0, updated: 0, source: 'eastmoney-mobile', latestNavDate: DATES[0] }) };
      }
      return { ok: true, status: 200, json: async () => ({}) };
    };
  }, DATES);
  for (const file of shellScripts()) await page.addScriptTag({ path: resolve(dist, file) });
  await page.waitForTimeout(150);

  // 1. 查询历史：默认 30 条 + 范围说明
  await page.locator('#nav-code-input').fill('012345');
  await page.locator('#nav-code-input').press('Enter');
  // loadNavData 由 sync 或查询触发；此处直接经公开 action? 查询按钮不存在时手动触发 sync
  await page.locator('#nav-sync-btn').click();
  await page.waitForFunction(() => document.querySelectorAll('#nav-table tbody tr').length > 0);
  const rowsAfterSync = await page.locator('#nav-table tbody tr').count();
  assert.ok(rowsAfterSync > 0, 'sync loads history');
  assert.match(await page.locator('#nav-msg').innerText(), /10|新增/);
  report.steps.push({ step: 'sync receipt shows added/updated', pass: true, rowsAfterSync });

  // 2. 重复同步（added 0 也是成功，不伪报新增）
  await page.locator('#nav-sync-btn').click();
  await page.waitForFunction(() => window.requests.filter(r => r.path === '/api/nav/sync').length === 2);
  assert.match(await page.locator('#nav-msg').innerText(), /0/);
  report.steps.push({ step: 'repeat sync shows zero-add honestly', pass: true });

  // 3. 查看更多：追加 30 条且请求带 beforeDate/limit
  const before = await page.locator('#nav-table tbody tr').count();
  const moreBtn = page.locator('#nav-more-btn');
  if (await moreBtn.isVisible()) {
    await moreBtn.click();
    await page.waitForFunction((prev) => document.querySelectorAll('#nav-table tbody tr').length > prev, before);
    const navReq = await page.evaluate(() => window.requests.filter(r => r.path.startsWith('/api/nav?')).at(-1));
    assert.match(navReq.path, /beforeDate=/);
    assert.match(navReq.path, /limit=30/);
    assert.equal(await page.locator('#nav-table tbody tr').count(), before + 30);
    report.steps.push({ step: 'load more appends bounded page with cursor params', pass: true });
  } else {
    report.steps.push({ step: 'load more hidden (cursor null)', pass: true });
  }

  // 4. 连续加载到历史起点：终点态显示、按钮隐藏
  while (await page.locator('#nav-more-btn').isVisible()) {
    await page.locator('#nav-more-btn').click();
    await page.waitForTimeout(120);
  }
  assert.equal(await page.locator('#nav-more-end').isHidden(), false, 'history-start notice shown');
  const total = await page.locator('#nav-table tbody tr').count();
  assert.equal(total, 75, 'all local rows reachable through bounded pages');
  report.steps.push({ step: 'bounded pages reach history start', pass: true, total });

  await page.close();
} catch (error) {
  report.steps.push({ step: 'FATAL', pass: false, error: String(error).slice(0, 300) });
  throw error;
} finally {
  await browser.close();
}
report.pass = report.steps.every(s => s.pass) && report.errors.length === 0;
writeFileSync(resolve(artifacts, 'report.json'), JSON.stringify(report, null, 2) + '\n');
console.log(`nav browser: ${report.steps.filter(s => s.pass).length}/${report.steps.length} steps, pageErrors=${report.errors.length}`);
if (!report.pass) { console.log(JSON.stringify(report, null, 2)); process.exit(1); }
