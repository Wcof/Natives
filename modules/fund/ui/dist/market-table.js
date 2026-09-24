(() => {
const { state, actions, api } = globalThis.Fund;
// Fund 领域模块：热门板块与成分股联动表。职责一句话——维护板块分类页签、
// 板块行情表（排序/选中）与板块成分股表（排序/选中联动标的切换）。
// 自选分组与自选行情表已按职责拆分至 watchlist-table.js。
// import 方向：core（状态/api）；跨域经 actions（app.selectSymbol 等）。

// ---------- 热门板块与成分股联动模块（去除分钟K线，上下双表联动） ----------
let currentSectorType = "all";
let currentSelectedSector = null;
let lastSectorsData = [];
let lastStocksData = [];
let sectorSort = { key: "changePct", asc: false };
let stockSort = { key: "changePct", asc: false };

const sTabs = document.querySelectorAll("#sector-category-tabs .group-tab");
sTabs.forEach(function(tab) {
  tab.addEventListener("click", function() {
    sTabs.forEach(function(t) { t.classList.remove("active"); });
    tab.classList.add("active");
    currentSectorType = tab.dataset.type;
    refreshSectors();
  });
});

function refreshSectors() {
  const tbody = document.querySelector("#sectors-table tbody");
  if (!tbody) return;
  tbody.innerHTML = "<tr><td colspan='9' style='text-align:center; padding: 20px;'>加载热门板块数据中…</td></tr>";

  api("GET", "/api/market/sectors?type=" + currentSectorType).then(function(res) {
    lastSectorsData = res.sectors || res.quotes || [];
    renderSectorsTable();
  }).catch(function(err) {
    tbody.innerHTML = "<tr><td colspan='9' class='err' style='text-align:center; padding: 20px;'>获取板块行情失败：" + err.message + "</td></tr>";
  });
}
actions.refreshSectors = refreshSectors;
actions.refreshMarket = refreshSectors;

function renderSectorsTable() {
  const tbody = document.querySelector("#sectors-table tbody");
  if (!tbody) return;
  tbody.innerHTML = "";

  const items = lastSectorsData.slice();
  if (sectorSort.key) {
    items.sort(function(a, b) {
      const va = a[sectorSort.key] != null ? a[sectorSort.key] : -999999;
      const vb = b[sectorSort.key] != null ? b[sectorSort.key] : -999999;
      return sectorSort.asc ? (va > vb ? 1 : -1) : (va < vb ? 1 : -1);
    });
  }

  if (items.length === 0) {
    tbody.innerHTML = "<tr><td colspan='9' style='text-align:center; padding: 20px;'>暂无板块数据</td></tr>";
    return;
  }

  items.forEach(function(s, idx) {
    const tr = document.createElement("tr");
    tr.className = "clickable";
    if (currentSelectedSector && currentSelectedSector.code === s.code) {
      tr.classList.add("selected-sector");
    }

    const pct = s.changePct != null ? (s.changePct > 0 ? "+" : "") + s.changePct.toFixed(2) + "%" : "–";
    const pctCls = s.changePct > 0 ? "up" : s.changePct < 0 ? "down" : "";
    const speed = s.speed != null ? s.speed.toFixed(2) : "–";
    const speedCls = s.speed > 0 ? "up" : s.speed < 0 ? "down" : "";
    const topName = s.topStockName || "–";
    const topPrice = s.topStockPrice != null ? s.topStockPrice.toFixed(2) : "–";
    const topPct = s.topStockChangePct != null ? (s.topStockChangePct > 0 ? "+" : "") + s.topStockChangePct.toFixed(2) + "%" : "–";
    const topCls = s.topStockChangePct > 0 ? "up" : s.topStockChangePct < 0 ? "down" : "";
    const pct5 = s.changePct5 != null ? (s.changePct5 > 0 ? "+" : "") + s.changePct5.toFixed(2) + "%" : "–";
    const pct5Cls = s.changePct5 > 0 ? "up" : s.changePct5 < 0 ? "down" : "";
    const pct20 = s.changePct20 != null ? (s.changePct20 > 0 ? "+" : "") + s.changePct20.toFixed(2) + "%" : "–";
    const pct20Cls = s.changePct20 > 0 ? "up" : s.changePct20 < 0 ? "down" : "";

    tr.innerHTML = "<td style='color:var(--natives-text-muted);'>" + (idx + 1) + "</td>" +
                   "<td><b>" + s.name + "</b></td>" +
                   "<td class='num " + pctCls + "'>" + pct + "</td>" +
                   "<td class='num " + speedCls + "'>" + speed + "</td>" +
                   "<td>" + topName + "</td>" +
                   "<td class='num " + topCls + "'>" + topPrice + "</td>" +
                   "<td class='num " + topCls + "'>" + topPct + "</td>" +
                   "<td class='num " + pct5Cls + "'>" + pct5 + "</td>" +
                   "<td class='num " + pct20Cls + "'>" + pct20 + "</td>";

    tr.addEventListener("click", function() {
      selectSector(s);
    });

    tbody.appendChild(tr);
  });

  // 首次默认选中第 1 个板块
  if (!currentSelectedSector && items.length > 0) {
    selectSector(items[0]);
  }
}

function selectSector(s) {
  currentSelectedSector = s;
  document.querySelectorAll("#sectors-table tbody tr").forEach(function(r) {
    r.classList.remove("selected-sector");
  });
  renderSectorsTableSelection();
  loadSectorStocks(s);
}

function renderSectorsTableSelection() {
  const rows = document.querySelectorAll("#sectors-table tbody tr");
  rows.forEach(function(r) {
    const nameCell = r.querySelector("td:nth-child(2)");
    if (nameCell && currentSelectedSector && nameCell.textContent.trim() === currentSelectedSector.name) {
      r.classList.add("selected-sector");
    }
  });
}

function loadSectorStocks(sector) {
  const titleEl = document.getElementById("sector-stocks-title");
  if (titleEl) {
    titleEl.innerHTML = "<b>" + sector.name + "</b> 板块个股";
  }
  const tbody = document.querySelector("#sector-stocks-table tbody");
  if (!tbody) return;
  tbody.innerHTML = "<tr><td colspan='10' style='text-align:center; padding: 20px;'>加载板块成分股中…</td></tr>";

  const url = "/api/market/sector/stocks?name=" + encodeURIComponent(sector.name) +
              "&code=" + encodeURIComponent(sector.code || "") +
              "&top=" + encodeURIComponent(sector.topStockCode || "");

  api("GET", url).then(function(res) {
    lastStocksData = res.quotes || [];
    if (titleEl) {
      titleEl.innerHTML = "<b>" + sector.name + "</b> 板块个股（共 " + lastStocksData.length + " 只）";
    }
    renderSectorStocksTable();
  }).catch(function(err) {
    tbody.innerHTML = "<tr><td colspan='10' class='err' style='text-align:center; padding: 20px;'>获取成分股失败：" + err.message + "</td></tr>";
  });
}

function renderSectorStocksTable() {
  const tbody = document.querySelector("#sector-stocks-table tbody");
  if (!tbody) return;
  tbody.innerHTML = "";

  const items = lastStocksData.slice();
  if (stockSort.key) {
    items.sort(function(a, b) {
      const va = a[stockSort.key] != null ? a[stockSort.key] : -999999;
      const vb = b[stockSort.key] != null ? b[stockSort.key] : -999999;
      return stockSort.asc ? (va > vb ? 1 : -1) : (va < vb ? 1 : -1);
    });
  }

  if (items.length === 0) {
    tbody.innerHTML = "<tr><td colspan='10' style='text-align:center; padding: 20px;'>暂无成分股数据</td></tr>";
    return;
  }

  items.forEach(function(st, idx) {
    const tr = document.createElement("tr");
    tr.className = "clickable";
    if (state.currentSymbol === st.code) {
      tr.classList.add("selected");
    }

    const price = st.price != null ? st.price.toFixed(2) : "–";
    const pct = st.changePct != null ? (st.changePct > 0 ? "+" : "") + st.changePct.toFixed(2) + "%" : "–";
    const change = st.change != null ? (st.change > 0 ? "+" : "") + st.change.toFixed(2) : "–";
    const cls = st.changePct > 0 ? "up" : st.changePct < 0 ? "down" : "";
    const vol = st.volume != null ? (st.volume >= 10000 ? (st.volume / 10000).toFixed(2) + "万" : st.volume.toFixed(0)) : "–";
    const turnover = st.turnover != null ? st.turnover.toFixed(2) : "–";
    const amp = st.amplitude != null ? st.amplitude.toFixed(2) : "–";
    const vratio = st.volumeRatio != null ? st.volumeRatio.toFixed(2) : "–";

    tr.innerHTML = "<td style='color:var(--natives-text-muted);'>" + (idx + 1) + "</td>" +
                   "<td><b>" + st.code + "</b></td>" +
                   "<td>" + st.name + "</td>" +
                   "<td class='num " + cls + "'>" + price + "</td>" +
                   "<td class='num " + cls + "'>" + pct + "</td>" +
                   "<td class='num " + cls + "'>" + change + "</td>" +
                   "<td class='num'>" + vol + "</td>" +
                   "<td class='num'>" + turnover + "</td>" +
                   "<td class='num'>" + amp + "</td>" +
                   "<td class='num'>" + vratio + "</td>";

    tr.addEventListener("click", function() {
      document.querySelectorAll("#sector-stocks-table tbody tr").forEach(function(r) { r.classList.remove("selected"); });
      tr.classList.add("selected");
      actions.selectSymbol(st.code, st.name);
    });

    tbody.appendChild(tr);
  });
}

// 表头排序绑定
document.querySelectorAll("#sectors-table th.sortable").forEach(function(th) {
  th.addEventListener("click", function() {
    const key = th.dataset.key;
    if (sectorSort.key === key) { sectorSort.asc = !sectorSort.asc; }
    else { sectorSort = { key: key, asc: false }; }
    document.querySelectorAll("#sectors-table th.sortable").forEach(function(o) { o.classList.remove("sort-asc", "sort-desc"); });
    th.classList.add(sectorSort.asc ? "sort-asc" : "sort-desc");
    renderSectorsTable();
  });
});

document.querySelectorAll("#sector-stocks-table th.sortable").forEach(function(th) {
  th.addEventListener("click", function() {
    const key = th.dataset.key;
    if (stockSort.key === key) { stockSort.asc = !stockSort.asc; }
    else { stockSort = { key: key, asc: false }; }
    document.querySelectorAll("#sector-stocks-table th.sortable").forEach(function(o) { o.classList.remove("sort-asc", "sort-desc"); });
    th.classList.add(stockSort.asc ? "sort-asc" : "sort-desc");
    renderSectorStocksTable();
  });
});

})();
