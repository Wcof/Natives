// 基金持仓摘要卡（P0 / 方案 01 F1、04 §4、05 §4）。
//
// 主数字 = 在持基金笔数（不称总资产/净资产）；次信息 = 账户数、成本状态；
// 最近变动日期是持仓流水日期，不是净值日期。成本未知时不显示完整合计，
// 只显示「已知成本部分」与未知笔数（05 §4 P0）。

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

export const investmentHoldingsWidget = {
  key: 'widget/investmentHoldings',
  name: 'Investment Holdings Summary',
  // 投资卡无用户配置；defaultData 为结构占位（目录/冒烟门禁要求）。
  defaultData: {},
  description: 'Display the number, accounts, and cost status of recorded securities holdings',
  render(container, _data = {}, _display = {}, ctx = {}) {
    const t = ctx.t || ((key, fallback) => fallback || key);
    container.className = 'Widget InvestmentHoldings';

    const renderState = (state) => {
      const holdings = state.snapshot?.registeredFundHoldings || null;
      const badge = statusBadge(state, holdings, t);
      const notice = staleResultNotice(state, t);
      const { main, body } = renderCardShell(container, {
        title: t('invHoldingsTitle'),
        badge,
        t,
        detailLabel: t('invOpenHoldings'),
        detailView: 'holdings',
        notice,
      });
      liveRegion(container, badge.text);

      if ((state.status === 'loading' || state.status === 'idle') && !state.snapshot) {
        // 稳定高度占位：加载不使相邻卡跳动（04 旅程 A）。
        main.innerHTML = `<span class="inv-main-sub">${t('invHoldingsLoading')}</span>`;
        return;
      }
      if (!holdings) {
        main.textContent = t('invStatusUnavailable');
        return;
      }
      if (holdings.quality === 'empty') {
        main.innerHTML = `<span class="inv-main-sub">${t('invHoldingsEmpty')}</span>`;
        return;
      }

      const count = Number(holdings.positionCount) || 0;
      const accountCount = Number(holdings.accountCount) || 0;
      const cost = holdings.cost || null;
      const moneyVisible = Boolean(state.moneyVisible);

      // 主数字：在持笔数；成本位置独立显示，不冒充总资产。
      main.textContent = `${count} ${t('invUnitPositions')}`;
      const costLine = document.createElement('span');
      costLine.className = 'inv-main-sub';
      if (!cost) {
        costLine.textContent = t('invCostMissing');
      } else if (cost.complete) {
        costLine.textContent = `${t('invKnownCost')} ${moneyText(cost.total?.amount, moneyVisible)}`;
      } else {
        const unknown = Number(cost.unknownCount) || 0;
        costLine.textContent = `${t('invPartialCost')} ${moneyText(cost.known?.amount, moneyVisible)} · ${t('invCostUnknownCount').replace('{n}', String(unknown))}`;
      }
      main.append(costLine);

      const dates = holdings.holdingDates || null;
      const bodyLines = [];
      const profit = state.snapshot?.portfolio?.unrealized?.amount;
      if (profit !== null && profit !== undefined) {
        bodyLines.push(`${t('invUnrealized')} ${moneyText(profit, moneyVisible)}`);
      }
      bodyLines.push(`${t('invAccountCount')} ${accountCount}`);
      if (dates?.latest) {
        bodyLines.push(`${t('invHoldingLatest')} ${dates.latest}`);
      }
      body.textContent = bodyLines.filter(Boolean).join(' · ');
    };

    return subscribeCard(container, renderState, ctx);
  },
  renderSettings(container, _data = {}, _onChange = {}, ctx = {}) {
    renderInvestmentSettings(container, ctx.t || ((k, f) => f || k));
  },
  styles: INVESTMENT_CARD_STYLES,
};
