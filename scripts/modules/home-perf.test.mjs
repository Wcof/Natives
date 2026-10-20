// Investment home performance acceptance (05 §8): cold-start samples with
// per-hop Native Messaging timing, plus 20 open/close EOF cycles.
// Usage: PROFILE=debug|release|optimized-local rtk proxy node scripts/modules/home-perf.test.mjs
// Same machine, same Chrome, same isolated data layout for both profiles;
// nothing is written to the real user product source or databases.
// optimized-local retains the existing debug-assertion development identity and trust policy;
// it is an optimized local candidate, never production Release evidence.
import assert from 'node:assert/strict';
import { chromium } from 'playwright-core';
import { spawn, spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, writeFileSync, readFileSync, existsSync, chmodSync } from 'node:fs';
import { resolve, join } from 'node:path';
import { homedir } from 'node:os';
import { buildExtension, STABLE_EXTENSION_ID } from '../extension-package.mjs';

const root = resolve(import.meta.dirname, '../..');
const optimizedLocal=process.env.PROFILE==='optimized-local';
const profile = process.env.PROFILE === 'release' || optimizedLocal ? 'release' : 'debug';
const SAMPLES = Number(process.env.SAMPLES || 5);
const CYCLES = Number(process.env.CYCLES || 20);
const SOAK_MS=Number(process.env.SOAK_MS||0);
const base = join(root, '.tmp/invest-home-perf');
mkdirSync(base, { recursive: true });
const out = mkdtempSync(join(base, `${profile}-`));
const fakeHome = join(out, 'home');
const realHome = homedir();
const env = {
  ...process.env,
  HOME: fakeHome,
  NATIVES_DEV_PRODUCT_SOURCE: join(fakeHome, '.natives-local/product-source'),
  // 构建工具链仍用真实 rustup/cargo home（隔离仅覆盖数据与产品源）。
  RUSTUP_HOME: join(realHome, '.rustup'),
  CARGO_HOME: join(realHome, '.cargo'),
};
if(optimizedLocal)env.CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS='true';
delete env.NATIVES_APPS_ROOT; delete env.NATIVES_HOME; delete env.CARGO_TARGET_DIR;
mkdirSync(fakeHome, { recursive: true });
const report = { profile:optimizedLocal?'optimized-local':profile, productionRelease:profile==='release'&&!optimizedLocal, samples: [], cycles: [], errors: [] };
function run(cmd, args) {
  const result = spawnSync(cmd, args, { env, cwd: root, encoding: 'utf8', timeout: 600000 });
  if (result.status !== 0) throw Error(`${cmd} failed: ${(result.stderr || result.stdout).slice(-2000)}`);
}
const chrome = join(realHome, '.cache/puppeteer/chrome/mac_arm-148.0.7778.97/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing');
async function waitForExit(pid, started) {
  while (true) {
    let alive=true;
    try {process.kill(pid,0);} catch(error) {if(error.code==='ESRCH')alive=false;else throw error;}
    const elapsed=performance.now()-started;
    assert.ok(elapsed<=2000,`Runtime ${pid} EOF latency ${elapsed.toFixed(1)}ms exceeds 2s`);
    if(!alive)return elapsed;
    await new Promise(resolve=>setTimeout(resolve,20));
  }
}
let context;
try {
  console.log(`Building ${profile} chain (runtime + core host)…`);
  // 密封链完整复用：refresh 负责 build + manifest 签名 + 隔离 product source。
  run('rtk', ['proxy', process.execPath, 'scripts/apps/refresh-dev-product-source.mjs', ...(profile === 'release' ? ['--release'] : [])]);
  run('rtk', ['env', '-u', 'CARGO_TARGET_DIR', 'cargo', 'build', '-p', 'native-file-host', ...(profile === 'release' ? ['--release'] : [])]);
  const runtimeBin = join(fakeHome, '.natives-local/product-source/hosts/natives-app-runtime');
  const coreBin = join(root, `target/${profile}/native-file-host`);
  assert.ok(existsSync(runtimeBin), 'runtime binary missing');
  assert.ok(existsSync(coreBin), 'core host binary missing');
  const ext = join(out, 'extension'); buildExtension(ext, root);
  const profileDir = join(out, 'chrome-profile');
  const hostDir = join(profileDir, 'NativeMessagingHosts'); mkdirSync(hostDir, { recursive: true });
  for (const [name, bin] of [
    ['com.natives.file_manager', coreBin],
    ['com.natives.local.app_runtime', runtimeBin],
  ]) {
    const wrapper = join(out, `${name}.js`);
    writeFileSync(wrapper, `#!${process.execPath}\nimport {spawn} from 'node:child_process';\nimport {appendFileSync} from 'node:fs';\nconst child=spawn(${JSON.stringify(bin)}, process.argv.slice(2), {env:{...process.env,...${JSON.stringify({HOME:fakeHome,NATIVES_DEV_PRODUCT_SOURCE:env.NATIVES_DEV_PRODUCT_SOURCE})}},stdio:'inherit'});\nappendFileSync(${JSON.stringify(join(out, 'spawns.jsonl'))},JSON.stringify({host:${JSON.stringify(name)},pid:child.pid,t:Date.now()})+'\\n');\nchild.on('exit',code=>process.exit(code||0));\n`);
    chmodSync(wrapper, 0o755);
    writeFileSync(join(hostDir, `${name}.json`), JSON.stringify({ name, description: 'Perf sampling', path: wrapper, type: 'stdio', allowed_origins: [`chrome-extension://${STABLE_EXTENSION_ID}/`] }));
  }
  console.log('Launching isolated Chrome…');
  context = await chromium.launchPersistentContext(profileDir, { executablePath: chrome, headless: true, env, ignoreDefaultArgs: ['--disable-extensions'], args: [`--disable-extensions-except=${ext}`, `--load-extension=${ext}`, '--disable-features=DisableLoadExtensionCommandLineSwitch', '--use-mock-keychain', '--password-store=basic', '--no-first-run'] });

  const spawnLog = () => readFileSync(join(out, 'spawns.jsonl'), 'utf8').trim().split('\n').map(JSON.parse);

  // ---- 冷启动样本 ----
  for (let i = 0; i < SAMPLES; i++) {
    const page = await context.newPage();
    page.on('pageerror', e => report.errors.push(`sample${i}: ${e.message}`));
    await page.addInitScript(() => {
      if (!globalThis.chrome?.runtime?.connectNative) return;
      const connect = chrome.runtime.connectNative.bind(chrome.runtime);
      window.nativeTimings = [];
      chrome.runtime.connectNative = name => {
        const port = connect(name), pending = new Map(), send = port.postMessage.bind(port);
        port.postMessage = request => { pending.set(request.id, { method: request.method, started: performance.now() }); return send(request); };
        port.onMessage.addListener(reply => { const item = pending.get(reply.id); if (item) { pending.delete(reply.id); window.nativeTimings.push({ host: name, method: item.method, ms: performance.now() - item.started }); } });
        return port;
      };
    });
    const start = performance.now();
    await page.goto(`chrome-extension://${STABLE_EXTENSION_ID}/app.html?app=fund`, { waitUntil: 'commit' });
    const frame = page.frameLocator('iframe');
    await frame.locator('#hc-total-value').waitFor({ state:'attached',timeout: 60000 });
    const coldOpenMs = performance.now() - start;
    const nativeTimings = await page.evaluate(() => window.nativeTimings);
    report.samples.push({ sample: i + 1, coldOpenMs, nativeTimings });
    console.log(`sample ${i + 1}: coldOpen=${coldOpenMs.toFixed(1)}ms apps:hs=${(nativeTimings.find(t => t.method === 'apps:handshake') || {}).ms?.toFixed(1) ?? '?'}ms app:hs=${(nativeTimings.find(t => t.method === 'app:handshake') || {}).ms?.toFixed(1) ?? '?'}ms`);
    await page.close();
    // 等该样本的 runtime 退出（EOF ≤ 2s 才开始下一个样本，避免互扰）。
    const pid = spawnLog().findLast(s => s.host === 'com.natives.local.app_runtime').pid;
    await waitForExit(pid,performance.now());
  }

  // ---- 20 次浏览器开关 EOF ----
  const switchPage = await context.newPage();
  switchPage.on('pageerror', e => report.errors.push(`cycle: ${e.message}`));
  for (let i = 0; i < CYCLES; i++) {
    await switchPage.goto(`chrome-extension://${STABLE_EXTENSION_ID}/app.html?app=fund`, { waitUntil: 'commit' });
    const frame = switchPage.frameLocator('iframe');
    await frame.locator('#hc-total-value').waitFor({ state:'attached',timeout: 60000 });
    const pid = spawnLog().findLast(s => s.host === 'com.natives.local.app_runtime').pid;
    const closedAt = performance.now();
    const exited=waitForExit(pid,closedAt);
    const [,exitMs]=await Promise.all([switchPage.goto('about:blank'),exited]);
    report.cycles.push({ cycle: i + 1, exitMs });
  }
  const exits = report.cycles.map(c => c.exitMs);
  report.cycleExit = { max: Math.max(...exits), p95: exits.slice().sort((a,b)=>a-b)[Math.floor(exits.length * 0.95) - 1] ?? Math.max(...exits) };
  console.log(`EOF cycles: max=${report.cycleExit.max.toFixed(1)}ms p95=${report.cycleExit.p95.toFixed(1)}ms (budget 2000ms)`);

  // Repeated real page/API work, then idle samples of the live Runtime (no mock memory).
  if(SOAK_MS>0){
    await switchPage.goto(`chrome-extension://${STABLE_EXTENSION_ID}/app.html?app=fund`);
    const f=switchPage.frameLocator('iframe');
    await f.locator('#home-privacy-btn').waitFor({timeout:30000});
    const pid=spawnLog().findLast(s=>s.host==='com.natives.local.app_runtime').pid;
    const started=performance.now();report.soak={durationMs:SOAK_MS,pid,samples:[]};
    while(performance.now()-started<SOAK_MS){
      for(const tab of ['tab-positions','tab-assets','tab-home'])await f.locator('#'+tab).click();
      await switchPage.frames().find(f=>f.url().startsWith('http://127.0.0.1:')).evaluate(()=>Fund.api('GET','/api/home/snapshot'));
      await switchPage.waitForTimeout(Math.min(30000,Math.max(0,SOAK_MS-(performance.now()-started))));
      const probe=spawnSync('ps',['-p',String(pid),'-o','rss=,pcpu='],{encoding:'utf8'});
      assert.equal(probe.status,0,'soak runtime must remain live');
      const [rssKiB,cpuPercent]=probe.stdout.trim().split(/\s+/).map(Number);
      assert.ok(Number.isFinite(rssKiB)&&rssKiB>0);
      report.soak.samples.push({elapsedMs:performance.now()-started,rssKiB,cpuPercent});
      console.log(`soak ${Math.round((performance.now()-started)/1000)}s RSS ${rssKiB} KiB CPU ${cpuPercent}%`);
    }
    const baseline=report.soak.samples[0].rssKiB,last=report.soak.samples.at(-1).rssKiB;
    report.soak.idleGrowthPercent=(last-baseline)/baseline*100;
    assert.ok(report.soak.idleGrowthPercent<=15,'30 minute idle RSS growth exceeds 15%');
    const closed=performance.now(),exited=waitForExit(pid,closed);
    const [,exitMs]=await Promise.all([switchPage.goto('about:blank'),exited]);
    report.soak.exitMs=exitMs;

  }

  // ---- 汇总 ----
  const colds = report.samples.map(s => s.coldOpenMs).sort((a, b) => a - b);
  const pick = (arr, q) => arr[Math.min(arr.length - 1, Math.floor(arr.length * q))];
  report.summary = {
    coldOpen: { min: colds[0], p50: pick(colds, 0.5), p75: pick(colds, 0.75), max: colds[colds.length - 1] },
    budget: { coldStartP75Ms: 2500, eofMaxMs: 2000 },
  };
  const hop = (host, method) => {
    const vals = report.samples.map(s => (s.nativeTimings.find(t => t.host === host && t.method === method) || {}).ms).filter(v => v != null);
    return vals.length ? { mean: vals.reduce((a, b) => a + b, 0) / vals.length, min: Math.min(...vals), max: Math.max(...vals) } : null;
  };
  report.hops = {
    appsHandshake: hop('com.natives.file_manager', 'apps:handshake'),
    appHandshake: hop('com.natives.local.app_runtime', 'app:handshake'),
    appStart: hop('com.natives.local.app_runtime', 'app:start'),
  };
  console.log('SUMMARY:', JSON.stringify(report.summary, null, 1));
  console.log('HOPS:', JSON.stringify(report.hops, null, 1));
  const failures = [];
  if (report.summary.coldOpen.p75 > 2500) failures.push(`cold open p75 ${report.summary.coldOpen.p75.toFixed(0)}ms > 2500ms`);
  if (report.cycleExit.max > 2000) failures.push(`EOF max ${report.cycleExit.max.toFixed(0)}ms > 2000ms`);
  report.budgetResult = failures.length ? { pass: false, failures } : { pass: true };
  assert.deepEqual(failures,[], 'performance budgets are acceptance gates');
  if (failures.length) console.log('BUDGET FAILURES:', failures.join('; '));
  else console.log('Budget met (cold p75 ≤ 2.5s, EOF ≤ 2s).');
  assert.deepEqual(report.errors, [], 'no page errors allowed');
  console.log(`Perf sampling passed (${optimizedLocal?"optimized-local (not production Release)":profile}, ${SAMPLES} samples, ${CYCLES} cycles).`);
} catch(error){
  report.failure=error.message;
  report.pages=await Promise.all((context?.pages()||[]).map(async p=>({url:p.url(),text:await p.locator('body').innerText().catch(()=>''),frames:await Promise.all(p.frames().map(async f=>({url:f.url(),text:await f.locator('body').innerText().catch(()=>'' )})))})));
  throw error;
} finally {
  await context?.close();
  writeFileSync(join(out, 'report.json'), JSON.stringify(report, null, 2));
  console.log(`Report: ${join(out, 'report.json')}`);
}
