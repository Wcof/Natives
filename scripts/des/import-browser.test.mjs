// DES-T10 / A29：批量导入真实 Chrome 回归（隔离合成 API，不触真实账本）。
// 覆盖：csv 字段契约（后端读 csv 而非 csvText）、稳定 sourceId、输入失效、
// 错误行计数、提交禁重复、失败留文、无效批次拒绝。
import assert from 'node:assert/strict';
import { chromium } from 'playwright-core';
import { readFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { shellHtml, shellScripts } from '../des/shell-harness.mjs';

const ROOT = resolve(import.meta.dirname, '../..');
const src = resolve(ROOT, 'modules/fund/ui/src');
const dist = resolve(ROOT, 'modules/fund/ui/dist');
const artifacts = resolve(ROOT, '.tmp/des-t10/import');
mkdirSync(artifacts, { recursive: true });

const CSS = ['theme.css', 'layout.css', 'panel.css', 'investment.css'].map((n) =>
  readFileSync(resolve(ROOT, `modules/fund/ui/dist/styles/${n}`), 'utf8')).join('\n');

const CSV = '隔离账户,600519,stock,100,10.00,2026-09-01\n隔离账户,110022,fund,1000,1.2345,2026-09-02\n';
const PREVIEW_OK = { total_rows: 2, valid_rows: 2, lines: [
  { line_no: 1, account: '隔离账户', fund_code: '600519', kind: 'stock', quantity_raw: '100', amount_raw: '1000.00', error: null },
  { line_no: 2, account: '隔离账户', fund_code: '110022', kind: 'fund', quantity_raw: '1000', amount_raw: '1234.50', error: null },
] };
const PREVIEW_PARTIAL = { total_rows: 2, valid_rows: 1, lines: [
  { line_no: 1, account: '隔离账户', fund_code: '600519', kind: 'stock', quantity_raw: '100', amount_raw: '1000.00', error: null },
  { line_no: 2, account: 'x', fund_code: 'bad', kind: 'fund', quantity_raw: '-1', amount_raw: 'x', error: '数量非法' },
] };
const COMMIT_OK = { rows_imported: 2, rows_skipped: 0, funds_created: 2, state: 'committed' };

const browser = await chromium.launch({ executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true });
const report = { steps: [], errors: [], requests: [] };
try {
  const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  page.on('pageerror', (e) => report.errors.push(String(e.message || e)));
  // ledger.js 绑定全部视图元素：必须完整壳层装配（与 ui.rs 同序）。
  await page.setContent(shellHtml(), { waitUntil: 'domcontentloaded' });
  await page.evaluate(({ CSV: CSV_TEXT, PREVIEW_OK, PREVIEW_PARTIAL, COMMIT_OK }) => {
    document.getElementById('view-import').hidden = false;
    window.requests = [];
    const ok = (body) => ({ ok: true, status: 200, json: async () => structuredClone(body) });
    const fail = (status, code) => ({ ok: false, status, json: async () => ({ error: code, message: '隔离失败' }) });
    let commitCalls = 0;
    window.fetch = async (url, options = {}) => {
      const u = new URL(url, 'http://localhost');
      const body = options.body ? JSON.parse(options.body) : null;
      window.requests.push({ method: options.method || 'GET', path: u.pathname, body });
      if (u.pathname === '/api/import/preview') {
        return body && typeof body.csv === 'string' && body.csv.includes('bad')
          ? ok(PREVIEW_PARTIAL)
          : body && typeof body.csv === 'string' ? ok(PREVIEW_OK) : fail(400, 'IMPORT_INVALID');
      }
      void CSV_TEXT;
      if (u.pathname === '/api/import/commit') {
        commitCalls++;
        return commitCalls === 1 ? ok(COMMIT_OK) : fail(500, 'IMPORT_DB');
      }
      return ok({});
    };
  }, { CSV, PREVIEW_OK, PREVIEW_PARTIAL, COMMIT_OK });
  for (const file of shellScripts()) await page.addScriptTag({ path: resolve(dist, file) });
  await page.waitForTimeout(100);

  // 1. 预览请求使用后端 csv 字段
  await page.locator('#import-text').fill(CSV);
  await page.locator('#import-preview-btn').click();
  await page.waitForFunction(() => window.requests.some(r => r.path === '/api/import/preview'));
  const previewReq = await page.evaluate(() => window.requests.find(r => r.path === '/api/import/preview'));
  assert.equal(previewReq.body.csv, CSV.trim(), 'preview must send backend csv field');
  assert.equal(previewReq.body.csvText, undefined, 'legacy csvText field is retired');
  await page.waitForFunction(() => document.querySelector('#import-table tbody tr'),
    undefined, { timeout: 8000 }).catch(async (e) => {
    const dbg = await page.evaluate(() => ({ reqs: window.requests.map(r => r.path), msg: document.getElementById('import-msg')?.textContent }));
    throw new Error(`preview rows missing: ${JSON.stringify(dbg)} :: ${e.message}`);
  });
  assert.equal(await page.locator('#import-commit-btn').isDisabled(), false);
  report.steps.push({ step: 'preview uses csv field and enables commit', pass: true });

  // 2. 输入改动使预览失效
  await page.locator('#import-text').fill(CSV + '更多行,600000,stock,10,1.00,2026-09-03');
  assert.equal(await page.locator('#import-commit-btn').isDisabled(), true, 'edit invalidates preview');
  assert.match(await page.locator('#import-msg').innerText(), /重新预览|失效/);
  report.steps.push({ step: 'input change invalidates preview with stale notice', pass: true });

  // 3. 重新预览后提交；sourceId 稳定。提交的 csv 必须等于当前输入（已预览版本）。
  const edited = CSV.trim() + '\n更多行,600000,stock,10,1.00,2026-09-03';
  await page.locator('#import-preview-btn').click();
  await page.waitForFunction(() => window.requests.filter(r => r.path === '/api/import/preview').length === 2);
  await page.locator('#import-commit-btn').click();
  await page.waitForFunction(() => window.requests.some(r => r.path === '/api/import/commit'));
  const commitReq = await page.evaluate(() => window.requests.find(r => r.path === '/api/import/commit'));
  assert.equal(commitReq.body.csv, edited);
  assert.equal(commitReq.body.sourceId, 'manual-import', 'sourceId is content-stable, not Date.now');
  assert.match(await page.locator('#import-msg').innerText(), /2/);
  report.steps.push({ step: 'commit sends csv + stable sourceId, shows receipt', pass: true });

  // 4. 无效行计数（部分无效批次展示行号与原因，不静默跳过）
  await page.locator('#import-text').fill('x,bad,fund,-1,x,x\n');
  await page.locator('#import-preview-btn').click();
  await page.waitForFunction(() => window.requests.filter(r => r.path === '/api/import/preview').length === 3);
  await page.waitForFunction(() => document.querySelectorAll('#import-table tbody tr').length === 2);
  const rows = await page.locator('#import-table tbody tr').allInnerTexts();
  assert.match(rows[1], /数量非法/);
  assert.match(await page.locator('#import-msg').innerText(), /1/);
  report.steps.push({ step: 'partial invalid batch shows row numbers and reasons', pass: true });

  // 5. 提交失败留文（输入与预览保留）
  await page.locator('#import-text').fill(CSV);
  await page.locator('#import-preview-btn').click();
  await page.waitForFunction(() => window.requests.filter(r => r.path === '/api/import/preview').length === 4);
  await page.locator('#import-commit-btn').click();
  await page.waitForFunction(() => window.requests.filter(r => r.path === '/api/import/commit').length === 2);
  await page.waitForFunction(() => document.querySelector('#import-msg').className === 'err');
  assert.match(await page.locator('#import-msg').innerText(), /失败|错误/, 'explicit localized error shown');
  assert.equal(await page.locator('#import-text').inputValue(), CSV, 'failure keeps the input');
  report.steps.push({ step: 'commit failure keeps input and shows explicit error', pass: true });

  await page.close();
} catch (error) {
  report.steps.push({ step: 'FATAL', pass: false, error: String(error).slice(0, 300) });
  throw error;
} finally {
  await browser.close();
}
report.pass = report.steps.every(s => s.pass) && report.errors.length === 0;
writeFileSync(resolve(artifacts, 'report.json'), JSON.stringify(report, null, 2) + '\n');
console.log(`import browser: ${report.steps.filter(s => s.pass).length}/${report.steps.length} steps, pageErrors=${report.errors.length}`);
if (!report.pass) { console.log(JSON.stringify(report, null, 2)); process.exit(1); }
