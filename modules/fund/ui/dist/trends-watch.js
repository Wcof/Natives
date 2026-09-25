(() => {
const { state, actions, authFetch } = globalThis.Fund;
const trends = (globalThis.Fund.trends = globalThis.Fund.trends || {});
const tState = trends.state;
const C = trends.C;

const STORAGE_WATCH = 'qs_watchlist';
const STORAGE_HISTORY = 'qs_history';
const STORAGE_SIGNALS = 'qs_signal_records';
const MAX_HISTORY = 30;
const MAX_SIGNAL_RECORDS = 200;
let _currentTab = 'watch';
let _panelOpen = false;
let _scanTimer = null;

// --- localStorage 读写 ---
function getWatchlist() {
  try { return JSON.parse(localStorage.getItem(STORAGE_WATCH) || '[]'); } catch(e) { return []; }
}
function saveWatchlist(list) {
  try { localStorage.setItem(STORAGE_WATCH, JSON.stringify(list)); } catch(e) {}
}
function getHistory() {
  try { return JSON.parse(localStorage.getItem(STORAGE_HISTORY) || '[]'); } catch(e) { return []; }
}
function saveHistory(list) {
  try { localStorage.setItem(STORAGE_HISTORY, JSON.stringify(list)); } catch(e) {}
}

// --- 自选股操作 ---
function toggleStar() {
  if (!tState.currentSymbol) return;
  const list = getWatchlist();
  const idx = list.findIndex(s => s.code === tState.currentSymbol);
  if (idx >= 0) {
    list.splice(idx, 1);
  } else {
    const name = tState.currentStockName || tState.currentSymbol;
    list.unshift({ code: tState.currentSymbol, name: name, addedAt: Date.now() });
  }
  saveWatchlist(list);
  updateStarButton(tState.currentSymbol);
  renderWatchlist();
  updateBadges();
}

function removeFromWatchlist(code) {
  const list = getWatchlist().filter(s => s.code !== code);
  saveWatchlist(list);
  updateStarButton(tState.currentSymbol);
  renderWatchlist();
  updateBadges();
}

function updateStarButton(symbol) {
  const btn = document.getElementById('star-btn');
  if (!btn || !symbol) return;
  const inWatch = getWatchlist().some(s => s.code === symbol);
  btn.textContent = inWatch ? '★' : '☆';
  btn.classList.toggle('starred', inWatch);
  btn.title = inWatch ? '从自选中移除' : '加入自选';
}

// --- 历史记录操作 ---
function addHistory(code, name, action, score) {
  let list = getHistory();
  list = list.filter(s => s.code !== code);
  list.unshift({ code, name, action, score, time: Date.now() });
  if (list.length > MAX_HISTORY) list = list.slice(0, MAX_HISTORY);
  saveHistory(list);
  if (_panelOpen && _currentTab === 'history') renderHistory();
  updateBadges();
}

function clearHistory() {
  saveHistory([]);
  renderHistory();
  updateBadges();
}

// --- 渲染 ---
function renderWatchlist() {
  const el = document.getElementById('wp-content-watch');
  if (!el) return;
  const list = getWatchlist();
  if (!list.length) {
    el.innerHTML = '<div class="wp-empty"><span class="wp-empty-icon">☆</span>还没有自选股<br>分析股票后点击 ☆ 添加</div>';
    return;
  }
  el.innerHTML = list.map(s => {
    const tag = sigTag(s.action, s.score);
    return `<div class="wp-item" onclick="analyze('${s.code}');closePanel()">
      <span class="code">${s.code}</span>
      <span class="name">${escHtml(s.name)}</span>
      ${tag}
      <button class="remove" onclick="event.stopPropagation();removeFromWatchlist('${s.code}')" title="移除">×</button>
    </div>`;
  }).join('');
}

function renderHistory() {
  const el = document.getElementById('wp-content-history');
  if (!el) return;
  const list = getHistory();
  if (!list.length) {
    el.innerHTML = '<div class="wp-empty"><span class="wp-empty-icon">🕐</span>暂无历史记录<br>分析过的股票会显示在这里</div>';
    return;
  }
  el.innerHTML = list.map(s => {
    const tag = sigTag(s.action, s.score);
    const t = fmtTime(s.time);
    return `<div class="wp-item" onclick="analyze('${s.code}');closePanel()">
      <span class="code">${s.code}</span>
      <span class="name">${escHtml(s.name)}</span>
      ${tag}
      <span class="time">${t}</span>
    </div>`;
  }).join('');
}

function sigTag(action, score) {
  if (!action) return '<span class="sig-tag none">--</span>';
  const cls = action === '买入' ? 'buy' : action === '卖出' ? 'sell' : 'watch';
  return `<span class="sig-tag ${cls}">${action}${score ? ' ' + score : ''}</span>`;
}

function fmtTime(ts) {
  if (!ts) return '';
  const d = new Date(ts);
  const now = new Date();
  const diff = now - d;
  if (diff < 60000) return '刚刚';
  if (diff < 3600000) return Math.floor(diff / 60000) + '分钟前';
  if (d.toDateString() === now.toDateString()) {
    return `今天 ${String(d.getHours()).padStart(2,'0')}:${String(d.getMinutes()).padStart(2,'0')}`;
  }
  const yest = new Date(now); yest.setDate(yest.getDate() - 1);
  if (d.toDateString() === yest.toDateString()) return '昨天';
  return `${d.getMonth()+1}/${d.getDate()}`;
}

function escHtml(s) {
  if (!s) return '';
  return s.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;');
}

// --- 面板控制 ---
function closePanel() {
  const panel = document.getElementById('wp-panel');
  if (panel) panel.classList.remove('show');
  _panelOpen = false;
  const wb = document.getElementById('watch-btn');
  if (wb) wb.classList.remove('active');
  const hb = document.getElementById('history-btn');
  if (hb) hb.classList.remove('active');
}

function togglePanel(tab) {
  _currentTab = tab;
  const panel = document.getElementById('wp-panel');
  if (!panel) return;
  if (_panelOpen && panel.classList.contains('show')) {
    if (_currentTab === tab) {
      closePanel();
      return;
    }
  } else {
    panel.classList.add('show');
    _panelOpen = true;
  }
  switchTab(tab);
}

function switchTab(tab) {
  _currentTab = tab;
  document.querySelectorAll('.wp-tab').forEach(t => {
    t.classList.toggle('active', t.dataset.tab === tab);
  });
  const wcw = document.getElementById('wp-content-watch');
  if (wcw) wcw.style.display = tab === 'watch' ? 'block' : 'none';
  const wch = document.getElementById('wp-content-history');
  if (wch) wch.style.display = tab === 'history' ? 'block' : 'none';
  const wco = document.getElementById('wp-content-overview');
  if (wco) wco.style.display = tab === 'overview' ? 'block' : 'none';
  const wb = document.getElementById('watch-btn');
  if (wb) wb.classList.toggle('active', tab === 'watch');
  const hb = document.getElementById('history-btn');
  if (hb) hb.classList.toggle('active', tab === 'history');
  if (tab === 'watch') renderWatchlist();
  else if (tab === 'history') renderHistory();
  else if (tab === 'overview') loadOverview();
  const footer = document.getElementById('wp-footer');
  const list = tab === 'watch' ? getWatchlist() : tab === 'history' ? getHistory() : [];
  if (footer) {
    if (list.length > 0 && tab !== 'overview') {
      footer.style.display = 'flex';
      const fInfo = document.getElementById('wp-footer-info');
      if (fInfo) fInfo.textContent = tab === 'watch'
        ? `共 ${list.length} 只自选股`
        : `共 ${list.length} 条记录`;
      const cBtn = document.getElementById('wp-clear-btn');
      if (cBtn) cBtn.textContent = tab === 'watch' ? '清空自选' : '清空历史';
    } else {
      footer.style.display = 'none';
    }
  }
  if (tab === 'overview') clearWatchChangeBadge();
}

function clearCurrentTab() {
  if (_currentTab === 'watch') {
    saveWatchlist([]);
    renderWatchlist();
    updateStarButton(tState.currentSymbol);
  } else {
    clearHistory();
  }
  updateBadges();
  switchTab(_currentTab);
}

function updateBadges() {
  const wl = getWatchlist().length;
  const hl = getHistory().length;
  const wb = document.getElementById('watch-count');
  const hb = document.getElementById('history-count');
  const wt = document.getElementById('wt-count');
  const ht = document.getElementById('ht-count');
  const ov = document.getElementById('ov-count');
  if (wb) { wb.textContent = wl; wb.style.display = wl > 0 ? 'inline-block' : 'none'; }
  if (hb) { hb.textContent = hl; hb.style.display = hl > 0 ? 'inline-block' : 'none'; }
  if (wt) wt.textContent = wl > 0 ? `(${wl})` : '';
  if (ht) ht.textContent = hl > 0 ? `(${hl})` : '';
  if (ov) ov.textContent = wl > 0 ? `(${wl})` : '';
}

// ===== 多股一览 =====
async function loadOverview() {
  const el = document.getElementById('wp-content-overview');
  if (!el) return;
  const list = getWatchlist();
  if (!list.length) {
    el.innerHTML = '<div class="wp-empty"><span class="wp-empty-icon">📊</span>还没有自选股<br>添加自选后可查看多股一览</div>';
    return;
  }
  el.innerHTML = '<div class="wp-ov-loading">正在获取行情数据...</div>';

  const results = await Promise.all(list.map(async s => {
    try {
      const r = await authFetch(`/api/trends/quote?symbol=${s.code}`);
      const q = await r.json();
      if (q.error) return { code: s.code, name: s.name, error: true };
      return {
        code: s.code, name: q.name || s.name,
        price: q.price, pct: q.pct, volume: q.volume,
        action: s.action || '', score: s.score || 0,
      };
    } catch(e) {
      return { code: s.code, name: s.name, error: true };
    }
  }));

  results.sort((a, b) => (b.pct || -999) - (a.pct || -999));

  el.innerHTML = results.map(s => {
    const pctCls = s.pct > 0 ? 'up' : s.pct < 0 ? 'down' : 'flat';
    const pctStr = s.pct != null ? (s.pct > 0 ? '+' : '') + s.pct.toFixed(2) + '%' : '--';
    const sigTag = s.action ? `<span class="wp-ov-sig-tag ${s.action === '买入' ? 'buy' : s.action === '卖出' ? 'sell' : 'watch'}">${s.action}</span>` : '<span class="wp-ov-sig-tag none">--</span>';
    const scoreStr = s.score ? s.score : '--';
    const scoreColor = s.score >= 60 ? C.up : s.score >= 40 ? '#ffc107' : s.score > 0 ? C.down : '#666';
    return `<div class="wp-ov-item" onclick="analyze('${s.code}');closePanel()">
      <span class="wp-ov-code">${s.code}</span>
      <span class="wp-ov-name">${escHtml(s.name)}</span>
      <span class="wp-ov-pct ${pctCls}">${pctStr}</span>
      <span class="wp-ov-score" style="color:${scoreColor}">${scoreStr}</span>
      <span class="wp-ov-sig">${sigTag}</span>
    </div>`;
  }).join('');
}

// ===== 信号准确率统计 =====
function getSignalRecords() {
  try { return JSON.parse(localStorage.getItem(STORAGE_SIGNALS) || '[]'); } catch(e) { return []; }
}
function saveSignalRecords(list) {
  try { localStorage.setItem(STORAGE_SIGNALS, JSON.stringify(list.slice(0, MAX_SIGNAL_RECORDS))); } catch(e) {}
}
function recordSignal(code, name, action, score, price) {
  const records = getSignalRecords();
  records.unshift({ code, name, action, score, price, time: Date.now() });
  saveSignalRecords(records);
}
function calcSignalAccuracy(code) {
  const records = getSignalRecords().filter(r => r.code === code);
  if (records.length < 2) return null;
  let correct = 0, total = 0;
  const details = [];
  for (let i = 0; i < records.length - 1; i++) {
    const cur = records[i];
    const prev = records[i + 1];
    if (!prev.price || !cur.price) continue;
    const priceChange = (cur.price - prev.price) / prev.price * 100;
    let isCorrect = false;
    if (prev.action === '买入' && priceChange > 0) isCorrect = true;
    else if (prev.action === '卖出' && priceChange < 0) isCorrect = true;
    else if (prev.action === '观望') isCorrect = Math.abs(priceChange) < 3;
    total++;
    if (isCorrect) correct++;
    if (details.length < 5) {
      details.push({
        action: prev.action, price: prev.price,
        nextPrice: cur.price, change: priceChange,
        correct: isCorrect, time: prev.time,
      });
    }
  }
  if (total === 0) return null;
  return {
    accuracy: Math.round(correct / total * 100),
    total, correct,
    buyCount: records.filter(r => r.action === '买入').length,
    sellCount: records.filter(r => r.action === '卖出').length,
    watchCount: records.filter(r => r.action === '观望').length,
    details: details.reverse(),
  };
}
function renderSignalAccuracy(code) {
  const card = document.getElementById('accuracy-card');
  const stats = calcSignalAccuracy(code);
  if (!card) return;
  if (!stats) {
    card.style.display = 'none';
    return;
  }
  card.style.display = 'block';
  const accColor = stats.accuracy >= 60 ? C.up : stats.accuracy >= 40 ? '#ffc107' : C.down;
  const saStats = document.getElementById('sa-stats');
  if (saStats) {
    saStats.innerHTML = `
      <div class="sa-item">
        <span class="sa-val" style="color:${accColor}">${stats.accuracy}%</span>
        <span class="sa-label">准确率</span>
      </div>
      <div class="sa-item">
        <span class="sa-val" style="color:#ddd">${stats.total}</span>
        <span class="sa-label">信号次数</span>
      </div>
      <div class="sa-item">
        <span class="sa-val" style="color:${C.up}">${stats.buyCount}</span>
        <span class="sa-label">买入</span>
      </div>
      <div class="sa-item">
        <span class="sa-val" style="color:${C.down}">${stats.sellCount}</span>
        <span class="sa-label">卖出</span>
      </div>
      <div class="sa-item">
        <span class="sa-val" style="color:#ffc107">${stats.watchCount}</span>
        <span class="sa-label">观望</span>
      </div>
    `;
  }
  const detailHtml = stats.details.map(d => {
    const cls = d.correct ? 'up' : 'down';
    const sign = d.change > 0 ? '+' : '';
    return `<div style="padding:2px 0">
      <span style="color:${d.action === '买入' ? C.up : d.action === '卖出' ? C.down : '#ffc107'}">${d.action}</span>
      @ ${d.price.toFixed(2)} → 后续 <span class="${cls}">${sign}${d.change.toFixed(1)}%</span>
      <span style="color:${d.correct ? C.up : C.down}">${d.correct ? '✓' : '✗'}</span>
    </div>`;
  }).join('');
  const saDetail = document.getElementById('sa-detail');
  if (saDetail) saDetail.innerHTML = detailHtml;
}

// ===== 信号变更提醒 =====
function checkSignalChange(code, name, action, score, price) {
  const prev = tState.lastSignal[code];
  if (prev && prev.action && prev.action !== action) {
    showToast(name, code, prev.action, action, price);
    const watch = getWatchlist();
    if (watch.some(s => s.code === code)) {
      const badge = document.getElementById('watch-change');
      if (badge) {
        const count = parseInt(badge.textContent || '0') + 1;
        badge.textContent = count;
        badge.style.display = 'inline-block';
      }
    }
  }
  tState.lastSignal[code] = { action, score, price, time: Date.now() };
}

function showToast(name, code, oldAction, newAction, price) {
  const container = document.getElementById('toast-container');
  if (!container) return;
  const cls = newAction === '买入' ? 'buy' : newAction === '卖出' ? 'sell' : '';
  const oldColor = oldAction === '买入' ? C.up : oldAction === '卖出' ? C.down : '#ffc107';
  const newColor = newAction === '买入' ? C.up : newAction === '卖出' ? C.down : '#ffc107';
  const toast = document.createElement('div');
  toast.className = `toast ${cls}`;
  toast.onclick = () => removeToast(toast);
  toast.innerHTML = `
    <div style="font-weight:bold;font-size:14px;margin-bottom:4px">${escHtml(name)} (${code})</div>
    <div>信号变更：<span style="color:${oldColor}">${oldAction}</span> → <span style="color:${newColor};font-weight:bold">${newAction}</span></div>
    <div style="font-size:11px;color:#666;margin-top:4px">当前价 ${price ? price.toFixed(2) : '--'}</div>
  `;
  container.appendChild(toast);
  setTimeout(() => removeToast(toast), 8000);
}

function removeToast(el) {
  if (!el || !el.parentNode) return;
  el.classList.add('removing');
  setTimeout(() => { if (el.parentNode) el.remove(); }, 300);
}

function clearWatchChangeBadge() {
  const badge = document.getElementById('watch-change');
  if (badge) {
    badge.textContent = '0';
    badge.style.display = 'none';
  }
}

// 扫描功能已拆分至独立的 trends-scan.js 模块
// 绑定全局与内联事件
window.removeFromWatchlist = removeFromWatchlist;
window.closePanel = closePanel;
window.toggleStar = toggleStar;
window.togglePanel = togglePanel;
window.switchTab = switchTab;
window.clearCurrentTab = clearCurrentTab;

const watchBtn = document.getElementById('watch-btn');
if (watchBtn) watchBtn.addEventListener('click', () => togglePanel('watch'));
const historyBtn = document.getElementById('history-btn');
if (historyBtn) historyBtn.addEventListener('click', () => togglePanel('history'));
const starBtn = document.getElementById('star-btn');
if (starBtn) starBtn.addEventListener('click', toggleStar);
const clearBtn = document.getElementById('wp-clear-btn');
if (clearBtn) clearBtn.addEventListener('click', clearCurrentTab);
document.querySelectorAll('.wp-tab').forEach(t => {
  t.addEventListener('click', () => switchTab(t.dataset.tab));
});

document.addEventListener('click', e => {
  if (_panelOpen && !e.target.closest('.watch-wrap')) {
    closePanel();
  }
});

const wpPanel = document.getElementById('wp-panel');
if (wpPanel) wpPanel.addEventListener('click', e => e.stopPropagation());

trends.watch = {
  getWatchlist,
  saveWatchlist,
  getHistory,
  toggleStar,
  removeFromWatchlist,
  updateStarButton,
  addHistory,
  clearHistory,
  renderWatchlist,
  renderHistory,
  closePanel,
  togglePanel,
  switchTab,
  updateBadges,
  loadOverview,
  recordSignal,
  calcSignalAccuracy,
  renderSignalAccuracy,
  checkSignalChange,
  openScan: (...args) => trends.scan?.openScan(...args),
  closeScan: (...args) => trends.scan?.closeScan(...args),
  startScan: (...args) => trends.scan?.startScan(...args),
  renderScanIdle: (...args) => trends.scan?.renderScanIdle(...args),
  analyzeFromScan: (...args) => trends.scan?.analyzeFromScan(...args),
};

})();
