// Production extension + real Chrome Native Messaging in disposable roots.
import assert from 'node:assert/strict';
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createHash, generateKeyPairSync } from 'node:crypto';
import { join, resolve } from 'node:path';
import { tmpdir } from 'node:os';
import { buildExtension, ROOT } from '../extension-package.mjs';
import { verifyCatalogSignature, CATALOG_URL, CATALOG_SIG_URL } from '../../extension/catalog-client.js';

if (process.platform !== 'darwin') throw new Error('unsupported platform: this gate currently has macOS evidence only');
const { chromium } = await import(process.env.NATIVES_PLAYWRIGHT_MODULE || 'playwright-core');
const chrome = process.env.NATIVES_CHROME_EXECUTABLE;
if (!chrome) throw new Error('NATIVES_CHROME_EXECUTABLE must identify a real Chrome/Chromium executable');
const headless = process.env.NATIVES_CHROME_HEADLESS !== '0';
const catalogBytes = readFileSync('dist/app-release/catalog-v3.json');
const signature = readFileSync('dist/app-release/catalog-v3.sig', 'utf8');
await verifyCatalogSignature({ catalogBytes, signatureB64: signature });
const entry = JSON.parse(catalogBytes).apps[0];
assert.equal(entry.kind, 'managed_local');
assert.equal(entry.published, true);

const root = mkdtempSync(join(tmpdir(), 'natives-chrome-native-'));
const profile = join(root, 'profile');
const manifests = join(profile, 'NativeMessagingHosts');
const extension = join(root, 'extension');
const evidence = resolve('dist/app-browser-evidence');
const coreHost = 'com.natives.app_center_test';
// AC-12: the runtimeHost must come from the SAME contract mapping as
// Core's app_activation::runtime_host_name — prefix `com.natives.app.a`
// followed by the full lowercase SHA-256 of the appId.
const runtimeHost = 'com.natives.app.a' + createHash('sha256').update(entry.app_id).digest('hex');
const coreManifest = join(manifests, coreHost + '.json');
const runtimeManifest = join(manifests, runtimeHost + '.json');
mkdirSync(evidence, { recursive: true });
let context;
try {
  buildExtension(extension);
  const { publicKey } = generateKeyPairSync('rsa', {
    modulusLength: 2048, publicKeyEncoding: { type: 'spki', format: 'der' },
  });
  const extensionId = createHash('sha256').update(publicKey).digest().subarray(0, 16).toString('hex')
    .replace(/[0-9a-f]/g, (n) => String.fromCharCode(97 + Number.parseInt(n, 16)));
  const origin = 'chrome-extension://' + extensionId + '/';
  const manifest = JSON.parse(readFileSync(join(extension, 'manifest.json')));
  manifest.key = publicKey.toString('base64');
  writeFileSync(join(extension, 'manifest.json'), JSON.stringify(manifest));
  for (const file of ['apps.js', 'app.js']) {
    const path = join(extension, file);
    writeFileSync(path, readFileSync(path, 'utf8').replaceAll('com.natives.file_manager', coreHost));
  }
  const binary = join(root, 'native-file-host');
  copyFileSync(join(ROOT, 'target/debug/native-file-host'), binary);
  const quote = (value) => "'" + value.replaceAll("'", "'\\''") + "'";
  const launcher = join(root, 'core-host');
  writeFileSync(launcher, '#!/bin/sh\nexec ' + quote(binary) + ' --app-fixture ' + quote(root)
    + ' "$1" --chrome-profile 2>>' + quote(join(root, 'core.stderr')) + '\n', { mode: 0o700 });
  mkdirSync(manifests, { recursive: true });
  writeFileSync(coreManifest, JSON.stringify({
    name: coreHost, description: 'Natives isolated managed-app gate',
    path: launcher, type: 'stdio', allowed_origins: [origin],
  }), { mode: 0o600 });

  context = await chromium.launchPersistentContext(profile, {
    executablePath: chrome, headless, viewport: { width: 1280, height: 900 },
    args: ['--disable-extensions-except=' + extension, '--load-extension=' + extension],
  });
  await context.route('https://**/*', (route) => {
    const url = route.request().url();
    if (url === CATALOG_URL) return route.fulfill({ body: catalogBytes, contentType: 'application/json' });
    if (url === CATALOG_SIG_URL) return route.fulfill({ body: signature, contentType: 'text/plain' });
    const pkg = entry.packages.find((item) => item.url === url);
    if (pkg) return route.fulfill({
      body: readFileSync(join('dist/app-release', new URL(url).pathname.split('/').at(-1))),
      contentType: 'application/octet-stream',
    });
    return route.abort();
  });
  const page = await context.newPage();
  page.on('console', (message) => {
    if (message.type() === 'error' || message.type() === 'warning') {
      console.error(`page ${message.type()}:`, message.text());
    }
  });
  await page.goto(origin + 'apps.html');
  const labels = await page.evaluate(() => Object.fromEntries(
    ['appsInstall', 'appsOpen', 'appsStop', 'appsUninstall', 'appsClearData']
      .map((key) => [key, chrome.i18n.getMessage(key)])));
  const card = page.locator('[data-app-id="' + entry.app_id + '"]');
  const button = (key) => card.getByRole('button', { name: labels[key], exact: true });
  await button('appsInstall').click({ timeout: 15_000 });
  await button('appsOpen').waitFor({ timeout: 20_000 });
  assert.ok(existsSync(runtimeManifest), 'Core must register the per-app runtime Host in the active profile');
  const registration = JSON.parse(readFileSync(runtimeManifest));
  assert.equal(registration.name, runtimeHost);
  assert.deepEqual(registration.allowed_origins, [origin]);
  assert.ok(registration.path.startsWith(join(root, 'apps', entry.app_id, 'runtime')));

  const opened = context.waitForEvent('page');
  await button('appsOpen').click();
  const app = await opened;
  await app.waitForSelector('iframe.managed-app-frame', { timeout: 15_000 });
  const frame = app.frameLocator('iframe.managed-app-frame');
  await frame.getByText('Sample App v' + entry.version).waitFor({ timeout: 15_000 });
  await frame.locator('#input').fill('real browser data');
  await frame.getByRole('button', { name: '保存', exact: true }).click();
  await frame.getByText('值: real browser data').waitFor();
  await app.screenshot({ path: join(evidence, 'chrome-native-managed-app.png'), fullPage: true });
  await page.screenshot({ path: join(evidence, 'chrome-native-app-center.png'), fullPage: true });

  await button('appsStop').click();
  await app.getByText(/应用已停止|App stopped/).waitFor({ timeout: 10_000 });
  const data = join(root, 'apps', entry.app_id, 'data', 'value.txt');
  assert.equal(readFileSync(data, 'utf8'), 'real browser data');

  await button('appsUninstall').click();
  let dialog = page.getByRole('alertdialog');
  await dialog.getByRole('button', { name: labels.appsUninstall, exact: true }).click();
  await button('appsClearData').waitFor({ timeout: 10_000 });
  assert.ok(!existsSync(runtimeManifest), 'uninstall must remove only this app registration');
  assert.ok(existsSync(data), 'default uninstall must retain user data');

  await button('appsClearData').click();
  dialog = page.getByRole('alertdialog');
  await dialog.getByRole('button', { name: labels.appsClearData, exact: true }).click();
  await dialog.getByRole('button', { name: labels.appsClearData, exact: true }).click();
  await button('appsClearData').waitFor({ state: 'detached' });
  assert.ok(!existsSync(join(root, 'apps', entry.app_id, 'data')));
  console.log(JSON.stringify({
    realChrome: true, browserVersion: context.browser()?.version(), platform: process.platform,
    arch: process.arch, headless, catalog: 3, install: 'passed', directAppHost: 'passed',
    sandboxUi: 'passed', stop: 'passed', registrationCleanup: 'passed',
    retainedDataAndPurge: 'passed', output: evidence,
  }));
} catch (error) {
  if (existsSync(join(root, 'core.stderr'))) console.error(readFileSync(join(root, 'core.stderr'), 'utf8'));
  for (const page of context?.pages() || []) {
    // Surface the page's own view of the failure (toast, inline errors, card
    // state) so a missing button reports its reason instead of a bare timeout.
    const pageState = await page.evaluate(() => ({
      url: location.href,
      toast: document.getElementById('apps-toast')?.textContent || null,
      listText: document.getElementById('apps-list')?.innerText?.slice(0, 2000) || null,
    })).catch(() => null);
    if (pageState) console.error('page state:', JSON.stringify(pageState));
    await page.screenshot({ path: join(evidence, 'chrome-native-failure.png'), fullPage: true }).catch(() => {});
  }
  throw error;
} finally {
  await context?.close().catch(() => {});
  rmSync(root, { recursive: true, force: true });
}
