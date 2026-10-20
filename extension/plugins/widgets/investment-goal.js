// 投资资产目标卡（P2 / 方案 01 F3、04 §4、05 §4 P2）。
//
// 进度 = 同口径已计价投资资产 ÷ 目标金额；数据不完整或估值过期时进度不可用，
// 显示原因而非零进度。目标已到期显示「已到期」，不自动预测达成时间；
// 超过 100% 显示真实比率，进度条视觉填充最多 100%。

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

export const investmentGoalWidget = {
  key: 'widget/investmentGoal',
  name: 'Investment Goal Progress',
  // 投资卡无用户配置；defaultData 为结构占位（目录/冒烟门禁要求）。
  defaultData: {},
  description: 'Display the progress of the recorded investment asset goal',
  render(container, _data = {}, _display = {}, ctx = {}) {
    const t = ctx.t || ((key, fallback) => fallback || key);
    container.className = 'Widget InvestmentGoal';

    const renderState = (state) => {
      const goal = state.snapshot?.goal || null;
      const badge = statusBadge(state, goal?.configured ? { quality: 'ready' } : null, t);
      const notice = staleResultNotice(state, t);
      const { main, body } = renderCardShell(container, {
        title: t('invGoalTitle'),
        badge,
        t,
        detailLabel: t('invOpenGoal'),
        detailView: 'goal',
        notice,
      });
      liveRegion(container, badge.text);

      if ((state.status === 'loading' || state.status === 'idle') && !state.snapshot) {
        main.innerHTML = `<span class="inv-main-sub">${t('invAssetsLoading')}</span>`;
        return;
      }
      if (!goal || !goal.configured) {
        main.innerHTML = `<span class="inv-main-sub">${t('invGoalNotSet')}</span>`;
        return;
      }

      const moneyVisible = Boolean(state.moneyVisible);
      const target = goal.targetAmount?.amount;
      const dateLine = goal.targetDate
        ? (goal.expired ? `${t('invGoalExpired')} ${goal.targetDate}` : goal.targetDate)
        : '';
      const progress = goal.progress || null;
      if (!progress?.computable) {
        main.textContent = t('invGoalNotComputable');
        const reasons = [];
        if (progress?.reasonCodes?.includes('assets_empty')) {
          reasons.push(t('invAssetsEmpty'));
        }
        if (progress?.reasonCodes?.includes('incomplete')) {
          reasons.push(t('invReasonIncomplete'));
        }
        if (progress?.reasonCodes?.includes('stale')) {
          reasons.push(t('invReasonStale'));
        }
        body.textContent = [dateLine, ...reasons].filter(Boolean).join(' · ');
        return;
      }

      const ratio = String(progress.ratioPercent ?? '');
      main.textContent = ratio ? `${ratio}%` : '';
      const currentValue = progress.currentValue?.amount;
      if (currentValue !== null && currentValue !== undefined) {
        const sub = document.createElement('span');
        sub.className = 'inv-main-sub';
        sub.textContent = `${moneyText(currentValue, moneyVisible)} / ${moneyText(target, moneyVisible)}`;
        main.append(sub);
      }
      // 进度条填充最多 100%，文字保留真实比率（05 §4 P2）。
      const ratioValue = Math.min(100, Math.max(0, parseFloat(ratio) || 0));
      const track = document.createElement('div');
      track.className = 'inv-progress';
      track.setAttribute('role', 'progressbar');
      track.setAttribute('aria-valuemin', '0');
      track.setAttribute('aria-valuemax', '100');
      track.setAttribute('aria-valuenow', String(Math.round(ratioValue)));
      track.setAttribute('aria-label', `${t('invGoalProgress')} ${ratio}%`);
      const fill = document.createElement('div');
      fill.className = 'inv-progress-fill';
      fill.style.width = `${ratioValue}%`;
      track.append(fill);
      body.textContent = dateLine;
      body.append(track);
    };

    return subscribeCard(container, renderState, ctx);
  },
  renderSettings(container, _data = {}, _onChange = {}, ctx = {}) {
    renderInvestmentSettings(container, ctx.t || ((k, f) => f || k));
  },
  styles: INVESTMENT_CARD_STYLES,
};
