(() => {
const { state, actions, api, themeColor, themeRgba } = globalThis.Fund;
// Fund 领域模块：实时数据与盘口看板。职责一句话——维护 WebSocket 推流订阅、
// 预警规则引擎、Overview KPI、跑马灯指数与标的详情（盘口/委比/委差）渲染。
// import 方向：core（状态/api）；跨域经 actions（app.selectSymbol、market.renderWatchlistTable 等）。

// ---------- WebSocket 全双工长连接管理 ----------
function initWebSocket() {
  try {
    state.ws = new WebSocket("ws://127.0.0.1:8765/ws");
    state.ws.onopen = function() {
      state.wsConnected = true;
      const badge = document.getElementById("ws-status");
      badge.textContent = "WS 推流中";
      badge.className = "ws-badge online";
      syncWSSubscriptions();
    };
    state.ws.onmessage = function(e) {
      try {
        const msg = JSON.parse(e.data);
        if (msg.type === "snapshot" || msg.type === "tick") {
          handleWSTick(msg.data);
        }
      } catch (err) {}
    };
    state.ws.onclose = function() {
      state.wsConnected = false;
      const badge = document.getElementById("ws-status");
      badge.textContent = "WS 离线(轮询中)";
      badge.className = "ws-badge offline";
      setTimeout(initWebSocket, 4000); // 自动指数退避重连
    };
    state.ws.onerror = function() {
      state.ws.close();
    };
  } catch (e) {
    state.wsConnected = false;
  }
}
actions.initWebSocket = initWebSocket;

function syncWSSubscriptions() {
  if (!state.ws || state.ws.readyState !== WebSocket.OPEN) return;
  const symbols = state.watchlistItems.map(function(it) { return it.symbol; });
  if (!symbols.includes(state.currentSymbol)) symbols.push(state.currentSymbol);
  state.ws.send(JSON.stringify({ action: "subscribe", symbols: symbols }));
}
actions.syncWSSubscriptions = syncWSSubscriptions;

function handleWSTick(data) {
  if (!Array.isArray(data)) data = [data];
  let updated = false;
  data.forEach(function(item) {
    if (!item || !item.symbol) return;
    let sym = item.symbol;
    if (sym.length === 6 && !sym.startsWith("sh") && !sym.startsWith("sz") && !sym.startsWith("bj")) {
      sym = (item.id && item.id.includes("A:")) ? ((item.symbol.startsWith("6") || item.symbol.startsWith("5")) ? "sh" + item.symbol : "sz" + item.symbol) : item.symbol;
    }
    state.lastQuotesMap[sym] = item.price;
    checkAlerts(sym, item);

    if (sym === state.currentSymbol || item.symbol === state.currentSymbol) {
      updateDetailFromNormalized(item);
    }
    updated = true;
  });
  if (updated) {
    actions.renderWatchlistTable();
    updateKPI();
  }
}

// ---------- 预警规则引擎 (Alerts Engine) ----------
function checkAlerts(symbol, item) {
  const threshold = state.alertRules[symbol];
  if (!threshold) return;
  const pct = Math.abs(item.changePercent || 0);
  if (pct >= threshold) {
    state.triggeredAlertsCount++;
    document.getElementById("kpi-active-alerts").textContent = state.triggeredAlertsCount;
    document.getElementById("kpi-alerts-count").textContent = state.triggeredAlertsCount + " 次";

    // 联动 Chrome Badge 与父级 Shell
    const sign = item.changePercent >= 0 ? "+" : "";
    const badgeText = sign + item.changePercent.toFixed(1) + "%";
    if (window.parent) {
      window.parent.postMessage({
        type: "invest:alert",
        symbol: symbol,
        name: item.name,
        changePercent: item.changePercent,
        badgeText: badgeText,
        isUp: item.changePercent >= 0
      }, "*");
    }

    // 系统 Notification
    if (window.Notification && Notification.permission === "granted") {
      new Notification("投资异动预警: " + item.name, {
        body: "标的 " + symbol + " 涨跌幅已达 " + badgeText + "，当前价 " + item.price.toFixed(2),
        icon: "icons/folder-32.png"
      });
    }
  }
}

document.getElementById("btn-set-alert").addEventListener("click", function() {
  const val = parseFloat(document.getElementById("alert-threshold-input").value) || 3.0;
  state.alertRules[state.currentSymbol] = val;
  const status = document.getElementById("alert-status-text");
  status.textContent = "已设 ±" + val + "%";
  status.className = "up";
  if (window.Notification && Notification.permission !== "granted") {
    Notification.requestPermission();
  }
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
    const chgPct = it._lastChangePct || 0;
    if (chgPct > 0) upCount++;
    else if (chgPct < 0) downCount++;

    if (!maxGainer || chgPct > maxGainer.pct) maxGainer = { name: it.name, pct: chgPct, price: qPrice };
    if (!maxLoser || chgPct < maxLoser.pct) maxLoser = { name: it.name, pct: chgPct, price: qPrice };
  });

  document.getElementById("kpi-total-count").textContent = total;
  document.getElementById("kpi-up-down").textContent = upCount + "涨 / " + downCount + "跌";
  const ratio = total > 0 ? Math.round((upCount / total) * 100) : 50;
  document.getElementById("kpi-bull-bear-ratio").textContent = "多头占比: " + ratio + "%";

  if (maxGainer && maxGainer.pct > 0) {
    document.getElementById("kpi-top-gainer-name").textContent = maxGainer.name;
    document.getElementById("kpi-top-gainer-pct").textContent = "+" + maxGainer.pct.toFixed(2) + "%";
    document.getElementById("kpi-top-gainer-price").textContent = "现价: " + (maxGainer.price ? maxGainer.price.toFixed(2) : "–");
  }
  if (maxLoser && maxLoser.pct < 0) {
    document.getElementById("kpi-top-loser-name").textContent = maxLoser.name;
    document.getElementById("kpi-top-loser-pct").textContent = maxLoser.pct.toFixed(2) + "%";
    document.getElementById("kpi-top-loser-price").textContent = "现价: " + (maxLoser.price ? maxLoser.price.toFixed(2) : "–");
  }
}
actions.updateKPI = updateKPI;

// ---------- 全局跑马灯大盘指数刷新 ----------
function refreshTickers() {
  api("GET", "/api/market/indices").then(function(res) {
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
  }).catch(function(e) {});
}
actions.refreshTickers = refreshTickers;

// ---------- 标的详情：头部价格 + 指标网格 + 五档盘口 ----------
function loadDetailQuote() {
  if (!state.currentSymbol) return;
  api("GET", "/api/market/detail?symbol=" + state.currentSymbol + "&name=" + encodeURIComponent(state.currentName || state.currentSymbol))
    .then(function(data) {
      updateDetailFromNormalized(data);
    }).catch(function(e) {});
}
actions.loadDetailQuote = loadDetailQuote;

function updateDetailFromNormalized(data) {
  const priceEl = document.getElementById("dash-price");
  const chgEl = document.getElementById("dash-change");
  priceEl.textContent = data.price.toFixed(2);
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

function renderDepth(data) {
  const asksEl = document.getElementById("depth-asks");
  const bidsEl = document.getElementById("depth-bids");
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
    askHtml += "<div class='depth-row' style='background:linear-gradient(to left, rgba(255,45,45,0.18) " + barRatio + "%, transparent " + barRatio + "%);'><span class='depth-label'>卖" + labels[i] + "</span><span class='num down'>" + p + "</span><span class='num'>" + v + "</span></div>";
  }
  asksEl.innerHTML = askHtml;

  let bidHtml = "";
  for (let j = 0; j < 5; j++) {
    const bitem = bids[j];
    let bp = "–", bv = "–", barRatio = 0;
    if (Array.isArray(bitem)) { bp = bitem[0] ? bitem[0].toFixed(2) : "–"; bv = bitem[1] || "–"; barRatio = Math.round((volOf(bitem) / maxDepthVol) * 100); }
    else if (bitem) { bp = bitem.price ? bitem.price.toFixed(2) : "–"; bv = bitem.volume || "–"; barRatio = Math.round((volOf(bitem) / maxDepthVol) * 100); }
    bidHtml += "<div class='depth-row' style='background:linear-gradient(to left, rgba(0,179,92,0.18) " + barRatio + "%, transparent " + barRatio + "%);'><span class='depth-label'>买" + labels[j] + "</span><span class='num up'>" + bp + "</span><span class='num'>" + bv + "</span></div>";
  }
  bidsEl.innerHTML = bidHtml;

  const bidVol = data.bidVol || 0;
  const askVol = data.askVol || 0;
  const ratio = (data.bidRatio || 0.5) * 100;
  document.getElementById("bid-vol-text").textContent = bidVol;
  document.getElementById("ask-vol-text").textContent = askVol;
  document.getElementById("bid-ratio-text").textContent = ratio.toFixed(1) + "%";
  document.getElementById("power-bar-fill").style.width = ratio + "%";

  // 委比/委差（五档口径）：委比 = (买五档总量-卖五档总量)/(买+卖) × 100%
  const weibi = (bidSum + askSum) > 0 ? (bidSum - askSum) / (bidSum + askSum) * 100 : null;
  const weicha = (bidSum || askSum) ? (bidSum - askSum) : null;
  const weibiEl = document.getElementById("m-weibi");
  const weichaEl = document.getElementById("m-weicha");
  if (weibi != null) {
    weibiEl.textContent = (weibi > 0 ? "+" : "") + weibi.toFixed(2) + "%";
    weibiEl.className = "metric-val " + (weibi > 0 ? "up" : weibi < 0 ? "down" : "");
  } else { weibiEl.textContent = "–"; weibiEl.className = "metric-val"; }
  if (weicha != null) {
    weichaEl.textContent = (weicha > 0 ? "+" : "") + weicha.toFixed(0);
    weichaEl.className = "metric-val " + (weicha > 0 ? "up" : weicha < 0 ? "down" : "");
  } else { weichaEl.textContent = "–"; weichaEl.className = "metric-val"; }
}

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
