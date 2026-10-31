// Production Web → authenticated Core → signed Runtime → isolated SQLite.
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, copyFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { homedir } from 'node:os';
import { randomBytes } from 'node:crypto';
import net from 'node:net';
import { chromium } from 'playwright-core';
import { verifyNativeWealth } from './home-wealth-native.mjs';

const root = resolve(import.meta.dirname, '../..'), scratch = resolve(root, '.tmp/home-wealth-details-20261026');
mkdirSync(scratch, { recursive: true });
const out = mkdtempSync(resolve(scratch, 'web-'));
const env = { ...process.env, HOME: resolve(out, 'home'), RUSTUP_HOME: process.env.RUSTUP_HOME || resolve(homedir(), '.rustup'), CARGO_HOME: process.env.CARGO_HOME || resolve(homedir(), '.cargo'), NATIVES_DEV_PRODUCT_SOURCE: resolve(out, 'product-source'), NATIVES_STATE_DIR: resolve(out, 'state'), NATIVES_WORKSPACE_DB: resolve(out, 'workspace.db'), NATIVES_APPS_ROOT: resolve(out, 'apps'), NATIVES_WEB_ROOT: resolve(root, 'extension') };
mkdirSync(env.HOME, { recursive: true });
const prepared = spawnSync('rtk', ['proxy', process.execPath, 'scripts/apps/refresh-dev-product-source.mjs'], { cwd: root, env, encoding: 'utf8', timeout: 120000 });
assert.equal(prepared.status, 0, prepared.stderr || prepared.stdout);
const binary = resolve(out, 'native-file-host');
copyFileSync(resolve(root, 'target/debug/native-file-host'), binary);
const port = await new Promise(done => { const server = net.createServer(); server.listen(0, '127.0.0.1', () => { const port = server.address().port; server.close(() => done(port)); }); });
const base = `http://127.0.0.1:${port}`, token = randomBytes(32).toString('base64url');
const core = spawn(binary, ['--service', '--port', String(port), '--bootstrap', token], { env, stdio: ['ignore', 'ignore', 'pipe'] });
let stderr = '', browser;
const report = { steps: [], errors: [] };
core.stderr.on('data', chunk => stderr += chunk);
try {
  for (let i = 0; ; i++) {
    try { if ((await fetch(base + '/healthz')).ok) break; } catch {}
    assert.ok(i < 100 && core.exitCode === null, stderr);
    await new Promise(r => setTimeout(r, 100));
  }
  browser = await chromium.launch({ executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true });
  const page = await browser.newPage();
  page.on('pageerror', error => report.errors.push(error.message));
  await page.goto(`${base}/app.html?app=fund#token=${token}`);
  const frame = page.frameLocator('iframe');
  await frame.locator('#home-privacy-btn').waitFor({ timeout: 30000 });
  await verifyNativeWealth(page, frame, report);
  assert.deepEqual(report.errors, []);
  console.log('Home wealth real Web/Core/Runtime/SQLite checks passed.');
} finally {
  await browser?.close();
  if (core.exitCode === null) core.kill('SIGTERM');
  writeFileSync(resolve(out, 'core.stderr.log'), stderr);
  writeFileSync(resolve(out, 'report.json'), JSON.stringify(report, null, 2));
  console.log(`Report: ${out}/report.json`);
}
