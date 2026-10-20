// 资产配置卡与配置检查卡（P2 / 方案 01 F3、04 §4、05 §4 P2）。
//
// 配置卡：最多 5 类的标签、百分比和金额（小空间用列表，不靠饼图识别）；
// 未知资产不悄悄并入「其他」。检查卡：只显示超过区间的类别数与一条最高
// 优先级事实；不能计算时显示待补信息数量；不出现「买入／卖出」按钮。

import {
  renderCardShell,
  statusBadge,
  staleResultNotice,
  subscribeCard,
  liveRegion,
  moneyText,
  INVESTMENT_CARD_STYLES,
  renderInvestmentSettings,
} from './investment-shared.js';

const RISK_LABELS = {
  equity: ['invRiskEquity', '权益'],
  gold: ['invRiskGold', '黄金'],
  fixed_income: ['invRiskFixedIncome', '固收'],
  cash: ['invRiskCash', '现金'],
  other: ['invRiskOther', '其他'],
};

export function riskClassLabel(riskClass, t) {
  const entry = RISK_LABELS[riskClass];
  if (entry) return t(entry[0], entry[1]);
  return t('invRiskUnknown');
}

function tenthToPercent(tenths) {
  const value = Number(tenths);
  if (!Number.isFinite(value)) return '';
  return `${(value / 10).toFixed(1)}%`;
}

export const investmentAllocationWidget = {
  key: 'widget/investmentAllocation',
  name: 'Asset Allocation',
  defaultData: {},
  description: 'Display the recorded asset allocation by primary risk class',
  render(container, _data = {}, _display = {}, ctx = {}) {
    const t = ctx.t || ((key, fallback) => fallback || key);
    container.className = 'Widget InvestmentAllocation';

    const renderState = (state) => {
      const groups = state.snapshot?.groupAllocation;
      const byGroup = Boolean(groups?.configured);
      const allocation = byGroup ? groups : state.snapshot?.allocation || null;
      const badge = statusBadge(state, allocation?.configured ? { quality: byGroup ? (groups.invalidRefCount > 0 ? 'incomplete' : groups.quality) : 'ready' } : null, t);
      const notice = staleResultNotice(state, t);
      const { main, body } = renderCardShell(container, {
        title: t('invAllocationTitle'),
        badge,
        t,
        detailLabel: t('invOpenAllocation'),
        detailView: 'allocation',
        notice,
      });
      liveRegion(container, badge.text);

      if ((state.status === 'loading' || state.status === 'idle') && !state.snapshot) {
        main.innerHTML = `<span class="inv-main-sub">${t('invAssetsLoading')}</span>`;
        return;
      }
      if (!allocation || !allocation.configured) {
        main.innerHTML = `<span class="inv-main-sub">${t('invAllocationNotSet')}</span>`;
        return;
      }
      if (!byGroup && !allocation.computable) {
        main.textContent = t('invAllocationNotComputable');
        const lines = [t('invReasonIncomplete')];
        const unknown = Number(allocation.unknownClassCount) || 0;
        if (unknown > 0) {
          lines.push(t('invUnknownClassCount').replace('{n}', String(unknown)));
        }
        body.textContent = lines.join(' · ');
        return;
      }

      const moneyVisible = Boolean(state.moneyVisible);
      const classes = byGroup ? (Array.isArray(groups.groups) ? groups.groups : []) : (Array.isArray(allocation.classes) ? allocation.classes : []);
      main.textContent = t(byGroup ? 'invByGroup' : 'invByRiskClass');
      const list = document.createElement('ul');
      list.className = 'inv-list';
      for (const item of classes.slice(0, 6)) {
        const li = document.createElement('li');
        const label = document.createElement('span');
        label.className = 'inv-li-label';
        label.textContent = byGroup ? item.name : riskClassLabel(item.riskClass, t);
        const value = document.createElement('span');
        value.className = 'inv-li-value';
        const actual = item.actualTenths !== null && item.actualTenths !== undefined
          ? tenthToPercent(item.actualTenths)
          : '--';
        const amount = item.amount?.amount;
        value.textContent = amount !== null && amount !== undefined
          ? `${actual} · ${moneyText(amount, moneyVisible)}`
          : actual;
        li.append(label, value);
        list.append(li);
      }
      body.textContent = '';
      body.append(list);
      if (byGroup) {
        const notes = [];
        if (groups.unassigned) notes.push(`${t('invUnassigned')} ${moneyText(groups.unassigned.amount, moneyVisible)}`);
        if (groups.invalidRefCount > 0) notes.push(t('invInvalidBindings').replace('{n}', String(groups.invalidRefCount)));
        if (classes.length > 6) notes.push(t('invMoreGroups').replace('{n}', String(classes.length - 6)));
        const note = document.createElement('span');
        note.textContent = notes.join(' · ');
        body.append(note);
      }
    };

    return subscribeCard(container, renderState, ctx);
  },
  renderSettings(container, _data = {}, _onChange = {}, ctx = {}) {
    renderInvestmentSettings(container, ctx.t || ((k, f) => f || k));
  },
  styles: INVESTMENT_CARD_STYLES,
};

export const investmentAllocationCheckWidget = {
  key: 'widget/investmentAllocationCheck',
  name: 'Allocation Check',
  defaultData: {},
  description: 'Display whether the asset allocation deviates from user targets',
  render(container, _data = {}, _display = {}, ctx = {}) {
    const t = ctx.t || ((key, fallback) => fallback || key);
    container.className = 'Widget InvestmentAllocationCheck';

    const renderState = (state) => {
      const check = state.snapshot?.allocationCheck || null;
      const badge = statusBadge(state, check?.computable ? { quality: 'ready' } : null, t);
      const notice = staleResultNotice(state, t);
      const { main, body } = renderCardShell(container, {
        title: t('invCheckTitle'),
        badge,
        t,
        detailLabel: t('invOpenCheck'),
        detailView: 'allocation',
        notice,
      });
      liveRegion(container, badge.text);

      if ((state.status === 'loading' || state.status === 'idle') && !state.snapshot) {
        main.innerHTML = `<span class="inv-main-sub">${t('invAssetsLoading')}</span>`;
        return;
      }
      if (!check || !check.configured) {
        main.innerHTML = `<span class="inv-main-sub">${t('invAllocationNotSet')}</span>`;
        return;
      }
      if (!check.computable) {
        main.textContent = t('invCheckNotComputable');
        const reasons = [];
        const unknown = Number(check.unknownClassCount) || 0;
        if (unknown > 0) {
          reasons.push(t('invUnknownClassCount').replace('{n}', String(unknown)));
        }
        for (const code of check.reasonCodes || []) {
          if (code === 'missing_nav') reasons.push(t('invMissingNavShort'));
          if (code === 'stale') reasons.push(t('invReasonStale'));
          if (code === 'currency_issue') reasons.push(t('invCurrencyIssueShort'));
          if (code === 'overlap_review') reasons.push(t('invOverlapShort'));
        }
        body.textContent = reasons.join(' · ');
        return;
      }

      const outOfRange = Array.isArray(check.outOfRange) ? check.outOfRange : [];
      if (!outOfRange.length) {
        main.textContent = t('invCheckAllWithin');
        body.textContent = t('invCheckFactOnly');
        return;
      }
      const count = outOfRange.length;
      main.textContent = t('invCheckOutOfRangeCount').replace('{n}', String(count));
      const first = outOfRange[0];
      const direction = first.direction === 'below'
        ? t('invCheckBelow')
        : t('invCheckAbove');
      body.textContent = `${riskClassLabel(first.riskClass, t)} ${direction}${first.actualTenths !== null && first.actualTenths !== undefined ? `（${tenthToPercent(first.actualTenths)}）` : ''}`;
    };

    return subscribeCard(container, renderState, ctx);
  },
  renderSettings(container, _data = {}, _onChange = {}, ctx = {}) {
    renderInvestmentSettings(container, ctx.t || ((k, f) => f || k));
  },
  styles: INVESTMENT_CARD_STYLES,
};
