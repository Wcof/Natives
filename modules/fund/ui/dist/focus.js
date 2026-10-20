(() => {
const { actions, state, t } = Fund;
const workspace = document.getElementById('focus-workspace');
const center = document.getElementById('focus-center');
const right = document.getElementById('focus-right');
const list = document.getElementById('focus-asset-list');
const source = document.getElementById('focus-source');
const resizer = document.getElementById('focus-resizer');
const narrowList = document.getElementById('focus-list-dialog');
const evidenceDialog = document.getElementById('focus-evidence-dialog');
let origin = 'trends';
let active = false, moved = [], returnFocus = null, scrollTop = 0, resizeDraft = null;
let width = 220;
try { width = Number(localStorage.getItem('natives-fund-focus-width')) || width; } catch (_) {}

// Layout only: search, data, drawings and chart instances retain their owners.
function identity() { return origin === 'detail' ? (actions.stockDetailSelection?.() || {}) : (Fund.trends?.coordinator?.selection?.() || {}); }
function resizeCharts() { origin === 'detail' ? actions.resizeStockDetailCharts?.() : actions.resizeTrendsCharts?.(); }
function applyWidth(value, persist = false) {
  width = Math.max(180, Math.min(360, value));
  document.documentElement.style.setProperty('--focus-left-width', width + 'px');
  resizer.setAttribute('aria-valuenow', String(width));
  if (persist) { try { localStorage.setItem('natives-fund-focus-width', String(width)); } catch (_) {} }
  resizeCharts();
}
function move(node, target) {
  if (!node || !target) return;
  moved.push({ node, parent: node.parentNode, next: node.nextSibling });
  target.append(node);
}
function restoreNodes() {
  moved.reverse().forEach(({ node, parent, next }) => parent.insertBefore(node, next?.parentNode === parent ? next : null));
  moved = [];
}
function items() {
  return source.value === 'history'
    ? (Fund.trends?.watch?.getHistory?.() || []).map(i => ({ symbol: i.code, name: i.name }))
    : state.watchlistItems;
}
function renderList() {
  if (!active) return;
  const selected = identity();
  const hidden = Fund.home?.displayMode?.() === 'hidden';
  document.getElementById('focus-badge').textContent = t('fundFocusBadge') + ' · ' + (hidden ? '****' : selected.name || selected.symbol);
  list.replaceChildren();
  if (hidden || !items().length) {
    const hint = document.createElement('p');
    hint.className = 'evidence-hint';
    hint.textContent = t(hidden ? 'fundHomeOverviewHistoryHidden' : 'fundFocusContextEmpty');
    list.append(hint);
    return;
  }
  items().slice(0, 200).forEach(item => {
    const row = document.createElement('button');
    row.className = 'focus-asset-row';
    row.type = 'button';
    row.dataset.symbol = item.symbol;
    row.setAttribute('role', 'option');
    const chosen = item.symbol.replace(/^(sh|sz|bj)/i, '') === selected.symbol?.replace(/^(sh|sz|bj)/i, '');
    row.classList.toggle('active', chosen);
    row.setAttribute('aria-selected', String(chosen));
    row.textContent = (item.name || item.symbol) + ' · ' + item.symbol;
    row.addEventListener('click', () => {
      if (origin === 'detail') actions.openStockDetail?.(item.symbol, item.name, item.assetType);
      else Fund.trends.coordinator.selectStock(item.symbol, item.name, '', item.assetType);
      renderList();
      if (narrowList.open) narrowList.close();
    });
    list.append(row);
  });
}
function enterFocus() {
  if (active) return;
  origin = document.getElementById('view-stock-detail')?.hidden === false ? 'detail' : 'trends';
  if (!identity().symbol) { document.getElementById('search-input')?.focus(); return; }
  returnFocus = document.activeElement;
  scrollTop = document.querySelector('.main-column')?.scrollTop || 0;
  active = true;
  if (origin === 'detail') {
    move(document.querySelector('#view-stock-detail .sd-topbar'), center);
    move(document.querySelector('#view-stock-detail .sd-chart-toolbar-wrap'), center);
    move(document.querySelector('#view-stock-detail .sd-chart-stage'), center);
    move(document.querySelector('#view-stock-detail .sd-right-panel'), right);
  } else {
    move(document.querySelector('#view-trends .topbar'), center);
    move(document.querySelector('#view-trends .chart-toolbar'), center);
    move(document.querySelector('#view-trends .chart-container'), center);
    move(document.getElementById('trends-right-panel'), right);
  }
  document.body.classList.add('focus-mode');
  workspace.hidden = false;
  renderList();
  resizeCharts();
  document.getElementById('focus-exit-btn').focus();
}
function finishResize(commit) {
  if (!resizeDraft) return;
  const saved = resizeDraft;
  resizeDraft = null;
  resizer.classList.remove('dragging');
  if (resizer.hasPointerCapture(saved.pointerId)) resizer.releasePointerCapture(saved.pointerId);
  applyWidth(commit ? width : saved.width, commit);
}
function closeContext() { if (narrowList.open) narrowList.close(); }
function exitFocus() {
  if (!active) return;
  finishResize(false);
  closeContext();
  if (evidenceDialog.open) evidenceDialog.close();
  if (origin === 'detail') actions.cancelStockDetailDrawing?.();
  else Fund.trends?.drawings?.cancelDraft?.();
  active = false;
  restoreNodes();
  document.body.classList.remove('focus-mode');
  workspace.hidden = true;
  document.querySelector('.main-column').scrollTop = scrollTop;
  resizeCharts();
  if (returnFocus?.isConnected) returnFocus.focus({ preventScroll: true });
  returnFocus = null;
}
actions.enterFocusMode = enterFocus;
actions.exitFocusMode = exitFocus;
source.addEventListener('change', renderList);
document.getElementById('btn-focus-mode').addEventListener('click', enterFocus);
document.getElementById('sd-btn-focus').addEventListener('click', enterFocus);
document.getElementById('focus-exit-btn').addEventListener('click', exitFocus);
document.getElementById('focus-list-btn').addEventListener('click', () => {
  move(document.getElementById('focus-left'), narrowList);
  narrowList.showModal();
});
document.getElementById('focus-list-close').addEventListener('click', closeContext);
narrowList.addEventListener('close', () => {
  const entry = moved.findIndex(entry => entry.node.id === 'focus-left');
  if (entry >= 0) {
    const { node, parent, next } = moved.splice(entry, 1)[0];
    parent.insertBefore(node, next?.parentNode === parent ? next : null);
  }
  if (active) document.getElementById('focus-list-btn').focus();
});
document.getElementById('focus-evidence-btn').addEventListener('click', () => {
  move(right, evidenceDialog);
  evidenceDialog.showModal();
  resizeCharts();
});
document.getElementById('focus-evidence-close').addEventListener('click', () => evidenceDialog.close());
evidenceDialog.addEventListener('close', () => {
  const index = moved.findIndex(entry => entry.node === right);
  if (index >= 0) {
    const { node, parent, next } = moved.splice(index, 1)[0];
    parent.insertBefore(node, next?.parentNode === parent ? next : null);
  }
  if (active) document.getElementById('focus-evidence-btn').focus();
});
list.addEventListener('keydown', e => {
  if (!['ArrowUp', 'ArrowDown', 'Home', 'End'].includes(e.key)) return;
  const rows = [...list.querySelectorAll('button')];
  if (!rows.length) return;
  e.preventDefault();
  const index = rows.indexOf(document.activeElement);
  const next = e.key === 'Home' ? 0 : e.key === 'End' ? rows.length - 1
    : (index + (e.key === 'ArrowUp' ? rows.length - 1 : 1)) % rows.length;
  rows[next].focus();
});
resizer.addEventListener('pointerdown', e => {
  if (e.button !== 0) return;
  resizeDraft = { pointerId: e.pointerId, width, x: e.clientX };
  resizer.setPointerCapture(e.pointerId);
  resizer.classList.add('dragging');
  e.preventDefault();
});
resizer.addEventListener('pointermove', e => {
  if (resizeDraft?.pointerId === e.pointerId) applyWidth(resizeDraft.width + e.clientX - resizeDraft.x);
});
resizer.addEventListener('pointerup', () => finishResize(true));
resizer.addEventListener('pointercancel', () => finishResize(false));
resizer.addEventListener('lostpointercapture', () => finishResize(false));
resizer.addEventListener('keydown', e => {
  if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(e.key)) return;
  e.preventDefault();
  applyWidth(e.key === 'Home' ? 180 : e.key === 'End' ? 360 : width + (e.key === 'ArrowLeft' ? -20 : 20), true);
});
document.addEventListener('keydown', e => {
  if (!active || e.key !== 'Escape' || document.querySelector('dialog[open]') || e.defaultPrevented) return;
  e.preventDefault();
  if (resizeDraft) { finishResize(false); return; }
  if (origin === 'detail' ? actions.cancelStockDetailDrawing?.() : Fund.trends?.drawings?.cancelDraft?.()) return;
  exitFocus();
});
document.addEventListener('fund:research-selection-changed', renderList);
document.addEventListener('fund:detail-selection-changed', renderList);
document.addEventListener('fund:language-changed', renderList);
document.addEventListener('fund:display-mode-changed', renderList);
window.addEventListener('blur', () => finishResize(false));
document.addEventListener('visibilitychange', () => { if (document.hidden) finishResize(false); });
window.addEventListener('pagehide', exitFocus);
applyWidth(width);
})();
