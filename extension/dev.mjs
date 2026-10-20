import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { platform } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { buildExtension, LOCAL_EXTENSION_ID } from '../scripts/extension-package.mjs';

const root = resolve(fileURLToPath(new URL('..', import.meta.url)));
const extensionSource = join(root, 'extension');
const devExtension = join(root, 'dist', 'dev-extension');
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
    const match = line.match(/^\s*(\d+)\s+(\w{3} \w{3}\s+\d+ \d\d:\d\d:\d\d \d{4})\s+(.+)$/);
    if (!match) return [];
    const pid = Number(match[1]);
    const command = match[3];
    if (pid === process.pid || !hostPaths.some(path => command === path || command.startsWith(`${path} `))) return [];
    return [{ pid, startedAt: Date.parse(match[2].replace(/\s+/, ' ')), command }];
  });
}

function runningProjectHostPids() {
  if (platform() === 'win32') {
    const quotedPaths = projectHostPaths.map(path => `'${path.replaceAll("'", "''")}'`).join(',');
    const script = `$paths=@(${quotedPaths}); Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -in $paths } | Select-Object ProcessId, CreationDate, CommandLine | ConvertTo-Json -Compress`;
    const result = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', script], { encoding: 'utf8' });
    if (result.status !== 0) throw new Error(`无法检查已有 Native Host：${result.stderr || result.error || 'PowerShell failed'}`);
    const rows = result.stdout.trim() ? JSON.parse(result.stdout.trim()) : [];
    return (Array.isArray(rows) ? rows : [rows])
      .filter(row => Number.isInteger(row?.ProcessId) && row.ProcessId > 0 && row.ProcessId !== process.pid)
      .map(row => ({ pid: row.ProcessId, startedAt: row.CreationDate ? Date.parse(row.CreationDate) : null, command: row.CommandLine || '' }));
  }
  const result = spawnSync('ps', ['-ww', '-axo', 'pid=,lstart=,command='], { encoding: 'utf8' });
  if (result.status !== 0) throw new Error(`无法检查已有 Native Host：${result.stderr || result.error || 'ps failed'}`);
  return posixHostPids(result.stdout);
}

// 读取单个进程的启动时间与命令行（终止前身份核验用；与
// runningProjectHostPids 同源，防 PID 复用误伤，方案 L2.5）。
function processIdentity(pid) {
  if (platform() === 'win32') {
    const script = `Get-CimInstance Win32_Process -Filter "ProcessId=${pid}" | Select-Object ProcessId, CreationDate, CommandLine | ConvertTo-Json -Compress`;
    const result = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', script], { encoding: 'utf8' });
    if (result.status !== 0 || !result.stdout.trim()) return null;
    const row = JSON.parse(result.stdout.trim());
    return row?.ProcessId ? { pid: row.ProcessId, startedAt: row.CreationDate ? Date.parse(row.CreationDate) : null, command: row.CommandLine || '' } : null;
  }
  const result = spawnSync('ps', ['-p', String(pid), '-ww', '-o', 'lstart=,command='], { encoding: 'utf8' });
  if (result.status !== 0 || !result.stdout.trim()) return null;
  const match = result.stdout.trim().match(/^(\w{3} \w{3}\s+\d+ \d\d:\d\d:\d\d \d{4})\s+(.+)$/);
  if (!match) return null;
  return { pid, startedAt: Date.parse(match[1].replace(/\s+/, ' ')), command: match[2] };
}

// PID 复用防护：终止自行登记的 PID 前核对启动时间一致；启动时间不可得时
// 退回命令行匹配核验。恢复信息不构成终止授权（方案 L2.5）。
function pidStillMatches(entry) {
  const current = processIdentity(entry.pid);
  if (!current) return false;
  if (!projectHostPaths.some(path => current.command === path || current.command.startsWith(`${path} `) || current.command.startsWith(`"${path}"`))) return false;
  if (entry.command && current.command !== entry.command) return false;
  if (entry.startedAt && current.startedAt) return current.startedAt === entry.startedAt;
  return entry.command ? current.command === entry.command : true;
}

const wait = ms => new Promise(resolvePromise => setTimeout(resolvePromise, ms));

// 调用方限定已授权范围；每次发信号前核验身份，TERM 后最多等 2s。
async function stopHosts(owned) {
  let stopped = 0;
  for (const entry of owned) {
    if (!pidStillMatches(entry)) continue;
    try { process.kill(entry.pid, 'SIGTERM'); } catch (error) { if (error.code !== 'ESRCH') throw error; continue; }
    const graceDeadline = Date.now() + 2000;
    while (pidStillMatches(entry) && Date.now() < graceDeadline) await wait(50);
    if (pidStillMatches(entry)) {
      try { process.kill(entry.pid, 'SIGKILL'); } catch (error) { if (error.code !== 'ESRCH') throw error; }
    }
    stopped++;
  }
  return stopped;
}

// 自动清理后再次核对，未释放或新出现的占用仍阻止覆盖产物。
function assertBinariesFree() {
  const busy = runningProjectHostPids();
  if (!busy.length) return 0;
  const detail = busy.map(entry => `  PID ${entry.pid}${entry.command ? ` ${entry.command}` : ''}`).join('\n');
  throw new Error(
    `待替换的开发二进制仍被使用，构建/重密封已阻止（不覆盖使用中的产物）：\n${detail}\n` +
    '请先关闭占用这些 Host 的 Chrome 页面或其他 dev 会话，再重新运行。',
  );
}

async function stopProjectHosts() {
  const hosts = runningProjectHostPids().sort((a, b) =>
    Number(a.command?.includes('--model-host-worker')) - Number(b.command?.includes('--model-host-worker')));
  const stopped = await stopHosts(hosts);
  if (stopped) console.log(`已停止当前仓库旧 Host：${stopped} 个`);
  assertBinariesFree();
  return stopped;
}

function extensionId(publicKey) {
  return createHash('sha256').update(publicKey).digest().subarray(0, 16)
    .toString('hex').replace(/[0-9a-f]/g, nibble => String.fromCharCode(97 + Number.parseInt(nibble, 16)));
}


async function prepareExtension() {
  buildExtension(devExtension);
  // 扩展 ID 必须唯一来源：dist/extension manifest 已带固定 key（与 Rust 侧
  // STABLE_EXTENSION_ID 守护测试同源，派生 gehmgcnlpdepnpmcbbdaijabcjdnbfmh）。
  // 此处严禁再用陈旧 dev key 覆盖 manifest.key 并注册 NM allowlist——那会使
  // NM 清单 ID 与 Chrome 实际加载的扩展 ID 漂移，触发
  // "Access to the specified native messaging host is forbidden"。
  const manifestPath = join(devExtension, 'manifest.json');
  const manifest = JSON.parse(await readFile(manifestPath, 'utf8'));
  if (!manifest.key) throw new Error('dev extension manifest is missing the stable key.');
  return extensionId(Buffer.from(manifest.key, 'base64'));
}

function run(command, args, failure) {
  const result = spawnSync(command, args, { cwd: root, stdio: 'inherit' });
  if (result.status !== 0) throw new Error(failure);
}

async function selfTest() {
  const id = await prepareExtension();
  const manifest = JSON.parse(await readFile(join(devExtension, 'manifest.json'), 'utf8'));
  // ID 由 manifest 固定 key 派生；不得再被 dev key 覆盖（见 prepareExtension 注释）。
  if (!/^[a-p]{32}$/.test(id) || !manifest.key) throw new Error('dev extension key/ID setup failed.');
  assert.deepEqual(posixHostPids(`  10 Sat Oct  4 12:00:00 2026 ${projectHostPaths[0]}\n  11 Fri Oct  3 09:30:59 2026 ${projectHostPaths[1]} --flag\n  12 Thu Oct  2 08:15:00 2026 ${projectHostPaths[2]}\n  13 Wed Oct  1 07:00:01 2026 ${projectHostPaths[3]} --relay\n  14 Thu Oct  2 08:15:00 2026 /tmp/native-file-host\n`).map(entry => entry.pid), [10, 11, 12, 13]);
  run(process.execPath, [join(extensionSource, 'dev-processes.test.mjs')], 'dev process cleanup regression failed.');
  console.log(`dev launcher self-test passed (${id})`);
}

if (process.argv.includes('--self-test')) {
  await selfTest();
  process.exit(0);
}

// 用户授权恢复开发重启：停止本仓库白名单旧 Host，保留身份校验与占用门禁。
await stopProjectHosts();

// 本会话登记的子进程（握手 Host、顶栏）：退出时只清理这些身份核验过的进程。
const ownedChildren = [];
run('rtk', ['env', '-u', 'CARGO_TARGET_DIR', 'cargo', 'build', '-p', 'native-file-host'], 'native-file-host build failed.');
if (!existsSync(nativeHost)) throw new Error(`native-file-host was not created: ${nativeHost}`);
const modelBuild = spawnSync('rtk', ['go', 'build', '-o', modelHost, '.'], { cwd: join(root, 'model-host'), stdio: 'inherit' });
if (modelBuild.status !== 0 || !existsSync(modelHost)) throw new Error('model-host build failed.');
const id = await prepareExtension();
run(process.execPath, [join(extensionSource, 'install-native-host.mjs'), '--extension-id', id, '--extension-id', LOCAL_EXTENSION_ID, '--host-path', nativeHost], 'Native Host registration failed.');
run(process.execPath, [join(extensionSource, 'install-native-host.mjs'), '--extension-id', id, '--extension-id', LOCAL_EXTENSION_ID, '--host-path', modelHost, '--host-name', 'com.natives.model_host', '--description', 'Natives model settings and local model proxy'], 'Model Host registration failed.');
run('rtk', ['env', '-u', 'CARGO_TARGET_DIR', 'cargo', 'build', '-p', 'app-runtime'], 'natives-app-runtime build failed.');
if (!existsSync(appRuntimeHost)) throw new Error(`natives-app-runtime was not created: ${appRuntimeHost}`);
run(process.execPath, [join(extensionSource, 'install-native-host.mjs'), '--extension-id', id, '--extension-id', LOCAL_EXTENSION_ID, '--host-path', appRuntimeHost, '--host-name', 'com.natives.local.app_runtime', '--description', 'Natives unified built-in app runtime'], 'App Runtime Host registration failed.');
// 重编译即重密封（ADR-0029 dev revision）：runtime 二进制变化后必须同步
// ~/.natives-local/product-source（重算 SHA、重签 dev manifest），否则扩展
// 首次 apps:handshake 触发的 configure_product 用的还是旧载荷哈希，
// app:handshake 会 fail closed（APP_PACKAGE_INVALID），表现为"模块无法打开"。
run(process.execPath, [join(root, 'scripts', 'apps', 'refresh-dev-product-source.mjs')], 'refresh-dev-product-source failed (dev product source re-seal).');

// 自动向 native-file-host 触发一次 apps:handshake，在构建期完成产品重配与 activation.json 密封。
// 该 Host 是本会话登记的子进程：登记启动身份，用后 SIGTERM 并有界等待回收。
const handshakeHost = await new Promise((resolve) => {
  const host = spawn(nativeHost, [`chrome-extension://${id}/`]);
  const identity = processIdentity(host.pid);
  const owned = { pid: host.pid, startedAt: identity?.startedAt ?? null };
  ownedChildren.push(owned);
  const msg = JSON.stringify({ id: 'dev-init-handshake', method: 'apps:handshake', params: { origin: `chrome-extension://${id}/` } });
  const buf = Buffer.alloc(4 + Buffer.byteLength(msg));
  buf.writeUInt32LE(Buffer.byteLength(msg), 0);
  buf.write(msg, 4);
  host.stdin.write(buf);
  host.stdout.on('data', () => {
    host.kill();
    resolve(owned);
  });
  host.on('error', () => resolve(owned));
  setTimeout(() => { try { host.kill(); } catch {} resolve(owned); }, 3000);
});
await stopHosts([handshakeHost]);

let statusbarProcess = null;
if (platform() === 'darwin') {
  const statusbarBin = join(root, 'target', 'debug', 'natives-statusbar');
  const buildStatusbar = spawnSync('clang', [
    '-fobjc-arc', '-O2', '-framework', 'AppKit', '-framework', 'Foundation', '-framework', 'WebKit', '-lsqlite3',
    '-I', join(root, 'installers/macos/resources'),
    join(root, 'installers/macos/resources/launcher-main.m'),
    join(root, 'installers/macos/resources/NativesStatusBar.m'),
    '-o', statusbarBin,
  ], { stdio: 'inherit' });

  if (buildStatusbar.status === 0 && existsSync(statusbarBin)) {
    statusbarProcess = spawn(statusbarBin, ['--status-bar'], { stdio: 'ignore' });
    const identity = processIdentity(statusbarProcess.pid);
    ownedChildren.push({ pid: statusbarProcess.pid, startedAt: identity?.startedAt ?? null });
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
  // Ctrl+C 只清理本会话登记并核验过身份的子进程（顶栏、握手 Host）；
  // Chrome 拉起的 Host 与其他会话实例不属于本会话清理范围。
  try {
    const stopped = await stopHosts(ownedChildren);
    if (stopped) console.log(`已停止本会话子进程：${stopped} 个`);
  } catch (error) { console.error(error.message); process.exitCode = 1; }
});
