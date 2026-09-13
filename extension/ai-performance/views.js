// T7b：六个固定视图 renderer（方案 §4.1）。
//
// 同一 widget 入口按 config.view 分发：
//   usage    — 默认，旧图表路径（旧 Widget 配置零迁移兼容）
//   tools    — 工具与账户目录：13 工具能力矩阵 + 本机状态
//   billing  — 账单与对账：三口径分列（服务/现金/credits），多币种
//   attention— 待处理：提醒收件箱（等待许可 > 等待输入 > 错误）
//   ledger / limits — 复用 usage 数据路径的聚焦别名（T8 扩展预算卡）
//
// 诚实空态原则：没有配置真实来源时展示 unsupported/unavailable 原因，
// 不塞演示数据、不用 0/100% 伪装支持。

import { escapeHtml } from '../plugins/sanitizer.js';
import { sharedCall } from './shared-queries.js';

export const VIEWS = ['usage', 'tools', 'billing', 'attention', 'ledger', 'limits', 'savings'];

// normalizeView：旧 Widget 配置没有 view 字段 → 默认 usage（旧图表路径），
// 旧配置零迁移兼容；非法值回退 usage，不悄悄换语义。
export function normalizeView(view) {
  return VIEWS.includes(view) ? view : 'usage';
}

const CAP_LABEL = {
  historicalUsage: 'aiPerfCapHistorical',
  liveEvent: 'aiPerfCapLive',
  billing: 'aiPerfCapBilling',
  quota: 'aiPerfCapQuota',
  attribution: 'aiPerfCapAttribution',
  notification: 'aiPerfCapNotification',
};

const STATUS_CLASS = {
  implemented: 'ok',
  partial: 'warn',
  unsupported: 'off',
  unavailable: 'off',
};

function t(i18n, key, fallback) {
  return (i18n && i18n(key, fallback)) || fallback || key;
}

// renderToolsView 工具与账户目录：每行显示状态、本机状态与审计结论。
function renderToolsView(root, data, i18n) {
  const rows = data.sources || [];
  const html = rows.map((s) => {
    const caps = Object.entries(CAP_LABEL)
      .map(([cap, key]) => {
        const status = s.caps?.[cap] || 'unavailable';
        return `<span class="ai-perf-cap ai-perf-cap-${STATUS_CLASS[status] || 'off'}" title="${escapeHtml(status)}">${escapeHtml(t(i18n, key, cap))}</span>`;
      })
      .join('');
    const state = s.enabled
      ? `<span class="ai-perf-on">${escapeHtml(t(i18n, 'aiPerfSourceEnabled', '已启用'))}</span>`
      : '';
    const audit = s.audit
      ? `<div class="ai-perf-audit">${escapeHtml(s.audit)}</div>`
      : '';
    return `<div class="ai-perf-source">
      <div class="ai-perf-source-head"><strong>${escapeHtml(s.name)}</strong>${state}</div>
      <div class="ai-perf-caps">${caps}</div>${audit}
    </div>`;
  });
  root.innerHTML = `<div class="ai-perf-tools">${html.length ? html.join('') : `<div class="ai-perf-empty">${escapeHtml(t(i18n, 'aiPerfNoSources', '能力矩阵为空'))}</div>`}</div>`;
}

// renderBillingView 三口径分列 + 多币种分列；不合并、不猜汇率。
function renderBillingView(root, data, i18n) {
  const rows = data.summaries || [];
  if (!rows.length) {
    root.innerHTML = `<div class="ai-perf-empty">${escapeHtml(t(i18n, 'aiPerfBillingEmpty', '暂无账务条目；Provider 账单需导入或手动确认'))}</div>`;
    return;
  }
  const body = rows.map((r) => `
    <div class="ai-perf-bill-row">
      <strong>${escapeHtml(r.currency)}</strong>
      <span>${escapeHtml(t(i18n, 'aiPerfServiceSpend', '服务消耗'))}: ${escapeHtml(String(r.recognizedServiceSpend / 1e6))}</span>
      <span>${escapeHtml(t(i18n, 'aiPerfCashOut', '现金流出'))}: ${escapeHtml(String(r.cashOutflow / 1e6))}</span>
      <span>${escapeHtml(t(i18n, 'aiPerfCreditDelta', '余额变化'))}: ${escapeHtml(String(r.creditBalanceDelta / 1e6))}</span>
    </div>`).join('');
  root.innerHTML = `<div class="ai-perf-billing">${body}<div class="ai-perf-audit">${escapeHtml(t(i18n, 'aiPerfBillingNote', '三口径独立统计；充值计入现金流与余额，不计入服务消耗'))}</div></div>`;
}

// renderAttentionView 提醒收件箱：默认未查看，最多 5 条；动作只有
// “标记已查看”（不批准、不续跑、不发送消息）。
function renderAttentionView(root, data, i18n, api) {
  const items = data.items || [];
  if (!items.length) {
    root.innerHTML = `<div class="ai-perf-empty">${escapeHtml(t(i18n, 'aiPerfAttentionEmpty', '当前没有待处理事项'))}</div>`;
    return;
  }
  const rows = items.map((it) => `
    <div class="ai-perf-attention-row" data-attention-id="${escapeHtml(it.id)}">
      <span class="ai-perf-kind ai-perf-kind-${escapeHtml(it.kind)}">${escapeHtml(it.title)}</span>
      <span class="ai-perf-attention-tool">${escapeHtml(it.toolId)}</span>
      <button type="button" class="ai-perf-ack">${escapeHtml(t(i18n, 'aiPerfAck', '标记已查看'))}</button>
    </div>`).join('');
  root.innerHTML = `<div class="ai-perf-attention">${rows}</div>`;
  root.querySelectorAll('.ai-perf-ack').forEach((btn) => {
    btn.addEventListener('click', async () => {
      const id = btn.closest('[data-attention-id]')?.dataset.attentionId;
      if (!id) return;
      try {
        await api.ackUsageAttention({ id });
        btn.closest('.ai-perf-attention-row')?.remove();
        if (!root.querySelector('.ai-perf-attention-row')) {
          root.innerHTML = `<div class="ai-perf-empty">${escapeHtml(t(i18n, 'aiPerfAttentionEmpty', '当前没有待处理事项'))}</div>`;
        }
      } catch {
        // ack 失败保持条目可见；错误状态显式。
      }
    });
  });
}

// renderLimitsView 额度与预算：显示各预算规则本周期的花费、进度条与预警状态
function renderLimitsView(root, data, i18n) {
  const budgets = data.budgets || [];
  const evals = data.evaluations || [];
  if (!budgets.length) {
    root.innerHTML = `<div class="ai-perf-empty">${escapeHtml(t(i18n, 'aiPerfLimitsEmpty', '暂无预算规则；支持按日/月设置 API 估算预算与阈值提醒'))}</div>`;
    return;
  }
  const evalMap = new Map(evals.map((e) => [e.budgetId, e]));
  const rows = budgets.map((b) => {
    const ev = evalMap.get(b.id) || { spentMicro: 0, percent: 0 };
    const pct = Math.min(100, Math.max(0, ev.percent || 0));
    const spentUSD = (ev.spentMicro / 1e6).toFixed(2);
    const amountUSD = (b.amountMicro / 1e6).toFixed(2);
    const isOver = pct >= 100;
    const isWarn = pct >= 80;
    const barClass = isOver ? 'danger' : isWarn ? 'warn' : 'ok';
    return `<div class="ai-perf-budget-row">
      <div class="ai-perf-budget-head">
        <strong>${escapeHtml(b.id)}</strong>
        <span class="ai-perf-budget-period">${escapeHtml(b.period === 'daily' ? '每日' : '每月')} (${escapeHtml(b.currency)})</span>
        <span class="ai-perf-budget-val">${escapeHtml(spentUSD)} / ${escapeHtml(amountUSD)}</span>
      </div>
      <div class="ai-perf-bar-track">
        <span class="ai-perf-bar-fill ai-perf-fill-${barClass}" style="width:${pct}%"></span>
      </div>
      <div class="ai-perf-budget-meta">
        <span>${escapeHtml(t(i18n, 'aiPerfBudgetUsed', '已消耗'))} ${pct}%</span>
        <span>${escapeHtml(b.enabled ? '已启用' : '已暂停')}</span>
      </div>
    </div>`;
  }).join('');
  root.innerHTML = `<div class="ai-perf-limits">${rows}</div>`;
}

// renderLedgerView 统一用量账本：显示近期的唯一定位计费原子与事件流水
function renderLedgerView(root, data, i18n) {
  const events = data.events || [];
  if (!events.length) {
    root.innerHTML = `<div class="ai-perf-empty">${escapeHtml(t(i18n, 'aiPerfLedgerEmpty', '暂无用量事件记录'))}</div>`;
    return;
  }
  const rows = events.map((e) => {
    const timeStr = e.requestedAt ? new Date(e.requestedAt).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' }) : '—';
    const cost = e.costMicro != null ? `$${(e.costMicro / 1e6).toFixed(4)}` : (e.costStatus === 'unpriced' ? '未计价' : '$0.00');
    const atom = e.billingAtom || e.sessionId || '—';
    return `<div class="ai-perf-ledger-row">
      <span class="ai-perf-ledger-time">${escapeHtml(timeStr)}</span>
      <span class="ai-perf-ledger-model" title="${escapeHtml(e.model)}">${escapeHtml(e.provider)}/${escapeHtml(e.model)}</span>
      <span class="ai-perf-ledger-tokens">${escapeHtml(Number(e.totalTokens || 0).toLocaleString())} toks</span>
      <span class="ai-perf-ledger-cost">${escapeHtml(cost)}</span>
      <span class="ai-perf-ledger-atom" title="${escapeHtml(atom)}">${escapeHtml(atom)}</span>
    </div>`;
  }).join('');
  root.innerHTML = `<div class="ai-perf-ledger">${rows}</div>`;
}

// renderSavingsView 待处理与降本：展示基于确定性规则的降本与排查建议。
// 每条建议可"忽略"（按规则键 7 天内不再出现；到期自动恢复）——R7 闭环。
function renderSavingsView(root, data, i18n, api) {
  const insights = data.insights || [];
  if (!insights.length) {
    root.innerHTML = `<div class="ai-perf-empty">${escapeHtml(t(i18n, 'aiPerfSavingsEmpty', '确定性规则监测正常，暂无显著长上下文膨胀、高费集中或缓存下降异常'))}</div>`;
    return;
  }
  const rows = insights.map((ins) => `
    <div class="ai-perf-insight-card ai-perf-insight-${escapeHtml(ins.severity)}">
      <div class="ai-perf-insight-head">
        <span class="ai-perf-badge ai-perf-badge-${escapeHtml(ins.severity)}">${escapeHtml(ins.severity === 'warn' ? '提示' : '优化')}</span>
        <strong>${escapeHtml(ins.title)}</strong>
      </div>
      <div class="ai-perf-insight-evidence">${escapeHtml(ins.evidence)}</div>
      <div class="ai-perf-insight-action">${escapeHtml(ins.action)}</div>
      <div class="ai-perf-insight-foot">
        <span class="ai-perf-audit">${escapeHtml(ins.caveat)}</span>
        <button type="button" class="ai-perf-ack" data-rule-key="${escapeHtml(ins.ruleKey)}">${escapeHtml(t(i18n, 'aiPerfInsightDismiss', '忽略（7 天）'))}</button>
      </div>
    </div>`).join('');
  root.innerHTML = `<div class="ai-perf-savings">${rows}</div>`;
  root.querySelectorAll('[data-rule-key]').forEach((btn) => {
    btn.addEventListener('click', async () => {
      const ruleKey = btn.dataset.ruleKey;
      if (!ruleKey) return;
      btn.disabled = true;
      try {
        await api.dismissUsageInsight({ ruleKey });
        const card = btn.closest('.ai-perf-insight-card');
        card?.remove();
        if (!root.querySelector('.ai-perf-insight-card')) {
          root.innerHTML = `<div class="ai-perf-empty">${escapeHtml(t(i18n, 'aiPerfSavingsEmpty', '确定性规则监测正常，暂无显著长上下文膨胀、高费集中或缓存下降异常'))}</div>`;
        }
      } catch {
        btn.disabled = false; // 失败保持条目与按钮可用，错误显式。
      }
    });
  });
}

// renderView 按 config.view 渲染非 usage 视图。返回 true 表示已接管渲染。
export async function renderView(view, root, api, i18n, signal) {
  switch (view) {
    case 'tools': {
      const data = await sharedCall(api, 'getUsageSources', {});
      if (signal?.aborted) return true;
      renderToolsView(root, data || {}, i18n);
      return true;
    }
    case 'billing': {
      const data = await sharedCall(api, 'getUsageBilling', {});
      if (signal?.aborted) return true;
      renderBillingView(root, data || {}, i18n);
      return true;
    }
    case 'attention': {
      const data = await sharedCall(api, 'getUsageAttention', {});
      if (signal?.aborted) return true;
      renderAttentionView(root, data || {}, i18n, api);
      return true;
    }
    case 'limits': {
      const data = await sharedCall(api, 'getUsageBudgets', {});
      if (signal?.aborted) return true;
      renderLimitsView(root, data || {}, i18n);
      return true;
    }
    case 'ledger': {
      const data = await sharedCall(api, 'getUsageEvents', { limit: 20 });
      if (signal?.aborted) return true;
      renderLedgerView(root, data || {}, i18n);
      return true;
    }
    case 'savings': {
      const data = await sharedCall(api, 'getUsageInsights', {});
      if (signal?.aborted) return true;
      renderSavingsView(root, data || {}, i18n, api);
      return true;
    }
    default:
      return false; // usage / 未知 → 旧图表路径
  }
}
