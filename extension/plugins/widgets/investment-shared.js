// 投资卡共享渲染层（ADR-0036 / 方案 04 §4-§7）。
//
// 只做展示：状态角标、金额遮蔽、刷新与详情入口按钮的统一结构。
// 业务数值全部来自 feed 的一次只读投影，不在此重算投资指标。

import {
  subscribeInvestmentCards,
  refreshInvestmentCards,
  openInvestmentDetail,
} from '../../space-investment-feed.js';

const MASKED_AMOUNT = '¥ ••••••';

// 金额文本：遮蔽态不把明文写进 DOM（含 aria），直接渲染占位。
// moneyVisible 来自 feed 页面级状态（04 §7：页面级统一遮蔽，内存态）。
export function moneyText(amount, moneyVisible) {
  if (amount === null || amount === undefined) return '';
  return moneyVisible ? `¥ ${amount}` : MASKED_AMOUNT;
}

export function formatMoney(amount) {
  if (amount === null || amount === undefined) return null;
  return `¥ ${amount}`;
}

// 状态角标：请求失败 > 完整性不足 > 估值过期 > 可用（方案 04 §6）。
export function statusBadge(state, aggregate, t) {
  if (state.status === 'loading' || state.status === 'idle') {
    // idle：投资卡已挂载但首次查询尚未发起（下一拍即转 loading）。
    return { key: 'loading', text: t('invStatusLoading') };
  }
  if (state.status === 'unavailable') {
    if (state.errorCode === 'APP_RUNNING_ELSEWHERE') {
      return { key: 'unavailable', text: t('invStatusElsewhere') };
    }
    if (state.errorCode === 'APP_DATA_NOT_READY') {
      const map = {
        uninitialized: t('invNeedInit'),
        migration_required: t('invNeedUpdate'),
        recovery_required: t('invNeedRecovery'),
        incompatible: t('invNeedUpgrade'),
        unreadable: t('invNeedCheck'),
      };
      return { key: 'unavailable', text: map[state.readiness] || map.unreadable };
    }
    if (state.errorCode === 'SUSPENDED') {
      // 主动让出（页面隐藏/打开投资详情）：不是查询失败，恢复可见时自动重查。
      return { key: 'unavailable', text: t('invStatusSuspended') };
    }
    return { key: 'unavailable', text: t('invStatusUnavailable') };
  }
  if (state.status === 'unsupported') {
    return { key: 'unavailable', text: t('invStatusUnsupported') };
  }
  if (aggregate) {
    if (aggregate.quality === 'empty') {
      return { key: 'empty', text: t('invStatusEmpty') };
    }
    if (aggregate.quality === 'incomplete') {
      return { key: 'incomplete', text: t('invStatusIncomplete') };
    }
    if (aggregate.quality === 'stale') {
      return { key: 'stale', text: t('invStatusStale') };
    }
    if (state.status === 'ready') {
      return { key: 'ready', text: t('invStatusReady') };
    }
  }
  return { key: 'ready', text: t('invStatusReady') };
}

// 刷新失败但本页有旧值：独立提示，不覆盖数据质量（方案 04 §6）。
// SUSPENDED 是主动让出而非失败，不出现在「更新失败」提示里。
export function staleResultNotice(state, t) {
  if (
    state.status === 'unavailable'
    && state.snapshot
    && state.fetchedAt
    && state.errorCode !== 'APP_DATA_NOT_READY'
    && state.errorCode !== 'CARD_SNAPSHOT_UNSUPPORTED'
    && state.errorCode !== 'SUSPENDED'
  ) {
    const time = new Date(state.fetchedAt).toLocaleTimeString();
    return t('invStaleNotice') + ` · ${time}`;
  }
  return null;
}

export function unsubscribeAll(unsubs) {
  for (const unsub of unsubs) {
    try {
      unsub?.();
    } catch { /* ignore */ }
  }
  unsubs.length = 0;
}

// 统一卡片骨架：header（标题+状态+刷新）→ main → body → footer（详情入口）。
export function renderCardShell(container, options) {
  const {
    title,
    badge,
    t,
    detailLabel,
    detailView,
    notice = null,
  } = options;
  container.classList.add('InvestmentCard');
  container.replaceChildren();

  const header = document.createElement('div');
  header.className = 'inv-card-header';
  const titleEl = document.createElement('h3');
  titleEl.className = 'inv-card-title';
  titleEl.textContent = title;
  const badgeEl = document.createElement('span');
  badgeEl.className = `inv-card-badge inv-badge-${badge.key}`;
  badgeEl.textContent = badge.text;
  const refreshBtn = document.createElement('button');
  refreshBtn.type = 'button';
  refreshBtn.className = 'inv-card-refresh';
  refreshBtn.title = t('invRefresh');
  refreshBtn.setAttribute('aria-label', t('invRefresh'));
  refreshBtn.innerHTML = '<svg aria-hidden="true" viewBox="0 0 24 24" width="14" height="14"><use href="#i-refresh" /></svg>';
  refreshBtn.addEventListener('click', () => refreshInvestmentCards());
  const badgeWrap = document.createElement('span');
  badgeWrap.className = 'inv-card-header-right';
  badgeWrap.append(badgeEl, refreshBtn);
  header.append(titleEl, badgeWrap);
  container.append(header);

  if (notice) {
    const noticeEl = document.createElement('p');
    noticeEl.className = 'inv-card-notice';
    noticeEl.textContent = notice;
    container.append(noticeEl);
  }

  const main = document.createElement('div');
  main.className = 'inv-card-main';
  container.append(main);
  const body = document.createElement('div');
  body.className = 'inv-card-body';
  container.append(body);

  if (detailView) {
    const footer = document.createElement('div');
    footer.className = 'inv-card-footer';
    const openBtn = document.createElement('button');
    openBtn.type = 'button';
    openBtn.className = 'inv-card-open';
    openBtn.textContent = detailLabel;
    openBtn.addEventListener('click', () => openDetailSafe(detailView));
    footer.append(openBtn);
    container.append(footer);
  }
  return { main, body, header };
}

// 详情入口独立成按钮（不做整卡嵌套按钮，方案 02 §5）。
function openDetailSafe(view) {
  try {
    openInvestmentDetail(view);
  } catch {
    /* 导航失败静默 */
  }
}

export function subscribeCard(container, render, ctx) {
  const renderState = (state) => {
    try {
      render(state);
    } catch {
      try {
        container.replaceChildren();
        const error = document.createElement('div');
        error.className = 'inv-card-error';
        error.textContent = ctx.t('invRenderError');
        container.append(error);
      } catch { /* ignore */ }
    }
  };
  const unsubscribe = subscribeInvestmentCards(renderState);
  return unsubscribe;
}

// aria-live 状态容器：加载/失败播报，金额重绘不重复播报（方案 04 §8）。
export function liveRegion(container, text) {
  let live = container.querySelector('.inv-live');
  if (!live) {
    live = document.createElement('span');
    live.className = 'inv-live';
    live.setAttribute('role', 'status');
    live.setAttribute('aria-live', 'polite');
    container.prepend(live);
  }
  if (live.dataset.lastText !== text) {
    live.textContent = text;
    live.dataset.lastText = text;
  }
}

// 投资卡无用户配置项；设置面板给出用途说明（结构门禁要求 renderSettings）。
export function renderInvestmentSettings(container, t) {
  container.replaceChildren();
  const note = document.createElement('p');
  note.className = 'inv-settings-note';
  note.textContent = t('invNoSettings');
  container.append(note);
}

export const INVESTMENT_CARD_STYLES = `
/* 视觉语言与 AI 效能卡对齐（方案 04 §7：复用 --space-widget-* 局部角色，
   不新增独立视觉系统）：玻璃卡底、22px 指标、12px 标签、tabular-nums。
   --space-widget-text / -text-secondary / -metric-size / -weight 由
   space-dashboard:applyWidgetDisplayStyles 注入本卡片根。 */
.InvestmentCard { display:flex; flex-direction:column; gap:6px; min-width:0; width:100%;
  box-sizing:border-box; padding:14px 16px; border-radius:16px;
  background:color-mix(in srgb, var(--space-widget-text, #ffffff) 8%, transparent);
  backdrop-filter:blur(20px); -webkit-backdrop-filter:blur(20px);
  border:1px solid color-mix(in srgb, var(--space-widget-text, #ffffff) 16%, transparent);
  color:var(--space-widget-text, inherit); }
.InvestmentCard .inv-card-header { display:flex; align-items:center; justify-content:space-between; gap:8px; min-width:0; }
.InvestmentCard .inv-card-title { margin:0; font-size:12px; line-height:1.4; font-weight:500;
  color:var(--space-widget-text-secondary, inherit); overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
.InvestmentCard .inv-card-header-right { display:inline-flex; align-items:center; gap:6px; flex:none; }
.InvestmentCard .inv-card-badge { font-size:11px; line-height:1.4; padding:1px 8px; border-radius:999px;
  font-weight:500; white-space:nowrap;
  background:color-mix(in srgb, var(--space-widget-text, #ffffff) 12%, transparent);
  border:1px solid color-mix(in srgb, var(--space-widget-text, #ffffff) 16%, transparent); }
.InvestmentCard .inv-badge-incomplete, .InvestmentCard .inv-badge-stale, .InvestmentCard .inv-badge-unavailable { font-weight:600; }
.InvestmentCard .inv-card-refresh, .InvestmentCard .inv-card-open {
  display:inline-flex; align-items:center; justify-content:center; gap:4px;
  min-width:32px; min-height:32px; padding:0 10px; border-radius:8px;
  background:color-mix(in srgb, var(--space-widget-text, #ffffff) 8%, transparent);
  border:1px solid color-mix(in srgb, var(--space-widget-text, #ffffff) 16%, transparent);
  color:inherit; font:inherit; font-size:12px; line-height:1.4; font-weight:500; cursor:pointer; }
.InvestmentCard .inv-card-refresh:hover, .InvestmentCard .inv-card-open:hover {
  background:color-mix(in srgb, var(--space-widget-text, #ffffff) 14%, transparent); }
.InvestmentCard .inv-card-refresh:focus-visible, .InvestmentCard .inv-card-open:focus-visible {
  outline:2px solid currentColor; outline-offset:2px; }
.InvestmentCard .inv-card-refresh svg { width:14px; height:14px; }
.InvestmentCard .inv-card-main { display:flex; flex-direction:column; align-items:flex-start; gap:2px; min-width:0;
  font-size:var(--space-widget-metric-size, 22px); line-height:1.15; color:currentColor;
  font-weight:var(--space-widget-weight, 700); letter-spacing:-0.02em; font-variant-numeric:tabular-nums; }
.InvestmentCard .inv-card-main .inv-main-sub { font-size:12px; font-weight:500; line-height:1.4;
  letter-spacing:0; color:var(--space-widget-text-secondary, inherit); font-variant-numeric:tabular-nums; }
.InvestmentCard .inv-card-body { display:flex; flex-direction:column; gap:4px; min-width:0;
  font-size:12px; line-height:1.4; font-weight:500; color:var(--space-widget-text-secondary, inherit);
  font-variant-numeric:tabular-nums; }
.InvestmentCard .inv-card-notice { margin:0; font-size:11px; line-height:1.4;
  color:var(--space-widget-text-secondary, inherit); }
.InvestmentCard .inv-card-footer { display:flex; }
.InvestmentCard .inv-card-error { font-size:12px; color:var(--space-widget-text-secondary, inherit); }
.InvestmentCard .inv-live { position:absolute; width:1px; height:1px; overflow:hidden;
  clip:rect(0 0 0 0); white-space:nowrap; }
.InvestmentCard .inv-list { display:flex; flex-direction:column; gap:6px; margin:0; padding:0; list-style:none; width:100%; }
.InvestmentCard .inv-list li { display:grid; grid-template-columns:minmax(48px,40%) 1fr; justify-content:space-between; gap:8px; min-width:0; }
.InvestmentCard .inv-list .inv-li-label { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; color:var(--space-widget-text, inherit); font-weight:500; }
.InvestmentCard .inv-list .inv-li-value { justify-self:end; font-variant-numeric: tabular-nums; }
.InvestmentCard .inv-progress { height:8px; width:100%; max-width:280px; border-radius:4px; overflow:hidden;
  background:color-mix(in srgb, var(--space-widget-text, #ffffff) 12%, transparent); }
.InvestmentCard .inv-progress .inv-progress-fill { height:100%; max-width:100%;
  border-radius:inherit; background:currentColor; }
.InvestmentCard .inv-settings-note { margin:.2em 0; font-size:12px; line-height:1.5; }
@media (prefers-reduced-motion: reduce) {
  .InvestmentCard, .InvestmentCard * { transition:none !important; animation:none !important; }
}
`
