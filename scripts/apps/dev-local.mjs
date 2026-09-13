// apps:dev — 本地开发模式：本机构建 Core 与标准样例，隔离目录安装并启动应用中心。
//
// 复用现有资产，不另建安装器或第二条生产链路：
//   - Core 走 Debug-only 的 --app-fixture stdio 适配（真实 frame/dispatcher/AppStore）；
//   - 安装走既有 App Store v4 安装事务（install_begin/chunk/finish/commit）；
//   - 运行走既有 app-host-support 支持库（运行锁、origin 校验、EOF 退出）。
//
// 开发例外四条件（缺一不可，不能仅凭 manifest.fixture=true 绕过验证）：
//   1. Debug 构建（显式 cargo debug 构建；Release 无 --app-fixture，握手探测失败即拒绝）；
//   2. 显式开发启动（仅本脚本创建隔离状态目录并注册进隔离浏览器配置）；
//   3. 隔离目录（状态/浏览器配置全部位于 dist/apps-dev/，不触碰 ~/.natives、
//      日常 NativeMessagingHosts 注册、日常浏览器数据与 Keychain）；
//   4. 已核验的本机构建产物（ad-hoc 签名校验 + quarantine 审计 + sha256 对账）。
//
// 正式 Release 不受影响：fixture 包仅在 cfg(debug_assertions) 下被 Core 接受；
// 发布门禁 publish-release.mjs 拒绝 fixture 目录并要求平台签名/公证证据。
// 本地开发不要求 Developer ID 与公证；macOS 拦截时只定位报告，不删 quarantine、
// 不关闭 Gatekeeper、不自动放行。
//
// 用法：
//   npm run apps:dev                 # 构建+安装+启动可交互应用中心
//   npm run apps:dev -- --verify     # 非交互验证：安装/打开/停止/重开
//   npm run apps:dev -- --doctor     # macOS 拦截诊断：定位具体文件与原因
import { spawn, spawnSync } from 'node:child_process';
import { createHash, generateKeyPairSync } from 'node:crypto';
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { buildExtension, ROOT } from '../extension-package.mjs';
import { nativePort } from './native-fixture.mjs';
import { signCatalog } from './catalog-signing.mjs';
import { verifyCatalogSignature } from '../../extension/catalog-client.js';

const digest = (bytes) => createHash('sha256').update(bytes).digest('hex');
const sha = (path) => digest(readFileSync(path));
const devRoot = resolve(ROOT, 'dist/apps-dev');
const stateDir = join(devRoot, 'state'); // --app-fixture 根：natives.db、apps/、profile/
const buildDir = join(devRoot, 'build'); // 已签名的开发 Catalog 与 .nap
const coreHostBinary = join(ROOT, 'target/debug/native-file-host');
const sampleBinary = join(ROOT, 'target/debug/sample-host');
const devKeyPath = resolve(ROOT, 'scripts/apps/keys/catalog-trust-dev.private.pem');
const verifyMode = process.argv.includes('--verify');
const doctorMode = process.argv.includes('--doctor');

function fail(message) {
  console.error(`apps:dev 失败：${message}`);
  process.exit(1);
}

// —— 条件 4：已核验的本机构建产物 ——
// 本机构建产物天然没有 quarantine xattr（quarantine 由下载程序盖戳，不是 codesign）；
// 因此本链路不触发 Gatekeeper，无需 Developer ID、公证或任何放行操作。
// ad-hoc 签名（linker-signed）校验确保产物来自当前 checkout 且未被替换/损坏。
function auditBinary(path, label) {
  if (!existsSync(path)) fail(`${label} 不存在：${path}（cargo debug 构建失败？）`);
  if (process.platform === 'darwin') {
    const verify = spawnSync('codesign', ['--verify', '--strict', path], { encoding: 'utf8' });
    if (verify.status !== 0) fail(`${label} 代码签名无效：${(verify.stderr || verify.stdout || '').trim()}`);
    const info = spawnSync('codesign', ['-dv', path], { encoding: 'utf8' });
    if (!/Signature=adhoc|linker-signed/.test(info.stderr || '')) {
      fail(`${label} 不是 ad-hoc 本机签名，拒绝作为开发产物：${(info.stderr || '').trim()}`);
    }
    const xattr = spawnSync('xattr', ['-p', 'com.apple.quarantine', path], { encoding: 'utf8' });
    if (xattr.status === 0) {
      fail(`${label} 带 quarantine 标记（${xattr.stdout.trim()}），macOS 将拦截 ad-hoc 程序。` +
        '本机构建产物不应有 quarantine；请勿把下载所得二进制放进 target/，请改用本机构建。' +
        '（按纪律不删除 quarantine、不关闭 Gatekeeper、不自动放行。）');
    }
  }
  return sha(path);
}

// —— 条件 1：Debug 构建（Release Core 无 --app-fixture，握手必然失败）——
function probeDebugFixture(binary) {
  const probeRoot = mkdtempSync(join(tmpdir(), 'natives-dev-probe-'));
  const origin = 'chrome-extension://abcdefghijklmnopabcdefghijklmnop/';
  const port = nativePort(binary, ['--app-fixture', probeRoot, origin]);
  return port.call('apps:handshake', { origin })
    .then(async (host) => {
      await port.close();
      rmSync(probeRoot, { recursive: true, force: true });
      if (host.appsProtocolVersion !== 4) fail(`Core fixture 协议版本异常：${host.appsProtocolVersion}`);
    })
    .catch((error) => {
      rmSync(probeRoot, { recursive: true, force: true });
      fail(`Core 未处于 Debug fixture 模式（开发例外条件 1 不满足）：${error.message}`);
    });
}

function build() {
  console.log('[1/5] rtk cargo debug 构建 native-file-host + sample-host-fixture（沿用当前 target）');
  const result = spawnSync('rtk',
    ['env', '-u', 'CARGO_TARGET_DIR', 'cargo', 'build', '-p', 'native-file-host', '-p', 'sample-host-fixture'],
    { cwd: ROOT, stdio: 'inherit' });
  if (result.status !== 0) fail('cargo debug 构建失败');

  console.log('[1b/5] 本机构建 Fund 并准备 Suite Seeds（无远端下载，P10-2）');
  const fundDir = resolve(ROOT, '../Natives-App-Fund');
  if (existsSync(fundDir)) {
    const seedBuild = spawnSync(process.execPath, ['scripts/apps/build-suite-seeds.mjs'], { cwd: ROOT, stdio: 'inherit' });
    if (seedBuild.status !== 0) fail('Suite Seeds 本机构建失败');
  }
}

async function packageAndSign() {
  console.log('[2/5] 打包标准样例并用本地开发信任根签名（仅限本机 fixture）');
  rmSync(buildDir, { recursive: true, force: true });
  mkdirSync(buildDir, { recursive: true });
  const pack = spawnSync(process.execPath,
    ['scripts/apps/package-demo.mjs', '--installable-fixture', buildDir],
    { cwd: ROOT, stdio: 'inherit', env: { ...process.env, NATIVES_SAMPLE_APP_ID: 'sample', NATIVES_SAMPLE_VERSION: '1.0.0' } });
  if (pack.status !== 0) fail('样例打包失败');
  signCatalog(join(buildDir, 'catalog-v3.json'), devKeyPath);
  const catalogBytes = readFileSync(join(buildDir, 'catalog-v3.json'));
  const signature = readFileSync(join(buildDir, 'catalog-v3.sig'), 'utf8');
  await verifyCatalogSignature({ catalogBytes, signatureB64: signature });
  const entry = JSON.parse(catalogBytes).apps[0];
  if (entry.manifest?.fixture !== true) fail('开发目录必须携带 fixture=true');
  return { entry, catalogBytes, signature };
}

// —— 条件 2/3：显式开发启动 + 隔离目录 ——
// macOS Chrome 会读取 --user-data-dir 下的 NativeMessagingHosts/（Chrome 官方文档：
// user-level manifest 位于用户配置目录的 NativeMessagingHosts 子目录），
// 因此注册只写入隔离 profile，不触碰日常 ~/Library/.../NativeMessagingHosts。
function prepareState(origin) {
  console.log('[3/5] 准备隔离开发状态目录与浏览器配置（dist/apps-dev/state）');
  if (process.platform !== 'win32') {
    const busy = spawnSync('pgrep', ['-f', `user-data-dir=${stateDir}`], { encoding: 'utf8' });
    if (busy.status === 0) {
      fail(`先前的开发浏览器仍在运行（pid: ${busy.stdout.trim().replaceAll('\n', ', ')}），请先退出再重跑，避免清理竞争。`);
    }
  }
  rmSync(stateDir, { recursive: true, force: true });
  const manifestDir = join(stateDir, 'profile/NativeMessagingHosts');
  mkdirSync(manifestDir, { recursive: true });

  const devSeedsDir = join(stateDir, 'seeds');
  mkdirSync(devSeedsDir, { recursive: true });
  const repoSeeds = join(ROOT, 'dist/seeds');
  if (existsSync(repoSeeds)) {
    for (const f of readdirSync(repoSeeds)) {
      copyFileSync(join(repoSeeds, f), join(devSeedsDir, f));
    }
  }

  const launcher = join(stateDir, 'core-host');
  const quote = (value) => "'" + value.replaceAll("'", "'\\''") + "'";
  writeFileSync(launcher, '#!/bin/sh\nexport NATIVES_SEEDS_DIR=' + quote(devSeedsDir) + '\nexec ' + quote(coreHostBinary)
    + ' --app-fixture ' + quote(stateDir) + ' "$1" --chrome-profile 2>>' + quote(join(stateDir, 'core.stderr')) + '\n',
    { mode: 0o700 });
  writeFileSync(join(manifestDir, 'com.natives.file_manager.json'), JSON.stringify({
    name: 'com.natives.file_manager', description: 'Natives dev app center (isolated)',
    path: launcher, type: 'stdio', allowed_origins: [origin],
  }), { mode: 0o600 });
  return launcher;
}

async function installViaCore(launcher, origin, catalog) {
  console.log('[4/5] 通过 App Store v4 安装事务安装样例（真实 Core，非旁路）');
  const core = nativePort(launcher, [origin]);
  try {
    const host = await core.call('apps:handshake', { origin });
    const tx = await core.call('apps:install_begin', {
      appId: catalog.entry.app_id,
      catalogBase64: catalog.catalogBytes.toString('base64'),
      signature: catalog.signature,
    });
    const pkg = catalog.entry.packages.find((item) => item.package_id === tx.package_id);
    const artifactName = new URL(pkg.url).pathname.split('/').at(-1);
    const artifact = readFileSync(join(buildDir, artifactName));
    if (artifact.length !== pkg.wire_size || sha(join(buildDir, artifactName)) !== pkg.artifact_sha256) {
      fail('打包产物与签名目录不一致（已核验产物条件不满足）');
    }
    for (let offset = 0; offset < artifact.length; offset += tx.chunk_size) {
      const chunk = artifact.subarray(offset, Math.min(offset + tx.chunk_size, artifact.length));
      await core.call('apps:install_chunk', {
        installId: tx.install_id, packageId: pkg.package_id, offset,
        dataBase64: chunk.toString('base64'), chunkSha256: digest(chunk),
      });
    }
    await core.call('apps:install_finish', {
      installId: tx.install_id, packageId: pkg.package_id, artifactBytes: artifact.length,
    });
    const installed = await core.call('apps:install_commit', { installId: tx.install_id });
    return { installed, runtimeHost: installed.runtime_host || installed.runtimeHost };
  } finally {
    await core.close();
  }
}

// 真实浏览器会话中的“打开”由 Chrome 按清单启动载荷进程；--verify 用同一注册清单
// 直连载荷，等价验证安装/打开/停止/重开与锁、EOF 生命周期（对应 apps:integration）。
async function verifyLifecycle(origin, runtimeHost) {
  console.log('[5/5] 验证：打开 → 写读数据 → 停止 → 重开');
  const manifest = JSON.parse(readFileSync(
    join(stateDir, 'profile/NativeMessagingHosts', runtimeHost + '.json'), 'utf8'));
  const runtime = resolve(manifest.path);
  if (!runtime.startsWith(join(stateDir, 'apps') + '/') || manifest.allowed_origins[0] !== origin) {
    fail('运行时注册不在隔离目录内或 origin 不符');
  }
  const open = async () => {
    const port = nativePort(runtime, [origin]);
    await port.call('app:handshake', { protocolVersion: 1, expectedAppId: 'sample' });
    const started = await port.call('app:start', { requestId: 'dev-verify' });
    const session = await port.call('app:session', {
      instanceId: started.instanceId, op: 'issue', challenge: 'dev-verify-challenge',
    });
    const options = { headers: { Authorization: `Bearer ${session.token}`, Origin: 'null' } };
    const saved = await fetch(`http://127.0.0.1:${started.port}/api/value`, { ...options, method: 'POST', body: 'dev-local' });
    if (!saved.ok) fail('样例保存请求失败');
    const data = await fetch(`http://127.0.0.1:${started.port}/api/value`, options).then((r) => r.json());
    if (data.value !== 'dev-local') fail(`回读值不符：${JSON.stringify(data)}`);
    return { port, started };
  };
  const first = await open();
  await first.port.call('app:stop', { instanceId: first.started.instanceId, reason: 'user', requestId: 'stop-1' });
  const closeMs = await first.port.close();
  auditBinary(runtime, '已安装的运行载荷');
  const second = await open();
  await second.port.call('app:stop', { instanceId: second.started.instanceId, reason: 'user', requestId: 'stop-2' });
  await second.port.close();
  console.log(`  打开/停止/重开 通过（EOF 关闭耗时 ${closeMs.toFixed(0)}ms，预算 2000ms）`);

  const core = nativePort(join(stateDir, 'core-host'), [origin]);
  try {
    const list = await core.call('apps:list');
    const fundApp = list.apps?.find((a) => a.app_id === 'fund');
    if (fundApp) {
      console.log(`  Suite Seed 预装验证通过：Fund 已就绪（v${fundApp.version}，未触发远端下载）`);
    }
  } finally {
    await core.close();
  }
}

function devExtensionOrigin() {
  const devExtension = join(ROOT, 'dist/dev-extension');
  const keyPath = join(ROOT, 'dist/.natives-dev-extension-public-key');
  mkdirSync(join(ROOT, 'dist'), { recursive: true });
  let publicKey;
  if (existsSync(keyPath)) publicKey = Buffer.from(readFileSync(keyPath, 'utf8').trim(), 'base64');
  else {
    publicKey = generateKeyPairSync('rsa', {
      modulusLength: 2048, publicKeyEncoding: { type: 'spki', format: 'der' },
    }).publicKey;
    writeFileSync(keyPath, publicKey.toString('base64') + '\n', { mode: 0o600 });
  }
  buildExtension(devExtension);
  const manifestPath = join(devExtension, 'manifest.json');
  const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
  manifest.key = publicKey.toString('base64');
  writeFileSync(manifestPath, JSON.stringify(manifest, null, 2) + '\n');
  const id = createHash('sha256').update(publicKey).digest().subarray(0, 16).toString('hex')
    .replace(/[0-9a-f]/g, (n) => String.fromCharCode(97 + Number.parseInt(n, 16)));
  return { origin: 'chrome-extension://' + id + '/', devExtension };
}

function findInstalledChrome() {
  const candidates = process.env.NATIVES_CHROME_EXECUTABLE
    ? [process.env.NATIVES_CHROME_EXECUTABLE]
    : process.platform === 'darwin'
      ? ['/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
        '/Applications/Chromium.app/Contents/MacOS/Chromium',
        '/Applications/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing']
      : ['/usr/bin/google-chrome', '/usr/bin/chromium'];
  const found = candidates.find((path) => existsSync(path));
  if (!found) fail(`未找到已安装的浏览器（检查过：${candidates.join('，')}）。按纪律不自动下载安装；可用 NATIVES_CHROME_EXECUTABLE 指定。`);
  return found;
}

function installedRuntimePath(runtimeHost) {
  const manifest = JSON.parse(readFileSync(
    join(stateDir, 'profile/NativeMessagingHosts', runtimeHost + '.json'), 'utf8'));
  const runtime = resolve(manifest.path);
  if (!runtime.startsWith(join(stateDir, 'apps') + '/')) fail('运行载荷不在隔离 apps 目录内');
  return runtime;
}

function summarize(paths, hashes) {
  console.log('\napps:dev 就绪（隔离开发模式，开发测试通过不代表正式平台验收或全部 A-Gate 通过）');
  console.log('运行文件：');
  for (const [label, path] of paths) console.log(`  ${label.padEnd(14)} ${path}`);
  console.log('产物摘要（sha256）：');
  for (const [label, value] of hashes) console.log(`  ${label.padEnd(14)} ${value}`);
}

if (doctorMode) {
  console.log('apps:dev --doctor：macOS 拦截诊断（只读，不做任何修改）');
  const appManifestPath = join(stateDir, 'profile/NativeMessagingHosts',
    // AC-12: contract mapping prefix `a` (app_activation::runtime_host_name).
                  'com.natives.app.a' + createHash('sha256').update('sample').digest('hex') + '.json');
  const files = [
    ['Core Host', coreHostBinary], ['样例载荷', sampleBinary],
    ['已安装运行载荷', existsSync(appManifestPath)
      ? JSON.parse(readFileSync(appManifestPath, 'utf8')).path
      : join(stateDir, 'apps/sample/runtime')],
    ['core-host 启动器（shell 脚本，无需签名）', join(stateDir, 'core-host')],
  ];
  for (const [label, path] of files) {
    if (!existsSync(path)) { console.log(`  [缺失] ${label}: ${path}`); continue; }
    const isScript = existsSync(path) && readFileSync(path).subarray(0, 2).toString() === '#!';
    const xattr = spawnSync('xattr', ['-p', 'com.apple.quarantine', path], { encoding: 'utf8' });
    console.log(`  [${label}] ${path}`);
    console.log(`    quarantine: ${xattr.status === 0 ? `有（${xattr.stdout.trim()}）→ Gatekeeper 会拦截 ad-hoc 程序` : '无'}`);
    if (!isScript) {
      const sign = spawnSync('codesign', ['--verify', '--strict', path], { encoding: 'utf8' });
      console.log(`    codesign:   ${sign.status === 0 ? '有效' : `无效：${(sign.stderr || '').trim()}`}`);
    }
  }
  const stderrPath = join(stateDir, 'core.stderr');
  if (existsSync(stderrPath)) {
    const tail = readFileSync(stderrPath, 'utf8').split('\n').slice(-20).join('\n');
    if (tail.trim()) console.log(`\ncore.stderr 末尾：\n${tail}`);
  }
  console.log('处理原则：不删除 quarantine、不关闭 Gatekeeper、不自动放行；请改用本机构建产物。');
  process.exit(0);
}

build();
const coreHashAfter = auditBinary(coreHostBinary, 'Core Host');
const sampleHashAfter = auditBinary(sampleBinary, '样例载荷');
await probeDebugFixture(coreHostBinary);
const catalog = await packageAndSign();
const { origin, devExtension } = devExtensionOrigin();
const launcher = prepareState(origin);
const { installed, runtimeHost } = await installViaCore(launcher, origin, catalog);
auditBinary(installedRuntimePath(runtimeHost), '已安装运行载荷');
summarize([
  ['状态目录', stateDir], ['App Store DB', join(stateDir, 'natives.db')],
  ['激活投影', join(stateDir, 'apps/sample/activation.json')],
  ['运行载荷', join(stateDir, 'apps/sample/runtime')],
  ['Core 注册清单', join(stateDir, 'profile/NativeMessagingHosts/com.natives.file_manager.json')],
  ['App 注册清单', join(stateDir, 'profile/NativeMessagingHosts', runtimeHost + '.json')],
  ['浏览器配置', join(stateDir, 'profile')],
  ['Core stderr', join(stateDir, 'core.stderr')],
  ['开发 Catalog', join(buildDir, 'catalog-v3.json')],
], [
  ['native-file-host', coreHashAfter], ['sample-host', sampleHashAfter],
  ['安装版本', String(installed.version ?? '1.0.0')],
]);
if (verifyMode) {
  await verifyLifecycle(origin, runtimeHost);
  console.log('apps:dev --verify：本地安装、打开、停止、重开 全部通过');
  process.exit(0);
}
console.log('\n[交互模式] 启动已安装浏览器（隔离 user-data-dir，不自动下载安装）…');
const chrome = findInstalledChrome();
const child = spawn(chrome, [
  `--user-data-dir=${join(stateDir, 'profile')}`,
  `--load-extension=${devExtension}`,
  '--no-first-run', '--no-default-browser-check',
  origin + 'apps.html',
], { stdio: 'ignore' });
child.once('exit', (code) => {
  console.log(`开发浏览器已退出（code=${code}）。隔离状态保留在 ${stateDir}，可 npm run apps:dev -- --doctor 诊断。`);
  process.exit(0);
});
process.once('SIGINT', () => { child.kill('SIGTERM'); });
