// Standalone Web Entry Chrome Browser Test for space.html (T04 Gate)
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { chromium } from 'playwright-core';

const PORT = 8771;
const BOOTSTRAP = 'chrome-space-bootstrap-test';

console.log('--- Launching native-file-host in Service Mode ---');
const srv = spawn('./target/debug/native-file-host', [
  '--service',
  '--port',
  String(PORT),
  '--bootstrap',
  BOOTSTRAP,
]);

await new Promise((r) => setTimeout(r, 600));

console.log('--- Launching clean Chrome browser (no extension) for space.html ---');
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
  const targetUrl = `http://127.0.0.1:${PORT}/space.html#token=${BOOTSTRAP}`;
  await page.goto(targetUrl, { waitUntil: 'load' });
  const exchangeRes = await exchangePromise;
  assert.equal(exchangeRes.status(), 200, 'exchange response must be 200');

  // Wait for space UI to initialize
  await page.waitForSelector('#dashboard-host', { state: 'attached' });
  console.log('✓ Space dashboard element attached in Web mode');

  // Test workspace session query via NativeClient in Web mode
  const wsResult = await page.evaluate(async () => {
    const { createNativeClient } = await import('./native-client.js');
    const client = createNativeClient({ host: 'com.natives.file_manager' });
    const sess = await client.call('workspace_session', {});
    const snap = await client.call('workspace_snapshot', { workspaceId: sess.activeWorkspaceId });
    return { sess, snap };
  });
  assert.ok(wsResult.snap, 'workspace_snapshot must return a snapshot');
  console.log('✓ workspace_snapshot query succeeded in Web mode:', Object.keys(wsResult.snap));

  // Screenshot standalone space page
  const shotPath = '/Users/ldh/Downloads/project/plan/memory/dual-entry-migration/evidence/20261015-run01/browser/web-after/web-space-1440.png';
  await page.screenshot({ path: shotPath });
  console.log('✓ Standalone Web space page screenshot captured:', shotPath);

  console.log('--- ALL T04 SPACE WEB ENTRY BROWSER CHECKS PASSED ---');
} finally {
  await browser.close();
  srv.kill();
}
