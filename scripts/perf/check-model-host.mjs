import { spawn, spawnSync } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { performance } from 'node:perf_hooks';

const root = resolve(new URL('../..', import.meta.url).pathname);
const binary = join(root, 'target', 'debug', process.platform === 'win32' ? 'model-host.exe' : 'model-host');
if (!existsSync(binary)) throw new Error(`model-host binary missing: ${binary}`);
const configDir = await mkdtemp(join(tmpdir(), 'natives-model-perf-'));
const child = spawn(binary, [], {
  env: { ...process.env, NATIVES_MODEL_HOST_CONFIG_DIR: configDir },
  stdio: ['pipe', 'pipe', 'ignore'],
});

function frame(value) {
  const payload = Buffer.from(JSON.stringify(value));
  const header = Buffer.alloc(4); header.writeUInt32LE(payload.length);
  return Buffer.concat([header, payload]);
}

const startedAt = performance.now();
child.stdin.write(frame({ id: 'perf-snapshot', method: 'model_snapshot', params: {} }));
let buffered = Buffer.alloc(0);
const response = await withTimeout(
  new Promise((resolvePromise, reject) => child.stdout.on('data', (chunk) => {
    buffered = Buffer.concat([buffered, chunk]);
    if (buffered.length < 4 || buffered.length < 4 + buffered.readUInt32LE(0)) return;
    try { resolvePromise(JSON.parse(buffered.subarray(4, 4 + buffered.readUInt32LE(0)))); } catch (error) { reject(error); }
  })), 3000, 'model-host snapshot timed out');
const snapshotMs = performance.now() - startedAt;
let rssKB = null;
let idleCPU = null;
if (process.platform !== 'win32') {
  const pids = [child.pid, ...childProcessIDs(child.pid)];
  const cpuStart = totalCPUTime(pids);
  const idleStartedAt = performance.now();
  await new Promise((resolvePromise) => setTimeout(resolvePromise, 1000));
  const cpuEnd = totalCPUTime(pids);
  if (cpuStart !== null && cpuEnd !== null) idleCPU = Math.max(0, (cpuEnd - cpuStart) / (performance.now() - idleStartedAt) * 100);
  const result = spawnSync('ps', ['-o', 'rss=', '-p', pids.join(',')], { encoding: 'utf8' });
  rssKB = result.stdout.trim().split(/\s+/).reduce((total, value) => total + Number(value), 0) || null;
}
const exitStartedAt = performance.now();
child.stdin.end();
const exit = await withTimeout(new Promise((resolvePromise) => child.once('exit', (code, signal) => resolvePromise({ code, signal }))), 2000, 'model-host did not exit after EOF', () => child.kill('SIGKILL'));
const eofMs = performance.now() - exitStartedAt;
const resident = await checkResidentLifecycle(configDir);
await rm(configDir, { recursive: true, force: true });

const report = {
  ok: response?.ok === true && snapshotMs <= 1500 && (rssKB === null || rssKB <= 262144) && (idleCPU === null || idleCPU <= 5) && exit.code === 0 && eofMs <= 1000 && resident.ok,
  binary,
  snapshotMs: Math.round(snapshotMs * 100) / 100,
  snapshotBudgetMs: 1500,
  idleRssKB: rssKB,
  idleRssBudgetKB: 262144,
  idleCPUPercent: idleCPU === null ? null : Math.round(idleCPU * 100) / 100,
  idleCPUBudgetPercent: 5,
  eofMs: Math.round(eofMs * 100) / 100,
  eofBudgetMs: 1000,
  exit,
  resident,
};
console.log(JSON.stringify(report, null, 2));
if (!report.ok) process.exit(1);

function withTimeout(promise, milliseconds, message, onTimeout) {
  let timer;
  const timeout = new Promise((_, reject) => { timer = setTimeout(() => { onTimeout?.(); reject(new Error(message)); }, milliseconds); });
  return Promise.race([promise, timeout]).finally(() => clearTimeout(timer));
}

function processCPUTime(pid) {
  const result = spawnSync('ps', ['-o', 'time=', '-p', String(pid)], { encoding: 'utf8' });
  const match = result.stdout.trim().match(/(?:(\d+)-)?(?:(\d+):)?(\d+):(\d+(?:\.\d+)?)/);
  if (!match) return null;
  return (((Number(match[1] || 0) * 24 + Number(match[2] || 0)) * 60 + Number(match[3])) * 60 + Number(match[4])) * 1000;
}

async function checkResidentLifecycle(directory) {
  const environment = { ...process.env, NATIVES_MODEL_HOST_CONFIG_DIR: directory };
  const primary = spawn(binary, [], { env: environment, stdio: ['pipe', 'pipe', 'ignore'] });
  const primaryClient = framedClient(primary);
  let relay;
  let workerPID;
  try {
    const enabled = await withTimeout(primaryClient.call({ id: 'resident-on', method: 'model_gateway_set_resident', params: { expectedRevision: 1, resident: true } }), 3000, 'resident enable timed out');
    workerPID = process.platform === 'win32' ? null : childProcessIDs(primary.pid)[0];
    if (process.platform !== 'win32' && !workerPID) throw new Error('detached model worker missing');
    primary.stdin.end();
    const primaryExit = await withTimeout(waitForExit(primary), 2000, 'native bridge did not exit after EOF');
    const stayedRunning = workerPID ? processRunning(workerPID) : null;
    relay = spawn(binary, [], { env: environment, stdio: ['pipe', 'pipe', 'ignore'] });
    const relayClient = framedClient(relay);
    const snapshot = await withTimeout(relayClient.call({ id: 'resident-snapshot', method: 'model_snapshot', params: {} }), 3000, 'resident relay timed out');
    const reusedWorker = workerPID ? processRunning(workerPID) && childProcessIDs(relay.pid).length === 0 : null;
    const revision = snapshot?.result?.revision;
    const disabled = await withTimeout(relayClient.call({ id: 'resident-off', method: 'model_gateway_set_resident', params: { expectedRevision: revision, resident: false } }), 3000, 'resident disable timed out');
    relay.stdin.end();
    const relayExit = await withTimeout(waitForExit(relay), 2000, 'resident relay did not exit');
    if (workerPID) await waitForProcessExit(workerPID);
    return { ok: enabled?.ok === true && stayedRunning !== false && reusedWorker !== false && snapshot?.result?.gateway?.resident === true && disabled?.result?.gateway?.resident === false && relayExit.code === 0 && primaryExit.code === 0, stayedRunning, reusedWorker, workerExited: workerPID ? !processRunning(workerPID) : null, relayExit, primaryExit };
  } finally {
    primary.kill('SIGKILL');
    relay?.kill('SIGKILL');
    if (workerPID && processRunning(workerPID)) process.kill(workerPID, 'SIGKILL');
  }
}

function childProcessIDs(parentPID) {
  const result = spawnSync('ps', ['-axo', 'pid=,ppid='], { encoding: 'utf8' });
  if (result.status !== 0) throw new Error('could not inspect model host process tree');
  return result.stdout.trim().split('\n').map((line) => line.trim().split(/\s+/).map(Number))
    .filter(([, ppid]) => ppid === parentPID).map(([pid]) => pid);
}

function totalCPUTime(pids) {
  const values = pids.map(processCPUTime);
  return values.some((value) => value === null) ? null : values.reduce((total, value) => total + value, 0);
}

function processRunning(pid) {
  try { process.kill(pid, 0); return true; } catch (error) { if (error.code === 'ESRCH') return false; throw error; }
}

async function waitForProcessExit(pid) {
  const deadline = performance.now() + 2000;
  while (processRunning(pid) && performance.now() < deadline) {
    await new Promise((resolvePromise) => setTimeout(resolvePromise, 20));
  }
  if (processRunning(pid)) throw new Error('non-resident worker did not exit after last client');
}

function framedClient(childProcess) {
  let bytes = Buffer.alloc(0);
  const pending = new Map();
  childProcess.stdout.on('data', (chunk) => {
    bytes = Buffer.concat([bytes, chunk]);
    while (bytes.length >= 4 && bytes.length >= 4 + bytes.readUInt32LE(0)) {
      const size = bytes.readUInt32LE(0);
      const message = JSON.parse(bytes.subarray(4, 4 + size));
      bytes = bytes.subarray(4 + size);
      pending.get(message.id)?.(message);
      pending.delete(message.id);
    }
  });
  return { call(message) { return new Promise((resolvePromise) => { pending.set(message.id, resolvePromise); childProcess.stdin.write(frame(message)); }); } };
}

function waitForExit(childProcess) {
  if (childProcess.exitCode !== null || childProcess.signalCode !== null) return Promise.resolve({ code: childProcess.exitCode, signal: childProcess.signalCode });
  return new Promise((resolvePromise) => childProcess.once('exit', (code, signal) => resolvePromise({ code, signal })));
}
