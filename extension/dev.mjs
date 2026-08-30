import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { cp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { createHash, generateKeyPairSync } from 'node:crypto';
import { platform } from 'node:os';
import { basename, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(fileURLToPath(new URL('..', import.meta.url)));
const extensionSource = join(root, 'extension');
const devExtension = join(root, 'dist', 'dev-extension');
const devKeyPath = join(root, 'dist', '.natives-dev-extension-public-key');
const nativeHost = join(root, 'target', 'debug', platform() === 'win32' ? 'native-file-host.exe' : 'native-file-host');
const projectHostPaths = [
  nativeHost,
  join(root, 'target', 'release', platform() === 'win32' ? 'native-file-host.exe' : 'native-file-host'),
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
  await rm(devExtension, { recursive: true, force: true });
  await cp(extensionSource, devExtension, {
    recursive: true,
    filter: source => !new Set([
      'README.md', 'check-lifecycle.mjs', 'dev.mjs', 'install-native-host.mjs',
      'launch-workbench.mjs', 'native-host-manifest.json', 'folder-source.svg', 'native-client.test.mjs',
    ]).has(basename(source)),
  });
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
  assert.deepEqual(posixHostPids(`  10 ${projectHostPaths[0]}\n  11 ${projectHostPaths[1]} --flag\n  12 /tmp/native-file-host\n`), [10, 11]);
  console.log(`dev launcher self-test passed (${id})`);
}

if (process.argv.includes('--self-test')) {
  await selfTest();
  process.exit(0);
}

await stopProjectHosts();
run('rtk', ['env', '-u', 'CARGO_TARGET_DIR', 'cargo', 'build', '-p', 'native-file-host'], 'native-file-host build failed.');
if (!existsSync(nativeHost)) throw new Error(`native-file-host was not created: ${nativeHost}`);
const id = await prepareExtension();
run(process.execPath, [join(extensionSource, 'install-native-host.mjs'), '--extension-id', id, '--host-path', nativeHost], 'Native Host registration failed.');
await stopProjectHosts();
console.log(`Natives dev ready.\nExtension: ${devExtension}\nNative Host: ${nativeHost}\nChrome will start the Host on demand; this command does not open a browser.\nPress Ctrl+C to stop dev.`);
const keepAlive = setInterval(() => {}, 2 ** 31 - 1);
let shuttingDown = false;
for (const signal of ['SIGINT', 'SIGTERM']) process.once(signal, async () => {
  if (shuttingDown) return;
  shuttingDown = true;
  clearInterval(keepAlive);
  try { await stopProjectHosts(); } catch (error) { console.error(error.message); process.exitCode = 1; }
});
