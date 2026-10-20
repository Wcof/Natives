// CPU 探针回归检查：证明门禁测量口径（cpu-probe.mjs）能发现持续 CPU 占用。
// 若探针失效（永远测得 0%），Model Host 空闲门禁将无法发现真实回归——
// 本测试用受控忙循环子进程守护该不变量（R-P12：门禁不能自欺）。
import test from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { sampleCPUWindow, totalCPUTime } from './cpu-probe.mjs';

test('sampleCPUWindow detects a sustained busy loop above the 5% idle budget', async () => {
  const busy = spawn(process.execPath, ['-e', 'const t = Date.now() + 5000; while (Date.now() < t) { Math.sqrt(Date.now()); }'], {
    stdio: 'ignore',
  });
  try {
    // 忙循环启动需要一点时间才反映到 ps 的累计 CPU 时间上。
    await new Promise((resolve) => setTimeout(resolve, 300));
    const { cpuPercent } = await sampleCPUWindow([busy.pid], 1000);
    assert.notEqual(cpuPercent, null, 'busy-loop process must stay alive during sampling');
    assert.ok(
      cpuPercent > 5,
      `sustained busy loop must be detected above the 5% idle budget, got ${cpuPercent?.toFixed(2)}%`,
    );
  } finally {
    busy.kill('SIGKILL');
  }
});

test('sampleCPUWindow reports near-zero CPU for a sleeping process', async () => {
  const idle = spawn(process.execPath, ['-e', 'setTimeout(() => {}, 5000)'], { stdio: 'ignore' });
  try {
    await new Promise((resolve) => setTimeout(resolve, 300));
    const { cpuPercent } = await sampleCPUWindow([idle.pid], 500);
    assert.notEqual(cpuPercent, null, 'sleeping process must stay alive during sampling');
    assert.ok(cpuPercent <= 5, `sleeping process must stay within the 5% idle budget, got ${cpuPercent?.toFixed(2)}%`);
  } finally {
    idle.kill('SIGKILL');
  }
});

test('totalCPUTime returns null when a pid no longer exists', () => {
  assert.equal(totalCPUTime([999999999]), null);
});
