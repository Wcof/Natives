// 投资卡端到端验证（ADR-0036 / 方案 03 P0 决策记录）：
// 真实 natives-app-runtime 进程 + 真实 fund 模块 + 隔离测试数据。
//
// 覆盖场景（P0 退出条件）：
//   1. readiness 门控：缺库时 data_status.readiness=uninitialized；
//      首次 start（等同用户进入投资详情初始化）后第二次连接 readiness=ready。
//   2. app:card_snapshot 投影数值与持仓回放对账（隔离测试数据）。
//   3. 单实例锁竞争：Ready 实例存在时第二个进程握手收到 APP_RUNNING_ELSEWHERE，
//      释放后可接管。
//   4. 20 次开关循环（进程级往返）与每次 EOF ≤2 秒退出。
//
// 说明：这是进程级证据。真实 Chrome 页面往返（space.html 打开→卡片渲染→
// 关闭）需要用户本机浏览器操作，不在本脚本内；页面级查询去重由
// extension/space-investment-feed.test.mjs 覆盖。
//
// 运行：node scripts/apps/investment-card-e2e.mjs
// 数据：全部使用 mkdtemp 临时目录，不触碰用户数据。

import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdtempSync, readFileSync, mkdirSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(fileURLToPath(new URL('../..', import.meta.url)));
const RUNTIME_BIN = process.env.NATIVES_RUNTIME_BIN ? resolve(process.env.NATIVES_RUNTIME_BIN) : join(ROOT, 'target/debug/natives-app-runtime');
const ORIGIN = 'chrome-extension://abcdefghijklmnopabcdefghijklmnop/';

function setupAppsRoot(root) {
  const appsRoot = join(root, 'apps');
  const runtimeSha = createHash('sha256').update(readFileSync(RUNTIME_BIN)).digest('hex');
  const act = {
    receiptVersion: 3,
    productVersion: '0.1.0',
    productGeneration: 1,
    productManifestSha256: '0'.repeat(64),
    runtime: { host: 'com.natives.local.app_runtime', protocolVersion: 2, sha256: runtimeSha },
    modules: {
      fund: { moduleApiVersion: 1, dataSchemaVersion: 6, capabilityVersion: 3, uiTreeSha256: '0'.repeat(64) },
      tokenusage: { moduleApiVersion: 1, dataSchemaVersion: 2, capabilityVersion: 2, uiTreeSha256: '0'.repeat(64) },
    },
    allowedOrigins: [ORIGIN],
  };
  mkdirSync(join(root, 'product'), { recursive: true });
  writeFileSync(join(root, 'product/activation.json'), JSON.stringify(act));
  mkdirSync(join(appsRoot, 'fund'), { recursive: true });
  return appsRoot;
}

function nativePort(binary, args, env) {
  const child = spawn(binary, args, { env: { ...process.env, ...env }, stdio: ['pipe', 'pipe', 'pipe'] });
  const pending = new Map();
  let buffer = Buffer.alloc(0);
  let sequence = 0;
  let stderr = '';
  const exit = new Promise((resolveExit) => child.once('exit', (code, signal) => resolveExit({ code, signal })));
  child.stderr.on('data', (chunk) => { stderr = (stderr + chunk).slice(-2048); if (process.env.E2E_STDERR) process.stderr.write(chunk); });
  const fail = (error) => { for (const item of pending.values()) item.reject(error); pending.clear(); };
  child.once('exit', () => fail(new Error('native port closed')));
  child.stdout.on('data', (chunk) => {
    buffer = Buffer.concat([buffer, chunk]);
    while (buffer.length >= 4 && buffer.length >= buffer.readUInt32LE(0) + 4) {
      const length = buffer.readUInt32LE(0);
      const message = JSON.parse(buffer.subarray(4, 4 + length));
      buffer = buffer.subarray(4 + length);
      const item = pending.get(message.id);
      if (item) { pending.delete(message.id); item.resolve(message); }
    }
  });
  async function call(method, params = {}) {
    const id = `req-${++sequence}`;
    const bytes = Buffer.from(JSON.stringify({ id, method, params }));
    const header = Buffer.alloc(4);
    header.writeUInt32LE(bytes.length);
    const response = await new Promise((resolveMessage, reject) => {
      const timer = setTimeout(() => { pending.delete(id); reject(new Error(`timeout: ${method}`)); }, 15_000);
      pending.set(id, { resolve: resolveMessage, reject });
      child.stdin.write(Buffer.concat([header, bytes]));
    });
    if (!response.ok) {
      const error = new Error(response.error?.message || String(response.error));
      error.code = response.error?.code;
      throw error;
    }
    return response.result;
  }
  return { child, call, exit, get stderr() { return stderr; } };
}

const EOF_LIMIT_MS = 2000;
async function stopWithEof(rt, label) {
  const t0 = process.hrtime.bigint();
  rt.child.stdin.end();
  const exit = await Promise.race([rt.exit, new Promise((r) => setTimeout(() => r({ slow: true }), EOF_LIMIT_MS + 500))]);
  assert.ok(!exit.slow, `${label}: EOF 退出超时`);
  const elapsedMs = Number(process.hrtime.bigint() - t0) / 1e6;
  assert.ok(elapsedMs <= EOF_LIMIT_MS, `${label}: EOF 退出 ${elapsedMs.toFixed(0)}ms 超出 ${EOF_LIMIT_MS}ms`);
  return elapsedMs;
}

async function main() {
  const temp = mkdtempSync(join(tmpdir(), 'natives-card-e2e-'));
  try {
    const appsRoot = setupAppsRoot(temp);
    const env = { NATIVES_APPS_ROOT: appsRoot };

    // --- 场景 1：缺库 readiness=uninitialized；start 初始化并写入测试数据 ---
    {
      const rt = nativePort(RUNTIME_BIN, [ORIGIN], env);
      const hs = await rt.call('app:handshake', { protocolVersion: 2, appId: 'fund', productVersion: '0.1.0', activationGeneration: 1 });
      assert.equal(hs.appId, 'fund');
      const status = await rt.call('app:data_status', {});
      assert.equal(status.readiness, 'uninitialized', JSON.stringify(status));
      const started = await rt.call('app:start', {});
      assert.equal(started.state, 'ready');
      const token = await issueToken(rt, started);
      await seedPositions(started.port, token);
      const elapsed = await stopWithEof(rt, 'scenario1');
      console.log(`scenario1: readiness gate + seed + EOF exit ${elapsed.toFixed(0)}ms (≤${EOF_LIMIT_MS}ms)`);
    }

    // --- 场景 2：第二次连接 readiness=ready，投影数值与 /api/positions 对账 ---
    {
      const rt = nativePort(RUNTIME_BIN, [ORIGIN], env);
      await rt.call('app:handshake', { protocolVersion: 2, appId: 'fund', productVersion: '0.1.0', activationGeneration: 1 });
      const status = await rt.call('app:data_status', {});
      assert.equal(status.readiness, 'ready', JSON.stringify(status));
      const started = await rt.call('app:start', {});
      const token = await issueToken(rt, started);
      const positions = await httpJson(started.port, token, 'GET', '/api/positions');
      const snap = await rt.call('app:card_snapshot', { instanceId: started.instanceId, generation: started.generation });
      assert.equal(snap.schema, 'fund.cards.v1');
      const holdings = snap.payload.registeredFundHoldings;
      const inHold = positions.positions.filter((p) => Number(p.quantity) > 0);
      assert.equal(holdings.positionCount, inHold.length, JSON.stringify({ holdings, positions }));
      assert.equal(holdings.accountCount, new Set(inHold.map((p) => p.account)).size);
      // 已清仓的 000001（份额 0）不得计入成本。
      const totalCostCents = inHold.reduce((acc, p) => acc + Math.round(Number(p.cost) * 100), 0);
      assert.equal(holdings.cost.complete, true);
      assert.equal(holdings.cost.total.amount, (totalCostCents / 100).toFixed(2).replace(/\.00$/, '').replace(/(\.\d)0$/, '$1'), `cost ${holdings.cost.total.amount} vs ${totalCostCents / 100}`);
      // 持仓 110022(123.45) + 161725(176.00) + 519674(105.00) = 404.45
      assert.equal(holdings.cost.total.amount, '404.45');
      assert.ok(snap.generatedAt.endsWith('+08:00'));
      const elapsed = await stopWithEof(rt, 'scenario2');
      console.log(`scenario2: snapshot reconciled (positions=${holdings.positionCount}, accounts=${holdings.accountCount}, cost=${holdings.cost.total.amount}), EOF ${elapsed.toFixed(0)}ms`);
    }

    // --- 场景 3：锁竞争（另一实例 Ready 时第二进程 APP_RUNNING_ELSEWHERE）---
    {
      const rt1 = nativePort(RUNTIME_BIN, [ORIGIN], env);
      await rt1.call('app:handshake', { protocolVersion: 2, appId: 'fund', productVersion: '0.1.0', activationGeneration: 1 });
      await rt1.call('app:start', {});
      const rt2 = nativePort(RUNTIME_BIN, [ORIGIN], env);
      await assert.rejects(
        rt2.call('app:handshake', { protocolVersion: 2, appId: 'fund', productVersion: '0.1.0', activationGeneration: 1 }),
        (error) => error.code === 'APP_RUNNING_ELSEWHERE',
        '第二个进程必须收到 APP_RUNNING_ELSEWHERE',
      );
      await stopWithEof(rt1, 'scenario3-rt1');
      // 让出锁后 rt2 可以接管（空间诚实的「稍后重试」路径）。
      await rt2.call('app:handshake', { protocolVersion: 2, appId: 'fund', productVersion: '0.1.0', activationGeneration: 1 });
      const started2 = await rt2.call('app:start', {});
      assert.equal(started2.state, 'ready');
      await stopWithEof(rt2, 'scenario3-rt2');
      console.log('scenario3: single-instance lock competition + takeover after release');
    }

    // --- 场景 4：20 次开关循环（进程级往返）：每次都握手→读取→snapshot→EOF 退出 ---
    {
      const cycles = 20;
      const eofMs = [];
      for (let i = 0; i < cycles; i += 1) {
        const rt = nativePort(RUNTIME_BIN, [ORIGIN], env);
        await rt.call('app:handshake', { protocolVersion: 2, appId: 'fund', productVersion: '0.1.0', activationGeneration: 1 });
        const status = await rt.call('app:data_status', {});
        assert.equal(status.readiness, 'ready', JSON.stringify(status));
        const started = await rt.call('app:start', {});
        assert.equal(started.state, 'ready');
        // 每周期只做一次 card_snapshot：卡片数量不增加查询/进程。
        const snap = await rt.call('app:card_snapshot', { instanceId: started.instanceId, generation: started.generation });
        assert.equal(snap.schema, 'fund.cards.v1');
        eofMs.push(await stopWithEof(rt, `scenario4-cycle-${i + 1}`));
      }
      const sorted = [...eofMs].sort((a, b) => a - b);
      const p50 = sorted[Math.floor(sorted.length / 2)];
      const max = sorted.at(-1);
      console.log(`scenario4: ${cycles} open/close cycles, EOF p50=${p50.toFixed(0)}ms max=${max.toFixed(0)}ms (≤${EOF_LIMIT_MS}ms)`);
    }

    // --- 场景 5（P1）：人工资产录入 → 币种确认 → 投影联动 ---
    {
      const rt = nativePort(RUNTIME_BIN, [ORIGIN], env);
      await rt.call('app:handshake', { protocolVersion: 2, appId: 'fund', productVersion: '0.1.0', activationGeneration: 1 });
      const started = await rt.call('app:start', {});
      const token = await issueToken(rt, started);

      // 重复键：同账户同代码先股票后 ETF 被拒（唯一键不参与产品类型，方案 05 §3）。
      await httpJson(started.port, token, 'POST', '/api/assets', {
        account: '测试账户A', productType: 'stock', market: 'SSE', code: '600519', name: '贵州茅台',
        riskClass: 'equity', marketValue: '1500000.00', valuedAt: today(),
      });
      await assertRejects(
        httpJson(started.port, token, 'POST', '/api/assets', {
          account: '测试账户A', productType: 'etf', market: 'SSE', code: '600519', name: '贵州茅台ETF',
          riskClass: 'unknown', marketValue: '1.00', valuedAt: today(),
        }),
        /ASSET_DUPLICATE/,
      );
      // 零元现金有效；未来日期拒绝。
      await httpJson(started.port, token, 'POST', '/api/assets', {
        account: '测试账户A', productType: 'cash', name: '现金', riskClass: 'cash',
        marketValue: '0.00', valuedAt: today(),
      });
      await assertRejects(
        httpJson(started.port, token, 'POST', '/api/assets', {
          account: '测试账户A', productType: 'gold', name: '金条', riskClass: 'gold',
          marketValue: '1.00', valuedAt: '2099-01-01',
        }),
        /ASSET_INVALID/,
      );
      // 同账户在持旧基金同代码：未确认时 ASSET_OVERLAP_REVIEW；用户确认不同资产后可存。
      await assertRejects(
        httpJson(started.port, token, 'POST', '/api/assets', {
          account: '测试账户A', productType: 'stock', market: 'SSE', code: '110022', name: '消费行业（其他市场）',
          riskClass: 'unknown', marketValue: '100.00', valuedAt: today(),
        }),
        /ASSET_OVERLAP_REVIEW/,
      );
      await httpJson(started.port, token, 'POST', '/api/assets', {
        account: '测试账户A', productType: 'stock', market: 'SSE', code: '110022', name: '消费行业（其他市场）',
        riskClass: 'unknown', marketValue: '100.00', valuedAt: today(), confirmDistinct: true,
      });

      // 流水价格不是净值；没有独立净值时，即使确认币种也须保持缺口。
      const fundId = (await httpJson(started.port, token, 'GET', '/api/fund?code=110022')).id;
      let assets = await httpJson(started.port, token, 'GET', '/api/assets');
      assert.equal(assets.funds.find((f) => f.code === '110022').valueGap, 'missing_nav');
      await httpJson(started.port, token, 'POST', '/api/fund-asset-metadata', { fundId, navCurrency: 'CNY', riskClass: 'equity' });
      assets = await httpJson(started.port, token, 'GET', '/api/assets');
      const fund = assets.funds.find((f) => f.code === '110022');
      assert.equal(fund.value, null);
      assert.equal(fund.valueGap, 'missing_nav');
      assert.equal(assets.summary.completeValue, null);
      assert.equal(assets.summary.quality, 'incomplete');
      // 卡片投影与详情同样诚实地保留净值缺口。
      const snap = await rt.call('app:card_snapshot', { instanceId: started.instanceId, generation: started.generation });
      assert.equal(snap.payload.recordedInvestmentAssets.completeValue, null);

      rt.child.stdin.end();
      await stopWithEof(rt, 'scenario5');
      console.log('scenario5: manual assets CRUD + overlap review + missing NAV remains incomplete');
    }

    // --- 场景 6（P2）：目标进度 + 配置检查边界 ---
    {
      const rt = nativePort(RUNTIME_BIN, [ORIGIN], env);
      await rt.call('app:handshake', { protocolVersion: 2, appId: 'fund', productVersion: '0.1.0', activationGeneration: 1 });
      const started = await rt.call('app:start', {});
      const token = await issueToken(rt, started);

      // 目标比例总和非 100.0% 拒绝。
      await assertRejects(
        httpJson(started.port, token, 'POST', '/api/allocation', {
          targets: [
            { riskClass: 'equity', targetTenths: 500, lowerTenths: 400, upperTenths: 600 },
            { riskClass: 'gold', targetTenths: 200, lowerTenths: 100, upperTenths: 300 },
            { riskClass: 'fixed_income', targetTenths: 100, lowerTenths: 0, upperTenths: 200 },
            { riskClass: 'cash', targetTenths: 100, lowerTenths: 0, upperTenths: 200 },
            { riskClass: 'other', targetTenths: 200, lowerTenths: 100, upperTenths: 300 },
          ],
        }),
        /PLAN_INVALID/,
      );
      await httpJson(started.port, token, 'POST', '/api/allocation', {
        targets: [
          { riskClass: 'equity', targetTenths: 500, lowerTenths: 400, upperTenths: 600 },
          { riskClass: 'gold', targetTenths: 200, lowerTenths: 100, upperTenths: 300 },
          { riskClass: 'fixed_income', targetTenths: 100, lowerTenths: 0, upperTenths: 200 },
          { riskClass: 'cash', targetTenths: 100, lowerTenths: 0, upperTenths: 200 },
          { riskClass: 'other', targetTenths: 100, lowerTenths: 0, upperTenths: 200 },
        ],
      });
      // 缺少独立净值时，不得用成交价推算目标进度。
      await httpJson(started.port, token, 'POST', '/api/goal', { target: '20000.00', targetDate: '2027-12-31' });
      const goal = await httpJson(started.port, token, 'GET', '/api/goal');
      assert.equal(goal.progress.computable, false);
      const cash = await httpJson(started.port, token, 'GET', '/api/finance/cash');
      const cashAccount = cash.accounts.find((account) => account.accountName === '测试账户A');
      await httpJson(started.port, token, 'POST', '/api/finance/reserve', { accountId: cashAccount.accountId, amount: '0', purpose: '测试明确无预留' });
      // 配置检查：港股人工项 risk_class=unknown → unknown_class，不可计算且不显示「正常」。
      const alloc = await httpJson(started.port, token, 'GET', '/api/allocation');
      assert.equal(alloc.computable, false);
      assert.ok(alloc.reasonCodes.includes('unknown_class'));
      // 分类核实后，剩余净值缺口仍阻止配置判断。
      const fundId = (await httpJson(started.port, token, 'GET', '/api/fund?code=110022')).id;
      const assets = await httpJson(started.port, token, 'GET', '/api/assets');
      await httpJson(started.port, token, 'POST', '/api/fund-asset-metadata', { fundId, navCurrency: 'CNY', riskClass: 'equity' });
      const legacy = await httpJson(started.port, token, 'GET', '/api/legacy-assets');
      await httpJson(started.port, token, 'POST', '/api/assets/update', {
        expectedVersion: legacy.assets.find(m => m.name === '消费行业（其他市场）').version,
        id: assets.manual.find((m) => m.name === '消费行业（其他市场）').id,
        account: '测试账户A', productType: 'stock', market: 'SSE', code: '110022', name: '消费行业（其他市场）',
        riskClass: 'equity', marketValue: '100.00', valuedAt: today(),
      });
      const alloc2 = await httpJson(started.port, token, 'GET', '/api/allocation');
      assert.equal(alloc2.computable, false);
      assert.ok(alloc2.reasonCodes.includes('missing_nav'));
      // 卡片投影检查区与详情一致。
      const snap = await rt.call('app:card_snapshot', { instanceId: started.instanceId, generation: started.generation });
      assert.equal(snap.payload.allocationCheck.computable, false);
      assert.equal(snap.payload.goal.progress.computable, false);

      rt.child.stdin.end();
      await stopWithEof(rt, 'scenario6');
      console.log('scenario6: incomplete goal/allocation boundaries + card/detail consistency');
    }

    console.log('investment-card-e2e passed');
  } finally {
    rmSync(temp, { recursive: true, force: true });
  }
}

function today() {
  const d = new Date(Date.now() + 8 * 3600 * 1000);
  return d.toISOString().slice(0, 10);
}

async function assertRejects(promise, pattern, message) {
  try {
    await promise;
  } catch (error) {
    if (!pattern.test(String(error.message))) {
      throw new Error(`错误不匹配 ${pattern}: ${error.message}`);
    }
    return;
  }
  throw new Error(message || `应当拒绝: ${pattern}`);
}

async function issueToken(rt, started) {
  return rt.call('app:session', { instanceId: started.instanceId, op: 'issue', challenge: 'challenge-1234567890abcdefghij' })
    .then((issued) => issued.token);
}

async function httpJson(port, token, method, path, body) {
  const response = await fetch(`http://127.0.0.1:${port}${path}`, {
    method,
    headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}`, Origin: 'null' },
    body: body ? JSON.stringify(body) : undefined,
  });
  const text = await response.text();
  let json;
  try {
    json = text ? JSON.parse(text) : {};
  } catch {
    throw new Error(`${path} -> ${response.status}: 非 JSON 响应: ${text.slice(0, 200)}`);
  }
  if (!response.ok) throw new Error(`${path} -> ${response.status}: ${JSON.stringify(json)}`);
  return json;
}

// 隔离测试数据（方案 05 §6 例题口径）：A 账户 2 笔在持、B 账户 1 笔在持、
// B 账户 1 笔买入后清仓（不进持有成本）。
async function seedPositions(port, token) {
  await httpJson(port, token, 'POST', '/api/accounts', { name: '测试账户A' });
  await httpJson(port, token, 'POST', '/api/accounts', { name: '测试账户B' });
  await tx(port, token, '测试账户A', '110022', '易方达消费行业', 'BUY', '100.0000', '1.2345', '2026-09-01', 'seed-a-110022');
  await tx(port, token, '测试账户A', '161725', '招商中证白酒', 'BUY', '200.0000', '0.8800', '2026-09-05', 'seed-a-161725');
  await tx(port, token, '测试账户B', '519674', '银河创新成长', 'BUY', '50.0000', '2.1000', '2026-09-10', 'seed-b-519674');
  await tx(port, token, '测试账户B', '000001', '华夏成长混合', 'BUY', '100.0000', '1.0000', '2026-09-02', 'seed-b-000001');
  await tx(port, token, '测试账户B', '000001', '华夏成长混合', 'SELL', '100.0000', '1.0500', '2026-09-12', 'seed-b-000001-sell');
}

async function tx(port, token, account, code, name, type, quantity, price, date, requestId) {
  // 现行账本契约要求显式 fee（2 位）；种子数据零费用语义不变。
  await httpJson(port, token, 'POST', '/api/transactions', {
    account, fundCode: code, fundName: name, type, state: 'confirmed', quantity, price, fee: '0.00', tradeDate: date, requestId,
  });
}

main().catch((error) => {
  console.error('investment-card-e2e FAILED:', error.message);
  process.exit(1);
});
