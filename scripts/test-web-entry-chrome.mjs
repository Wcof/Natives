// Standalone Web Entry Chrome Browser Test (without Extension) - T03 Exit Gate
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { chromium } from 'playwright-core';

const PORT = 8771;
const BOOTSTRAP = 'chrome-web-entry-bootstrap-test';

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

// Launch Chrome without extension in a fresh temporary profile
console.log('--- Launching clean Chrome browser (no extension) ---');
const browser = await chromium.launch({
  executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  headless: true,
});

const context = await browser.newContext({
  viewport: { width: 1440, height: 1000 },
});

try {
  const page = await context.newPage();

  const consoleErrors = [];
  page.on('request', (req) => {
    console.log(`REQ: ${req.method()} ${req.url()}`);
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

  // 1. Unauthenticated direct access check
  console.log('1. Testing unauthenticated RPC rejection...');
  await page.goto(`http://127.0.0.1:${PORT}/healthz`);
  const unauthRes = await page.evaluate(async (port) => {
    try {
      const res = await fetch(`http://127.0.0.1:${port}/web/rpc`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ id: '1', method: 'version', params: {} }),
      });
      return { status: res.status, ok: res.ok };
    } catch (e) {
      return { error: e.message };
    }
  }, PORT);
  assert.equal(unauthRes.status, 401, 'unauthenticated RPC must return 401');
  console.log('✓ Unauthenticated RPC rejected with 401');

  // 2. Open Web Entry with one-time bootstrap token fragment
  console.log('2. Opening Web Entry with bootstrap token...');
  const exchangePromise = page.waitForResponse((res) => res.url().includes('/web/session/exchange'));
  const targetUrl = `http://127.0.0.1:${PORT}/files.html#token=${BOOTSTRAP}`;
  await page.goto(targetUrl, { waitUntil: 'load' });
  const exchangeRes = await exchangePromise;
  assert.equal(exchangeRes.status(), 200, 'exchange response must be 200');

  // Wait for bootstrap exchange to settle and hash to be cleared
  await page.waitForFunction(() => !window.location.hash.includes('token='));
  console.log('✓ Bootstrap token fragment immediately cleared from URL');

  // Verify session cookie was set
  const cookies = await context.cookies(`http://127.0.0.1:${PORT}`);
  const sessionCookie = cookies.find((c) => c.name === 'natives_dev_session' || c.name === 'natives_session');
  assert.ok(sessionCookie, 'session cookie must be present in browser context');
  assert.equal(sessionCookie.httpOnly, true, 'session cookie must be HttpOnly');
  assert.equal(sessionCookie.sameSite, 'Strict', 'session cookie must be SameSite=Strict');
  console.log('✓ Session cookie verified (HttpOnly, SameSite=Strict)');

  // 3. Authenticated query through NativeClient in Web Mode
  console.log('3. Testing NativeClient query over Web RPC...');
  const clientResult = await page.evaluate(async () => {
    const { createNativeClient } = await import('./native-client.js');
    const client = createNativeClient({ host: 'com.natives.file_manager' });
    const res = await client.call('version', {});
    return res;
  });
  assert.ok(clientResult, 'version query must return result');
  assert.equal(clientResult.protocolVersion, 1);
  assert.equal(clientResult.hostVersion, '0.1.0');
  console.log('✓ NativeClient query succeeded in standalone web mode:', clientResult);

  // 4. Test theme and language setting persistence
  console.log('4. Testing theme and language settings in Web mode...');
  await page.evaluate(async () => {
    const { storageGet, storageSet } = await import('./files-preferences.js');
    await storageSet('natives-theme', 'volt');
    const readBack = await storageGet('natives-theme', 'archive');
    return readBack;
  });
  const themeReadBack = await page.evaluate(async () => {
    const { storageGet } = await import('./files-preferences.js');
    return storageGet('natives-theme', 'archive');
  });
  assert.equal(themeReadBack, 'volt', 'theme setting persisted and read back in Web mode');
  console.log('✓ Settings persistence verified in Web mode');

  // 5. Take screenshot of Web entry in clean Chrome
  const shotPath = '/Users/ldh/Downloads/project/plan/memory/dual-entry-migration/evidence/20261015-run01/browser/web-after/web-files-1440.png';
  await page.screenshot({ path: shotPath });
  console.log('✓ Standalone Web files page screenshot captured:', shotPath);

  console.log('--- ALL T03 STANDALONE WEB ENTRY BROWSER CHECKS PASSED ---');
} finally {
  await browser.close();
  srv.kill();
}
