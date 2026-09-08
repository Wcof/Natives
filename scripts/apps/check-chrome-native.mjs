// Real Chrome Native Messaging with registration and App data inside a disposable Chrome profile.
import assert from 'node:assert/strict';
import { readFileSync, writeFileSync, mkdtempSync, mkdirSync, existsSync, rmSync, copyFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { join, resolve } from 'node:path';
import { tmpdir } from 'node:os';
import { createHash, generateKeyPairSync } from 'node:crypto';
import { buildExtension, ROOT } from '../extension-package.mjs';
import { verifyCatalogSignature, CATALOG_URL, CATALOG_SIG_URL } from '../../extension/catalog-client.js';

if (process.platform !== 'darwin') throw new Error('This registration harness currently targets macOS Chrome');
const { chromium } = await import(process.env.NATIVES_PLAYWRIGHT_MODULE || 'playwright');
if (!process.env.NATIVES_CHROME_EXECUTABLE) throw new Error('NATIVES_CHROME_EXECUTABLE must identify Chrome for Testing');
const demoHost = 'com.natives.app.demo';
const testHost = 'com.natives.app_center_test';
const catalogBytes = readFileSync('dist/app-release/catalog-v1.json');
const signature = readFileSync('dist/app-release/catalog-v1.sig', 'utf8');
await verifyCatalogSignature({ catalogBytes, signatureB64: signature });
const entry = JSON.parse(catalogBytes).apps[0];
assert.equal(entry.app_id, demoHost);
const root = mkdtempSync(join(tmpdir(), 'natives-chrome-native-'));
const directory = join(root, 'profile/NativeMessagingHosts');
const demoManifest = join(directory, demoHost + '.json'), coreManifest = join(directory, testHost + '.json');
const extension = join(root, 'extension'), evidence = resolve('dist/app-browser-evidence');
mkdirSync(evidence, { recursive: true });
let context;
try {
  buildExtension(extension);
  const { publicKey } = generateKeyPairSync('rsa', { modulusLength: 2048, publicKeyEncoding: { type: 'spki', format: 'der' } });
  const id = createHash('sha256').update(publicKey).digest().subarray(0, 16).toString('hex')
    .replace(/[0-9a-f]/g, (n) => String.fromCharCode(97 + Number.parseInt(n, 16)));
  const origin = `chrome-extension://${id}/`;
  const manifest = JSON.parse(readFileSync(join(extension, 'manifest.json')));
  manifest.key = publicKey.toString('base64');
  writeFileSync(join(extension, 'manifest.json'), JSON.stringify(manifest));
  for (const file of ['apps.js', 'app.js']) {
    const path = join(extension, file);
    writeFileSync(path, readFileSync(path, 'utf8').replaceAll('com.natives.file_manager', testHost));
  }
  const quote = (value) => "'" + value.replaceAll("'", "'\\''") + "'";
  const launcher = join(root, 'fixture-host');
  // Chrome must not need macOS Downloads-folder access just to launch the fixture binary.
  const binary = join(root, 'native-file-host');
  copyFileSync(join(ROOT, 'target/debug/native-file-host'), binary);
  writeFileSync(launcher, `#!/bin/sh\nexec ${quote(binary)} --app-fixture ${quote(root)} "$1" --chrome-profile 2>>${quote(join(root, 'host.stderr'))}\n`, { mode: 0o700 });
  mkdirSync(directory, { recursive: true });
  writeFileSync(coreManifest, JSON.stringify({ name: testHost, description: 'Natives isolated App verification',
    path: launcher, type: 'stdio', allowed_origins: [origin] }), { flag: 'wx', mode: 0o600 });
  context = await chromium.launchPersistentContext(join(root, 'profile'), {
    executablePath: process.env.NATIVES_CHROME_EXECUTABLE, headless: true, viewport: { width: 1280, height: 900 },
    args: [`--disable-extensions-except=${extension}`, `--load-extension=${extension}`],
  });
  await context.route('https://**/*', (route) => {
    const url = route.request().url();
    if (url === CATALOG_URL) return route.fulfill({ body: catalogBytes, contentType: 'application/json' });
    if (url === CATALOG_SIG_URL) return route.fulfill({ body: signature });
    if (entry.packages.some((pkg) => pkg.url === url)) {
      return route.fulfill({ body: readFileSync(join('dist/app-release', new URL(url).pathname.split('/').at(-1))) });
    }
    return route.abort();
  });
  const page = await context.newPage();
  await page.goto(origin + 'apps.html');
  const nativeProbe = await page.evaluate((host) => new Promise((done) => {
    const port = chrome.runtime.connectNative(host);
    const timer = setTimeout(() => { port.disconnect(); done({ error: 'Native handshake timed out' }); }, 5000);
    port.onMessage.addListener((response) => { clearTimeout(timer); port.disconnect(); done(response); });
    port.onDisconnect.addListener(() => { clearTimeout(timer); done({ error: chrome.runtime.lastError?.message }); });
    port.postMessage({ id: 'probe', method: 'apps:handshake', params: { origin: chrome.runtime.getURL('') } });
  }), testHost);
  assert.equal(nativeProbe.ok, true, JSON.stringify(nativeProbe));
  const labels = await page.evaluate(() => Object.fromEntries(['appsInstall', 'appsOpen', 'appsUninstall', 'appsClearData'].map((key) => [key, chrome.i18n.getMessage(key)])));
  const card = page.locator(`[data-app-id="${entry.app_id}"]`);
  const button = (key) => card.getByRole('button', { name: labels[key], exact: true });
  await button('appsInstall').click({ timeout: 15_000 });
  await button('appsOpen').waitFor({ timeout: 15_000 });
  const installed = JSON.parse(readFileSync(demoManifest));
  assert.deepEqual(installed.allowed_origins, [origin], 'Core must register the origin provided by Chrome');
  assert.ok(installed.path.startsWith(root));
  const opened = context.waitForEvent('page');
  await button('appsOpen').click();
  const app = await opened;
  await app.locator('#demo-host-status.ok').waitFor({ timeout: 10_000 });
  await app.screenshot({ path: join(evidence, 'chrome-native-demo.png'), fullPage: true });
  const runtimePids = () => execFileSync('ps', ['-axo', 'pid=,comm='], { encoding: 'utf8' }).split('\n')
    .filter((line) => line.trim().replace(/^\d+\s+/, '') === installed.path);
  assert.equal(runtimePids().length, 1);
  const personal = join(root, 'apps', entry.app_id, 'data');
  mkdirSync(personal, { recursive: true }); writeFileSync(join(personal, 'notes'), 'preserved');
  // Uninstall while the App tab is open verifies page coordination plus the OS runtime lock.
  await button('appsUninstall').click();
  const dialog = page.getByRole('alertdialog');
  assert.equal(await dialog.getByRole('checkbox').isChecked(), false);
  const started = performance.now();
  await dialog.getByRole('button', { name: labels.appsUninstall, exact: true }).click();
  await button('appsClearData').waitFor({ timeout: 10_000 });
  assert.equal(runtimePids().length, 0);
  assert.ok(!existsSync(demoManifest));
  assert.ok(existsSync(personal));
  await button('appsClearData').click();
  await dialog.getByRole('button', { name: labels.appsClearData, exact: true }).click();
  await dialog.getByRole('button', { name: labels.appsClearData, exact: true }).click();
  await button('appsClearData').waitFor({ state: 'detached' });
  assert.ok(!existsSync(personal));
  console.log(JSON.stringify({ chromeNativeMessaging: true, version: entry.version, install: 'passed', launchOrigin: 'passed',
    appRuntime: 'passed', liveAppUninstall: 'passed', elapsedMs: Math.round(performance.now() - started), retainedDataPurge: 'passed' }));
} catch (error) {
  if (existsSync(join(root, 'host.stderr'))) console.error(readFileSync(join(root, 'host.stderr'), 'utf8'));
  for (const page of context?.pages() || []) {
    if (page.url().endsWith('/apps.html')) {
      await page.screenshot({ path: join(evidence, 'chrome-native-failure.png') });
      console.error(await page.locator('body').innerText());
    }
  }
  throw error;
} finally {
  await context?.close();
  for (const file of [coreManifest, demoManifest]) {
    if (!existsSync(file)) continue;
    const registration = JSON.parse(readFileSync(file));
    if (registration.path.startsWith(root + '/')) rmSync(file);
  }
  rmSync(root, { recursive: true, force: true });
}
