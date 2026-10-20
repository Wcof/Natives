// Standalone Web Entry Chrome Browser Test for app.html (Fund & Tokenusage in Web Mode)
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { chromium } from 'playwright-core';

const PORT = 8771;
const BOOTSTRAP = 'chrome-app-runtime-bootstrap-test';

console.log('--- Launching native-file-host in Service Mode ---');
const srv = spawn('./target/debug/native-file-host', [
  '--service',
  '--port',
  String(PORT),
  '--bootstrap',
  BOOTSTRAP,
]);

srv.stderr.on('data', (d) => {
  const msg = d.toString().trim();
  if (msg) console.log('[Service stderr]', msg);
});

await new Promise((r) => setTimeout(r, 600));

console.log('--- Launching clean Chrome browser (no extension) for app.html ---');
const browser = await chromium.launch({
  executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  headless: true,
});

const context = await browser.newContext({
  viewport: { width: 1440, height: 1000 },
});

try {
  const page = await context.newPage();

  page.on('request', (req) => {
    if (req.url().includes('/web/rpc')) {
      console.log('RPC REQ BODY:', req.postData(), 'CSRF:', req.headers()['x-csrf-token']);
    }
  });
  page.on('response', (res) => {
    if (res.status() >= 400) console.log(`HTTP ${res.status()}:`, res.url());
  });
  page.on('console', (msg) => {
    console.log(`PAGE LOG [${msg.type()}]:`, msg.text());
  });

  page.on('pageerror', (err) => {
    console.log('PAGE ERROR:', err.message);
  });

  // 1. Visit app.html?app=fund
  console.log('1. Loading app.html?app=fund in Web mode...');
  const exchangePromise = page.waitForResponse((res) => res.url().includes('/web/session/exchange'));
  await page.goto(`http://127.0.0.1:${PORT}/app.html?app=fund#token=${BOOTSTRAP}`, { waitUntil: 'load' });
  const exchangeRes = await exchangePromise;
  assert.equal(exchangeRes.status(), 200, 'exchange response must be 200');

  // Wait for app stage and title to resolve
  await page.waitForSelector('#app-stage', { state: 'attached' });
  console.log('✓ App stage attached');

  await page.waitForTimeout(2000);
  console.log('STAGE HTML:', await page.locator('#app-stage').innerHTML());
  console.log('TOAST TEXT:', await page.locator('#app-toast').textContent());

  // Wait for managed-app-frame or boot loader
  const frameLocator = page.locator('iframe.managed-app-frame');
  await frameLocator.waitFor({ state: 'attached', timeout: 15000 });
  console.log('✓ Managed app sandbox iframe attached for fund');

  const frameSrc = await frameLocator.getAttribute('src');
  assert.ok(frameSrc.startsWith('http://127.0.0.1:'), 'frame src must be dynamic loopback port');
  console.log('✓ Fund sandbox iframe loaded dynamically on:', frameSrc);

  // Take screenshot of Fund in Web mode
  const shotPath = '/Users/ldh/Downloads/project/plan/memory/dual-entry-migration/evidence/20261015-run01/browser/web-after/web-app-fund-1440.png';
  await page.screenshot({ path: shotPath });
  console.log('✓ Standalone Web fund app screenshot captured:', shotPath);

  // 2. Visit app.html?app=tokenusage
  console.log('2. Loading app.html?app=tokenusage in Web mode...');
  await page.goto(`http://127.0.0.1:${PORT}/app.html?app=tokenusage`, { waitUntil: 'load' });
  const tuFrameLocator = page.locator('iframe.managed-app-frame');
  await tuFrameLocator.waitFor({ state: 'attached', timeout: 15000 });
  console.log('✓ Managed app sandbox iframe attached for tokenusage');

  const tuFrameSrc = await tuFrameLocator.getAttribute('src');
  assert.ok(tuFrameSrc.startsWith('http://127.0.0.1:'), 'frame src must be dynamic loopback port');
  console.log('✓ Tokenusage sandbox iframe loaded dynamically on:', tuFrameSrc);

  const tuShotPath = '/Users/ldh/Downloads/project/plan/memory/dual-entry-migration/evidence/20261015-run01/browser/web-after/web-app-tokenusage-1440.png';
  await page.screenshot({ path: tuShotPath });
  console.log('✓ Standalone Web tokenusage app screenshot captured:', tuShotPath);

  console.log('--- ALL APP RUNTIME WEB ENTRY BROWSER CHECKS PASSED ---');
} finally {
  await browser.close();
  srv.kill();
}
