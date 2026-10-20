// 已记录投资资产卡（P1 / 方案 01 F2、04 §4、05 §4 P1）。
//
// 数据齐全时显示人民币合计；否则显示「已计价部分」与待更新计数；
// 零元与未登记分开（empty 不显示 ¥0.00 伪装已核验总额）。
// 该合计只覆盖已登记投资资产，绝不推断未登记资产为零。

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

function pendingLabel(state, t) {
  const items = [];
  const assets = state.snapshot?.recordedInvestmentAssets || {};
  const missing = Number(assets.missingPriceCount) || 0;
  const currency = (Number(assets.currencyUnconfirmedCount) || 0)
    + (Number(assets.currencyUnsupportedCount) || 0);
  const overlap = Number(assets.overlapReviewCount) || 0;
  if (missing > 0) items.push(t('invMissingNavCount').replace('{n}', String(missing)));
  if (currency > 0) items.push(t('invCurrencyIssueCount').replace('{n}', String(currency)));
  if (overlap > 0) items.push(t('invOverlapCount').replace('{n}', String(overlap)));
  const oldVal = Number(assets.oldValuationCount) || 0;
  if (oldVal > 0) items.push(t('invOldValuationCount').replace('{n}', String(oldVal)));
  return items;
}

export const investmentAssetsWidget = {
  key: 'widget/investmentAssets',
  name: 'Recorded Investment Assets',
  // 投资卡无用户配置；defaultData 为结构占位（目录/冒烟门禁要求）。
  defaultData: {},
  description: 'Display the total and completeness of recorded investment assets',
  render(container, _data = {}, _display = {}, ctx = {}) {
    const t = ctx.t || ((key, fallback) => fallback || key);
    container.className = 'Widget InvestmentAssets';

    const renderState = (state) => {
      const assets = state.snapshot?.recordedInvestmentAssets || null;
      const badge = statusBadge(state, assets, t);
      const notice = staleResultNotice(state, t);
      const { main, body } = renderCardShell(container, {
        title: t('invAssetsTitle'),
        badge,
        t,
        detailLabel: t('invOpenAssets'),
        detailView: 'assets',
        notice,
      });
      liveRegion(container, badge.text);

      if ((state.status === 'loading' || state.status === 'idle') && !state.snapshot) {
        main.innerHTML = `<span class="inv-main-sub">${t('invAssetsLoading')}</span>`;
        return;
      }
      if (!assets) {
        main.textContent = t('invStatusUnavailable');
        return;
      }
      if (assets.quality === 'empty') {
        main.innerHTML = `<span class="inv-main-sub">${t('invAssetsEmpty')}</span>`;
        return;
      }

      const moneyVisible = Boolean(state.moneyVisible);
      const complete = assets.completeValue?.amount;
      const known = assets.knownValue?.amount;
      const mainEl = document.createElement('span');
      if (complete !== null && complete !== undefined) {
        mainEl.textContent = moneyText(complete, moneyVisible);
        if (assets.quality === 'stale') {
          const staleNote = document.createElement('span');
          staleNote.className = 'inv-main-sub';
          staleNote.textContent = t('invStaleHistory');
          mainEl.append(staleNote);
        }
      } else if (known !== null && known !== undefined) {
        mainEl.textContent = `${t('invPricedPart')} ${moneyText(known, moneyVisible)}`;
      } else {
        mainEl.textContent = t('invNothingPriced');
      }
      main.textContent = '';
      main.append(mainEl);

      const lines = pendingLabel(state, t);
      if (!lines.length) {
        const fundValue = assets.fundValue?.amount;
        const manualValue = assets.manualValue?.amount;
        if (fundValue !== null && fundValue !== undefined && manualValue !== null && manualValue !== undefined) {
          lines.push(`${t('invFundPart')} ${moneyText(fundValue, moneyVisible)} · ${t('invManualPart')} ${moneyText(manualValue, moneyVisible)}`);
        }
      }
      const dates = assets.valuationDates;
      const cash = state.snapshot?.cash;
      if (cash) {
        lines.push(`${t('invCash')} ${cash.cash ? moneyText(cash.cash.amount, moneyVisible) : t('invCashUnknown')}`);
        lines.push(`${t('invAvailableCash')} ${cash.available ? moneyText(cash.available.amount, moneyVisible) : t('invCashUnknown')}`);
      }
      if (dates?.earliest && dates?.latest) {
        lines.push(`${t('invValuationRange')} ${dates.earliest} ~ ${dates.latest}`);
      }
      body.textContent = lines.filter(Boolean).join(' · ');
    };

    return subscribeCard(container, renderState, ctx);
  },
  renderSettings(container, _data = {}, _onChange = {}, ctx = {}) {
    renderInvestmentSettings(container, ctx.t || ((k, f) => f || k));
  },
  styles: INVESTMENT_CARD_STYLES,
};
