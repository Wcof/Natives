// Standalone Web Entry Chrome Browser Test for apps.html (T04 Gate)
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { chromium } from 'playwright-core';

const PORT = 8771;
const BOOTSTRAP = 'chrome-apps-bootstrap-test';

console.log('--- Launching native-file-host in Service Mode ---');
const srv = spawn('./target/debug/native-file-host', [
  '--service',
  '--port',
  String(PORT),
  '--bootstrap',
  BOOTSTRAP,
]);

await new Promise((r) => setTimeout(r, 600));

console.log('--- Launching clean Chrome browser (no extension) for apps.html ---');
const browser = await chromium.launch({
  executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  headless: true,
});

const context = await browser.newContext({
  viewport: { width: 1440, height: 1000 },
});

try {
  const page = await context.newPage();
  const exchangePromise = page.waitForResponse((res) => res.url().includes('/web/session/exchange'));
  const targetUrl = `http://127.0.0.1:${PORT}/apps.html#token=${BOOTSTRAP}`;
  await page.goto(targetUrl, { waitUntil: 'load' });
  const exchangeRes = await exchangePromise;
  assert.equal(exchangeRes.status(), 200, 'exchange response must be 200');

  // Wait for apps list to render
  await page.waitForSelector('#apps-list', { state: 'attached' });
  console.log('✓ Apps list element attached in Web mode');

  // Verify apps query via NativeClient in Web mode
  const appsResult = await page.evaluate(async () => {
    const { createNativeClient } = await import('./native-client.js');
    const client = createNativeClient({ host: 'com.natives.file_manager' });
    const list = await client.call('apps:list', {});
    return list;
  });
  assert.ok(appsResult, 'apps:list must return apps array');
  assert.ok(Array.isArray(appsResult.apps), 'apps must be an array');
  console.log('✓ apps:list query succeeded in Web mode, apps count:', appsResult.apps.length);

  // Screenshot standalone apps page
  const shotPath = '/Users/ldh/Downloads/project/plan/memory/dual-entry-migration/evidence/20261015-run01/browser/web-after/web-apps-1440.png';
  await page.screenshot({ path: shotPath });
  console.log('✓ Standalone Web apps page screenshot captured:', shotPath);

  console.log('--- ALL T04 APPS WEB ENTRY BROWSER CHECKS PASSED ---');
} finally {
  await browser.close();
  srv.kill();
}
