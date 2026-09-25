(() => {
const { state, actions, api, themeColor, themeRgba } = globalThis.Fund;
// Fund 领域模块：记账与数据维护。职责一句话——持仓记账、交易流水、
// 批量导入与公募净值四个视图的表单与取数渲染。
// import 方向：core（状态/api）；表单提交经 api 直达模块 Host。

// ---------- 持仓记账模块 ----------
function loadPositions() {
  api("GET", "/api/positions").then(function(res) {
    const list = res.positions || [];
    const tbody = document.querySelector("#positions-table tbody");
    tbody.innerHTML = "";
    let totCost = 0;
    let totRealized = 0;

    list.forEach(function(p) {
      totCost += Number(p.cost || 0);
      totRealized += Number(p.realized || 0);
      const tr = document.createElement("tr");
      tr.innerHTML = "<td>" + p.account + "</td>" +
                     "<td><b>" + p.fundCode + "</b></td>" +
                     "<td>" + (p.fundName || p.fundCode) + "</td>" +
                     "<td class='num'>" + p.quantity + "</td>" +
                     "<td class='num'>" + p.cost + "</td>" +
                     "<td class='num " + (p.realized > 0 ? "up" : p.realized < 0 ? "down" : "") + "'>" + p.realized + "</td>" +
                     "<td class='num'>" + p.asOf + "</td>";
      tbody.appendChild(tr);
    });
    document.getElementById("pos-total-cost").textContent = totCost.toFixed(2);
    document.getElementById("pos-total-realized").textContent = totRealized.toFixed(2);
    document.getElementById("pos-total-realized").className = "num " + (totRealized > 0 ? "up" : totRealized < 0 ? "down" : "");
    state.lastPositionsList = list;
    updatePositionsSidebar(list);
  }).catch(function(e) {});
}
actions.loadPositions = loadPositions;

function updatePositionsSidebar(posList) {
  const list = posList || state.lastPositionsList || [];
  let totCost = 0, totRealized = 0;
  list.forEach(function(p) {
    totCost += Number(p.cost || 0);
    totRealized += Number(p.realized || 0);
  });
  const costEl = document.getElementById("side-pos-cost-val");
  const realizedEl = document.getElementById("side-pos-realized-val");
  const countEl = document.getElementById("side-pos-count-val");
  const distEl = document.getElementById("side-pos-dist-list");

  if (costEl) costEl.textContent = totCost.toFixed(2);
  if (realizedEl) {
    realizedEl.textContent = (totRealized > 0 ? "+" : "") + totRealized.toFixed(2);
    realizedEl.className = "num " + (totRealized > 0 ? "up" : totRealized < 0 ? "down" : "");
  }
  if (countEl) countEl.textContent = list.length + " 只";

  if (distEl) {
    if (list.length === 0) {
      distEl.innerHTML = "<div style='color:var(--natives-text-muted); text-align:center; padding:10px 0;'>暂无持仓记录</div>";
    } else {
      let html = "";
      list.slice(0, 5).forEach(function(p) {
        const cost = Number(p.cost || 0);
        const weight = totCost > 0 ? Math.round((cost / totCost) * 100) : 0;
        html += "<div style='margin-bottom:8px;'>" +
          "<div style='display:flex; justify-content:space-between; margin-bottom:2px;'>" +
            "<span><b>" + (p.fundName || p.fundCode) + "</b> <span style='font-size:10px; color:var(--natives-text-muted);'>" + p.fundCode + "</span></span>" +
            "<span class='num'>" + weight + "% (" + cost.toFixed(0) + "元)</span>" +
          "</div>" +
          "<div style='height:4px; background:var(--natives-surface-3); border-radius:2px; overflow:hidden;'>" +
            "<div style='height:100%; width:" + weight + "%; background:var(--natives-accent); border-radius:2px;'></div>" +
          "</div>" +
        "</div>";
      });
      distEl.innerHTML = html;
    }
  }
}
actions.updatePositionsSidebar = updatePositionsSidebar;

// ---------- 交易流水模块 ----------
function loadTx() {
  api("GET", "/api/transactions").then(function(res) {
    const txs = res.transactions || [];
    const tbody = document.querySelector("#tx-table tbody");
    tbody.innerHTML = "";
    txs.forEach(function(t) {
      const tr = document.createElement("tr");
      tr.innerHTML = "<td>" + t.tradeDate + "</td>" +
                     "<td>" + t.account + "</td>" +
                     "<td><b>" + t.fundCode + "</b></td>" +
                     "<td><span class='badge " + (t.type === "BUY" ? "up" : "down") + "'>" + t.type + "</span></td>" +
                     "<td class='num'>" + t.quantity + "</td>" +
                     "<td class='num'>" + t.price + "</td>" +
                     "<td class='num'>" + t.amount + "</td>" +
                     "<td class='num'>" + (t.fee || "0.00") + "</td>";
      tbody.appendChild(tr);
    });
    state.lastTxList = txs;
    updateTxSidebar(txs);
  }).catch(function(e) {});
}
actions.loadTx = loadTx;

function updateTxSidebar(txList) {
  const list = txList || state.lastTxList || [];
  let buyCount = 0, sellCount = 0, buyAmount = 0, sellAmount = 0, totFee = 0;
  list.forEach(function(t) {
    const amt = Number(t.amount || (t.quantity * t.price) || 0);
    const fee = Number(t.fee || 0);
    totFee += fee;
    if (t.type === "BUY") {
      buyCount++;
      buyAmount += amt;
    } else {
      sellCount++;
      sellAmount += amt;
    }
  });

  const totalEl = document.getElementById("side-tx-total-count");
  const buyEl = document.getElementById("side-tx-buy-amount");
  const sellEl = document.getElementById("side-tx-sell-amount");
  const feeEl = document.getElementById("side-tx-fee-amount");
  const ratioEl = document.getElementById("side-tx-ratio");

  if (totalEl) totalEl.textContent = list.length + " 笔";
  if (buyEl) buyEl.textContent = buyAmount.toFixed(2);
  if (sellEl) sellEl.textContent = sellAmount.toFixed(2);
  if (feeEl) feeEl.textContent = totFee.toFixed(2);
  if (ratioEl) ratioEl.textContent = buyCount + " 买 / " + sellCount + " 卖";
}
actions.updateTxSidebar = updateTxSidebar;

function updateNavSidebar() {
  api("GET", "/api/nav").then(function(res) {
    const list = res.navs || res.items || [];
    const fundsMap = {};
    list.forEach(function(n) { if (n.fundCode) fundsMap[n.fundCode] = true; });
    const totalFunds = Object.keys(fundsMap).length;
    const fundsEl = document.getElementById("side-nav-total-funds");
    const recordsEl = document.getElementById("side-nav-total-records");
    if (fundsEl) fundsEl.textContent = totalFunds + " 只";
    if (recordsEl) recordsEl.textContent = list.length + " 条";
  }).catch(function() {});
}
actions.updateNavSidebar = updateNavSidebar;

document.getElementById("tx-form").addEventListener("submit", function(e) {
  e.preventDefault();
  const fd = new FormData(e.target);
  const body = {
    account: fd.get("account"),
    fundCode: fd.get("fundCode"),
    type: fd.get("type"),
    quantity: fd.get("quantity"),
    price: fd.get("price"),
    fee: fd.get("fee") || "0.00",
    tradeDate: fd.get("tradeDate"),
    source: "manual",
    requestId: "tx-" + Date.now()
  };
  const msg = document.getElementById("tx-msg");
  msg.textContent = "记账中…";
  api("POST", "/api/transactions", body).then(function() {
    msg.textContent = "记账成功！";
    msg.className = "up";
    loadTx();
  }).catch(function(err) {
    msg.textContent = "记账失败：" + err.message;
    msg.className = "err";
  });
});

// ---------- 批量导入模块 ----------
document.getElementById("import-preview-btn").addEventListener("click", function() {
  const text = document.getElementById("import-text").value.trim();
  if (!text) return;
  const msg = document.getElementById("import-msg");
  msg.textContent = "解析中…";
  api("POST", "/api/import/preview", {
    csvText: text,
    templateVersion: 1,
    sourceId: "manual-import"
  }).then(function(res) {
    state.pendingImportPreview = res;
    document.getElementById("import-commit-btn").disabled = false;
    msg.textContent = "解析完成：共 " + res.total_rows + " 行，有效 " + res.valid_rows + " 行。";
    const tbody = document.querySelector("#import-table tbody");
    tbody.innerHTML = "";
    (res.lines || []).forEach(function(l) {
      const tr = document.createElement("tr");
      tr.innerHTML = "<td>" + l.line_no + "</td><td>" + l.account + "</td><td>" + l.fund_code + "</td><td>" + l.kind + "</td><td class='num'>" + l.quantity_raw + "</td><td class='num'>" + l.amount_raw + "</td><td>" + (l.error || "有效") + "</td>";
      tbody.appendChild(tr);
    });
  }).catch(function(err) { msg.textContent = "导入错误：" + err.message; msg.className = "err"; });
});

document.getElementById("import-commit-btn").addEventListener("click", function() {
  if (!state.pendingImportPreview) return;
  const text = document.getElementById("import-text").value.trim();
  api("POST", "/api/import/commit", {
    csvText: text,
    templateVersion: 1,
    sourceId: "manual-import-" + Date.now()
  }).then(function(res) {
    document.getElementById("import-msg").textContent = "提交成功！新增基金 " + res.funds_created + "，导入行数 " + res.rows_imported;
    state.pendingImportPreview = null;
    document.getElementById("import-commit-btn").disabled = true;
  });
});

// ---------- 公募基金净值模块 ----------
document.getElementById("nav-sync-btn").addEventListener("click", function() {
  const code = document.getElementById("nav-code-input").value.trim();
  if (!code) return;
  const msg = document.getElementById("nav-msg");
  msg.textContent = "同步净值中…";
  api("POST", "/api/nav/sync", { fundCode: code }).then(function(res) {
    msg.textContent = "同步成功！新增净值 " + res.added + " 条。";
    loadNavData(code);
  }).catch(function(err) { msg.textContent = "同步失败：" + err.message; msg.className = "err"; });
});

function loadNavData(code) {
  api("GET", "/api/nav?code=" + code).then(function(res) {
    const navs = res.nav || [];
    const tbody = document.querySelector("#nav-table tbody");
    tbody.innerHTML = "";
    navs.forEach(function(n) {
      const tr = document.createElement("tr");
      tr.innerHTML = "<td><b>" + n.fundCode + "</b></td><td>" + n.navDate + "</td><td class='num'>" + n.unitNav + "</td><td>" + n.source + "</td><td>" + new Date(Number(n.fetchedAt)*1000).toLocaleString() + "</td>";
      tbody.appendChild(tr);
    });
  });
}

})();
