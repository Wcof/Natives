// CPU 探针共享库：基于 `ps -o time=` 的进程树 CPU 累计时间采样。
// 供 check-model-host.mjs 门禁与 cpu-probe.test.mjs 回归检查共用，
// 保证门禁判定与回归检查使用同一测量口径。
import { spawnSync } from 'node:child_process';
import { performance } from 'node:perf_hooks';

// 读取单个进程自启动以来的累计 CPU 时间（毫秒）；进程不存在返回 null。
export function processCPUTime(pid) {
  const result = spawnSync('ps', ['-o', 'time=', '-p', String(pid)], { encoding: 'utf8' });
  const match = result.stdout.trim().match(/(?:(\d+)-)?(?:(\d+):)?(\d+):(\d+(?:\.\d+)?)/);
  if (!match) return null;
  return (((Number(match[1] || 0) * 24 + Number(match[2] || 0)) * 60 + Number(match[3])) * 60 + Number(match[4])) * 1000;
}

// 列出 parentPID 的直接子进程。
export function childProcessIDs(parentPID) {
  const result = spawnSync('ps', ['-axo', 'pid=,ppid='], { encoding: 'utf8' });
  if (result.status !== 0) throw new Error('could not inspect process tree');
  return result.stdout.trim().split('\n').map((line) => line.trim().split(/\s+/).map(Number))
    .filter(([, ppid]) => ppid === parentPID).map(([pid]) => pid);
}

// 进程树 CPU 累计时间；任一进程读取失败返回 null。
export function totalCPUTime(pids) {
  const values = pids.map(processCPUTime);
  return values.some((value) => value === null) ? null : values.reduce((total, value) => total + value, 0);
}

// 严格验证 model_collect_status 响应（fail closed）：
// ok!==true、result.collecting 缺失或类型非 boolean 一律抛错，
// 绝不静默当作 collecting=false 放行（R-P12：门禁不能自欺）。
export function validateCollectStatusResponse(res) {
  if (!res || res.ok !== true) {
    throw new Error(`model_collect_status returned non-ok response: ${JSON.stringify(res)}`);
  }
  const collecting = res?.result?.collecting;
  if (typeof collecting !== 'boolean') {
    throw new Error(`model_collect_status missing/invalid collecting field: ${JSON.stringify(res?.result)}`);
  }
  return { collecting };
}

export async function sampleCPUWindow(pids, durationMs, sleep = (ms) => new Promise((r) => setTimeout(r, ms)), now = () => performance.now()) {
  const cpuStart = totalCPUTime(pids);
  const startedAt = now();
  await sleep(durationMs);
  const cpuEnd = totalCPUTime(pids);
  const windowMs = now() - startedAt;
  if (cpuStart === null || cpuEnd === null) return { cpuPercent: null, windowMs };
  return {
    cpuPercent: Math.max(0, (cpuEnd - cpuStart) / windowMs * 100),
    windowMs,
  };
}
