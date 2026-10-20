import { spawn, spawnSync } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { performance } from 'node:perf_hooks';
import { childProcessIDs, totalCPUTime, sampleCPUWindow, validateCollectStatusResponse } from './cpu-probe.mjs';

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

const client = framedClient(child);
const startedAt = performance.now();
const response = await withTimeout(
  client.call({ id: 'perf-snapshot', method: 'model_snapshot', params: {} }),
  3000, 'model-host snapshot timed out');
const snapshotMs = performance.now() - startedAt;

// Native Messaging 连接建立会触发一次事件驱动的日志采集（TriggerCollectAsync，
// 产品要求的行为，不可移除）。采集期间 CPU 升高是预期工作负载，不冒充空闲；
// 空闲门禁只对“采集完成后的稳态”生效。时序：先查 collecting，若正在采集则
// 立即采样采集阶段，再等 collecting=false，最后测稳态空闲。
// model_collect_status 查询失败/响应非法时 fail closed（抛错终止门禁），
// 绝不静默当作 collecting=false 放行。

let rssKB = null;
let idleCPU = null;
let collectPhaseCPU = null;
let collectPhaseObserved = false;
if (process.platform !== 'win32') {
  const pids = [child.pid, ...childProcessIDs(child.pid)];

  // 阶段一：查询采集状态；正在采集则立即采样，单独记录，不参与门禁判定。
  const firstStatus = await queryCollecting(client);
  if (firstStatus.collecting === true) {
    collectPhaseObserved = true;
    const phase = await sampleCPUWindow(pids, 1000);
    collectPhaseCPU = phase.cpuPercent;
  }

  // 阶段二：等待采集结束（collecting=false），fail closed：非法响应直接抛错。
  await waitForCollectQuiescence(client, 30000);

  // 阶段三：稳态空闲测量（采集已确认结束），参与 5% 预算判定。
  const idle = await sampleCPUWindow(pids, 1000);
  idleCPU = idle.cpuPercent;
  const result = spawnSync('ps', ['-o', 'rss=', '-p', pids.join(',')], { encoding: 'utf8' });
  rssKB = result.stdout.trim().split(/\s+/).reduce((total, value) => total + Number(value), 0) || null;
}
const exitStartedAt = performance.now();
child.stdin.end();
const exit = await withTimeout(new Promise((resolvePromise) => child.once('exit', (code, signal) => resolvePromise({ code, signal }))), 2000, 'model-host did not exit after EOF', () => child.kill('SIGKILL'));
const eofMs = performance.now() - exitStartedAt;
const resident = await checkResidentLifecycle(configDir);
await rm(configDir, { recursive: true, force: true });

// 门禁判定只约束稳态空闲阶段；采集阶段单独报告，不让工作负载伪装成空闲，
// 也不把工作负载计入空闲预算（R-P12：门禁不能自欺，两个方向都不许）。
const report = {
  ok: response?.ok === true && snapshotMs <= 1500 && (rssKB === null || rssKB <= 262144) && (idleCPU === null || idleCPU <= 5) && exit.code === 0 && eofMs <= 1000 && resident.ok,
  binary,
  snapshotMs: Math.round(snapshotMs * 100) / 100,
  snapshotBudgetMs: 1500,
  collectPhase: collectPhaseObserved
    ? { observed: true, collectPhaseCPUPercent: collectPhaseCPU === null ? null : Math.round(collectPhaseCPU * 100) / 100 }
    : { observed: false, note: 'collect finished before first status probe (empty config dir, expected)' },
  idleRssKB: rssKB,
  idleRssBudgetKB: 262144,
  idleCPUPercent: idleCPU === null ? null : Math.round(idleCPU * 100) / 100,
  idleCPUBudgetPercent: 5,
  idlePhase: 'measured after collect quiescence (model_collect_status collecting=false)',
  eofMs: Math.round(eofMs * 100) / 100,
  eofBudgetMs: 1000,
  exit,
  resident,
};
console.log(JSON.stringify(report, null, 2));
if (!report.ok) process.exit(1);

// 轮询 model_collect_status 直到一次采集结束（collecting=false）。
// 超时抛错，不静默放宽；非法响应由 queryCollecting 直接抛错（fail closed）。
async function waitForCollectQuiescence(clientHandle, timeoutMs = 30000) {
  const deadline = performance.now() + timeoutMs;
  while (performance.now() < deadline) {
    const status = await queryCollecting(clientHandle);
    if (status.collecting === false) return;
    await new Promise((resolvePromise) => setTimeout(resolvePromise, 200));
  }
  throw new Error('model usage collection did not reach quiescence within timeout');
}

// 发送一次只读 model_collect_status 询问并严格验证响应。
// fail closed：ok!==true、字段缺失、类型非 boolean 一律抛错，
// 绝不静默当作 collecting=false 放行（R-P12：门禁不能自欺）。
// 校验逻辑在 validateCollectStatusResponse（cpu-probe.mjs），与测试共用同一实现。
async function queryCollecting(clientHandle) {
  const res = await withTimeout(clientHandle.call({ id: 'collect-status-' + Math.random().toString(36).slice(2), method: 'model_collect_status', params: {} }), 2000, 'collect status timed out');
  return validateCollectStatusResponse(res);
}

function withTimeout(promise, milliseconds, message, onTimeout) {
  let timer;
  const timeout = new Promise((_, reject) => { timer = setTimeout(() => { onTimeout?.(); reject(new Error(message)); }, milliseconds); });
  return Promise.race([promise, timeout]).finally(() => clearTimeout(timer));
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
