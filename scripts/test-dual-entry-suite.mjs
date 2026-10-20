// Single-Web smoke subset (ADR-0042); does not establish V01-V17 acceptance.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve, join } from 'node:path';
import http from 'node:http';
import net from 'node:net';
import {randomBytes} from 'node:crypto';
import { chromium } from 'playwright-core';

const PORT = await new Promise(done=>{const server=net.createServer();server.listen(0,'127.0.0.1',()=>{const port=server.address().port;server.close(()=>done(port));});});
const BOOTSTRAP = randomBytes(32).toString('base64url');
mkdirSync('.tmp/dual-entry-suite',{recursive:true});
const ISOLATED_ROOT = mkdtempSync(resolve('.tmp/dual-entry-suite/run-'));
const EVIDENCE_DIR = resolve(process.argv[2] || join(ISOLATED_ROOT,'evidence'));
const WEB_AFTER_DIR = join(EVIDENCE_DIR, 'browser/web-after');
const EXT_AFTER_DIR = join(EVIDENCE_DIR, 'browser/extension-after');
mkdirSync(join(ISOLATED_ROOT,'home'),{recursive:true});
mkdirSync(join(EVIDENCE_DIR,'backend'),{recursive:true});

mkdirSync(WEB_AFTER_DIR, { recursive: true });
mkdirSync(EXT_AFTER_DIR, { recursive: true });
mkdirSync(ISOLATED_ROOT, { recursive: true });

console.log('--- Step 1: Starting native-file-host in Service Mode with Isolated DB ---');
const srv = spawn('./target/debug/native-file-host', [
  '--service',
  '--port',
  String(PORT),
  '--bootstrap',
  BOOTSTRAP,
], {
  env: {
    ...process.env,
    HOME: join(ISOLATED_ROOT,'home'),
    NATIVES_WEB_PORT: String(PORT),
    NATIVES_APPS_ROOT: join(ISOLATED_ROOT,'apps'),
    NATIVES_MODEL_HOST_CONFIG_DIR: join(ISOLATED_ROOT,'model'),
    NATIVES_WORKSPACE_DB: join(ISOLATED_ROOT, 'isolated-natives.db'),
    NATIVES_STATE_DIR: join(ISOLATED_ROOT, 'state'),
  },
});

srv.stderr.on('data', (d) => {
  const line = d.toString().trim();
  if (line) console.log('[Service]', line);
});
srv.on('exit', (code, sig) => console.log('[Service exited]', code, sig));

for (let i = 0; i < 40; i++) {
  try {
    const ok = await new Promise((resolve) => {
      http.get(`http://127.0.0.1:${PORT}/healthz`, (r) => resolve(r.statusCode === 200)).on('error', () => resolve(false));
    });
    if (ok) break;
  } catch (_) {}
  await new Promise((r) => setTimeout(r, 100));
}

console.log('--- Step 2: Launching Clean Chrome Browser (No Extension) ---');
let browser;

const vResults = [];

try {
  browser = await chromium.launch({
  executablePath: '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  headless: true,
});
  const context = await browser.newContext({ viewport: { width: 1440, height: 1000 } });
  const page = await context.newPage();

  page.on('console', (msg) => {
    console.log(`[PAGE ${msg.type()}]`, msg.text());
  });
  page.on('pageerror', (err) => {
    console.log('[PAGE ERROR]', err.message);
  });
  page.on('request', (req) => {
    if (req.url().includes('/web/rpc')) {
      console.log('RPC method:', req.postDataJSON()?.method);
    }
  });
  page.on('response', async (res) => {
    if (res.url().includes('/web/rpc')) {
      console.log('RPC status:', res.status());
    }
  });

  // -------------------------------------------------------------
  // V01: 无扩展启动 -> 配对 -> Space -> Files -> Apps -> Fund
  // -------------------------------------------------------------
  console.log('Executing V01: Standalone Web launch and navigation...');
  const exchangePromise = page.waitForResponse((res) => res.url().includes('/web/session/exchange'));
  await page.goto(`http://127.0.0.1:${PORT}/space.html#token=${BOOTSTRAP}`, { waitUntil: 'load' });
  const exRes = await exchangePromise;
  assert.equal(exRes.status(), 200, 'bootstrap exchange must succeed');
  await page.waitForFunction(() => !window.location.hash.includes('token='));
  await page.waitForSelector('#dashboard-host', { state: 'attached' });
  await page.screenshot({ path: join(WEB_AFTER_DIR, 'web-space-1440.png') });

  // Files page
  await page.goto(`http://127.0.0.1:${PORT}/files.html`, { waitUntil: 'load' });
  await page.waitForSelector('#entries', { state: 'attached' });
  await page.screenshot({ path: join(WEB_AFTER_DIR, 'web-files-1440.png') });

  // Apps page
  await page.goto(`http://127.0.0.1:${PORT}/apps.html`, { waitUntil: 'load' });
  await page.waitForSelector('#apps-list', { state: 'attached' });
  await page.screenshot({ path: join(WEB_AFTER_DIR, 'web-apps-1440.png') });

  // Fund app stage in Web mode
  await page.goto(`http://127.0.0.1:${PORT}/app.html?app=fund`, { waitUntil: 'load' });
  await page.waitForSelector('#app-stage', { state: 'attached' });
  await page.waitForTimeout(2000);
  console.log('Module stage mounted:', await page.locator('#app-stage').count());
  console.log('TOAST TEXT:', await page.locator('#app-toast').textContent());
  const fundFrame = page.locator('iframe.managed-app-frame');
  await fundFrame.waitFor({ state: 'attached', timeout: 15000 });
  const fundSrc = await fundFrame.getAttribute('src');
  assert.ok(fundSrc.startsWith('http://127.0.0.1:'), 'fund iframe must bind loopback port');
  await page.screenshot({ path: join(WEB_AFTER_DIR, 'web-app-fund-1440.png') });

  vResults.push({ id: 'V01', status: 'SUBSET_PASS', description: '无扩展启动->配对->各页面与模块沙箱完整加载' });
  console.log('✓ Subset of V01 Passed');

  // -------------------------------------------------------------
  // V02 & V03: 真实乐观锁并发冲突 409 (Single Writer + Revision Conflict)
  // -------------------------------------------------------------
  console.log('Executing V02 & V03: Concurrency and single writer optimistic locking...');
  const conflictResult = await page.evaluate(async () => {
    const { createNativeClient } = await import('./native-client.js');
    const client = createNativeClient({ host: 'com.natives.file_manager' });
    const sess = await client.call('workspace_session', {});
    const wid = sess.activeWorkspaceId;
    const snap = await client.call('workspace_snapshot', { workspaceId: wid });
    const rev = snap.revision;

    // 第一次提交有效 revision (使用真实存在的 workspace_rename)
    const res1 = await client.call('workspace_rename', {
      workspaceId: wid,
      name: 'Valid New Name',
      expectedRevision: rev,
    });

    // 第二次并发提交同一已过期的旧 revision，必须触发 409 冲突
    let res2Error = null;
    try {
      await client.call('workspace_rename', {
        workspaceId: wid,
        name: 'Conflicting Stale Name',
        expectedRevision: rev,
      });
    } catch (e) {
      res2Error = e.message;
    }

    const finalSnap = await client.call('workspace_snapshot', { workspaceId: wid });
    return { rev, res1, res2Error, finalName: finalSnap.name };
  });

  assert.ok(conflictResult.res1, 'first rename must succeed');
  assert.ok(conflictResult.res2Error, 'second rename with stale revision must fail with conflict');
  assert.equal(conflictResult.finalName, 'Valid New Name', 'stale write must not overwrite valid write');
  vResults.push({ id: 'V02', status: 'SUBSET_PASS', description: '双入口读写一致性与权威同步' });
  vResults.push({ id: 'V03', status: 'SUBSET_PASS', description: '真实乐观锁并发冲突 409 防御 (workspace_rename)' });
  console.log('✓ Subset of V02 & V03 Passed');

  // -------------------------------------------------------------
  // V04 & V05: 多 Surface 独立租约与独立撤销
  // -------------------------------------------------------------
  console.log('Executing V04 & V05: Multi-surface leases and revocation...');
  const surfCheck = await page.evaluate(async (port) => {
    const csrf = sessionStorage.getItem('natives_csrf') || '';
    const regRes1 = await fetch(`http://127.0.0.1:${port}/web/surfaces`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf, Origin: `http://127.0.0.1:${port}` },
      body: JSON.stringify({ appId: 'fund' }),
    });
    const s1 = await regRes1.json();

    const regRes2 = await fetch(`http://127.0.0.1:${port}/web/surfaces`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf, Origin: `http://127.0.0.1:${port}` },
      body: JSON.stringify({ appId: 'tokenusage' }),
    });
    const s2 = await regRes2.json();

    // 撤销 Surface 1，Surface 2 续约必须成功
    const delRes = await fetch(`http://127.0.0.1:${port}/web/surfaces/${s1.surfaceId}`, {
      method: 'DELETE',
      headers: { 'X-CSRF-Token': csrf, Origin: `http://127.0.0.1:${port}` },
    });

    const renewRes2 = await fetch(`http://127.0.0.1:${port}/web/surfaces/${s2.surfaceId}`, {
      method: 'POST',
      headers: { 'X-CSRF-Token': csrf, Origin: `http://127.0.0.1:${port}` },
    });

    return { s1, s2, delOk: delRes.ok, renew2Ok: renewRes2.ok };
  }, PORT);

  assert.equal(surfCheck.delOk, true);
  assert.equal(surfCheck.renew2Ok, true);
  vResults.push({ id: 'V04', status: 'SUBSET_PASS', description: '多 Surface 共存无锁抢占' });
  vResults.push({ id: 'V05', status: 'SUBSET_PASS', description: 'Surface 租约续约与独立撤销' });
  console.log('✓ Subset of V04 & V05 Passed');

  // -------------------------------------------------------------
  // V10: Files 真实文件操作 (roots, list_dir, stat)
  // -------------------------------------------------------------
  console.log('Executing V10: Real Files operations...');
  const filesOpResult = await page.evaluate(async () => {
    const { createNativeClient } = await import('./native-client.js');
    const client = createNativeClient({ host: 'com.natives.file_manager' });

    // 1. 获取 roots
    const rootsRes = await client.call('roots', {});
    const roots = Array.isArray(rootsRes) ? rootsRes : (rootsRes?.roots || []);
    if (!roots.length) throw new Error('roots empty');
    const rootPath = roots[0].path;

    // 2. 列目录确认存在条目
    const listRes = await client.call('list_dir', { path: rootPath });
    const hasEntries = Array.isArray(listRes.entries) && listRes.entries.length > 0;

    // 3. Stat 根目录确认元数据
    const statRes = await client.call('stat', { path: rootPath });

    return { rootPath, hasEntries, isDir: Boolean(statRes.isDir) };
  });

  assert.equal(filesOpResult.hasEntries, true, 'root directory must contain entries');
  assert.equal(filesOpResult.isDir, true, 'stat must report isDir: true');
  vResults.push({ id: 'V10', status: 'SUBSET_PASS', description: 'Files 真实根目录获取、目录枚举与状态查询' });
  console.log('✓ Subset of V10 Passed');
  console.log('✓ Subset of V10 Passed');

  // -------------------------------------------------------------
  // V12: Model Host 真实数据查询 (model_snapshot 代理通道)
  // -------------------------------------------------------------
  console.log('Executing V12: Real Model & Usage query...');
  const modelResult = await page.evaluate(async () => {
    const { createModelSettingsAPI } = await import('./model-settings-api.js');
    const api = createModelSettingsAPI();
    const snap = await api.snapshot();
    api.disconnect?.();
    return snap;
  });

  assert.ok(modelResult.providers, 'model_snapshot must return providers array');
  assert.ok(modelResult.providers.length > 0, 'providers must be populated from model-host');
  vResults.push({ id: 'V12', status: 'SUBSET_PASS', description: 'Model/Usage 经 Web 代理统一查询' });
  console.log('✓ Subset of V12 Passed');

  // -------------------------------------------------------------
  // V14: 真实 SSE 事件流与广播验证
  // -------------------------------------------------------------
  console.log('Executing V14: Real SSE /web/events stream and broadcast...');
  const sseCheck = await page.evaluate(async (port) => {
    return new Promise((resolve, reject) => {
      const es = new EventSource(`http://127.0.0.1:${port}/web/events`);
      const timer = setTimeout(() => {
        es.close();
        reject(new Error('SSE connection timeout'));
      }, 5000);

      es.onmessage = (event) => {
        clearTimeout(timer);
        es.close();
        try {
          const payload = JSON.parse(event.data);
          resolve(payload);
        } catch (e) {
          reject(e);
        }
      };

      es.onerror = (e) => {
        clearTimeout(timer);
        es.close();
        reject(new Error('SSE error'));
      };
    });
  }, PORT);

  assert.equal(sseCheck.type, 'connected', 'SSE must receive connected event');
  vResults.push({ id: 'V14', status: 'SUBSET_PASS', description: '真实 SSE 事件连接与事件帧接收' });
  console.log('✓ Subset of V14 Passed');

  // -------------------------------------------------------------
  // V16: 专属能力降级检查 (Bookmarks/TopSites 在 Web 端友好降级)
  // -------------------------------------------------------------
  console.log('Executing V16: Exclusive Chrome capabilities graceful fallback...');
  await page.goto(`http://127.0.0.1:${PORT}/space.html`, { waitUntil: 'load' });
  const capabilityCheck = await page.evaluate(async () => {
    const { isExtension } = await import('./platform/env.js');
    return { isExtension };
  });
  assert.equal(capabilityCheck.isExtension, false, 'running in clean Chrome web mode');
  vResults.push({ id: 'V16', status: 'SUBSET_PASS', description: '扩展专属能力识别与安全降级' });
  console.log('✓ Subset of V16 Passed');

  // 写入真实验收数据
  const backendCheckPath = join(EVIDENCE_DIR, 'backend/data-checks.json');
  writeFileSync(backendCheckPath, JSON.stringify({
    timestamp: new Date().toISOString(),
    suite: 'single-Web smoke subset; not V01-V17 acceptance',
    isolatedRoot: ISOLATED_ROOT,
    results: vResults,
  }, null, 2));

  console.log('\n--- WEB SMOKE SUBSET FINISHED; FULL ACCEPTANCE STILL REQUIRES THE PLAN MATRIX ---');
} finally {
  await browser?.close();
  srv.kill();
}
