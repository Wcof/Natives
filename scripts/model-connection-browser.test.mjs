// Production settings view/controller with an isolated Native API fixture, not a provider test.
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { extname, resolve } from 'node:path';
import { chromium } from 'playwright-core';

const root = resolve(import.meta.dirname, '..');
const scratch = resolve(root, '.tmp/global-ai-connection');
mkdirSync(scratch, { recursive: true });
const server = createServer((req, res) => {
  const path = new URL(req.url, 'http://localhost').pathname;
  if (path === '/') {
    res.setHeader('Content-Type', 'text/html');
    res.end('<!doctype html><html><meta name="viewport" content="width=device-width,initial-scale=1"><link rel="stylesheet" href="/extension/theme.css"><link rel="stylesheet" href="/extension/files.css"><link rel="stylesheet" href="/extension/model-settings.css"><body></body></html>');
    return;
  }
  const file = resolve(root, '.' + path);
  if (!file.startsWith(root + '/extension/')) { res.writeHead(403).end(); return; }
  try {
    res.setHeader('Content-Type', ({ '.js': 'text/javascript', '.css': 'text/css', '.json': 'application/json' })[extname(file)] || 'text/plain');
    res.end(readFileSync(file));
  } catch { res.writeHead(404).end(); }
});
await new Promise(r => server.listen(0, '127.0.0.1', r));
const browser = await chromium.launch({ executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true });
const checks = [];
try {
  for (const theme of ['archive', 'volt']) for (const language of ['zh_CN', 'en']) for (const width of [1280, 360]) {
    const page = await browser.newPage({ viewport: { width, height: 900 } });
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.goto(`http://127.0.0.1:${server.address().port}`);
    await page.evaluate(async ({ theme, language }) => {
      document.documentElement.dataset.theme = theme;
      const messages = await (await fetch(`/extension/_locales/${language}/messages.json`)).json();
      const t = (key, fallback) => messages[key]?.message || fallback;
      window.fixture = { snapshot: { revision: 1, providers: [], resident: false, defaultModel: {} }, requests: [], fail: false };
      window.__NATIVES_DEV_NATIVE_CONNECT__ = () => {
        const listeners = [];
        return {
          onMessage: { addListener: fn => listeners.push(fn) }, onDisconnect: { addListener() {} }, disconnect() {},
          postMessage(message) {
            const f = window.fixture;
            f.requests.push(message);
            let result = f.snapshot;
            let error;
            if (message.method === 'model_provider_create') {
              if (f.fail) error = { code: 'revision_conflict', message: 'Fixture version conflict' };
              else {
                const p = message.params;
                const id = `fixture-${f.snapshot.providers.length}`;
                const model = { id: p.modelId, displayName: p.modelId, enabled: true };
                f.snapshot = { ...f.snapshot, revision: f.snapshot.revision + 1, providers: [...f.snapshot.providers, { ...p, apiKey: undefined, id, kind: 'custom', models: [model] }] };
                if (p.setAsDefault) f.snapshot.defaultModel = { providerId: id, modelId: p.modelId };
                result = f.snapshot;
              }
            } else if (message.method === 'model_default_model_set') {
              f.snapshot = { ...f.snapshot, revision: f.snapshot.revision + 1, defaultModel: message.params };
              result = f.snapshot;
            } else if (message.method === 'model_provider_test') result = { modelCount: 1 };
            queueMicrotask(() => listeners.forEach(fn => fn({ id: message.id, ok: !error, result, error })));
          },
        };
      };
      const { openModelSettings } = await import('/extension/model-settings.js');
      await openModelSettings({ t, language });
    }, { theme, language });
    const select = page.locator('select[name="defaultModel"]');
    await select.waitFor();
    assert.equal(await select.inputValue(), '');
    await page.locator('[data-action="new-provider"]').click();
    assert.equal(await page.locator('[name="name"]').evaluate(el => el === document.activeElement), true);
    const form = page.locator('[data-new-provider]');
    assert.equal(await form.locator('[name="setAsDefault"]').isChecked(), true);
    await form.locator('[name="name"]').fill('Global fixture');
    await form.locator('[name="baseUrl"]').fill('https://example.test/v1');
    await form.locator('[name="apiKey"]').fill('synthetic-only');
    await form.locator('[name="modelId"]').fill('model::version');
    await form.locator('[data-action="test-new-provider"]').click();
    await page.locator('[data-role="notice"]:not([hidden])').waitFor();
    await page.evaluate(() => { fixture.fail = true; });
    await form.locator('button[type="submit"]').focus();
    await page.keyboard.press('Enter');
    await page.locator('[data-role="error"]:not([hidden])').waitFor();
    assert.equal(await form.locator('[name="modelId"]').inputValue(), 'model::version');
    assert.equal(await page.evaluate(() => fixture.snapshot.providers.length), 0);
    await page.evaluate(() => { fixture.fail = false; });
    await form.locator('button[type="submit"]').click();
    await page.waitForFunction(() => document.querySelector('select[name="defaultModel"]').value === 'fixture-0::model::version');
    const request = await page.evaluate(() => fixture.requests.findLast(r => r.method === 'model_provider_create').params);
    assert.equal(request.setAsDefault, true);
    assert.equal(request.enabled, true);
    assert.equal(request.modelId, 'model::version');
    await select.selectOption('');
    await page.waitForFunction(() => fixture.snapshot.defaultModel.modelId === '');
    await select.selectOption('fixture-0::model::version');
    await page.waitForFunction(() => fixture.snapshot.defaultModel.modelId === 'model::version');
    await page.locator('[data-action="new-provider"]').click();
    assert.equal(await form.locator('[name="setAsDefault"]').isChecked(), false);
    assert.equal(await select.inputValue(), 'fixture-0::model::version');
    await page.locator('[data-action="cancel-new-provider"]').click();
    await page.evaluate(() => { fixture.snapshot.providers[0].enabled = false; });
    await page.locator('[data-role="refresh"]').click();
    await page.waitForFunction(() => document.querySelector('select[name="defaultModel"] option:checked')?.disabled);
    assert.match(await select.locator('option:checked').innerText(), language === 'en' ? /unavailable/ : /不可用/);
    assert.equal(await page.evaluate(() => document.querySelector('.model-settings-workspace').scrollWidth > document.querySelector('.model-settings-workspace').clientWidth + 1), false);
    if (theme === 'volt' && language === 'en' && width === 360) await page.screenshot({ path: resolve(scratch, 'settings-narrow.png') });
    assert.deepEqual(errors, []);
    checks.push({ theme, language, width, keyboard: true, setup: true, failureDraftPreserved: true, defaultPreserved: true, unavailable: true });
    await page.close();
  }
  writeFileSync(resolve(scratch, 'browser-report.json'), JSON.stringify(checks, null, 2));
  console.log(`Global AI connection Chrome checks passed: ${checks.length} layouts; isolated API fixture.`);
} finally {
  await browser.close();
  server.close();
}
