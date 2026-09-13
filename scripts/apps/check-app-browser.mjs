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
// AC-12: browser evidence must exercise the CURRENT Catalog v3 wire
// format and the v4 install transaction, not the retired v2 catalog.
const catalogBytes = readFileSync('dist/app-release/catalog-v3.json');
const signature = readFileSync('dist/app-release/catalog-v3.sig', 'utf8');
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
  // playwright-core has no browser registry: point it at the cached
  // ms-playwright chromium build explicitly.
  // The loopback app server must be reachable from the page: bypass the
  // system proxy for loopback addresses only (route interception still
  // covers every https URL the harness serves).
  const launchOptions = { headless: true, args: ['--proxy-bypass-list=<-loopback>'] };
  if (process.env.NATIVES_BROWSER_EXECUTABLE) launchOptions.executablePath = process.env.NATIVES_BROWSER_EXECUTABLE;
  else if (!process.env.NATIVES_BROWSER_CHANNEL) launchOptions.channel = undefined;
  if (process.env.NATIVES_BROWSER_CHANNEL) launchOptions.channel = process.env.NATIVES_BROWSER_CHANNEL;
  browser = await chromium.launch(launchOptions);
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
    // Serve the signed v3 catalog/artifacts FIRST (the release source is on
    // github.com, so these must be matched before the network-blackhole rule
    // below) — exercising the fixed mirror path without public network.
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
    // Blackhole everything else (the release host is github.com; catalog,
    // signature and .nap requests were already served above).
    if (url.startsWith('https://github.com/')) return route.abort('internetdisconnected');
    return route.abort();
  });
  const page = await context.newPage();
  page.on('console', (m) => { if (m.text().includes('[app-diag]')) console.error('DIAG:', m.text()); });
  await page.goto(base + '/apps.html');
  const card = page.locator(`[data-app-id="${entry.app_id}"]`);
  const button = (name) => card.getByRole('button', { name, exact: true });
  const waitButton = async (name) => { await button(name).waitFor({ state: 'visible', timeout: 15_000 }); };
  await waitButton('安装');
  packageMode = 'blocked';
  // artifactSources serves the direct release URL (no third-party mirror).
  const downloading = page.waitForRequest((request) => request.url().endsWith('.nap'));
  await button('安装').click();
  await downloading;
  // The cancel aborts the install and the card re-renders, detaching the
  // button mid-click; tolerate the detachment and wait for recovery instead.
  await button('取消').click({ force: true }).catch(() => {});
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
  assert.ok(requests.some((url) => url.endsWith('.nap')), 'package download must use the signed release URL');
  await page.screenshot({ path: join(output, 'center-desktop.png'), fullPage: true });
  const opened = context.waitForEvent('page');
  await button('打开').click();
  const app = await opened;
  app.on('console', (m) => { if (m.text().includes('[app-diag]')) console.error('DIAG-APP:', m.text()); });
  // The v1 sample UI renders its own status element inside the sandboxed
  // iframe (opaque origin): #status flips to 就绪 once the session token is
  // accepted and the loopback API answers (AC-12: real app v1 DOM, not the
  // retired demo host page).
  const demoFrame = app.frameLocator('iframe.managed-app-frame');
  await demoFrame.locator('#status').filter({ hasText: '就绪' }).waitFor({ timeout: 15_000 });
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
