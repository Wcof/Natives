// ADR-0042 / 09 §5: count requests, Ports, SSE, timers, listeners and
// inflight work for 1/5/20 same-domain Space cards and both entries.
// SQL counts are not exposed by the production Core; the CSV records that gap
// instead of substituting an RPC count for a SQL count.
import assert from 'node:assert/strict';
import { spawn, execFileSync } from 'node:child_process';
import { chmodSync, copyFileSync, cpSync, existsSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { randomBytes } from 'node:crypto';
import { resolve } from 'node:path';
import { tmpdir } from 'node:os';
import net from 'node:net';
import { chromium } from 'playwright-core';

const root = resolve(process.env.NATIVES_ACCEPTANCE_ROOT || '.tmp/dual-entry-counts');
const report = resolve(process.argv[2] || '.tmp/dual-entry-counts/report');
mkdirSync(report, { recursive: true });
const state = mkdtempSync(tmpdir() + '/natives-counts-');
const home = `${state}/home`, profile = `${state}/extension-profile`, source = `${state}/product-source`;
if (!existsSync(`${root}/product-source`)) throw new Error(`missing ${root}/product-source; set NATIVES_ACCEPTANCE_ROOT to a prepared isolated product-source directory`);
cpSync(`${root}/product-source`, source, { recursive: true });
copyFileSync(resolve('target/debug/native-file-host'), `${state}/native-file-host`); chmodSync(`${state}/native-file-host`, 0o755);
for (const dir of [home, profile, `${home}/Desktop`, `${home}/Documents`, `${home}/Downloads`]) mkdirSync(dir, { recursive: true });
const port = await new Promise(done => { const s = net.createServer(); s.listen(0, '127.0.0.1', () => { const p = s.address().port; s.close(() => done(p)); }); });
const base = `http://127.0.0.1:${port}`, token = randomBytes(32).toString('base64url');
const env = { ...process.env, HOME: home, NATIVES_WORKSPACE_DB: `${state}/workspace.db`, NATIVES_APPS_ROOT: `${state}/apps`, NATIVES_STATE_DIR: `${state}/state`, NATIVES_DEV_PRODUCT_SOURCE: source, NATIVES_WEB_PORT: String(port) };
const quote = v => "'" + v.replaceAll("'", "'\"'\"'") + "'";
const wrapper = `${state}/isolated-native-core.sh`;
const overrides = Object.entries(env).filter(([k]) => k === 'HOME' || k.startsWith('NATIVES_')).map(([k, v]) => quote(`${k}=${v}`)).join(' ');
writeFileSync(wrapper, `#!/bin/sh\nexec /usr/bin/env ${overrides} ${quote(state + '/native-file-host')} "$@" 2>>${quote(state + '/native.log')}\n`); chmodSync(wrapper, 0o755);
for (const dir of [`${home}/Library/Application Support/Google/Chrome/NativeMessagingHosts`, `${home}/Library/Application Support/Google/Chrome for Testing/NativeMessagingHosts`, `${profile}/NativeMessagingHosts`]) {
  mkdirSync(dir, { recursive: true });
  writeFileSync(`${dir}/com.natives.local.file_manager.json`, JSON.stringify({ name: 'com.natives.local.file_manager', description: 'Isolated count probe', path: wrapper, type: 'stdio', allowed_origins: ['chrome-extension://fcemcngngoblajnpmoegpjfpdokiofie/'] }));
}
const service = spawn(`${state}/native-file-host`, ['--service', '--port', String(port), '--bootstrap', token], { env, cwd: state, stdio: ['ignore', 'ignore', 'pipe'] });
let stderr = ''; service.stderr.on('data', x => { stderr += x; });

const instrument = () => {
  const counts = { ports: [], timers: new Set(), listeners: 0, fetches: 0, inflight: 0, requests: [], sse: [] };
  // Keep connection/listener/timer registries: they describe current owners.
  const reset = () => { counts.fetches = 0; counts.inflight = 0; counts.requests = []; };
  Object.defineProperty(globalThis, '__counts', { value: counts, configurable: true });
  const timer = (orig, repeating) => function (fn, ms, ...args) {
    let id;
    const wrapped = (...a) => { if (!repeating) counts.timers.delete(id); return typeof fn === 'function' ? fn(...a) : undefined; };
    id = orig(wrapped, ms, ...args);
    counts.timers.add(id);
    const clear = repeating ? clearInterval : clearTimeout;
    const originalClear = clear;
    // The native clear functions are replaced below so every path updates the set.
    return id;
  };
  const st = globalThis.setTimeout, si = globalThis.setInterval;
  globalThis.setTimeout = timer(st, false); globalThis.setInterval = timer(si, true);
  const ct = globalThis.clearTimeout, ci = globalThis.clearInterval;
  globalThis.clearTimeout = id => { counts.timers.delete(id); return ct(id); };
  globalThis.clearInterval = id => { counts.timers.delete(id); return ci(id); };
  const add = EventTarget.prototype.addEventListener, remove = EventTarget.prototype.removeEventListener;
  const seen = new WeakMap();
  EventTarget.prototype.addEventListener = function (type, fn, opts) {
    if (fn) { const key = `${type}`; const set = seen.get(this) || new Set(); if (!set.has(fn)) { set.add(fn); seen.set(this, set); counts.listeners += 1; } }
    return add.call(this, type, fn, opts);
  };
  EventTarget.prototype.removeEventListener = function (type, fn, opts) {
    if (fn) { const set = seen.get(this); if (set && set.delete(fn)) counts.listeners = Math.max(0, counts.listeners - 1); }
    return remove.call(this, type, fn, opts);
  };
  const of = globalThis.fetch;
  globalThis.fetch = async function (...args) {
    const url = String(args[0]?.url || args[0]); counts.fetches += 1; counts.inflight += 1;
    counts.requests.push(url);
    try { return await of.apply(this, args); } finally { counts.inflight -= 1; }
  };
  const ES = globalThis.EventSource;
  if (ES) globalThis.EventSource = class extends ES {
    constructor(...args) { super(...args); const source = this; counts.sse.push({ url: args[0], source }); }
  };
  if (globalThis.chrome?.runtime?.connectNative) {
    const connect = globalThis.chrome.runtime.connectNative.bind(globalThis.chrome.runtime);
    globalThis.chrome.runtime.connectNative = function (...args) {
      const p = connect(...args); const entry = { active: true, methods: {}, messages: 0 };
      counts.ports.push(entry);
      const pm = p.postMessage.bind(p);
      p.postMessage = message => { entry.messages += 1; const m = message?.method || '?'; entry.methods[m] = (entry.methods[m] || 0) + 1; return pm(message); };
      p.onDisconnect.addListener(() => { entry.active = false; });
      return p;
    };
  }
  globalThis.__countReset = reset;
};

async function collect(page) {
  return page.evaluate(() => {
    const c = globalThis.__counts || {};
    return {
      visibility: document.visibilityState,
      requests: c.requests?.length || 0,
      fetch: c.fetches || 0,
      inflight: Math.max(0, c.inflight || 0),
      timers: c.timers?.size ?? null,
      listeners: c.listeners ?? null,
      sse: (c.sse || []).filter(x => x.source?.readyState !== 2).length,
      ports: (c.ports || []).filter(p => p.active).length,
      portMessages: (c.ports || []).reduce((n, p) => n + p.messages, 0),
      portMethods: (c.ports || []).reduce((o, p) => { for (const [k, v] of Object.entries(p.methods)) o[k] = (o[k] || 0) + v; return o; }, {}),
    };
  });
}

let extension, web;
const results = [];
try {
  for (let i = 0; i < 100; i++) { try { if ((await fetch(base + '/healthz')).ok) break; } catch { } await new Promise(r => setTimeout(r, 50)); }
  const executablePath = process.env.CHROME_EXECUTABLE;
  process.chdir(state);
  extension = await chromium.launchPersistentContext(profile, { executablePath, headless: true, env, viewport: { width: 1440, height: 1000 }, ignoreDefaultArgs: ['--disable-extensions'], args: [`--disable-extensions-except=${source}/ChromeExtension`, `--load-extension=${source}/ChromeExtension`] });
  await extension.addInitScript(instrument);
  const e = await extension.newPage();
  const eurl = p => `chrome-extension://fcemcngngoblajnpmoegpjfpdokiofie/${p}`;
  await e.goto(eurl('manifest.json'));
  await e.waitForTimeout(1500);
  const probe = await e.evaluate(() => new Promise(done => { const p = chrome.runtime.connectNative('com.natives.local.file_manager'); p.onDisconnect.addListener(() => done({ error: chrome.runtime.lastError?.message })); p.onMessage.addListener(v => { if (typeof v.ok !== 'boolean') return; done({ ok: v.ok, response: v, error: chrome.runtime.lastError?.message }); p.disconnect(); }); p.onDisconnect.addListener(()=>done({disconnected:true,error:chrome.runtime.lastError?.message})); p.postMessage({ id: 'probe', method: 'version', params: {} }); setTimeout(() => { done({ timeout: true, error: chrome.runtime.lastError?.message }); p.disconnect(); }, 5000); }));
  assert.ok(probe.ok, JSON.stringify(probe));
  // Initialize the real Fund worker first; card queries require readiness=ready
  // and must not start a module on their own.
  await e.goto(eurl('app.html?app=fund'));
  await e.locator('iframe').waitFor({ timeout: 20000 });
  await e.frameLocator('iframe').locator('#view-home').waitFor({ state: 'visible', timeout: 20000 });
  web = await chromium.launchPersistentContext(`${state}/web-profile`, { executablePath, headless: true, env, viewport: { width: 1440, height: 1000 } });
  await web.addInitScript(instrument);
  const w = await web.newPage();
  await w.goto(`${base}/space.html#token=${token}`);
  await w.waitForFunction(async () => (await (await fetch('/web/session')).json()).authenticated);
  const rpc = (page, method, params = {}) => page.evaluate(async ({ method, params }) => { const { createPlatformClient } = await import('./platform/transport.js'); const c = createPlatformClient({ host: 'com.natives.local.file_manager' }); try { return await c.call(method, params); } finally { c.disconnect(); } }, { method, params });

  async function seed(name, count) {
    const created = await rpc(e, 'workspace_create', { name, template: 'blank' });
    const id = created.workspace.id;
    await e.evaluate(async ({ id, count }) => {
      const { createNativeClient } = await import('./native-client.js');
      const { widgetPlugins } = await import('./plugins/widgets/index.js');
      const { INVESTMENT_WIDGET_KEYS } = await import('./space-investment-feed.js');
      const client = createNativeClient({ host: 'com.natives.local.file_manager' });
      try {
        let snapshot = await client.call('workspace_snapshot', { workspaceId: id });
        for (let i = 0; i < count; i++) {
          const key = INVESTMENT_WIDGET_KEYS[i % INVESTMENT_WIDGET_KEYS.length];
          const definition = widgetPlugins[key];
          snapshot = await client.call('workspace_widget_upsert', { workspaceId: id, expectedRevision: snapshot.revision, widget: { id: '', key, enabled: true, order: snapshot.widgets.length, configJson: definition.defaultData || {}, displayJson: { position: 'middleCentre' } } });
        }
      } finally { client.disconnect(); }
    }, { id, count });
    return id;
  }

  async function measure(label, page, expected) {
    await page.bringToFront();
    await page.waitForTimeout(2500);
    const loaded = await collect(page);
    await page.evaluate(() => globalThis.__countReset?.());
    const started = Date.now();
    await page.waitForTimeout(6000);
    const idle = await collect(page);
    const row = { scenario: label, entry: page === e ? 'E' : 'W', windowMs: Date.now() - started, loaded, idle, expected };
    results.push(row); console.log(JSON.stringify(row));
  }

  for (const count of [1, 5, 20]) {
    const id = await seed(`counts-${count}`, count);
    await e.goto(`${eurl('space.html')}?workspace=${id}`);
    await e.locator('.Widget').nth(count - 1).waitFor({ timeout: 30000 });
    await measure(`cards-${count}-E`, e, 'shared card snapshot/port/SSE; no linear growth');
  }
  const id20 = await seed('counts-dual', 20);
  await e.goto(`${eurl('space.html')}?workspace=${id20}`);
  await w.goto(`${base}/space.html?workspace=${id20}`);
  for (const page of [e, w]) await page.locator('.Widget').nth(19).waitFor({ timeout: 30000 });
  const loadedE = await collect(e), loadedW = await collect(w);
  await e.evaluate(() => globalThis.__countReset?.()); await w.evaluate(() => globalThis.__countReset?.());
  const started = Date.now();
  await e.waitForTimeout(6000);
  const ps = execFileSync('rtk', ['proxy', 'ps', '-axo', 'command='], { encoding: 'utf8' }).split('\n');
  const dual = { scenario: 'cards-20-dual', windowMs: Date.now() - started, loaded: { extension: loadedE, web: loadedW }, idle: { extension: await collect(e), web: await collect(w) }, processes: { nativeFileHost: ps.filter(x => x.includes(state) && x.includes('native-file-host')).length, appRuntime: ps.filter(x => x.includes(state) && x.includes('natives-app-runtime')).length }, expected: 'one worker per appId; one SSE per document; ports per entry' };
  results.push(dual); console.log(JSON.stringify(dual));
  writeFileSync(`${report}/counts.json`, JSON.stringify({ status: 'measured', sql: 'NOT_EXPOSED_BY_PRODUCTION_CORE', databaseConnections: 'one Core SQLite writer + one per active module worker (not separately exposed)', results, stderr }, null, 2));
} catch (error) {
  writeFileSync(`${report}/counts-failure.json`, JSON.stringify({ error: error.message, results, stderr, state }, null, 2));
  throw error;
} finally {
  await extension?.close(); await web?.close(); service.kill('SIGTERM');
}
