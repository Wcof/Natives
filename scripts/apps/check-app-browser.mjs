// Browser UI + real Native processes in a disposable root. Chrome registration is a separate release gate.
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { readFileSync, mkdirSync, writeFileSync, existsSync } from 'node:fs';
import { join, extname, resolve } from 'node:path';
import { appFixture } from './native-fixture.mjs';
import { buildExtension, ROOT } from '../extension-package.mjs';
import { CATALOG_URL, CATALOG_SIG_URL, verifyCatalogSignature } from '../../extension/catalog-client.js';

const { chromium } = await import(process.env.NATIVES_PLAYWRIGHT_MODULE || 'playwright');
const output = resolve('dist/app-browser-evidence');
mkdirSync(output, { recursive: true });
const catalogBytes = readFileSync('dist/app-release/catalog-v2.json');
const signature = readFileSync('dist/app-release/catalog-v2.sig', 'utf8');
await verifyCatalogSignature({ catalogBytes, signatureB64: signature });
const entry = JSON.parse(catalogBytes).apps[0];
const fixture = appFixture();
const bundle = join(fixture.root, 'extension');
const assets = new Set(buildExtension(bundle));
const mime = { '.js': 'text/javascript', '.css': 'text/css', '.html': 'text/html', '.json': 'application/json', '.png': 'image/png' };
const server = createServer((req, res) => {
  const name = new URL(req.url, 'http://localhost').pathname.slice(1);
  if (!assets.has(name)) { res.writeHead(404).end(); return; }
  res.writeHead(200, { 'Content-Type': mime[extname(name)] || 'application/octet-stream' });
  res.end(readFileSync(join(bundle, name)));
});
await new Promise((resolveListening) => server.listen(0, '127.0.0.1', resolveListening));
const base = `http://127.0.0.1:${server.address().port}`;
let browser;
const pagePorts = new Map(), closing = new Set(), errors = [], calls = [], requests = [], nativeErrors = [];
let packageMode = 'ok';
const heldDownloads = [];
try {
  browser = await chromium.launch({ headless: true, channel: process.env.NATIVES_BROWSER_CHANNEL || undefined });
  const context = await browser.newContext({ viewport: { width: 1280, height: 900 }, locale: 'zh-CN' });
  context.on('page', (page) => {
    page.on('pageerror', (error) => errors.push(error.message));
    page.on('close', () => {
      for (const port of pagePorts.get(page)?.values() || []) {
        const close = port.close(); closing.add(close); close.finally(() => closing.delete(close));
      }
      pagePorts.delete(page);
    });
  });
  await context.exposeBinding('__appNative', async ({ page }, { op, key, host, request }) => {
    if (!pagePorts.has(page)) pagePorts.set(page, new Map());
    const ports = pagePorts.get(page);
    if (op === 'connect') { ports.set(key, fixture.connect(host)); return; }
    const port = ports.get(key);
    if (op === 'close') { ports.delete(key); return port?.close(); }
    assert.ok(port, 'Native request needs an open page-owned port');
    calls.push(request);
    const response = await port.raw(request);
    if (!response.ok) nativeErrors.push({ method: request.method, error: response.error });
    return response;
  });
  const messages = JSON.parse(readFileSync(join(ROOT, 'extension/_locales/zh_CN/messages.json')));
  const purgeLabel = messages.appsClearData.message;
  await context.addInitScript(({ messages, base }) => {
    const local = {};
    globalThis.chrome = {
      i18n: { getMessage: (key) => messages[key]?.message || '', getUILanguage: () => 'zh-CN' },
      storage: { local: { get: async (key) => ({ [key]: local[key] }), set: async (value) => Object.assign(local, value) } },
      tabs: { create: ({ url }) => window.open(new URL(url, base).href) },
      runtime: {
        getURL: (path) => `chrome-extension://abcdefghijklmnopabcdefghijklmnop/${path}`,
        getManifest: () => ({ version: '0.1.0' }),
      },
    };
    globalThis.__NATIVES_DEV_NATIVE_CONNECT__ = (host) => {
      const key = crypto.randomUUID(), messages = [], disconnects = [];
      let closed = false;
      const ready = globalThis.__appNative({ op: 'connect', key, host });
      const fail = () => { if (!closed) { closed = true; disconnects.forEach((fn) => fn()); } };
      ready.catch(fail);
      return {
        onMessage: { addListener: (fn) => messages.push(fn) },
        onDisconnect: { addListener: (fn) => disconnects.push(fn) },
        postMessage(request) {
          ready.then(() => globalThis.__appNative({ op: 'request', key, request }))
            .then((message) => { if (!closed) messages.forEach((fn) => fn(message)); }).catch(fail);
        },
        disconnect() { closed = true; void ready.then(() => globalThis.__appNative({ op: 'close', key })).catch(() => {}); },
      };
    };
  }, { messages, base });
  await context.route('https://**/*', async (route) => {
    const url = route.request().url(); requests.push(url);
    // Exercise the fixed mirror path without depending on public network availability.
    if (url.startsWith('https://github.com/')) return route.abort('internetdisconnected');
    if (url.endsWith(new URL(CATALOG_URL).pathname)) return route.fulfill({ body: catalogBytes, contentType: 'application/json' });
    if (url.endsWith(new URL(CATALOG_SIG_URL).pathname)) return route.fulfill({ body: signature, contentType: 'text/plain' });
    if (url.endsWith('.nap')) {
      if (packageMode === 'blocked') return new Promise((resolveDownload) => {
        heldDownloads.push(async () => { await route.abort().catch(() => {}); resolveDownload(); });
      });
      if (packageMode === 'offline') return route.abort('internetdisconnected');
      const bytes = readFileSync(join('dist/app-release', new URL(url).pathname.split('/').at(-1)));
      return route.fulfill({ body: packageMode === 'corrupt' ? Buffer.alloc(bytes.length) : bytes, contentType: 'application/octet-stream' });
    }
    return route.abort();
  });
  const page = await context.newPage();
  await page.goto(base + '/apps.html');
  const card = page.locator(`[data-app-id="${entry.app_id}"]`);
  const button = (name) => card.getByRole('button', { name, exact: true });
  const waitButton = async (name) => { await button(name).waitFor({ state: 'visible', timeout: 15_000 }); };
  await waitButton('安装');
  packageMode = 'blocked';
  const downloading = page.waitForRequest((request) => request.url().startsWith('https://ghproxy.net/') && request.url().endsWith('.nap'));
  await button('安装').click();
  await downloading;
  await button('取消').click();
  await waitButton('安装');
  await Promise.all(heldDownloads.splice(0).map((release) => release()));
  assert.equal(calls.filter((request) => request.method === 'apps:install_commit').length, 0);
  packageMode = 'corrupt';
  await button('安装').click();
  await card.locator('.apps-inline-error').waitFor();
  assert.equal(calls.filter((request) => request.method === 'apps:install_commit').length, 0);
  packageMode = 'offline';
  await button('安装').click();
  await card.locator('.apps-inline-error').waitFor();
  packageMode = 'ok';
  await button('安装').click();
  await waitButton('打开');
  assert.ok(requests.some((url) => url.startsWith('https://ghproxy.net/')));
  await page.screenshot({ path: join(output, 'center-desktop.png'), fullPage: true });
  const opened = context.waitForEvent('page');
  await button('打开').click();
  const app = await opened;
  await app.locator('#demo-host-status.ok').waitFor({ timeout: 10_000 });
  await app.screenshot({ path: join(output, 'demo-desktop.png'), fullPage: true });
  await app.setViewportSize({ width: 390, height: 844 });
  await app.screenshot({ path: join(output, 'demo-mobile.png'), fullPage: true });
  assert.equal(await app.locator('#app-stage').evaluate((el) => el.scrollWidth > el.clientWidth), false);
  const personal = join(fixture.root, 'apps', entry.app_id, 'data');
  mkdirSync(personal, { recursive: true }); writeFileSync(join(personal, 'notes'), 'preserved');
  await app.close();
  await Promise.all([...closing]);
  await button('卸载').click();
  const dialog = page.getByRole('alertdialog');
  assert.equal(await dialog.getByRole('checkbox').isChecked(), false);
  await dialog.getByRole('button', { name: '卸载', exact: true }).click();
  await waitButton(purgeLabel);
  assert.ok(existsSync(join(personal, 'notes')));
  await page.setViewportSize({ width: 390, height: 844 });
  await page.screenshot({ path: join(output, 'center-mobile.png'), fullPage: true });
  await button(purgeLabel).click();
  await dialog.getByRole('button', { name: purgeLabel, exact: true }).click();
  await dialog.getByRole('button', { name: '取消', exact: true }).click();
  assert.ok(existsSync(personal), 'cancelling second confirmation preserves data');
  await button(purgeLabel).click();
  await dialog.getByRole('button', { name: purgeLabel, exact: true }).click();
  await page.screenshot({ path: join(output, 'purge-mobile.png'), fullPage: true });
  await dialog.getByRole('button', { name: purgeLabel, exact: true }).click();
  await button(purgeLabel).waitFor({ state: 'detached' });
  assert.ok(!existsSync(personal));
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
  assert.deepEqual(errors, []);
  console.log(JSON.stringify({ browser: 'Chromium', bridge: 'isolated real Native processes', install: 'passed',
    mirroredDownload: 'passed', cancellation: 'passed', corruptAndOfflineRetry: 'passed', open: 'passed', uninstall: 'passed', purge: 'passed', output }));
  await context.close();
} catch (error) {
  for (const page of browser?.contexts().flatMap((context) => context.pages()) || []) {
    if (page.isClosed()) continue;
    await page.screenshot({ path: join(output, 'failure.png'), fullPage: true });
    console.error(JSON.stringify({ ui: await page.locator('body').innerText(), nativeErrors, errors,
      calls: calls.map((request) => request.method), requests }));
  }
  throw error;
} finally {
  await browser?.close();
  await Promise.allSettled([...closing]);
  await fixture.dispose();
  await new Promise((resolveClosed) => server.close(resolveClosed));
}
