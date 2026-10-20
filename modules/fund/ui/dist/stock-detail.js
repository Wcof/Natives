(() => {
const { state, actions, authFetch, api, t } = globalThis.Fund;

// Fund 股票详情子系统：看盘全屏双栏工作台控制器（Stock Detail Controller）
// 核心职责：
// 1. 驱动右侧盘口卡片流（复用 actions.renderDepth 共享渲染与算法）
// 2. 行情数据加载流水线（防闪烁静默刷新、超时与竞态防护）
// 3. 交易时段自适应轮询调度与生命周期（open/exit/pause/resume）
// 4. UI 事件绑定（周期/指标/复权/搜索/侧栏/拖拽）
// 图表渲染层（ECharts 初始化、option 构建、缩放联动、主题重绘）已按职责
// 拆分至 stock-detail-chart.js，共享状态挂载在 StockDetailShared。

const S = globalThis.Fund.StockDetailShared;
const sdState = S.sdState;
const C = S.charts;

let _refreshTimer = null;
let _loadSeq = 0;
let _lastMinuteSig = null;
let _lastKlineSig = null;
let _chartAbortCtrl = null;
let returnView = null;
let cancelSideResize = () => {};

function safeDecode(str) {
  if (!str) return '';
  try {
    return decodeURIComponent(str);
  } catch (e) {
    return str;
  }
}

function showChartError(text) {
  const errEl = document.getElementById('sd-chart-error');
  const txtEl = document.getElementById('sd-error-text');
  if (errEl && txtEl) {
    txtEl.textContent = text || t('fundDetailLoadFailed');
    errEl.style.display = 'flex';
  }
}

function hideChartError() {
  const errEl = document.getElementById('sd-chart-error');
  if (errEl) errEl.style.display = 'none';
}

// ---------- 视图与周期切换 ----------
function switchPeriod(p) {
  if(p!==sdState.period){sdState.klineData=[];sdState.minuteData=null;sdState.navData=[];S.drawingsCtrl?.cancelDraft();document.dispatchEvent(new Event('fund:detail-selection-changed'));}
  sdState.period = p;
  const navMode = sdState.instrumentKind === 'fund';
  document.getElementById('sd-view-minute').disabled = navMode;
  document.getElementById('sd-draw-toolbar').hidden = navMode;
  document.getElementById('sd-right-research-status')?.remove();
  const right = document.querySelector('.sd-right-panel') || document.querySelector('#focus-right .sd-right-panel');
  if (right) right.querySelectorAll('.side-card').forEach(card=>card.hidden=navMode || (sdState.instrumentKind==='etf' && card.classList.contains('valuation-card')));
  if (right && navMode) { const status=document.createElement('p');status.id='sd-right-research-status';status.className='evidence-hint';status.textContent=Fund.t('fundTrendsNavCapability');right.prepend(status); }
  document.querySelectorAll('#view-stock-detail .sd-tb-group button[data-ind]').forEach(button=>button.disabled=navMode);

  // 更新周期按钮状态
  const group = document.querySelector('.sd-chart-toolbar-wrap');
  if (group) {
    group.querySelectorAll('.sd-tb-group .tb-btn').forEach(btn => {
      btn.classList.toggle('active', btn.id === 'sd-view-' + p);
    });
  }

  const rangeGroup = document.getElementById('sd-range-group');
  const sepRange = document.getElementById('sd-sep-range');
  const indBar = document.getElementById('sd-indicator-bar');
  const sepInd = document.getElementById('sd-sep-ind');
  const fqGroup = document.getElementById('sd-fq-group');

  const mEl = document.getElementById('sd-minute-chart');
  const mvEl = document.getElementById('sd-minute-vol');
  const kEl = document.getElementById('sd-kline-chart');
  const vEl = document.getElementById('sd-volume-chart');
  const indEl = document.getElementById('sd-indicator-chart');

  if (p === 'minute') {
    if (rangeGroup) rangeGroup.style.display = 'none';
    if (sepRange) sepRange.style.display = 'none';
    if (indBar) indBar.style.display = 'none';
    if (sepInd) sepInd.style.display = 'none';
    if (fqGroup) fqGroup.style.display = 'none';

    if (mEl) mEl.style.display = 'block';
    if (mvEl) mvEl.style.display = 'block';
    if (kEl) kEl.style.display = 'none';
    if (vEl) vEl.style.display = 'none';
    if (indEl) indEl.style.display = 'none';

    // 清空非当前周期实例，杜绝周期切换竞态下的旧图残影叠加
    if (C.kline) C.kline.clear();
    if (C.volume) C.volume.clear();
    if (C.indicator) C.indicator.clear();

    if (S.drawingsCtrl && C.minute) {
      S.drawingsCtrl.attachChart(C.minute);
    }
  } else {
    if (rangeGroup) rangeGroup.style.display = 'inline-flex';
    if (sepRange) sepRange.style.display = 'inline-block';
    if (indBar) indBar.style.display = 'inline-flex';
    if (sepInd) sepInd.style.display = 'inline-block';
    if (fqGroup) fqGroup.style.display = navMode ? 'none' : 'inline-flex';

    if (mEl) mEl.style.display = 'none';
    if (mvEl) mvEl.style.display = 'none';
    if (kEl) kEl.style.display = 'block';
    if (vEl) vEl.style.display = 'block';
    if (indEl) indEl.style.display = sdState.indicator !== 'none' ? 'block' : 'none';

    // 同上：切回日K周期时清空分时实例
    if (C.minute) C.minute.clear();
    if (C.minuteVol) C.minuteVol.clear();

    if (S.drawingsCtrl && C.kline) {
      S.drawingsCtrl.attachChart(C.kline);
    }
  }

  S.resizeAllCharts();
  loadChartData(true);
}

// ---------- 1:1 驱动右侧原始 Dashboard 卡片（复用 actions.renderDepth） ----------
function updateDetailPanel(detail) {
  if (!detail) return;
  sdState.detail = detail;

  const cleanName = safeDecode(detail.name || sdState.name);

  function setElText(el, text) {
    if (el && el.textContent !== text) el.textContent = text;
  }
  function setVal(id, v) {
    const el = document.getElementById(id);
    const str = (v != null && v !== '') ? String(v) : '-';
    if (el && el.textContent !== str) el.textContent = str;
  }

  // 1. 标的报价卡
  const isUp = (detail.change || 0) >= 0;
  const cClass = isUp ? 'up' : 'down';
  const sign = isUp ? '+' : '';

  const nameEl = document.getElementById('sd-dash-name');
  const codeEl = document.getElementById('sd-dash-code');
  const priceEl = document.getElementById('sd-dash-price');
  const changeEl = document.getElementById('sd-dash-change');

  setElText(nameEl, cleanName);
  setElText(codeEl, detail.symbol || sdState.symbol);
  if (priceEl) {
    const pStr = detail.price != null ? Number(detail.price).toFixed(2) : '--';
    if (priceEl.textContent !== pStr) priceEl.textContent = pStr;
    const targetCls = 'price-large ' + cClass;
    if (priceEl.className !== targetCls) priceEl.className = targetCls;
  }
  if (changeEl) {
    const ch = detail.change != null ? `${sign}${Number(detail.change).toFixed(2)}` : '';
    const pct = detail.changePct != null ? `${sign}${Number(detail.changePct).toFixed(2)}%` : '';
    const chgStr = `${ch} (${pct})`;
    if (changeEl.textContent !== chgStr) changeEl.textContent = chgStr;
    if (changeEl.className !== cClass) changeEl.className = cClass;
  }

  // 顶部行情条同步
  const tName = document.getElementById('sd-stock-name');
  const tCode = document.getElementById('sd-stock-code');
  const tPrice = document.getElementById('sd-stock-price');
  const tChange = document.getElementById('sd-stock-change');
  setElText(tName, cleanName);
  setElText(tCode, detail.symbol || sdState.symbol);
  if (tPrice) {
    const pStr = detail.price != null ? Number(detail.price).toFixed(2) : '--';
    if (tPrice.textContent !== pStr) tPrice.textContent = pStr;
    const targetCls = 'sd-price-num ' + cClass;
    if (tPrice.className !== targetCls) tPrice.className = targetCls;
  }
  if (tChange) {
    const ch = detail.change != null ? `${sign}${Number(detail.change).toFixed(2)}` : '';
    const pct = detail.changePct != null ? `${sign}${Number(detail.changePct).toFixed(2)}%` : '';
    const chgStr = `${ch} (${pct})`;
    if (tChange.textContent !== chgStr) tChange.textContent = chgStr;
    const targetCls = 'sd-change-num ' + cClass;
    if (tChange.className !== targetCls) tChange.className = targetCls;
  }

  // 涨跌停与均价辅助速览
  setVal('sd-limit-up', detail.limitUp ? Number(detail.limitUp).toFixed(2) : '--');
  setVal('sd-limit-down', detail.limitDown ? Number(detail.limitDown).toFixed(2) : '--');
  setVal('sd-avg-price', detail.avgPrice ? Number(detail.avgPrice).toFixed(2) : '--');

  // 动态概念/行情特征标签（指纹比对，内容不变不刷 DOM）
  const tagsEl = document.getElementById('sd-tags');
  if (tagsEl) {
    const liveTags = [];
    if (detail.pe && detail.pe > 0 && detail.pe < 28) liveTags.push(t('fundDetailStockTagLowPE',{value:detail.pe.toFixed(1)}));
    if (detail.turnover && detail.turnover > 3.0) liveTags.push(t('fundDetailStockTagTurnover',{value:detail.turnover.toFixed(1)}));
    if (detail.volumeRatio && detail.volumeRatio > 1.6) liveTags.push(t('fundDetailStockTagVolume',{value:detail.volumeRatio.toFixed(1)}));
    if (detail.changePct != null && Math.abs(detail.changePct) >= 2.5) {
      liveTags.push(detail.changePct > 0 ? t('fundDetailStockTagStrength',{value:detail.changePct.toFixed(1)}) : t('fundDetailStockTagWeakness',{value:detail.changePct.toFixed(1)}));
    }
    if (detail.amplitude && detail.amplitude >= 3.5) liveTags.push(t('fundDetailStockTagAmplitude',{value:detail.amplitude.toFixed(1)}));
    const tagSig = liveTags.join('|');
    if (tagsEl._lastSig !== tagSig) {
      tagsEl._lastSig = tagSig;
      tagsEl.innerHTML = liveTags.map(t => `<span class="sd-tag">${t}</span>`).join('');
    }
  }

  // 2. 公司市值与估值股本（全量展开）
  function fmtShares(v) {
    if (v == null || isNaN(v) || v === 0) return '-';
    return t('fundDetailStockHundredMillionShares',{value:(v/1e8).toFixed(2)});
  }
  setVal('sd-v-mktcap', detail.marketCap ? Number(detail.marketCap).toFixed(2) : '-');
  setVal('sd-v-circ-mktcap', detail.circMarketCap ? Number(detail.circMarketCap).toFixed(2) : '-');
  // 卡片标签为「市盈率(PE动)」：优先 PE 动（peDyn），上游缺失时回退 TTM pe
  const peShown = detail.peDyn != null && Number(detail.peDyn) > 0 ? Number(detail.peDyn) : (detail.pe ? Number(detail.pe) : null);
  setVal('sd-v-pe', peShown ? peShown.toFixed(2) : '-');
  setVal('sd-v-pb', detail.pb ? Number(detail.pb).toFixed(2) : '-');
  setVal('sd-v-div', detail.dividendYield ? Number(detail.dividendYield).toFixed(2) + '%' : '-');
  setVal('sd-v-circ-shares', fmtShares(detail.circShares));
  setVal('sd-v-total-shares', fmtShares(detail.totalShares));
  setVal('sd-v-avg', detail.avgPrice ? Number(detail.avgPrice).toFixed(2) : '-');

  // 3. 五档盘口与买卖深度（复用 actions.renderDepth）
  if (actions.renderDepth) {
    actions.renderDepth(detail, 'sd-');
  }

  // 4. 盘口关键指标网格
  setVal('sd-m-high', detail.high != null ? Number(detail.high).toFixed(2) : '-');
  setVal('sd-m-low', detail.low != null ? Number(detail.low).toFixed(2) : '-');
  setVal('sd-m-open', detail.open != null ? Number(detail.open).toFixed(2) : '-');
  setVal('sd-m-prev', detail.prevClose != null ? Number(detail.prevClose).toFixed(2) : '-');
  setVal('sd-m-amount', detail.amount ? (Number(detail.amount) / 10000).toFixed(1) : '-');
  setVal('sd-m-turnover', detail.turnover != null ? Number(detail.turnover).toFixed(2) + '%' : '-');
  setVal('sd-m-vol-ratio', detail.volumeRatio != null ? Number(detail.volumeRatio).toFixed(2) : '-');
  setVal('sd-m-amplitude', detail.amplitude != null ? Number(detail.amplitude).toFixed(2) + '%' : '-');
  // 委比%（(买五总-卖五总)/(买五总+卖五总)×100）与委差（手，上游委托差位）
  setVal('sd-m-weibi', detail.weiBi != null ? Number(detail.weiBi).toFixed(2) + '%' : '-');
  setVal('sd-m-weicha', detail.weiCha != null ? Number(detail.weiCha).toFixed(0) : '-');

  // 同步自选按钮文字
  updateFavButtons();
}

function updateFavButtons() {
  const isFav = state.watchlistItems && state.watchlistItems.some(i => i.symbol === sdState.symbol);
  const topStar = document.getElementById('sd-btn-star');
  const topText = document.getElementById('sd-star-text');
  const dashFav = document.getElementById('sd-btn-dash-fav');

  if (topText) {
    const str = Fund.t(isFav?'fundWatchAdded':'fundTrendsUi4');
    if (topText.textContent !== str) topText.textContent = str;
  }
  if (topStar) topStar.classList.toggle('btn-accent', !isFav);
  if (dashFav) {
    const favHtml = isFav
      ? '<svg class="svg-icon" aria-hidden="true"><use href="#i-star" /></svg> '+Fund.t('fundWatchAdded')
      : '<svg class="svg-icon" aria-hidden="true"><use href="#i-star" /></svg> '+Fund.t('fundTrendsUi4');
    if (dashFav.innerHTML !== favHtml) dashFav.innerHTML = favHtml;
    dashFav.classList.toggle('btn-accent', !isFav);
  }
}
document.addEventListener('fund:language-changed',()=>{updateFavButtons();if(sdState.active){updateDetailPanel(sdState.detail);actions.reloadStockDetailTheme?.();}});

let detailMembershipBusy=false;
async function toggleFav() {
  if(detailMembershipBusy)return;detailMembershipBusy=true;
  const buttons=[document.getElementById('sd-btn-star'),document.getElementById('sd-btn-dash-fav')].filter(Boolean);buttons.forEach(button=>button.disabled=true);
  try {await actions.setWatchlistMember({symbol:sdState.symbol,name:sdState.name,assetType:sdState.instrumentKind},!state.watchlistItems.some(item=>item.symbol===sdState.symbol));updateFavButtons();}
  catch(error){showChartError(error.message || Fund.t('fundHomeRequestFailed'));}
  finally {detailMembershipBusy=false;buttons.forEach(button=>button.disabled=false);}
}

// ---------- 加载数据核心流水线（带防闪烁静默刷新保护） ----------
async function loadChartData(showLoading = false) {
  const seq = ++_loadSeq;
  const loading = document.getElementById('sd-loading');

  // 中止上一个未完成的请求，避免并发竞态
  if (_chartAbortCtrl) {
    try { _chartAbortCtrl.abort(); } catch (e) {}
    _chartAbortCtrl = null;
  }
  const currentCtrl = new AbortController();
  _chartAbortCtrl = currentCtrl;

  if (showLoading) {
    hideChartError();
    if (loading) loading.style.display = 'flex';
  }

  // 10 秒超时防护
  const timeoutId = setTimeout(() => {
    if (_chartAbortCtrl === currentCtrl) {
      currentCtrl.abort('timeout');
    }
  }, 10000);

  try {
    if (sdState.instrumentKind === 'fund') {
      const rows = await Fund.researchSeries.nav(sdState.symbol.replace(/^jj/, ''), currentCtrl.signal);
      if (seq !== _loadSeq || !sdState.active) return;
      sdState.navData = rows;
      C.kline?.setOption(Fund.researchSeries.navOption(Fund.researchSeries.navRows(rows, sdState.period)), true);
      document.getElementById('sd-volume-chart').style.display = 'none';
      document.getElementById('sd-indicator-chart').style.display = 'none';
      const latest = rows.at(-1);
      document.getElementById('sd-stock-price').textContent = latest?.unitNav || '–';
      document.getElementById('sd-stock-change').textContent = latest?.navDate || Fund.t('fundTrendsNavEmpty');
      hideChartError(); return;
    }
    if (sdState.period === 'minute') {
      const res = await authFetch('/api/market/minute?symbol=' + encodeURIComponent(sdState.symbol), {
        signal: currentCtrl.signal,
      });
      const json = await res.json();
      if (seq !== _loadSeq) return;

      if (!res.ok || (json && json.error)) {
        if (showLoading && !sdState.minuteData) {
          showChartError(Fund.localizeApiError(res.status,json?.error));
        }
        return;
      }

      hideChartError();
      const points = (json && (json.points || json.items)) || [];
      const last = points.length ? points[points.length - 1] : null;
      const sig = last ? `${sdState.symbol}_min_${last.time}_${last.price}_${last.volume || 0}_${points.length}` : '';
      if (!showLoading && sig && sig === _lastMinuteSig) {
        return; // 数据无变动直接跳过，杜绝重绘闪烁
      }
      _lastMinuteSig = sig;

      sdState.minuteData = json;
      S.renderMinute(json);
      // renderMinute 内部 notMerge:true 会整体擦掉 graphic 层，立即用内存
      // 画线重挂；否则轮询期间（showLoading=false）划线会闪没
      if (S.drawingsCtrl) S.drawingsCtrl.render();
      if (showLoading) setTimeout(S.resizeAllCharts, 40);
    } else {
      const p = sdState.period === 'week' ? 'week' : (sdState.period === 'month' ? 'month' : 'day');
      const res = await authFetch(`/api/market/kline?symbol=${encodeURIComponent(sdState.symbol)}&period=${p}&fq=${sdState.fq}`, {
        signal: currentCtrl.signal,
      });
      const json = await res.json();
      if (seq !== _loadSeq) return;

      if (!res.ok || (json && json.error)) {
        if (showLoading && (!sdState.klineData || !sdState.klineData.length)) {
          showChartError(Fund.localizeApiError(res.status,json?.error));
        }
        return;
      }

      hideChartError();
      sdState.seriesSource=json.source || 'unknown';
      const rawList = (json && (json.candles || json.klines)) || [];
      const last = rawList.length ? rawList[rawList.length - 1] : null;
      const sig = last ? `${sdState.symbol}_${p}_${last.time || last.date}_${last.close}_${last.high || 0}_${last.low || 0}_${last.volume || 0}_${rawList.length}` : '';
      if (!showLoading && sig && sig === _lastKlineSig) {
        return;
      }
      _lastKlineSig = sig;

      sdState.klineData = rawList.map(c => ({
        date: c.time || c.date,
        open: Number(c.open),
        close: Number(c.close),
        high: Number(c.high),
        low: Number(c.low),
        volume: Number(c.volume || 0),
      }));
      S.renderKline(sdState.klineData);
      // 同上：renderKline notMerge 擦除后先以内存画线秒级恢复，reload 异步对账
      if (S.drawingsCtrl) S.drawingsCtrl.render();
      if (showLoading) setTimeout(S.resizeAllCharts, 40);
    }

    if (showLoading && S.drawingsCtrl) {
      S.drawingsCtrl.reload();
    }
  } catch (e) {
    if (seq === _loadSeq && showLoading) {
      if (e.name === 'AbortError' || currentCtrl.signal.aborted) {
        showChartError(t('fundDetailStockQuoteTimeout'));
      } else {
        showChartError(t('fundDetailStockQuoteConnection'));
      }
    }
  } finally {
    clearTimeout(timeoutId);
    if (_chartAbortCtrl === currentCtrl) {
      _chartAbortCtrl = null;
    }
    if (seq === _loadSeq && loading) loading.style.display = 'none';
  }
}

let detailQuoteController=null, detailQuoteGeneration=0;
async function loadDetailQuote() {
  detailQuoteController?.abort(); detailQuoteController=new AbortController();const signal=detailQuoteController.signal,seq=++detailQuoteGeneration,symbol=sdState.symbol;
  try { const json=await api('GET','/api/market/detail?symbol='+encodeURIComponent(symbol)+'&name='+encodeURIComponent(sdState.name),null,{signal});if(seq!==detailQuoteGeneration||!sdState.active||symbol!==sdState.symbol)return;updateDetailPanel(json); }
  catch(e) { if(!signal.aborted&&seq===detailQuoteGeneration&&sdState.active)showChartError(Fund.localizeApiError(e.status||500,e.code)); }
}

// ---------- 外部主入口：openStockDetail ----------
function openStockDetail(symbol, name, instrumentKind, holdingContext) {
  if (!symbol) return;
  Fund.leaveFundDetail?.();
  Fund.holdingSummary?.set(holdingContext);
  if (!sdState.active) {
    const section = [...document.querySelectorAll(".main-column > section")].find(node => !node.hidden);
    returnView = { focus: document.activeElement, scroll: [document.scrollingElement, section, ...(section?.querySelectorAll("*") || [])].filter(node => node && (node.scrollTop || node.scrollLeft)).map(node => ({node, top:node.scrollTop, left:node.scrollLeft})) };
  }
  if (actions.leavePositions) actions.leavePositions();
  actions.leaveAssets?.();actions.leaveTx?.();
  const wasActive = sdState.active, changed = sdState.symbol !== symbol;
  sdState.instrumentKind = Fund.researchSeries.kind(symbol, '', instrumentKind);
  if (sdState.instrumentKind === 'fund' && sdState.period === 'minute') sdState.period = 'dayk';
  if (changed) {
    sdState.klineData = []; sdState.minuteData = null; sdState.navData = []; sdState.detail = null;
    Object.values(C).forEach(chart => chart?.clear());
    document.getElementById('sd-stock-name').textContent = name || symbol;
    document.getElementById('sd-stock-code').textContent = symbol;
    document.getElementById('sd-stock-price').textContent = '–';
    document.getElementById('sd-stock-change').textContent = '–';
  }
  S.drawingsCtrl?.cancelDraft();
  sdState.active = true;
  sdState.symbol = symbol;
  document.dispatchEvent(new Event('fund:detail-selection-changed'));
  sdState.name = safeDecode(name || symbol);
  _lastMinuteSig = null;
  _lastKlineSig = null;
  hideChartError();

  // 记录先前的页签
  const activeView=document.querySelector('.main-column > section:not([hidden])');
  if(activeView&&!wasActive)sdState.previousTab=activeView.id.replace('view-','');

  // 隐藏其他所有普通视图
  document.querySelectorAll('.main-column > section').forEach(sec => {
    if (sec.id !== 'view-stock-detail') sec.hidden = true;
  });

  // 主导航取消选中高亮（保持原7个tab不变，不插入个股详情title）
  document.querySelectorAll('nav.main-nav button').forEach(b => {
    b.setAttribute('aria-selected', 'false');
  });

  // 隐藏外部侧边栏，启用全宽双栏看盘，右侧看板默认完全展开展示
  const sideBody = document.querySelector('.workspace-body');
  if (sideBody) {
    sideBody.classList.add('stock-detail-active');
    sideBody.classList.remove('sd-side-collapsed');
  }
  // 右栏折叠统一由顶栏 #btn-toggle-side 接管：进入详情时同步其视觉态
  const sideToggleBtn = document.getElementById('btn-toggle-side');
  if (sideToggleBtn) {
    sideToggleBtn.setAttribute('aria-pressed', 'false');
    sideToggleBtn.classList.remove('active');
    sideToggleBtn.dataset.i18nTitle='fundDetailStockUiCollapsePanel';sideToggleBtn.title=t(sideToggleBtn.dataset.i18nTitle);
  }

  const v = document.getElementById('view-stock-detail');
  if (v) v.hidden = false;

  // 确保 DOM 展开完成并计算出真实像素尺寸后再初始化图表
  requestAnimationFrame(() => {
    const { ensureEcharts } = globalThis.Fund;
    const boot = () => {
      S.initCharts();
      switchPeriod(sdState.period || 'minute');
      resumeStockDetail();
      setTimeout(S.resizeAllCharts, 60);
    };
    if (ensureEcharts) {
      ensureEcharts().catch(() => {}).then(boot);
    } else {
      boot();
    }
  });
}

// ---------- 交易时段自适应调度 (工作日开市3秒秒级刷新 / 闭市单次定格静止) ----------
function isSessionOpen() {
  const ms = globalThis.Fund.marketSession;
  if (ms && typeof ms.isMarketOpen === 'function') {
    return ms.isMarketOpen();
  }
  const d = new Date();
  const day = d.getDay();
  if (day === 0 || day === 6) return false;
  const hm = d.getHours() * 100 + d.getMinutes();
  return (hm >= 915 && hm <= 1130) || (hm >= 1300 && hm <= 1505);
}

function pauseStockDetail() {
  cancelSideResize();
  _chartAbortCtrl?.abort(); ++_loadSeq;
  S.drawingsCtrl?.pause();
  detailQuoteController?.abort(); ++detailQuoteGeneration;
  if (_refreshTimer) {
    clearTimeout(_refreshTimer);
    clearInterval(_refreshTimer);
    _refreshTimer = null;
  }
}

function resumeStockDetail() {
  pauseStockDetail();
  if (!sdState.active || !sdState.symbol) return;

  // 1. 无论开闭市，进入或激活个股详情时必然请求一次最新数据
  if (sdState.instrumentKind !== 'fund') loadDetailQuote();
  loadChartData(true);

  // 2. 自适应调度：
  //    - 开市期间：3000ms 秒级平滑静默刷新
  //    - 闭市期间：停止高频轮询，60s 仅做低频时钟换挡检查，页面完全保持静止
  function scheduleNextTick() {
    if (!sdState.active || !sdState.symbol) return;
    const open = isSessionOpen();
    const delay = open ? ([3,10,30,60].includes(Fund.state.quoteRefreshSeconds)?Fund.state.quoteRefreshSeconds:3)*1000 : 60000;

    _refreshTimer = setTimeout(() => {
      if (!sdState.active || !sdState.symbol) return;
      if (isSessionOpen() && sdState.instrumentKind !== 'fund') {
        loadDetailQuote();
        loadChartData(false);
      }
      scheduleNextTick();
    }, delay);
  }

  scheduleNextTick();
}

// ---------- 返回上一页 ----------
function exitStockDetail() {
  actions.exitFocusMode?.();
  S.drawingsCtrl?.pause();
  sdState.active = false;
  Fund.holdingSummary?.clear();
  pauseStockDetail();
  hideChartError();
  if (_chartAbortCtrl) {
    try { _chartAbortCtrl.abort(); } catch (e) {}
    _chartAbortCtrl = null;
  }

  const sideBody = document.querySelector('.workspace-body');
  if (sideBody) {
    sideBody.classList.remove('stock-detail-active');
    sideBody.classList.remove('sd-side-collapsed');
  }
  // 顶栏折叠按钮在详情期间托管了右栏态，退出时恢复其外层侧栏视觉
  if (typeof actions.syncStockDetailSideBtn === 'function') actions.syncStockDetailSideBtn();

  const v = document.getElementById('view-stock-detail');
  if (v) v.hidden = true;

  // 切换回先前的页签并激活对应主导航按钮
  const targetTab = document.getElementById('tab-' + (sdState.previousTab || 'watchlist'));
  if (targetTab) {
    targetTab.click();
    requestAnimationFrame(() => {
      returnView?.scroll.forEach(({node,top,left}) => {if(node.isConnected){node.scrollTop=top;node.scrollLeft=left;}});
      (returnView?.focus?.isConnected ? returnView.focus : targetTab).focus({preventScroll:true});
      returnView=null;
    });
  }
}

// ---------- 绑定 UI 事件 ----------
function bindEvents() {
  const backBtn = document.getElementById('sd-btn-back');
  if (backBtn) backBtn.addEventListener('click', exitStockDetail);

  const resetBtn = document.getElementById('sd-zoom-reset-btn');
  if (resetBtn) resetBtn.addEventListener('click', S.resetZoomAll);

  const starBtn = document.getElementById('sd-btn-star');
  if (starBtn) starBtn.addEventListener('click', toggleFav);
  const dashFavBtn = document.getElementById('sd-btn-dash-fav');
  if (dashFavBtn) dashFavBtn.addEventListener('click', toggleFav);

  const refreshBtn = document.getElementById('sd-btn-refresh');
  if (refreshBtn) {
    refreshBtn.addEventListener('click', () => {
      loadDetailQuote();
      loadChartData(true);
    });
  }
  const retryBtn = document.getElementById('sd-retry-btn');
  if (retryBtn) {
    retryBtn.addEventListener('click', () => {
      loadDetailQuote();
      loadChartData(true);
    });
  }

  // 周期按钮
  ['minute', 'dayk', 'week', 'month'].forEach(p => {
    const btn = document.getElementById('sd-view-' + p);
    if (btn) btn.addEventListener('click', () => switchPeriod(p));
  });

  // 缩放范围按钮（支持全部 800 根展示）
  const rangeGroup = document.getElementById('sd-range-group');
  if (rangeGroup) {
    rangeGroup.querySelectorAll('button[data-range]').forEach(btn => {
      btn.addEventListener('click', () => {
        rangeGroup.querySelectorAll('button[data-range]').forEach(b => b.classList.remove('active'));
        btn.classList.add('active');
        sdState.range = parseInt(btn.dataset.range, 10) || 0;
        if (sdState.range === 0) {
          S.resetZoomAll();
        } else {
          const rows=detailRows(),total=rows.length;
          if (total > 0 && C.kline) {
            const latest=new Date(rows.at(-1).date+'T00:00:00Z'),months={20:1,60:3,120:6,250:12}[sdState.range];
            let first=0;if(months&&Number.isFinite(latest.getTime())){latest.setUTCMonth(latest.getUTCMonth()-months);first=Math.max(0,rows.findIndex(row=>row.date>=latest.toISOString().slice(0,10)));}
            const startPct=first/Math.max(1,total-1)*100;
            C.kline.dispatchAction({ type: 'dataZoom', start: startPct, end: 100 });
            if (C.volume) C.volume.dispatchAction({ type: 'dataZoom', start: startPct, end: 100 });
            if (C.indicator) C.indicator.dispatchAction({ type: 'dataZoom', start: startPct, end: 100 });
          }
        }
      });
    });
  }

  // 副图指标按钮
  const indBar = document.getElementById('sd-indicator-bar');
  if (indBar) {
    indBar.querySelectorAll('.it-btn').forEach(btn => {
      btn.addEventListener('click', () => {
        indBar.querySelectorAll('.it-btn').forEach(b => b.classList.remove('active'));
        btn.classList.add('active');
        sdState.indicator = btn.dataset.ind || 'none';
        S.renderSubIndicator(sdState.klineData);
      });
    });
  }

  // 复权方式按钮切换 (qfq/hfq/bf)
  const fqGroup = document.getElementById('sd-fq-group');
  if (fqGroup) {
    fqGroup.querySelectorAll('button[data-fq]').forEach(btn => {
      btn.addEventListener('click', () => {
        fqGroup.querySelectorAll('button[data-fq]').forEach(b => b.classList.remove('active'));
        btn.classList.add('active');
        sdState.fq = btn.dataset.fq || 'qfq';
        sdState.klineData=[];C.kline?.clear();S.drawingsCtrl?.cancelDraft();document.dispatchEvent(new Event('fund:detail-selection-changed'));
        loadChartData(true);
      });
    });
  }

  // Explicit result selection; stale search replies cannot change the current instrument.
  const searchInput = document.getElementById('sd-search-input'), searchResults = document.getElementById('sd-search-results');
  let searchTimer, searchController, searchGeneration=0, hits=[], selectedHit=-1;
  const closeSearch = () => { clearTimeout(searchTimer); searchController?.abort(); ++searchGeneration; hits=[]; selectedHit=-1; searchResults.replaceChildren(); searchResults.hidden=true; searchInput.setAttribute('aria-expanded','false'); searchInput.removeAttribute('aria-activedescendant'); };
  const searchStatus = key => { searchResults.replaceChildren(); const p=document.createElement('p');p.textContent=Fund.t(key);searchResults.append(p);searchResults.hidden=false;searchResults.style.display='block';searchInput.setAttribute('aria-expanded','true'); };
  const choose = hit => { const kind=Fund.researchSeries.kind(hit.symbol||hit.code,hit.market); closeSearch();searchInput.value=[hit.code,hit.name].filter(Boolean).join(' ');openStockDetail(hit.symbol||hit.code,hit.name,kind); };
  const search = async keyword => {
    searchController?.abort();searchController=new AbortController();const signal=searchController.signal,seq=++searchGeneration;
    searchStatus('fundFocusSearchLoading');
    try { const res=await api('GET','/api/market/suggest?input='+encodeURIComponent(keyword),null,{signal});if(seq!==searchGeneration||searchInput.value.trim()!==keyword)return;
      hits=(res.suggestions||[]).slice(0,50);selectedHit=-1;
      if(!hits.length){searchStatus('fundLedgerNoResults');return;}
      searchResults.replaceChildren();searchResults.hidden=false;
      hits.forEach((hit,index)=>{const button=document.createElement('button');button.type='button';button.className='sd-search-item';button.id='detail-search-'+index;button.setAttribute('role','option');button.setAttribute('aria-selected','false');button.textContent=[hit.name,hit.code,hit.market].filter(Boolean).join(' · ');button.onclick=()=>choose(hit);searchResults.append(button);});
    } catch(e) { if(!signal.aborted&&seq===searchGeneration)searchStatus('fundFocusSearchFailed'); }
  };
  searchInput.setAttribute('role','combobox');searchInput.setAttribute('aria-controls','sd-search-results');searchInput.setAttribute('aria-autocomplete','list');searchInput.setAttribute('aria-expanded','false');searchResults.setAttribute('role','listbox');
  searchInput.addEventListener('input',()=>{closeSearch();const keyword=searchInput.value.trim();if(keyword)searchTimer=setTimeout(()=>search(keyword),250);});
  searchInput.addEventListener('keydown',event=>{
    if(event.key==='Escape'){event.preventDefault();event.stopPropagation();closeSearch();}
    else if(['ArrowDown','ArrowUp'].includes(event.key)&&hits.length){event.preventDefault();selectedHit=selectedHit<0?(event.key==='ArrowDown'?0:hits.length-1):(selectedHit+(event.key==='ArrowDown'?1:hits.length-1))%hits.length;[...searchResults.children].forEach((button,i)=>{button.classList.toggle('active',i===selectedHit);button.setAttribute('aria-selected',String(i===selectedHit));});searchInput.setAttribute('aria-activedescendant','detail-search-'+selectedHit);}
    else if(event.key==='Enter'){event.preventDefault();if(hits[selectedHit])choose(hits[selectedHit]);else if(searchInput.value.trim()){clearTimeout(searchTimer);search(searchInput.value.trim());}}
  });
  document.addEventListener('click',event=>{if(!event.target.closest('.sd-search-wrap'))closeSearch();});
  window.addEventListener('pagehide',closeSearch);
  document.addEventListener('fund:language-changed',()=>{closeSearch();searchInput.setAttribute('aria-label',Fund.t('fundFocusSearchLabel'));searchInput.placeholder=Fund.t('fundFocusSearchLabel');});

  // 右栏折叠统一由顶栏 #btn-toggle-side 接管（app.js 统一分发点击与 ⌘B）：
  // 个股详情激活时切换单独的右栏（sd-side-collapsed），不触碰外层侧栏状态
  function setSdSideCollapsed(collapsed) {
    const wb = document.querySelector('.workspace-body');
    if (!wb) return;
    wb.classList.toggle('sd-side-collapsed', collapsed);
    const btn = document.getElementById('btn-toggle-side');
    if (btn) {
      btn.setAttribute('aria-pressed', String(collapsed));
      btn.classList.toggle('active', collapsed);
      btn.dataset.i18nTitle=collapsed?'fundDetailStockUiExpandPanel':'fundDetailStockUiCollapsePanel';btn.title=t(btn.dataset.i18nTitle);
    }
    S.resizeAllCharts();
    setTimeout(S.resizeAllCharts, 50);
  }
  actions.isStockDetailActive = () => sdState.active;
  actions.toggleStockDetailSide = () => {
    const wb = document.querySelector('.workspace-body');
    setSdSideCollapsed(!(wb && wb.classList.contains('sd-side-collapsed')));
  };
  actions.syncStockDetailSideBtn = () => {
    // 退出个股详情时恢复顶栏按钮对外层侧栏的视觉态
    const btn = document.getElementById('btn-toggle-side');
    if (!btn) return;
    const collapsed = document.querySelector('.workspace-body')?.classList.contains('side-collapsed');
    btn.setAttribute('aria-pressed', String(!!collapsed));
    btn.classList.toggle('active', !!collapsed);
    btn.dataset.i18nTitle=collapsed?'fundDetailStockUiExpandPanel':'fundDetailStockUiCollapsePanel';btn.title=t(btn.dataset.i18nTitle);
  };

  // 左右双栏拖拽调宽分隔条 (#sd-side-resizer)
  const sdResizer = document.getElementById('sd-side-resizer');
  if (sdResizer) {
    let gesture = null;
    const width = () => parseInt(getComputedStyle(document.documentElement).getPropertyValue('--fund-side-width'),10) || 320;
    const apply = value => {const next=Math.max(260,Math.min(680,value));document.documentElement.style.setProperty('--fund-side-width',next+'px');sdResizer.setAttribute('aria-valuenow',String(next));S.resizeAllCharts();return next;};
    const persist = () => {try{localStorage.setItem('natives-fund-side-width',String(width()));}catch{}};
    const end = commit => {if(!gesture)return;const current=gesture;gesture=null;if(!commit)apply(current.width);else persist();sdResizer.classList.remove('dragging');document.body.style.cursor='';document.body.style.userSelect='';if(sdResizer.hasPointerCapture(current.id))sdResizer.releasePointerCapture(current.id);};
    cancelSideResize = () => end(false);
    sdResizer.setAttribute('aria-valuemin','260');sdResizer.setAttribute('aria-valuemax','680');sdResizer.setAttribute('aria-valuenow',String(width()));
    sdResizer.addEventListener('pointerdown',event=>{if(event.button!==0)return;gesture={id:event.pointerId,width:width(),x:event.clientX};sdResizer.setPointerCapture(event.pointerId);sdResizer.classList.add('dragging');document.body.style.cursor='col-resize';document.body.style.userSelect='none';event.preventDefault();});
    sdResizer.addEventListener('pointermove',event=>{if(gesture?.id===event.pointerId)apply(gesture.width+gesture.x-event.clientX);});
    sdResizer.addEventListener('pointerup',event=>{if(gesture?.id===event.pointerId)end(true);});
    sdResizer.addEventListener('pointercancel',cancelSideResize);
    sdResizer.addEventListener('lostpointercapture',cancelSideResize);
    window.addEventListener('blur',cancelSideResize);window.addEventListener('pagehide',cancelSideResize);
    document.addEventListener('keydown',event=>{if(event.key==='Escape'&&gesture){event.preventDefault();cancelSideResize();}});
    sdResizer.addEventListener('keydown',event=>{if(!['ArrowLeft','ArrowRight','Home','End'].includes(event.key))return;event.preventDefault();apply(event.key==='Home'?260:event.key==='End'?680:width()+(event.key==='ArrowLeft'?20:-20));persist();});
  }

  // 右侧盘口卡片流折叠手风琴交互
  document.querySelectorAll('.sd-right-panel .side-card-header').forEach(header => {
    header.addEventListener('click', () => {
      const card=header.closest('.side-card');card?.classList.toggle('collapsed');header.setAttribute('aria-expanded',String(!card?.classList.contains('collapsed')));
    });
  });
}

// 注册全局动作
actions.openStockDetail = openStockDetail;
actions.openStockDetailView = openStockDetail;
actions.exitStockDetail = exitStockDetail;
actions.stockDetailSelection = () => ({ symbol: sdState.symbol, name: sdState.name, instrumentKind:sdState.instrumentKind });
const detailRows=()=>sdState.instrumentKind==='fund'?Fund.researchSeries.navRows(sdState.navData||[],sdState.period).map(row=>({...row,date:row.navDate})):sdState.period==='minute'?(sdState.minuteData?.points||[]).map(row=>({...row,date:[sdState.minuteData.date,row.time].filter(Boolean).join(' ')})):sdState.klineData;
const detailReader=Fund.researchSeries.reader({id:'sd',toolbar:document.querySelector('.sd-chart-toolbar-wrap'),rows:detailRows,chart:()=>sdState.period==='minute'?C.minute:C.kline,keys:()=>sdState.instrumentKind==='fund'?['date','unitNav','source','fetchedAt']:sdState.period==='minute'?['date','price','volume']:['date','open','high','low','close','volume'],chartNodes:[document.getElementById('sd-kline-chart'),document.getElementById('sd-minute-chart')]});
document.addEventListener('fund:detail-selection-changed',detailReader.reset);
document.getElementById('sd-ai-review').addEventListener('click',()=>{
  const identity=()=>JSON.stringify({symbol:sdState.symbol,period:sdState.period,fq:sdState.fq,range:(sdState.period==='minute'?C.minute:C.kline)?.getOption().dataZoom?.[0]});
  Fund.home.openAI({research:{identity,capture:()=>{
    const nav=sdState.instrumentKind==='fund',minute=sdState.period==='minute';
    const rows=nav?Fund.researchSeries.navRows(sdState.navData || [],sdState.period).map(row=>({date:row.navDate,unitNav:row.unitNav})):minute?(sdState.minuteData?.points || []).map(row=>({date:[sdState.minuteData.date,row.time].filter(Boolean).join(' '),price:row.price,volume:row.volume})):sdState.klineData;
    return Fund.researchSeries.capture({symbol:sdState.symbol,period:sdState.period,fq:nav?'not-applicable':minute?'bf':sdState.fq,seriesKind:nav?'NAV':minute?'minute':'OHLC'},rows,minute?C.minute:C.kline,nav?sdState.navData?.at(-1)?.source:sdState.seriesSource);
  }}});
});
actions.cancelStockDetailDrawing = () => S.drawingsCtrl?.cancelDraft();
actions.pauseStockDetail = pauseStockDetail;
actions.resumeStockDetail = resumeStockDetail;

document.addEventListener('DOMContentLoaded', bindEvents);

})();
