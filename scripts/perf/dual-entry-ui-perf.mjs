// ADR-0042 UI perf: cold interactive, local feedback, cached page switch and
// initial transfer bytes for the extension and the extension-free Web entry.
// Real production pages; isolated Core/profile. Long-task/FPS remain explicit
// gaps because this probe does not claim a tracing-based result.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { chmodSync, copyFileSync, cpSync, existsSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { randomBytes } from 'node:crypto';
import { resolve } from 'node:path';
import { tmpdir } from 'node:os';
import net from 'node:net';
import { chromium } from 'playwright-core';

const root = resolve(process.env.NATIVES_ACCEPTANCE_ROOT || '.tmp/dual-entry-counts');
const report = resolve(process.argv[2] || '.tmp/dual-entry-counts/ui-perf');
mkdirSync(report, { recursive: true });
const state = mkdtempSync(tmpdir() + '/natives-ui-perf-');
const home = `${state}/home`, profile = `${state}/extension-profile`, source = `${state}/product-source`;
assert.ok(existsSync(`${root}/product-source`), `missing ${root}/product-source`);
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
  writeFileSync(`${dir}/com.natives.local.file_manager.json`, JSON.stringify({ name: 'com.natives.local.file_manager', description: 'Isolated UI perf', path: wrapper, type: 'stdio', allowed_origins: ['chrome-extension://fcemcngngoblajnpmoegpjfpdokiofie/'] }));
}
const service = spawn(`${state}/native-file-host`, ['--service', '--port', String(port), '--bootstrap', token], { env, cwd: state, stdio: ['ignore', 'ignore', 'pipe'] });
let stderr = ''; service.stderr.on('data', x => { stderr += x; });
let extension, web;
const pct = (values, p) => { const s = [...values].sort((a, b) => a - b); return s[Math.min(s.length - 1, Math.ceil(s.length * p) - 1)]; };
const result = { scope: 'real extension and clean Web, isolated Core; debug build', cold: {}, interaction: {}, cached: {}, bytes: {}, errors: [], longTasks: null, fps: null };
try {
  for (let i = 0; i < 100; i++) { try { if ((await fetch(base + '/healthz')).ok) break; } catch { } await new Promise(r => setTimeout(r, 50)); }
  process.chdir(state);
  extension = await chromium.launchPersistentContext(profile, { executablePath: process.env.CHROME_EXECUTABLE, headless: true, env, viewport: { width: 1440, height: 1000 }, ignoreDefaultArgs: ['--disable-extensions'], args: [`--disable-extensions-except=${source}/ChromeExtension`, `--load-extension=${source}/ChromeExtension`] });
  await extension.addInitScript(() => { globalThis.__long = []; try { new PerformanceObserver(list => { for (const e of list.getEntries()) globalThis.__long.push({ start: e.startTime, duration: e.duration }); }).observe({ entryTypes: ['longtask'] }); } catch { } });
  web = await chromium.launchPersistentContext(`${state}/web-profile`, { executablePath: process.env.CHROME_EXECUTABLE, headless: true, env, viewport: { width: 1440, height: 1000 } });
  await web.addInitScript(() => { globalThis.__long = []; try { new PerformanceObserver(list => { for (const e of list.getEntries()) globalThis.__long.push({ start: e.startTime, duration: e.duration }); }).observe({ entryTypes: ['longtask'] }); } catch { } });
  const e = await extension.newPage(); e.on('pageerror', x => result.errors.push({ entry: 'E', error: x.message }));
  const w = await web.newPage(); w.on('pageerror', x => result.errors.push({ entry: 'W', error: x.message }));
  const eurl = p => `chrome-extension://fcemcngngoblajnpmoegpjfpdokiofie/${p}`;
  await w.goto(`${base}/space.html#token=${token}`); await w.waitForFunction(async () => (await (await fetch('/web/session')).json()).authenticated);
  async function cold(entry, page, n) {
    const values = [];
    for (let i = 0; i < n; i++) {
      const started = performance.now();
      await page.goto(entry === 'E' ? eurl('space.html') : `${base}/space.html`, { waitUntil: 'load' });
      await page.locator('#settings-entry').waitFor({ state: 'visible', timeout: 15000 });
      values.push(performance.now() - started);
    }
    return { samples: values, p75: pct(values, 0.75) };
  }
  result.cold.E = await cold('E', e, 5);
  result.cold.W = await cold('W', w, 5);
  async function interaction(page) {
    const values = [];
    for (let i = 0; i < 12; i++) {
      const started = performance.now();
      await page.locator('#settings-entry').click();
      await page.locator('#settings-menu').waitFor({ state: 'visible' });
      values.push(performance.now() - started);
      await page.keyboard.press('Escape');
      await page.waitForTimeout(80);
    }
    return { samples: values, p95: pct(values, 0.95) };
  }
  result.interaction.E = await interaction(e);
  result.interaction.W = await interaction(w);
  async function cached(entry, page) {
    const filesUrl = entry === 'E' ? eurl('files.html') : `${base}/files.html`;
    const spaceUrl = entry === 'E' ? eurl('space.html') : `${base}/space.html`;
    const values = [];
    for (let i = 0; i < 5; i++) {
      await page.goto(filesUrl, { waitUntil: 'load' });
      await page.locator('#entries').waitFor({ state: 'visible', timeout: 15000 });
      const started = performance.now();
      await page.goto(spaceUrl, { waitUntil: 'load' });
      await page.locator('#dashboard-host').waitFor({ state: 'visible', timeout: 15000 });
      values.push(performance.now() - started);
    }
    return { samples: values, p95: pct(values, 0.95) };
  }
  result.cached.E = await cached('E', e);
  result.cached.W = await cached('W', w);
  const bytes = page => page.evaluate(() => ({
    document: performance.getEntriesByType('navigation')[0]?.transferSize || 0,
    resources: performance.getEntriesByType('resource').map(r => ({ name: r.name, transferSize: r.transferSize || 0 })),
    total: (performance.getEntriesByType('navigation')[0]?.transferSize || 0) + performance.getEntriesByType('resource').reduce((s, r) => s + (r.transferSize || 0), 0),
  }));
  result.bytes.E = await bytes(e); result.bytes.W = await bytes(w);
  result.longTasks = { E: await e.evaluate(() => Math.max(0, ...(globalThis.__long || []).map(x => x.duration))), W: await w.evaluate(() => Math.max(0, ...(globalThis.__long || []).map(x => x.duration))) };
  async function fps(page){return page.evaluate(()=>new Promise(resolve=>{let frames=0;const start=performance.now();function tick(now){frames++;if(now-start<2000)requestAnimationFrame(tick);else resolve({frames,durationMs:now-start,fps:frames/((now-start)/1000)});}requestAnimationFrame(tick);}));}
  result.fps = { E: await fps(e), W: await fps(w) };
  result.budgets = { coldP75: 2500, interactionP95: 100, cachedP95: 300, initialTransferIsNotGzip: true };
  result.pass = {
    cold: result.cold.E.p75 <= 2500 && result.cold.W.p75 <= 2500,
    interaction: result.interaction.E.p95 <= 100 && result.interaction.W.p95 <= 100,
    cached: result.cached.E.p95 <= 300 && result.cached.W.p95 <= 300,
  };
  writeFileSync(`${report}/ui-perf.json`, JSON.stringify(result, null, 2));
  console.log(JSON.stringify({ cold: result.cold, interaction: result.interaction, cached: result.cached, bytes: { E: result.bytes.E.total, W: result.bytes.W.total }, longTasks: result.longTasks, fps: result.fps, pass: result.pass, errors: result.errors.length }, null, 2));
} catch (error) {
  writeFileSync(`${report}/ui-perf-failure.json`, JSON.stringify({ error: error.message, stderr, result }, null, 2));
  throw error;
} finally { await extension?.close(); await web?.close(); service.kill('SIGTERM'); }
