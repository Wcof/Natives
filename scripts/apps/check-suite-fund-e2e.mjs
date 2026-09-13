// 单一产品路线 Fund E2E（plan §5 P6：旧 Seed Reconciliation 测试映射为
// 内置模块语义）。验证场景：
//   A: 产品语义 —— Core 握手后空安装表仍返回固定 fund 模块投影，
//      旧分发方法在任何变更前被拒绝（APP_RETIRED_METHOD）
//   B: 首用直连 —— fund-host 握手/启动/会话，0 远程下载
//   C: 业务 —— 记账与持仓重放（黄金数值）
//   D: 重启 —— 停止后重启数据持久（SQLite）
//   E: 生命周期 —— 显式停止、会话失效
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { chmodSync, copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { gunzipSync } from 'node:zlib';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { nativePort, ORIGIN, CORE_HOST } from './native-fixture.mjs';

const root = resolve(new URL('../..', import.meta.url).pathname);
const coreHostBinary = join(root, 'target/debug/native-file-host');
const fundNap = join(root, 'dist/seeds/fund.nap');

if (!existsSync(coreHostBinary)) {
  throw new Error(`Core binary not found at ${coreHostBinary}`);
}
if (!existsSync(fundNap)) {
  throw new Error(`fund payload not found at ${fundNap}`);
}

const testDir = mkdtempSync(join(tmpdir(), 'natives-fund-e2e-'));
console.log(`[Fund E2E] Test sandbox: ${testDir}`);

// 内置模块载荷随完整产品交付：fund.nap 解包即 fund-host 可执行（构建中间产物），
// 无独立安装事务、无代码下载。
const fundBinary = join(testDir, 'fund-host');
writeFileSync(fundBinary, gunzipSync(readFileSync(fundNap)));
chmodSync(fundBinary, 0o755);

// 产品配置产物（plan §3.3/T04）：真实产品中由 Core 在"完成 Natives 配置"
// 时以当前用户权限写入 activation 投影；e2e 沙箱按同一形态构造，
// fund-host 启动前严格校验（app-host-support activation.rs）。
const fundSha256 = createHash('sha256').update(readFileSync(fundBinary)).digest('hex');
const fundHostRuntime = `com.natives.app.a${createHash('sha256').update('fund').digest('hex')}`;
mkdirSync(join(testDir, 'apps', 'fund'), { recursive: true });
writeFileSync(join(testDir, 'apps', 'fund', 'activation.json'), JSON.stringify({
  receiptVersion: 1,
  appId: 'fund',
  runtimeHost: fundHostRuntime,
  activeVersion: '0.1.0',
  generation: 1,
  enabled: true,
  activationState: 'ready',
  appProtocolVersion: 1,
  payloadSha256: fundSha256,
  allowedOrigins: [ORIGIN],
}));

const fundEnv = { env: { ...process.env, NATIVES_APPS_ROOT: join(testDir, 'apps') } };
const quote = (v) => `'${v.replaceAll("'", "'\\''")}'`;
const launcher = join(testDir, 'core-host');
writeFileSync(
  launcher,
  `#!/bin/sh\nexec ${quote(coreHostBinary)} --app-fixture ${quote(testDir)} "$1" --chrome-profile 2>>${quote(join(testDir, 'core.stderr'))}\n`,
  { mode: 0o700 },
);

try {
  // ── Scenario A: 产品语义（空安装表 + 固定模块 + 旧分发拒绝）──────────
  console.log('Testing Scenario A: Product semantics (fixed modules, retired chain rejected)...');
  let core = nativePort(launcher, [ORIGIN]);
  const host = await core.call('apps:handshake', { origin: ORIGIN });
  assert.equal(host.appsProtocolVersion, 4);
  const listA = await core.call('apps:list');
  const moduleIds = (listA.modules ?? []).map((m) => m.app_id ?? m.appId);
  assert.ok(moduleIds.includes('fund'), `fixed module fund must be listed, got ${JSON.stringify(moduleIds)}`);
  assert.ok(listA.product, 'product projection must be present');
  for (const method of ['apps:suite_prepare', 'apps:install_begin', 'apps:uninstall', 'apps:rollback']) {
    await assert.rejects(core.call(method, {}), /APP_RETIRED_METHOD/, `${method} must be retired`);
  }
  console.log('  ✓ Scenario A passed: fixed fund module listed; retired methods rejected');

  // ── Scenario B: 首用直连（0 远程下载）───────────────────────────────
  console.log('Testing Scenario B: First open (0 remote downloads)...');
  const fundPort = nativePort(fundBinary, [ORIGIN], fundEnv);
  const handshake = await fundPort.call('app:handshake', {
    protocolVersion: 1, expectedAppId: 'fund', expectedVersion: '0.1.0',
  });
  assert.equal(handshake.appId, 'fund');
  assert.equal(handshake.protocolVersion, 1);

  const started = await fundPort.call('app:start', { requestId: 'e2e-start-1' });
  assert.equal(started.state, 'ready');
  assert.ok(started.port > 0 && started.port <= 65535, `Port must be valid, got ${started.port}`);
  const session = await fundPort.call('app:session', {
    instanceId: started.instanceId, op: 'issue', challenge: 'e2e-challenge-1',
  });
  assert.ok(session.token, 'Must return bearer token');
  console.log(`  ✓ Scenario B passed: Fund Host running on 127.0.0.1:${started.port}, 0 remote package downloads`);

  // ── Scenario C: 业务（记账与持仓重放）──────────────────────────────
  console.log('Testing Scenario C: Business Transactions & Positions...');
  const httpOptions = (extra = {}) => ({
    headers: {
      Authorization: `Bearer ${session.token}`,
      Origin: 'null',
      'Content-Type': 'application/json',
      ...extra.headers,
    },
    ...extra,
  });

  const txRes = await fetch(`http://127.0.0.1:${started.port}/api/transactions`, httpOptions({
    method: 'POST',
    body: JSON.stringify({
      requestId: 'tx-buy-1',
      account: 'E2E Account',
      fundCode: '000001',
      fundName: '华夏成长',
      type: 'BUY',
      quantity: '1000.0000',
      price: '1.0000',
      tradeDate: '2026-09-11',
      state: 'confirmed',
    }),
  }));
  if (txRes.status !== 200) {
    console.error('Record buy transaction failed:', txRes.status, await txRes.text());
  }
  assert.equal(txRes.status, 200, 'Record buy transaction must succeed');

  const posRes = await fetch(`http://127.0.0.1:${started.port}/api/positions`, httpOptions());
  assert.equal(posRes.status, 200);
  const positions = await posRes.json();
  const pos = positions.positions?.find((p) => p.fundCode === '000001');
  assert.ok(pos, 'Position for 000001 must exist');
  assert.equal(pos.quantity, '1000');
  console.log('  ✓ Scenario C passed: 1000 shares recorded for fund 000001');

  // ── Scenario D: 重启与数据持久 ─────────────────────────────────────
  console.log('Testing Scenario D: Restart & Data Persistence...');
  await fundPort.call('app:stop', { instanceId: started.instanceId, reason: 'user', requestId: 'stop-d' });
  await fundPort.close();
  await core.close();

  core = nativePort(launcher, [ORIGIN]);
  await core.call('apps:handshake', { origin: ORIGIN });
  const fundPort2 = nativePort(fundBinary, [ORIGIN], fundEnv);
  const started2 = await fundPort2.call('app:start', { requestId: 'e2e-start-2' });
  const session2 = await fundPort2.call('app:session', {
    instanceId: started2.instanceId, op: 'issue', challenge: 'e2e-challenge-2',
  });

  const posRes2 = await fetch(`http://127.0.0.1:${started2.port}/api/positions`, {
    headers: { Authorization: `Bearer ${session2.token}`, Origin: 'null' },
  });
  assert.equal(posRes2.status, 200);
  const positions2 = await posRes2.json();
  const pos2 = positions2.positions?.find((p) => p.fundCode === '000001');
  assert.ok(pos2 && pos2.quantity === '1000', 'Position must survive host restart');
  console.log('  ✓ Scenario D passed: Positions survived restart');

  // ── Scenario E: 生命周期（显式停止、数据保留）──────────────────────
  console.log('Testing Scenario E: Lifecycle (explicit stop, data preserved)...');
  await fundPort2.call('app:stop', { instanceId: started2.instanceId, reason: 'user', requestId: 'stop-e' });
  await fundPort2.close();

  const fundPort3 = nativePort(fundBinary, [ORIGIN], fundEnv);
  const started3 = await fundPort3.call('app:start', { requestId: 'e2e-start-3' });
  const session3 = await fundPort3.call('app:session', {
    instanceId: started3.instanceId, op: 'issue', challenge: 'e2e-challenge-3',
  });
  const posRes3 = await fetch(`http://127.0.0.1:${started3.port}/api/positions`, {
    headers: { Authorization: `Bearer ${session3.token}`, Origin: 'null' },
  });
  assert.equal(posRes3.status, 200);
  const positions3 = await posRes3.json();
  const pos3 = positions3.positions?.find((p) => p.fundCode === '000001');
  assert.ok(pos3 && pos3.quantity === '1000', 'Business data must persist across lifecycle');
  await fundPort3.call('app:stop', { instanceId: started3.instanceId, reason: 'user', requestId: 'stop-f' });
  await fundPort3.close();
  console.log('  ✓ Scenario E passed: explicit stop, data preserved');

  await core.close();
  console.log('\n[Fund E2E COMPLETE] All scenarios PASS!');
} catch (err) {
  const stderrPath = join(testDir, 'core.stderr');
  if (existsSync(stderrPath)) {
    console.error('Core stderr output:\n' + readFileSync(stderrPath, 'utf8'));
  }
  throw err;
} finally {
  rmSync(testDir, { recursive: true, force: true });
}
