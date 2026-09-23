import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { createHash, generateKeyPairSync } from 'node:crypto';
import { platform } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { buildExtension } from '../scripts/extension-package.mjs';

const root = resolve(fileURLToPath(new URL('..', import.meta.url)));
const extensionSource = join(root, 'extension');
const devExtension = join(root, 'dist', 'dev-extension');
const devKeyPath = join(root, 'dist', '.natives-dev-extension-public-key');
const nativeHost = join(root, 'target', 'debug', platform() === 'win32' ? 'native-file-host.exe' : 'native-file-host');
const modelHost = join(root, 'target', 'debug', platform() === 'win32' ? 'model-host.exe' : 'model-host');
// ADR-0031：App Center 打开内置模块需要统一 App Runtime Host；
// dev 与安装包一致，缺它会导致 app.html 连接失败（Native Host 未连接）。
const appRuntimeHost = join(root, 'target', 'debug', platform() === 'win32' ? 'natives-app-runtime.exe' : 'natives-app-runtime');
const statusbarHost = join(root, 'target', 'debug', 'natives-statusbar');
const projectHostPaths = [
  nativeHost,
  join(root, 'target', 'release', platform() === 'win32' ? 'native-file-host.exe' : 'native-file-host'),
  modelHost,
  join(root, 'target', 'release', platform() === 'win32' ? 'model-host.exe' : 'model-host'),
  appRuntimeHost,
  join(root, 'target', 'release', platform() === 'win32' ? 'natives-app-runtime.exe' : 'natives-app-runtime'),
  statusbarHost,
].map(path => resolve(path));

function posixHostPids(output, hostPaths = projectHostPaths) {
  return String(output).split('\n').flatMap(line => {
    const match = line.match(/^\s*(\d+)\s+(.+)$/);
    if (!match) return [];
    const pid = Number(match[1]);
    const command = match[2];
    return pid !== process.pid && hostPaths.some(path => command === path || command.startsWith(`${path} `)) ? [pid] : [];
  });
}

function runningProjectHostPids() {
  if (platform() === 'win32') {
    const quotedPaths = projectHostPaths.map(path => `'${path.replaceAll("'", "''")}'`).join(',');
    const script = `$paths=@(${quotedPaths}); Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -in $paths } | Select-Object -ExpandProperty ProcessId`;
    const result = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', script], { encoding: 'utf8' });
    if (result.status !== 0) throw new Error(`无法检查已有 Native Host：${result.stderr || result.error || 'PowerShell failed'}`);
    return result.stdout.split(/\s+/).map(Number).filter(pid => Number.isInteger(pid) && pid > 0 && pid !== process.pid);
  }
  const result = spawnSync('ps', ['-ww', '-axo', 'pid=,command='], { encoding: 'utf8' });
  if (result.status !== 0) throw new Error(`无法检查已有 Native Host：${result.stderr || result.error || 'ps failed'}`);
  return posixHostPids(result.stdout);
}

const wait = ms => new Promise(resolvePromise => setTimeout(resolvePromise, ms));

async function stopProjectHosts() {
  const pids = [...new Set(runningProjectHostPids())];
  if (!pids.length) return 0;
  if (platform() === 'win32') {
    const result = spawnSync('taskkill.exe', [...pids.flatMap(pid => ['/PID', String(pid)]), '/T', '/F'], { stdio: 'ignore' });
    if (result.status !== 0) throw new Error(`无法停止已有 Native Host：${pids.join(', ')}`);
  } else {
    for (const pid of pids) {
      try { process.kill(pid, 'SIGTERM'); } catch (error) { if (error.code !== 'ESRCH') throw error; }
    }
    await wait(250);
    const survivors = runningProjectHostPids().filter(pid => pids.includes(pid));
    for (const pid of survivors) {
      try { process.kill(pid, 'SIGKILL'); } catch (error) { if (error.code !== 'ESRCH') throw error; }
    }
  }
  console.log(`已停止旧 Native Host：${pids.join(', ')}`);
  return pids.length;
}

function extensionId(publicKey) {
  return createHash('sha256').update(publicKey).digest().subarray(0, 16)
    .toString('hex').replace(/[0-9a-f]/g, nibble => String.fromCharCode(97 + Number.parseInt(nibble, 16)));
}

async function loadDevKey() {
  await mkdir(join(root, 'dist'), { recursive: true });
  if (existsSync(devKeyPath)) return Buffer.from((await readFile(devKeyPath, 'utf8')).trim(), 'base64');
  const { publicKey } = generateKeyPairSync('rsa', {
    modulusLength: 2048,
    publicKeyEncoding: { type: 'spki', format: 'der' },
  });
  await writeFile(devKeyPath, `${publicKey.toString('base64')}\n`, { mode: 0o600 });
  return publicKey;
}

async function prepareExtension() {
  const publicKey = await loadDevKey();
  buildExtension(devExtension);
  const manifestPath = join(devExtension, 'manifest.json');
  const manifest = JSON.parse(await readFile(manifestPath, 'utf8'));
  manifest.key = publicKey.toString('base64');
  await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
  return extensionId(publicKey);
}

function run(command, args, failure) {
  const result = spawnSync(command, args, { cwd: root, stdio: 'inherit' });
  if (result.status !== 0) throw new Error(failure);
}

async function selfTest() {
  const id = await prepareExtension();
  const manifest = JSON.parse(await readFile(join(devExtension, 'manifest.json'), 'utf8'));
  if (!/^[a-p]{32}$/.test(id) || manifest.key !== (await readFile(devKeyPath, 'utf8')).trim()) throw new Error('dev extension key/ID setup failed.');
  assert.deepEqual(posixHostPids(`  10 ${projectHostPaths[0]}\n  11 ${projectHostPaths[1]} --flag\n  12 ${projectHostPaths[2]}\n  13 ${projectHostPaths[3]} --relay\n  14 /tmp/native-file-host\n`), [10, 11, 12, 13]);
  console.log(`dev launcher self-test passed (${id})`);
}

if (process.argv.includes('--self-test')) {
  await selfTest();
  process.exit(0);
}

await stopProjectHosts();
run('rtk', ['env', '-u', 'CARGO_TARGET_DIR', 'cargo', 'build', '-p', 'native-file-host'], 'native-file-host build failed.');
if (!existsSync(nativeHost)) throw new Error(`native-file-host was not created: ${nativeHost}`);
const modelBuild = spawnSync('rtk', ['go', 'build', '-o', modelHost, '.'], { cwd: join(root, 'model-host'), stdio: 'inherit' });
if (modelBuild.status !== 0 || !existsSync(modelHost)) throw new Error('model-host build failed.');
const id = await prepareExtension();
run(process.execPath, [join(extensionSource, 'install-native-host.mjs'), '--extension-id', id, '--host-path', nativeHost], 'Native Host registration failed.');
run(process.execPath, [join(extensionSource, 'install-native-host.mjs'), '--extension-id', id, '--host-path', modelHost, '--host-name', 'com.natives.model_host', '--description', 'Natives model settings and local model proxy'], 'Model Host registration failed.');
run('rtk', ['env', '-u', 'CARGO_TARGET_DIR', 'cargo', 'build', '-p', 'app-runtime'], 'natives-app-runtime build failed.');
if (!existsSync(appRuntimeHost)) throw new Error(`natives-app-runtime was not created: ${appRuntimeHost}`);
run(process.execPath, [join(extensionSource, 'install-native-host.mjs'), '--extension-id', id, '--host-path', appRuntimeHost, '--host-name', 'com.natives.local.app_runtime', '--description', 'Natives unified built-in app runtime'], 'App Runtime Host registration failed.');
// 重编译即重密封（ADR-0029 dev revision）：runtime 二进制变化后必须同步
// ~/.natives-local/product-source（重算 SHA、重签 dev manifest），否则扩展
// 首次 apps:handshake 触发的 configure_product 用的还是旧载荷哈希，
// app:handshake 会 fail closed（APP_PACKAGE_INVALID），表现为"模块无法打开"。
run(process.execPath, [join(root, 'scripts', 'apps', 'refresh-dev-product-source.mjs')], 'refresh-dev-product-source failed (dev product source re-seal).');

// 自动向 native-file-host 触发一次 apps:handshake，在构建期完成产品重配与 activation.json 密封
await new Promise((resolve) => {
  const host = spawn(nativeHost, [`chrome-extension://${id}/`]);
  const msg = JSON.stringify({ id: 'dev-init-handshake', method: 'apps:handshake', params: { origin: `chrome-extension://${id}/` } });
  const buf = Buffer.alloc(4 + Buffer.byteLength(msg));
  buf.writeUInt32LE(Buffer.byteLength(msg), 0);
  buf.write(msg, 4);
  host.stdin.write(buf);
  host.stdout.on('data', () => {
    host.kill();
    resolve(true);
  });
  host.on('error', () => resolve(false));
  setTimeout(() => { try { host.kill(); } catch {} resolve(false); }, 3000);
});

await stopProjectHosts();

let statusbarProcess = null;
if (platform() === 'darwin') {
  const statusbarBin = join(root, 'target', 'debug', 'natives-statusbar');
  const buildStatusbar = spawnSync('clang', [
    '-O2', '-framework', 'AppKit', '-framework', 'Foundation', '-framework', 'WebKit', '-lsqlite3',
    '-I', join(root, 'installers/macos/resources'),
    join(root, 'installers/macos/resources/launcher-main.m'),
    join(root, 'installers/macos/resources/NativesStatusBar.m'),
    '-o', statusbarBin,
  ], { stdio: 'inherit' });

  if (buildStatusbar.status === 0 && existsSync(statusbarBin)) {
    statusbarProcess = spawn(statusbarBin, ['--status-bar'], { stdio: 'ignore' });
    console.log(`macOS 菜单栏状态项已挂载 (PID: ${statusbarProcess.pid})`);
  }
}

console.log(`Natives dev ready.\nExtension: ${devExtension}\nFiles Host: ${nativeHost}\nModel Host: ${modelHost}\nChrome will start the Host on demand for each capability; this command does not open a browser.\nPress Ctrl+C to stop dev.`);
const keepAlive = setInterval(() => {}, 2 ** 31 - 1);
let shuttingDown = false;
for (const signal of ['SIGINT', 'SIGTERM']) process.once(signal, async () => {
  if (shuttingDown) return;
  shuttingDown = true;
  clearInterval(keepAlive);
  if (statusbarProcess && !statusbarProcess.killed) {
    try { statusbarProcess.kill('SIGTERM'); } catch {}
  }
  try { await stopProjectHosts(); } catch (error) { console.error(error.message); process.exitCode = 1; }
});
