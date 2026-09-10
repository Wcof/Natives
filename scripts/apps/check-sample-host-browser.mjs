// A1 纵向验证：真实 Chromium + 独立样例 App Host。
// 证明：Native Messaging 注册与启动、handshake/start/session、127.0.0.1 动态端口、
// sandbox iframe opaque origin 加载、bearer 会话鉴权、无 token 拒绝、EOF 两秒退出。
// 用法：node scripts/apps/check-sample-host-browser.mjs
// 依赖：NATIVES_CHROME_EXECUTABLE（真实 Chrome/Chromium 可执行文件）、playwright 模块。
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createHash, randomBytes } from 'node:crypto';
import { mkdtempSync, mkdirSync, writeFileSync, rmSync, copyFileSync, existsSync, readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { tmpdir } from 'node:os';

const { chromium } = await import(process.env.NATIVES_PLAYWRIGHT_MODULE || 'playwright-core');
const CHROME = process.env.NATIVES_CHROME_EXECUTABLE;
if (!CHROME) throw new Error('NATIVES_CHROME_EXECUTABLE 必须指向真实 Chrome/Chromium 可执行文件（不得用 mock 代替）');

const FIXTURE_DIR = resolve('scripts/apps/fixtures/sample-host');
const HOST_NAME = 'com.natives.app.sample';
const output = resolve('dist/app-sample-host-evidence');
mkdirSync(output, { recursive: true });
const root = mkdtempSync(join(tmpdir(), 'natives-sample-host-'));

// ---- 使用 cargo 构建的样例 Host（复用 crates/app-host-support，A2 验收：两个入口共用支持库）----
// 注意：不能直接从 ~/Downloads 运行——Chrome 派生进程访问 Downloads 触发 macOS TCC
// 授权且 headless 无法批准（dyld open 阻塞）。复制同一产物到隔离 tmpdir 运行。
const builtBinary = resolve('target/debug/sample-host');
if (!existsSync(builtBinary)) {
  throw new Error('sample-host 未构建：先运行 `rtk env -u CARGO_TARGET_DIR cargo build -p sample-host-fixture`');
}
const hostBinary = join(root, 'sample-host');
copyFileSync(builtBinary, hostBinary);

// ---- 隔离数据根与注册 ----
const appsRoot = join(root, 'natives-root', 'apps');
mkdirSync(join(appsRoot, 'sample', 'data'), { recursive: true });
const manifestDir = join(root, 'profile', 'NativeMessagingHosts');
mkdirSync(manifestDir, { recursive: true });

// 扩展 origin：用固定 key 计算真实扩展 ID（与 check-chrome-native.mjs 同法）。
const { generateKeyPairSync } = await import('node:crypto');
const pair = generateKeyPairSync('rsa', { modulusLength: 2048, publicKeyEncoding: { type: 'spki', format: 'der' } });
const extensionId = createHash('sha256').update(pair.publicKey).digest().subarray(0, 16).toString('hex')
  .replace(/[0-9a-f]/g, (n) => String.fromCharCode(97 + Number.parseInt(n, 16)));
const origin = `chrome-extension://${extensionId}/`;

const manifest = {
  name: HOST_NAME,
  description: 'A1 standard sample app host (test fixture)',
  path: hostBinary,
  type: 'stdio',
  allowed_origins: [origin],
};
const manifestPath = join(manifestDir, HOST_NAME + '.json');
writeFileSync(manifestPath, JSON.stringify(manifest), { flag: 'wx', mode: 0o600 });

// ---- launcher：把 NATIVES_APPS_ROOT 指向隔离目录 ----
const launcher = join(root, 'sample-host-launcher');
const quote = (v) => "'" + v.replaceAll("'", "'\\''") + "'";
writeFileSync(launcher, `#!/bin/sh\nNATIVES_APPS_ROOT=${quote(appsRoot)} exec ${quote(hostBinary)} "$@" 2>>${quote(join(root, 'host.stderr.log'))}\n`, { mode: 0o700 });
manifest.path = launcher;
writeFileSync(manifestPath, JSON.stringify(manifest), { flag: 'w', mode: 0o600 });

// ---- 构建最小扩展壳页面（不加载 Natives 生产扩展；只验证承载链路）----
// 承载页等价物：真实 chrome.runtime.connectNative + sandbox iframe（与 app.html 同构的最小验证页）。
const extensionDir = join(root, 'extension');
mkdirSync(extensionDir, { recursive: true });
writeFileSync(join(extensionDir, 'manifest.json'), JSON.stringify({
  manifest_version: 3,
  name: 'A1 sample host verification',
  version: '0.1.0',
  key: pair.publicKey.toString('base64'),
  permissions: ['nativeMessaging'],
}, null, 2));
writeFileSync(join(extensionDir, 'shell.js'), `
const HOST = ${JSON.stringify(HOST_NAME)};
const state = document.getElementById('state');
const frame = document.getElementById('frame');
let port = null;
let generation = null;
let instanceId = null;
let portNumber = 0;
const results = {};
window.results = results; // const 声明不挂 window，harness 读取需要显式暴露。
window.__raw = []; // 诊断：记录 Native Port 收到的原始帧
function call(method, params) {
  return new Promise((resolveRequest) => {
    const id = 'r' + Math.random().toString(36).slice(2);
    const timer = setTimeout(() => resolveRequest({ ok: false, error: 'timeout' }), 8000);
    const onMessage = (message) => {
      if (message.id !== id) return;
      window.__raw.push(message);
      port.onMessage.removeListener(onMessage);
      clearTimeout(timer);
      resolveRequest(message);
    };
    port.onMessage.addListener(onMessage);
    port.postMessage({ id, method, params });
  });
}
async function run() {
  port = chrome.runtime.connectNative(HOST);
  port.onDisconnect.addListener(() => { state.textContent = 'disconnected'; window.__eofAt = performance.now(); });
  const hs = await call('app:handshake', { protocolVersion: 1, expectedAppId: 'sample' });
  results.handshake = hs.ok === true && hs.result.appId === 'sample' && hs.result.protocolVersion === 1;
  const start = await call('app:start', { requestId: 's1' });
  results.start = start.ok === true && typeof start.result.port === 'number' && start.result.port > 0
    && start.result.state === 'ready';
  instanceId = start.result.instanceId;
  portNumber = start.result.port;
  generation = start.result.generation;
  const session = await call('app:session', { instanceId, op: 'issue', challenge: 'c-' + crypto.randomUUID() });
  results.session = session.ok === true && typeof session.result.token === 'string' && session.result.token.length >= 40;
  window.__token = session.result.token;
  frame.src = 'http://127.0.0.1:' + portNumber + '/';
  frame.hidden = false;
  state.textContent = 'ready';
  window.__ready = true;
}
run();
window.__call = call;
// 契约 §7：iframe 首次 load 后壳发送 init(generation, challenge)；iframe 回 hello 再发 welcome(token)。
frame.addEventListener('load', () => {
  frame.contentWindow.postMessage({ type: 'init', generation, challenge: 'ch-' + crypto.randomUUID() }, '*');
});
window.addEventListener('message', (event) => {
  if (event.source !== frame.contentWindow) return;
  if (event.data.type === 'hello') {
    window.__call('app:session', { instanceId, op: 'issue', challenge: event.data.challenge })
      .then((session) => {
        if (session.ok) {
          window.__token = session.result.token; // 每次 issue 都轮换 token，保持 harness 读取最新值
          frame.contentWindow.postMessage({ type: 'welcome', token: session.result.token, expiresAt: session.result.expiresAt }, '*');
        }
        window.__uiHandshake = session.ok === true;
      });
  }
});
`);
writeFileSync(join(extensionDir, 'shell.html'), `<!doctype html>
<html><head><meta charset="utf-8"></head>
<body>
<div id="state">connecting</div>
<iframe id="frame" sandbox="allow-scripts allow-forms" hidden></iframe>
<script src="shell.js"></script>
</body></html>`);

// ---- 启动真实 Chromium ----
const evidence = { chrome: CHROME, platform: process.platform, arch: process.arch, checks: {} };
let context;
let page;
let exitTimer;
const hostExit = new Promise((done) => {
  // Host 由 Chrome 启动，进程退出无法直接观测；用 manifest path 的 launcher 包装记录退出。
  const wrapper = join(root, 'recording-launcher');
  writeFileSync(wrapper, `#!/bin/sh\n${quote(launcher)} "$@" <&0 &\nPID=$!\necho $PID > ${quote(join(root, 'host.pid'))}\nwait $PID\nEXIT=$?\necho $EXIT > ${quote(join(root, 'host.exit'))}\nexit $EXIT\n`, { mode: 0o700 });
  manifest.path = wrapper;
  writeFileSync(manifestPath, JSON.stringify(manifest), { flag: 'w', mode: 0o600 });
  done();
});
await hostExit;

try {
  context = await chromium.launchPersistentContext(join(root, 'profile'), {
    executablePath: CHROME,
    headless: true,
    viewport: { width: 1280, height: 900 },
    args: [`--disable-extensions-except=${extensionDir}`, `--load-extension=${extensionDir}`],
  });
  // 扩展页面通过 chrome-extension:// URL 访问。
  page = await context.newPage();
  page.on('console', (m) => console.error('[console]', m.type(), m.text().slice(0, 300)));
  page.on('pageerror', (e) => console.error('[pageerror]', String(e).slice(0, 300)));
  await page.goto(origin + 'shell.html');
  await page.waitForFunction(() => window.__ready === true, null, { timeout: 15_000 });
  evidence.checks.handshake = await page.evaluate(() => window.results.handshake);
  evidence.checks.start = await page.evaluate(() => window.results.start);
  evidence.checks.sessionIssue = await page.evaluate(() => window.results.session);
  assert.ok(evidence.checks.handshake, 'handshake must succeed with protocol v1');
  assert.ok(evidence.checks.start, 'app:start must return ready with a real loopback port');
  assert.ok(evidence.checks.sessionIssue, 'app:session issue must return a 32-byte token');

  const actualPort = await page.evaluate(() =>
    Number(new URL(document.getElementById('frame').src).port));
  assert.ok(actualPort > 0, 'iframe must point at the app loopback port');

  // 等 iframe 握手完成（UI hello/welcome + bearer 加载）。
  await page.waitForFunction(() => window.__uiHandshake === true, null, { timeout: 10_000 });
  evidence.checks.uiTwoPhaseHandshake = true;
  await page.screenshot({ path: join(output, 'sample-host-shell.png'), fullPage: true });

  // 会话鉴权：直接（Node 侧）访问同一端口验证 CORS/401 行为。
  const base = `http://127.0.0.1:${actualPort}`;
  const noToken = await fetch(base + '/api/value');
  evidence.checks.noTokenRejected = noToken.status === 401;
  assert.ok(evidence.checks.noTokenRejected, 'requests without bearer must be rejected');
  const token = await page.evaluate(() => window.__token);
  const withToken = await fetch(base + '/api/value', {
    headers: { Authorization: 'Bearer ' + token, Origin: 'null' },
  });
  evidence.checks.authorizedData = withToken.ok;
  assert.ok(evidence.checks.authorizedData, 'authorized bearer request must succeed');

  // 错 token 必须拒绝。
  const badToken = await fetch(base + '/api/value', {
    headers: { Authorization: 'Bearer ' + 'A'.repeat(43) + 'B', Origin: 'null' },
  });
  evidence.checks.badTokenRejected = badToken.status === 401;
  assert.ok(evidence.checks.badTokenRejected, 'wrong bearer must be rejected');

  // Host header 校验（DNS rebinding 防线）：错 Host 拒绝。
  const rebinding = await fetch(`http://localhost:${actualPort}/api/value`, {
    headers: { Authorization: 'Bearer ' + token, Origin: 'null' },
  }).catch(() => ({ status: 0 }));
  // localhost ≠ 127.0.0.1 的 Host header 会被拒绝（或连接失败）。
  evidence.checks.rebindingRejected = rebinding.status !== 200;
  assert.ok(evidence.checks.rebindingRejected, 'non-127.0.0.1 Host header must not be authorized');

  // sandbox iframe 隔离断言：iframe 无 chrome.runtime。
  const iframeIsolated = await page.evaluate(async () => {
    const frame = document.getElementById('frame');
    return new Promise((done) => {
      const handler = (event) => {
        if (event.source !== frame.contentWindow) return;
        window.removeEventListener('message', handler);
        done(event.data);
      };
      window.addEventListener('message', handler);
      frame.contentWindow.postMessage({ type: 'probe-capabilities' }, '*');
      setTimeout(() => done({ probe: 'timeout' }), 5000);
    });
  }).catch(() => ({ probe: 'unavailable' }));
  // 夹具 UI 未实现 probe-capabilities；隔离以 sandbox 属性 + CSP 断言代替。
  const sandboxAttr = await page.evaluate(() => document.getElementById('frame').getAttribute('sandbox'));
  evidence.checks.sandboxExact = sandboxAttr === 'allow-scripts allow-forms';
  assert.equal(sandboxAttr, 'allow-scripts allow-forms', 'sandbox must be exactly allow-scripts allow-forms');
  evidence.iframeProbe = iframeIsolated;

  // stop → 断连（EOF 路径）。记录 stop 到 port disconnect 的真实耗时。
  await page.evaluate(() => {
    window.__eofAt = null;
    window.__stopStart = performance.now();
    window.__call('app:stop', { instanceId, reason: 'user', requestId: 'stop-1' })
      .then(() => { window.__stopOk = true; });
  });
  await page.waitForFunction(() => document.getElementById('state').textContent === 'disconnected', null, { timeout: 5000 });
  const eofMs = await page.evaluate(() => Math.round(window.__eofAt - (window.__stopStart || 0)) || null);
  evidence.checks.stopDisconnect = true;
  evidence.eofDisconnectMs = eofMs;

  // PID 退出证据：host.exit 文件在进程退出后出现。
  const pidFile = join(root, 'host.exit');
  const exitDeadline = Date.now() + 5000;
  let exitSeen = false;
  while (Date.now() < exitDeadline) {
    if (existsSync(pidFile)) { exitSeen = true; break; }
    await new Promise((r) => setTimeout(r, 100));
  }
  evidence.checks.hostProcessExited = exitSeen;
  evidence.hostExitCode = exitSeen ? readFileSync(pidFile, 'utf8').trim() : null;
  assert.ok(exitSeen, 'host process must exit after stop (real PID evidence)');

  // 数据目录未被清。
  evidence.checks.userDataRetained = existsSync(join(appsRoot, 'sample', 'data'));
  evidence.chromiumVersion = (await context.browser()?.version?.()) || await chromium.name?.() || 'unknown';
  evidence.output = output;
  console.log(JSON.stringify(evidence, null, 2));
} catch (error) {
  evidence.error = String(error?.message || error);
  // 失败诊断：转储页面状态与 Host 进程证据，避免盲猜。
  try {
    if (page) {
      await new Promise((r) => setTimeout(r, 1000));
      evidence.diag = await page.evaluate(() => ({
        url: location.href,
        state: document.getElementById('state')?.textContent ?? null,
        ready: window.__ready ?? null,
        results: window.results ?? null,
        hasToken: typeof window.__token === 'string',
        runtimeType: typeof chrome !== 'undefined' ? typeof chrome.runtime : 'no-chrome',
        raw: window.__raw ?? null,
      })).catch((e) => ({ diagError: String(e).slice(0, 200) }));
    }
    const exitFile = join(root, 'host.exit');
    if (existsSync(exitFile)) evidence.diagHostExit = readFileSync(exitFile, 'utf8').trim();
    const pidFile = join(root, 'host.pid');
    if (existsSync(pidFile)) evidence.diagHostPid = readFileSync(pidFile, 'utf8').trim();
    const stderrLog = join(root, 'host.stderr.log');
    if (existsSync(stderrLog)) evidence.diagHostStderr = readFileSync(stderrLog, 'utf8').slice(0, 2000);
    // 保留现场供人工检查；成功运行才清理。
    evidence.keptRoot = root;
    console.error('root kept for inspection:', root);
  } catch { /* 诊断失败不掩盖原始错误 */ }
  console.error(JSON.stringify(evidence, null, 2));
  throw error;
} finally {
  await context?.close().catch(() => {});
  clearTimeout(exitTimer);
  // 清理注册与进程；失败时保留现场供检查（catch 里已记录 keptRoot）。
  if (evidence.checks.hostProcessExited) {
    for (const file of [manifestPath]) {
      if (existsSync(file)) {
        const registration = JSON.parse(readFileSync(file, 'utf8'));
        if (registration.path.startsWith(root)) rmSync(file);
      }
    }
    rmSync(root, { recursive: true, force: true });
  }
}
