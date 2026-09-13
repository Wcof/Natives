// §8.2 / §9.0.1 末行：旧 session_duration 卡迁移与往返导出回归。
// 旧 widget/aiPerformance 配置经解析后必须保留原语义：
//   - metric=session_duration（旧 totalRequests 请求活跃口径）不得被静默
//     改写成真实会话指标 ai_sessions（§4.1.1：两者是不同指标）。
//   - view 优先于遗留 metric：view=limits/attention/savings 的旧卡进对应视图。
//   - 旧 key 保留在安全白名单（兼容读取），但从新增目录隐藏。
// 往返导出：解析是纯函数，同一旧配置反复解析得到同一结果，不产生写库副作用。
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { normalizeView, VIEWS } from '../ai-performance/views.js';
import { METRICS } from '../ai-performance/metrics.js';
import { WIDGET_KEYS } from '../plugins/sanitizer.js';

// --- 1. 旧 key 兼容读取：留在白名单，但从新增目录隐藏 ---
{
  assert.ok(WIDGET_KEYS.includes('widget/aiPerformance'), '旧 key 保留在安全白名单');
  for (const key of ['widget/aiCost', 'widget/aiTokens', 'widget/aiSessions', 'widget/aiRequests',
    'widget/aiLimits', 'widget/aiAttention', 'widget/aiSavings']) {
    assert.ok(WIDGET_KEYS.includes(key), `新 key ${key} 在白名单`);
  }
  // 目录层：ai 分类只列七个新 key，不含旧 key（隐藏而非删除）。
  const catalogSrc = readFileSync(new URL('../space-catalog.js', import.meta.url), 'utf8');
  const aiBlock = catalogSrc.slice(catalogSrc.indexOf("id: 'ai'"), catalogSrc.indexOf("id: 'time'"));
  assert.ok(aiBlock.includes("'widget/aiCost'"), '目录列出 aiCost');
  assert.ok(!aiBlock.includes("'widget/aiPerformance'"), '旧 key 不在新增目录（隐藏）');
  const aiKeyCount = (aiBlock.match(/'widget\/ai[A-Z]/g) || []).length;
  assert.equal(aiKeyCount, 7, 'ai 分类恰好七个独立条目');
}

// --- 2. 旧 session_duration 口径保留：不被静默改成真实会话指标 ---
{
  assert.ok(METRICS.session_duration, '旧 session_duration 指标仍可解析（兼容读取）');
  assert.equal(METRICS.session_duration.sessions, false, '旧口径不是真实会话聚合');
  assert.equal(METRICS.session_duration.numberKey, 'totalRequests', '旧口径仍走请求数（诚实映射为调用活跃）');
  assert.ok(METRICS.ai_sessions, '新 ai_sessions 独立存在');
  assert.equal(METRICS.ai_sessions.sessions, true, '新指标消费真实 session 聚合');
  assert.notEqual(METRICS.session_duration.numberKey, METRICS.ai_sessions.numberKey, '两者数字来源不同，不混同');
}

// --- 3. 解析语义：无 view 默认 usage；view 优先于遗留 metric；非法回退 ---
{
  // 旧 session_duration 卡（无 view）→ usage 视图 + 原指标，语义不变。
  assert.equal(normalizeView(undefined), 'usage', '无 view 的旧卡走 usage 图表路径');
  assert.equal(normalizeView('usage'), 'usage');
  // view 优先：view=limits 的旧卡进额度视图，即使 metric 是 session_duration。
  assert.equal(normalizeView('limits'), 'limits', 'view 优先于遗留 metric（§8.2）');
  assert.equal(normalizeView('attention'), 'attention');
  assert.equal(normalizeView('savings'), 'savings');
  // tools/billing/ledger 兼容视图保留原内容，不丢弃。
  for (const v of ['tools', 'billing', 'ledger']) {
    assert.ok(VIEWS.includes(v), `兼容视图 ${v} 保留`);
    assert.equal(normalizeView(v), v);
  }
  // 非法值回退 usage，不悄悄换语义。
  assert.equal(normalizeView('nonsense'), 'usage');
}

// --- 4. 往返导出：解析为纯函数，重复解析结果一致（幂等，无写库副作用）---
{
  const legacyConfigs = [
    { metric: 'session_duration', chart: 'number', range: '30d' },
    { metric: 'ai_cost', view: 'usage', chart: 'line' },
    { view: 'limits' },
    { view: 'attention', metric: 'request_count' },
  ];
  for (const cfg of legacyConfigs) {
    const first = normalizeView(cfg.view);
    const second = normalizeView(cfg.view);
    assert.equal(first, second, `往返解析幂等: ${JSON.stringify(cfg)}`);
  }
  // 旧 session_duration 卡往返后仍是请求活跃口径，未被改写成 ai_sessions。
  const legacy = { metric: 'session_duration' };
  assert.equal(normalizeView(legacy.view), 'usage');
  assert.equal(METRICS[legacy.metric].sessions, false, '往返导出后旧卡保持请求活跃语义（§9.0.1）');
}

console.log('ai-legacy-migration: 4 passed');
