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
  }).catch(function(e) {});
}
actions.loadPositions = loadPositions;

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
  }).catch(function(e) {});
}
actions.loadTx = loadTx;

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
