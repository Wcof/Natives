(() => {
const { state, actions, api, themeColor, themeRgba, t } = globalThis.Fund;
// Fund 领域模块：实时数据与盘口看板。职责一句话——消费轮询报价流（ingestQuoteTick）、
// 预警规则引擎、Overview KPI、跑马灯指数与标的详情（盘口/委比/委差）渲染。
// import 方向：core（状态/api）；跨域经 actions（app.selectSymbol、market.renderWatchlistTable 等）。

// 报价只驱动显示，预警统一使用持久化规则引擎。
actions.ingestQuoteTick = function () {};
document.getElementById("btn-set-alert").addEventListener("click", async function () {
  const value=Number(document.getElementById("alert-threshold-input").value);
  const status=document.getElementById("alert-status-text");
  if(!Number.isFinite(value)||value<=0||!state.currentSymbol){status.textContent="请填写有效异动阈值并选择标的";return;}
  try {await api("POST","/api/rules",{name:`${state.currentName||state.currentSymbol} 异动`,ruleType:"daily_change",objectKey:state.currentSymbol,params:{threshold:value,direction:"absolute"},period:"quote",enabled:true});status.textContent=t("alertSetPrefix")+value+t("alertSetSuffix");status.className="up";}
  catch(e){status.textContent=e.message;}
});

// ---------- Overview KPI 计算 (token-monitor 1:1) ----------
function updateKPI() {
  const total = state.watchlistItems.length;
  let upCount = 0;
  let downCount = 0;
  let maxGainer = null;
  let maxLoser = null;

  state.watchlistItems.forEach(function(it) {
    const qPrice = state.lastQuotesMap[it.symbol];
    const chgPct = it._lastChangePct;
    if (chgPct == null) return;
    if (chgPct > 0) upCount++;
    else if (chgPct < 0) downCount++;

    if (!maxGainer || chgPct > maxGainer.pct) maxGainer = { name: it.name, pct: chgPct, price: qPrice };
    if (!maxLoser || chgPct < maxLoser.pct) maxLoser = { name: it.name, pct: chgPct, price: qPrice };
  });

  document.getElementById("kpi-total-count").textContent = total;
  document.getElementById("kpi-up-down").textContent = t('fundMarketUpDown', {up:upCount,down:downCount});
  const priced = state.watchlistItems.filter(it => it._lastChangePct != null).length;
  const ratio = priced > 0 ? Math.round((upCount / priced) * 100) : null;
  document.getElementById("kpi-bull-bear-ratio").textContent = t('fundMarketPricedRatio', {ratio:ratio == null ? '–' : ratio + '%',n:priced});
  for (const id of ['gainer','loser']) {
    document.getElementById('kpi-top-'+id+'-name').textContent='–';
    document.getElementById('kpi-top-'+id+'-pct').textContent='–';
    document.getElementById('kpi-top-'+id+'-price').textContent='–';
  }

  if (maxGainer && maxGainer.pct > 0) {
    document.getElementById("kpi-top-gainer-name").textContent = maxGainer.name;
    document.getElementById("kpi-top-gainer-pct").textContent = "+" + maxGainer.pct.toFixed(2) + "%";
    document.getElementById("kpi-top-gainer-price").textContent = maxGainer.price != null ? maxGainer.price.toFixed(2) : "–";
  }
  if (maxLoser && maxLoser.pct < 0) {
    document.getElementById("kpi-top-loser-name").textContent = maxLoser.name;
    document.getElementById("kpi-top-loser-pct").textContent = maxLoser.pct.toFixed(2) + "%";
    document.getElementById("kpi-top-loser-price").textContent = maxLoser.price != null ? maxLoser.price.toFixed(2) : "–";
  }
}
actions.updateKPI = updateKPI;

// ---------- 全局跑马灯大盘指数刷新 ----------
let tickerPending=false;
function refreshTickers() {
  if(tickerPending)return;
  tickerPending=true;
  api("GET", "/api/market/indices").then(function(res) {
    document.dispatchEvent(new CustomEvent("fund:market-indices",{detail:res}));
    const quotes = res.quotes || [];
    const map = {};
    quotes.forEach(function(q) { map[q.code] = q; });
    const items = document.querySelectorAll("#market-tickers .ticker-item");
    items.forEach(function(el) {
      const c = el.dataset.code;
      const q = map[c];
      if (q && q.price != null) {
        const valSpan = el.querySelector(".ticker-val");
        const pct = (q.changePct || 0).toFixed(2);
        const sign = q.changePct > 0 ? "+" : "";
        valSpan.textContent = q.price.toFixed(2) + " (" + sign + pct + "%)";
        valSpan.className = "ticker-val " + (q.changePct > 0 ? "up" : q.changePct < 0 ? "down" : "");
      }
    });
  }).catch(function(e) {document.dispatchEvent(new Event("fund:market-indices-failed"));}).finally(()=>{tickerPending=false;});
}
actions.refreshTickers = refreshTickers;

// ---------- 标的详情：头部价格 + 指标网格 + 五档盘口 ----------
function loadDetailQuote() {
  if (!state.currentSymbol) return;
  const symbol=state.currentSymbol;
  api("GET", "/api/market/detail?symbol=" + encodeURIComponent(symbol) + "&name=" + encodeURIComponent(state.currentName || symbol))
    .then(function(data) {
      if(symbol!==state.currentSymbol)return;
      updateDetailFromNormalized(data);
    }).catch(function(e) {});
}
actions.loadDetailQuote = loadDetailQuote;

function updateDetailFromNormalized(data) {
  const priceEl = document.getElementById("dash-price");
  const chgEl = document.getElementById("dash-change");
  priceEl.textContent = data.price != null ? data.price.toFixed(2) : '–';
  const pct = (data.changePct || data.changePercent || 0).toFixed(2);
  const chg = (data.change || 0).toFixed(2);
  const sign = (data.changePct || data.changePercent) > 0 ? "+" : "";
  chgEl.textContent = sign + chg + " (" + sign + pct + "%)";

  const isUp = (data.changePct || data.changePercent) >= 0;
  priceEl.className = "price-large " + (isUp ? "up" : "down");
  chgEl.className = isUp ? "up" : "down";

  renderDepth(data);

  document.getElementById("m-high").textContent = data.high ? data.high.toFixed(2) : "–";
  document.getElementById("m-low").textContent = data.low ? data.low.toFixed(2) : "–";
  document.getElementById("m-open").textContent = data.open ? data.open.toFixed(2) : "–";
  document.getElementById("m-prev").textContent = data.prevClose ? data.prevClose.toFixed(2) : "–";
  document.getElementById("m-amount").textContent = data.amount ? (data.amount > 100000 ? (data.amount / 10000).toFixed(1) : data.amount.toFixed(1)) : "–";
  document.getElementById("m-turnover").textContent = data.turnover ? data.turnover.toFixed(2) : "–";
  document.getElementById("m-vol-ratio").textContent = data.volumeRatio ? data.volumeRatio.toFixed(2) : "–";
  document.getElementById("m-amplitude").textContent = data.amplitude ? data.amplitude.toFixed(2) : "–";
}

function renderDepth(data, prefix = "") {
  const pfx = prefix || "";
  const asksEl = document.getElementById(pfx + "depth-asks");
  const bidsEl = document.getElementById(pfx + "depth-bids");
  if (!asksEl || !bidsEl) return;
  const asks = (data.depth && data.depth.asks) || data.asks || [];
  const bids = (data.depth && data.depth.bids) || data.bids || [];
  const labels = ["一", "二", "三", "四", "五"];

  const volOf = function(x) {
    if (Array.isArray(x)) return Number(x[1]) || 0;
    return (x && x.volume != null) ? (Number(x.volume) || 0) : 0;
  };
  let bidSum = 0, askSum = 0;
  let maxDepthVol = 1;
  bids.forEach(function(x) { const v = volOf(x); bidSum += v; if (v > maxDepthVol) maxDepthVol = v; });
  asks.forEach(function(x) { const v = volOf(x); askSum += v; if (v > maxDepthVol) maxDepthVol = v; });

  let askHtml = "";
  for (let i = 4; i >= 0; i--) {
    const item = asks[i];
    let p = "–", v = "–", barRatio = 0;
    if (Array.isArray(item)) { p = item[0] ? item[0].toFixed(2) : "–"; v = item[1] || "–"; barRatio = Math.round((volOf(item) / maxDepthVol) * 100); }
    else if (item) { p = item.price ? item.price.toFixed(2) : "–"; v = item.volume || "–"; barRatio = Math.round((volOf(item) / maxDepthVol) * 100); }
    askHtml += "<div class='depth-row' style='background:linear-gradient(to left, " + themeRgba('--color-down',.18) + " " + barRatio + "%, transparent " + barRatio + "%);'><span class='depth-label'>" + t('fundMarketAsk') + (i+1) + "</span><span class='num down'>" + p + "</span><span class='num'>" + v + "</span></div>";
  }
  if (asksEl.innerHTML !== askHtml) asksEl.innerHTML = askHtml;

  let bidHtml = "";
  for (let j = 0; j < 5; j++) {
    const bitem = bids[j];
    let bp = "–", bv = "–", barRatio = 0;
    if (Array.isArray(bitem)) { bp = bitem[0] ? bitem[0].toFixed(2) : "–"; bv = bitem[1] || "–"; barRatio = Math.round((volOf(bitem) / maxDepthVol) * 100); }
    else if (bitem) { bp = bitem.price ? bitem.price.toFixed(2) : "–"; bv = bitem.volume || "–"; barRatio = Math.round((volOf(bitem) / maxDepthVol) * 100); }
    bidHtml += "<div class='depth-row' style='background:linear-gradient(to right, " + themeRgba('--color-up',.18) + " " + barRatio + "%, transparent " + barRatio + "%);'><span class='depth-label'>" + t('fundMarketBid') + (j+1) + "</span><span class='num up'>" + bp + "</span><span class='num'>" + bv + "</span></div>";
  }
  if (bidsEl.innerHTML !== bidHtml) bidsEl.innerHTML = bidHtml;

  const bidVol = data.bidVol != null ? data.bidVol : Math.round(bidSum);
  const askVol = data.askVol != null ? data.askVol : Math.round(askSum);
  const ratio = data.bidRatio != null ? data.bidRatio * 100 : (bidSum + askSum) > 0 ? bidSum / (bidSum + askSum) * 100 : null;
  const bvEl = document.getElementById(pfx + "bid-vol-text");
  const avEl = document.getElementById(pfx + "ask-vol-text");
  const brEl = document.getElementById(pfx + "bid-ratio-text");
  const pbEl = document.getElementById(pfx + "power-bar-fill");
  if (bvEl) bvEl.textContent = bidVol;
  if (avEl) avEl.textContent = askVol;
  if (brEl) brEl.textContent = ratio == null ? '–' : ratio.toFixed(1) + "%";
  if (pbEl) { pbEl.style.width = (ratio ?? 0) + "%"; pbEl.parentElement.hidden = ratio == null; }

  // 委比/委差（五档口径）：委比 = (买五档总量-卖五档总量)/(买+卖) × 100%
  const weibi = (bidSum + askSum) > 0 ? (bidSum - askSum) / (bidSum + askSum) * 100 : null;
  const weicha = (bidSum || askSum) ? (bidSum - askSum) : null;
  const weibiEl = document.getElementById(pfx + "m-weibi");
  const weichaEl = document.getElementById(pfx + "m-weicha");
  if (weibiEl) {
    if (weibi != null) {
      weibiEl.textContent = (weibi > 0 ? "+" : "") + weibi.toFixed(2) + "%";
      weibiEl.className = "metric-val " + (weibi > 0 ? "up" : weibi < 0 ? "down" : "");
    } else { weibiEl.textContent = "–"; weibiEl.className = "metric-val"; }
  }
  if (weichaEl) {
    if (weicha != null) {
      weichaEl.textContent = (weicha > 0 ? "+" : "") + weicha.toFixed(0);
      weichaEl.className = "metric-val " + (weicha > 0 ? "up" : weicha < 0 ? "down" : "");
    } else { weichaEl.textContent = "–"; weichaEl.className = "metric-val"; }
  }
}
actions.renderDepth = renderDepth;

// ---------- 右侧看板卡片折叠交互 ----------
document.querySelectorAll(".side-card-header").forEach(function(header) {
  header.addEventListener("click", function() {
    const card = header.closest(".side-card");
    if (card) {
      card.classList.toggle("collapsed");
    }
  });
});

})();
