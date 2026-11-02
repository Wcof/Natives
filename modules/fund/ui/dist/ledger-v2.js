// 账本 V2 前端：概览 / 持仓 / 流水。
// 数据全部来自 /api/v2/*；所有用户数据经 textContent 写入，不拼 innerHTML（R-I4）。
(function () {
  "use strict";
  if (!window.Fund) return;
  const { api, t } = Fund;

  const state = {
    view: "overview",
    accounts: [],
    instruments: [],
    holdings: [],
    overview: null,
    loaded: false,
  };

  const $ = (id) => document.getElementById(id);

  function el(tag, cls, text) {
    const node = document.createElement(tag);
    if (cls) node.className = cls;
    if (text !== undefined && text !== null) node.textContent = String(text);
    return node;
  }

  function todayLocal() {
    const d = new Date();
    const p = (n) => String(n).padStart(2, "0");
    return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
  }

  /** 金额展示：后端返回规范化定点字符串（可能无小数位），前端统一补两位并加千分位。 */
  function money(v) {
    if (v === null || v === undefined || v === "") return "–";
    const n = Number(v);
    if (!Number.isFinite(n)) return String(v);
    return n.toLocaleString("zh-CN", { minimumFractionDigits: 2, maximumFractionDigits: 2 });
  }

  function signed(node, n) {
    if (n === null || n === undefined || !Number.isFinite(Number(n))) return node;
    const v = Number(n);
    node.classList.add(v >= 0 ? "lg-up" : "lg-down");
    return node;
  }

  function status(message, tone) {
    const bar = $("lg-status");
    if (!bar) return;
    if (!message) {
      bar.hidden = true;
      bar.textContent = "";
      return;
    }
    bar.hidden = false;
    bar.textContent = message;
    bar.dataset.tone = tone || "warn";
  }

  function errText(e) {
    return (e && e.message) || t("fundLgLoadFailed");
  }

  // ---------- 数据 ----------

  async function loadMaster() {
    const [accounts, instruments] = await Promise.all([
      api("GET", "/api/v2/accounts"),
      api("GET", "/api/v2/instruments"),
    ]);
    state.accounts = accounts || [];
    state.instruments = instruments || [];
  }

  async function loadView() {
    try {
      if (state.view === "overview") {
        state.overview = await api("GET", "/api/v2/overview");
        renderOverview();
      } else if (state.view === "holdings") {
        state.holdings = await api("GET", "/api/v2/holdings");
        renderHoldings();
      } else {
        renderFlows(await api("GET", "/api/v2/flows"));
      }
      status("");
    } catch (e) {
      status(errText(e), "error");
    }
  }

  async function enter() {
    if (!state.loaded) {
      try {
        await loadMaster();
        state.loaded = true;
      } catch (e) {
        status(errText(e), "error");
        return;
      }
    }
    await loadView();
  }

  // ---------- 概览 ----------

  function renderOverview() {
    const o = state.overview;
    if (!o) return;
    $("lg-asof").textContent = o.as_of ? `${t("fundLgAsOf")} ${o.as_of}` : "";
    $("lg-networth").textContent = money(o.net_worth);
    $("lg-cost").textContent = money(o.cost);

    const un = $("lg-unrealized");
    un.textContent = money(o.unrealized);
    un.className = "num";
    signed(un, o.unrealized);

    const re = $("lg-realized");
    re.textContent = money(o.realized);
    re.className = "num";
    signed(re, o.realized);

    const sub = $("lg-hero-sub");
    sub.textContent = o.incomplete ? t("fundLgIncomplete") : "";
    sub.classList.toggle("lg-hint", !!o.incomplete);

    renderChart(o.series || []);
    renderClasses(o.by_class || []);

    const note = $("lg-series-note");
    note.textContent = (o.series || []).length
      ? t("fundLgDays").replace("{n}", String(o.series.length))
      : "";
  }

  /** 极简折线：只画走势，不做坐标轴刻度，避免引入图表库与二次主题适配。 */
  function renderChart(series) {
    const host = $("lg-chart");
    host.textContent = "";
    if (!series.length) {
      host.appendChild(el("p", "lg-empty", t("fundLgNoSeries")));
      return;
    }
    const W = 640;
    const H = 160;
    const pad = 8;
    const values = series.map((p) => Number(p.net_worth) || 0);
    const min = Math.min(...values);
    const max = Math.max(...values);
    const span = max - min || 1;
    const step = series.length > 1 ? (W - pad * 2) / (series.length - 1) : 0;

    const ns = "http://www.w3.org/2000/svg";
    const svg = document.createElementNS(ns, "svg");
    svg.setAttribute("viewBox", `0 0 ${W} ${H}`);
    svg.setAttribute("role", "img");
    svg.setAttribute("aria-label", t("fundLgNetWorthTrend"));

    const line = document.createElementNS(ns, "polyline");
    const pts = values.map((v, i) => {
      const x = pad + step * i;
      const y = H - pad - ((v - min) / span) * (H - pad * 2);
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    });
    line.setAttribute("points", pts.join(" "));
    line.setAttribute("fill", "none");
    line.setAttribute("stroke", "var(--natives-accent)");
    line.setAttribute("stroke-width", "1.5");
    line.setAttribute("stroke-linejoin", "round");
    svg.appendChild(line);

    const area = document.createElementNS(ns, "polygon");
    area.setAttribute("points", `${pad},${H - pad} ${pts.join(" ")} ${pad + step * (values.length - 1)},${H - pad}`);
    area.setAttribute("fill", "var(--natives-accent)");
    area.setAttribute("fill-opacity", "0.08");
    svg.insertBefore(area, line);

    host.appendChild(svg);
  }

  function renderClasses(rows) {
    const host = $("lg-classes");
    host.textContent = "";
    if (!rows.length) {
      host.appendChild(el("p", "lg-empty", t("fundLgNoHoldings")));
      return;
    }
    rows.forEach((row) => {
      const wrap = el("div", "lg-class-row");
      wrap.appendChild(el("span", "lg-class-name", labelClass(row.asset_class)));
      const bar = el("div", "lg-class-bar");
      const fill = el("span");
      fill.style.width = `${Math.max(0, Math.min(100, Number(row.weight) || 0))}%`;
      bar.appendChild(fill);
      wrap.appendChild(bar);
      wrap.appendChild(el("span", "lg-class-value", money(row.market_value)));
      wrap.appendChild(el("span", "lg-class-weight", `${Number(row.weight || 0).toFixed(1)}%`));
      host.appendChild(wrap);
    });
  }

  function labelClass(name) {
    const map = {
      stock: t("fundLgClassStock"),
      fund: t("fundLgClassFund"),
      crypto: t("fundLgClassCrypto"),
    };
    return map[name] || name;
  }

  // ---------- 持仓 ----------

  function renderHoldings() {
    const host = $("lg-holdings");
    host.textContent = "";
    const rows = state.holdings || [];
    $("lg-holdings-note").textContent = rows.length
      ? t("fundLgHoldingCount").replace("{n}", String(rows.length))
      : "";
    if (!rows.length) {
      host.appendChild(el("p", "lg-empty", t("fundLgNoHoldings")));
      return;
    }

    const table = el("table", "lg-table");
    const thead = el("thead");
    const htr = el("tr");
    [
      ["fundLgColName", "left"],
      ["fundLgColAccount", "left"],
      ["fundLgQuantity", "num"],
      ["fundLgColAvgCost", "num"],
      ["fundLgPrice", "num"],
      ["fundLgColValue", "num"],
      ["fundLgColPnl", "num"],
      ["fundLgColDays", "num"],
      ["fundLgColDaily", "num"],
      ["fundLgAiCheck", "left"],
    ].forEach(([key, align]) => {
      htr.appendChild(el("th", align === "num" ? "num" : "", t(key)));
    });
    thead.appendChild(htr);
    table.appendChild(thead);

    const tbody = el("tbody");
    let currentClass = null;
    rows.forEach((h) => {
      if (h.asset_class !== currentClass) {
        currentClass = h.asset_class;
        const groupRow = el("tr", "lg-group-row");
        const cell = el("td", "", labelClass(h.asset_class));
        cell.colSpan = 10;
        groupRow.appendChild(cell);
        tbody.appendChild(groupRow);
      }
      const tr = el("tr");
      const name = `${h.name || h.code}${h.share_class ? ` ${h.share_class}` : ""}`;
      tr.appendChild(el("td", "", name));
      tr.appendChild(el("td", "", h.account_name || "–"));
      tr.appendChild(el("td", "num", h.quantity));
      tr.appendChild(el("td", "num", money(h.avg_cost)));
      tr.appendChild(el("td", "num", h.price ? money(h.price) : t("fundLgNoPrice")));
      tr.appendChild(el("td", "num", money(h.market_value)));
      const pnl = el("td", "num", money(h.unrealized));
      signed(pnl, h.unrealized);
      tr.appendChild(pnl);
      tr.appendChild(el("td", "num", String(h.held_days || 0)));
      const daily = el("td", "num", money(h.daily_gain));
      signed(daily, h.daily_gain);
      tr.appendChild(daily);
      const aiCell = el("td");
      const aiBtn = el("button", "btn", t("fundLgAiCheck"));
      aiBtn.type = "button";
      aiBtn.addEventListener("click", () => runAi(h, aiBtn));
      aiCell.appendChild(aiBtn);
      tr.appendChild(aiCell);
      tbody.appendChild(tr);
    });
    table.appendChild(tbody);
    host.appendChild(table);
  }

  // ---------- 行情同步 ----------

  async function syncQuotes() {
    const btn = $("lg-sync");
    const original = btn.textContent;
    btn.disabled = true;
    btn.textContent = t("fundLgSyncing");
    try {
      const r = await api("POST", "/api/v2/quotes/refresh");
      let msg = t("fundLgSyncDone")
        .replace("{updated}", String(r.updated))
        .replace("{missing}", String(r.missing))
        .replace("{skipped}", String(r.skipped));
      if (r.calendar && r.calendar.days_written > 0) msg += ` · ${t("fundLgCalendarDone").replace("{n}", String(r.calendar.days_written))}`;
      if (r.calendar && r.calendar.days_written > 0) msg += ` · ${t("fundLgCalendarDone").replace("{n}", String(r.calendar.days_written))}`;
      if (r.fx && r.fx.updated > 0) msg += ` · ${t("fundLgFxDone").replace("{n}", String(r.fx.updated))}`;
      if (r.errors && r.errors.length) msg += ` · ${r.errors[0]}`;
      // 有价才值得补历史；补算失败不覆盖刷新结果
      if (r.updated > 0) {
        try {
          const b = await api("POST", "/api/v2/snapshots/backfill");
          if (b.days > 0) msg += ` · ${t("fundLgBackfillDone").replace("{days}", String(b.days))}`;
        } catch (e) {
          msg += ` · ${errText(e)}`;
        }
      }
      status(msg);
      await loadView();
    } catch (e) {
      status(errText(e), "error");
    } finally {
      btn.disabled = false;
      btn.textContent = original;
    }
  }

  // ---------- AI ----------

  function toneLabel(tone) {
    const key =
      { steady: "fundLgAiToneSteady", watch: "fundLgAiToneWatch", alert: "fundLgAiToneAlert" }[
        tone
      ] || "fundLgAiToneWatch";
    return t(key);
  }

  function fmtEpochDay(secs) {
    const n = Number(secs);
    if (!Number.isFinite(n) || n <= 0) return "";
    const d = new Date(n * 1000);
    const p2 = (x) => String(x).padStart(2, "0");
    return `${d.getFullYear()}-${p2(d.getMonth() + 1)}-${p2(d.getDate())}`;
  }

  function renderAiTrend(host, points) {
    host.textContent = "";
    const W = 560;
    const H = 90;
    const pad = 6;
    const n = points.length;
    const step = (W - pad * 2) / (n - 1);
    const ns = "http://www.w3.org/2000/svg";
    const svg = document.createElementNS(ns, "svg");
    svg.setAttribute("viewBox", `0 0 ${W} ${H}`);
    svg.setAttribute("role", "img");
    svg.setAttribute("aria-label", t("fundLgAiTrend"));
    const coords = points.map((p, i) => {
      const x = pad + step * i;
      const score = Math.max(0, Math.min(100, Number(p.score) || 0));
      const y = H - pad - (score / 100) * (H - pad * 2);
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    });
    const line = document.createElementNS(ns, "polyline");
    line.setAttribute("points", coords.join(" "));
    line.setAttribute("fill", "none");
    line.setAttribute("stroke", "var(--natives-accent)");
    line.setAttribute("stroke-width", "1.5");
    line.setAttribute("stroke-linejoin", "round");
    svg.appendChild(line);
    const tail = coords[coords.length - 1].split(",");
    const dot = document.createElementNS(ns, "circle");
    dot.setAttribute("cx", tail[0]);
    dot.setAttribute("cy", tail[1]);
    dot.setAttribute("r", "2.5");
    dot.setAttribute("fill", "var(--natives-accent)");
    svg.appendChild(dot);
    host.appendChild(svg);
  }

  function showAiReport(h, ev, previous, points) {
    const title = $("lg-ai-report-title");
    title.textContent = `${h.name || h.code} · ${t("fundLgAiReport")}`;
    const body = $("lg-ai-report-body");
    body.textContent = "";

    const head = el("div", "lg-ai-head");
    head.append(
      el("div", `lg-ai-score lg-ai-tone-${ev.tone}`, String(ev.score)),
      el("div", `lg-ai-label lg-ai-tone-${ev.tone}`, ev.label),
      el("div", "lg-hint", toneLabel(ev.tone))
    );
    body.appendChild(head);
    body.appendChild(el("p", "lg-ai-summary", ev.summary));

    const list = el("ul", "lg-ai-concerns");
    const concerns = ev.concerns && ev.concerns.length ? ev.concerns : [t("fundLgAiNoConcerns")];
    concerns.forEach((c) => list.appendChild(el("li", "", c)));
    body.appendChild(list);

    if (previous) {
      body.appendChild(
        el(
          "p",
          "lg-hint",
          t("fundLgAiPrevious")
            .replace("{score}", String(previous.score))
            .replace("{tone}", toneLabel(previous.tone))
            .replace("{date}", fmtEpochDay(previous.created_at))
        )
      );
    } else {
      body.appendChild(el("p", "lg-hint", t("fundLgAiFirst")));
    }

    if (points && points.length >= 2) {
      const block = el("div", "lg-ai-trend");
      block.appendChild(el("div", "lg-k", t("fundLgAiTrend")));
      const chart = el("div", "lg-ai-trend-chart");
      renderAiTrend(chart, points);
      block.appendChild(chart);
      block.appendChild(
        el("p", "lg-hint", t("fundLgAiTrendHint").replace("{n}", String(points.length)))
      );
      body.appendChild(block);
    }

    $("lg-ai-report").showModal();
    title.focus();
  }

  async function runAi(h, btn) {
    if (!Fund.ledgerAI) {
      status(t("fundLgAiUnavailable"), "error");
      return;
    }
    const original = btn.textContent;
    btn.disabled = true;
    btn.textContent = t("fundLgAiRunning");
    try {
      const saved = await Fund.ledgerAI.evaluate({
        account_id: h.account_id,
        instrument_id: h.instrument_id,
      });
      let previous = null;
      let points = [];
      try {
        points = await Fund.ledgerAI.trend(h.account_id, h.instrument_id, 30);
        previous = points.length >= 2 ? points[points.length - 2] : null;
      } catch (e) {
        /* 趋势取不到不影响主结果 */
      }
      showAiReport(h, saved, previous, points);
      status("");
    } catch (e) {
      status(errText(e), "error");
    } finally {
      btn.disabled = false;
      btn.textContent = original;
    }
  }

  // ---------- 流水 ----------

  function renderFlows(rows) {
    const host = $("lg-flows");
    host.textContent = "";
    const list = rows || [];
    $("lg-flows-note").textContent = list.length
      ? t("fundLgFlowCount").replace("{n}", String(list.length))
      : "";
    if (!list.length) {
      host.appendChild(el("p", "lg-empty", t("fundLgNoFlows")));
      return;
    }
    const byId = new Map(state.instruments.map((i) => [i.id, i]));
    const table = el("table", "lg-table");
    const thead = el("thead");
    const htr = el("tr");
    [
      ["fundLgDate", "left"],
      ["fundLgType", "left"],
      ["fundLgInstrument", "left"],
      ["fundLgQuantity", "num"],
      ["fundLgPrice", "num"],
      ["fundLgAmount", "num"],
      ["fundLgFee", "num"],
      ["fundLgActions", "left"],
    ].forEach(([key, align]) => {
      htr.appendChild(el("th", align === "num" ? "num" : "", t(key)));
    });
    thead.appendChild(htr);
    table.appendChild(thead);

    const tbody = el("tbody");
    list.forEach((f) => {
      const tr = el("tr");
      tr.appendChild(el("td", "", f.trade_date));
      tr.appendChild(el("td", "", labelType(f.type)));
      const inst = byId.get(f.instrument_id);
      tr.appendChild(el("td", "", inst ? inst.name || inst.code : `#${f.instrument_id}`));
      tr.appendChild(el("td", "num", f.quantity));
      tr.appendChild(el("td", "num", money(f.price)));
      tr.appendChild(el("td", "num", money(f.amount)));
      tr.appendChild(el("td", "num", money(f.fee)));
      const actions = el("td");
      const del = el("button", "btn", t("fundLgDelete"));
      del.type = "button";
      del.addEventListener("click", () => removeFlow(f.id));
      actions.appendChild(del);
      tr.appendChild(actions);
      tbody.appendChild(tr);
    });
    table.appendChild(tbody);
    host.appendChild(table);
  }

  function labelType(type) {
    const map = {
      BUY: t("fundLgTypeBuy"),
      SELL: t("fundLgTypeSell"),
      DIVIDEND: t("fundLgTypeDividend"),
      SPLIT: t("fundLgTypeSplit"),
      FEE: t("fundLgTypeFee"),
    };
    return map[type] || type;
  }

  async function removeFlow(id) {
    try {
      await api("DELETE", `/api/v2/flows/${id}`);
      await loadView();
    } catch (e) {
      status(errText(e), "error");
    }
  }

  // ---------- 分区切换 ----------

  function setView(view) {
    state.view = view;
    document.querySelectorAll("#view-ledger .lg-seg button").forEach((btn) => {
      btn.setAttribute("aria-selected", btn.dataset.lg === view ? "true" : "false");
    });
    document.querySelectorAll("#view-ledger .lg-panel").forEach((panel) => {
      panel.hidden = panel.dataset.lgPanel !== view;
    });
    loadView();
  }

  // ---------- 记一笔 ----------

  function fillSelect(select, items, format) {
    select.textContent = "";
    items.forEach((item) => {
      const opt = document.createElement("option");
      opt.value = String(item.id);
      opt.textContent = format(item);
      select.appendChild(opt);
    });
  }

  function syncDialogFields() {
    const type = $("lg-f-type").value;
    document.querySelectorAll("#lg-flow-dialog [data-lg-when]").forEach((field) => {
      const allowed = field.dataset.lgWhen.split(" ");
      field.hidden = !allowed.includes(type);
    });
  }

  function openFlowDialog() {
    fillSelect($("lg-f-account"), state.accounts.filter((a) => !a.archived), (a) => `${a.name} · ${a.currency}`);
    fillSelect($("lg-f-instrument"), state.instruments, (i) => `${i.code} ${i.name}`.trim());
    $("lg-f-date").value = todayLocal();
    ["lg-f-quantity", "lg-f-price", "lg-f-amount", "lg-f-fee", "lg-f-note", "lg-f-split-from", "lg-f-split-to"].forEach(
      (id) => {
        $(id).value = "";
      }
    );
    $("lg-f-type").value = "BUY";
    $("lg-form-error").hidden = true;
    syncDialogFields();
    $("lg-flow-dialog").showModal();
    $("lg-flow-dialog-title").focus();
  }

  async function submitFlow(event) {
    event.preventDefault();
    const type = $("lg-f-type").value;
    const payload = {
      account_id: Number($("lg-f-account").value),
      instrument_id: Number($("lg-f-instrument").value),
      type,
      quantity: $("lg-f-quantity").value.trim(),
      price: $("lg-f-price").value.trim(),
      amount: $("lg-f-amount").value.trim(),
      fee: $("lg-f-fee").value.trim(),
      trade_date: $("lg-f-date").value,
      note: $("lg-f-note").value.trim(),
      split_from: type === "SPLIT" ? Number($("lg-f-split-from").value || 0) : null,
      split_to: type === "SPLIT" ? Number($("lg-f-split-to").value || 0) : null,
    };
    const err = $("lg-form-error");
    if (!payload.account_id || !payload.instrument_id) {
      err.textContent = t("fundLgNeedMaster");
      err.hidden = false;
      return;
    }
    try {
      await api("POST", "/api/v2/flows", payload);
      $("lg-flow-dialog").close();
      state.loaded = true;
      await loadView();
    } catch (e) {
      err.textContent = errText(e);
      err.hidden = false;
    }
  }

  // ---------- 基础维护 ----------

  function renderMaster() {
    const accountHost = $("lg-account-list");
    accountHost.textContent = "";
    state.accounts.forEach((a) => {
      const row = el("div", "lg-master-item");
      row.appendChild(el("span", "", `${a.name} · ${a.currency}`));
      row.appendChild(el("span", "lg-muted", a.channel));
      accountHost.appendChild(row);
    });
    if (!state.accounts.length) accountHost.appendChild(el("p", "lg-empty", t("fundLgNoAccounts")));

    const instHost = $("lg-instrument-list");
    instHost.textContent = "";
    state.instruments.forEach((i) => {
      const row = el("div", "lg-master-item");
      row.appendChild(el("span", "", `${i.code} ${i.name}`.trim()));
      row.appendChild(el("span", "lg-muted", `${i.asset_class} · ${i.market}`));
      instHost.appendChild(row);
    });
    if (!state.instruments.length) instHost.appendChild(el("p", "lg-empty", t("fundLgNoInstruments")));
  }

  async function addAccount() {
    const name = $("lg-a-name").value.trim();
    if (!name) return;
    try {
      await api("POST", "/api/v2/accounts", {
        name,
        channel: $("lg-a-channel").value,
        currency: $("lg-a-currency").value,
      });
      $("lg-a-name").value = "";
      await loadMaster();
      renderMaster();
    } catch (e) {
      status(errText(e), "error");
    }
  }

  async function addInstrument() {
    const code = $("lg-i-code").value.trim();
    if (!code) return;
    try {
      await api("POST", "/api/v2/instruments", {
        code,
        name: $("lg-i-name").value.trim(),
        asset_class: $("lg-i-class").value,
        market: $("lg-i-market").value,
        currency: $("lg-i-currency").value,
        group_code: $("lg-i-group").value.trim() || null,
        share_class: null,
      });
      $("lg-i-code").value = "";
      $("lg-i-name").value = "";
      $("lg-i-group").value = "";
      await loadMaster();
      renderMaster();
    } catch (e) {
      status(errText(e), "error");
    }
  }

  // ---------- AI 设置 ----------

  async function openAiDialog() {
    try {
      const s = await api("GET", "/api/v2/ai/settings");
      $("lg-ai-provider").value = s.provider_id || "";
      $("lg-ai-model").value = s.model_id || "";
      $("lg-ai-style").value = s.style || "neutral";
    } catch (e) {
      status(errText(e), "error");
      return;
    }
    $("lg-ai-dialog").showModal();
    $("lg-ai-title").focus();
  }

  async function saveAiSettings() {
    try {
      await api("PUT", "/api/v2/ai/settings", {
        provider_id: $("lg-ai-provider").value.trim() || null,
        model_id: $("lg-ai-model").value.trim() || null,
        style: $("lg-ai-style").value,
        show_on_overview: true,
      });
      $("lg-ai-dialog").close();
    } catch (e) {
      status(errText(e), "error");
    }
  }

  // ---------- 事件绑定 ----------

  function bind() {
    document.querySelectorAll("#view-ledger .lg-seg button").forEach((btn) => {
      btn.addEventListener("click", () => setView(btn.dataset.lg));
    });
    $("lg-add-flow")?.addEventListener("click", openFlowDialog);
    $("lg-flow-form")?.addEventListener("submit", submitFlow);
    $("lg-f-type")?.addEventListener("change", syncDialogFields);
    $("lg-settings-open")?.addEventListener("click", openAiDialog);
    $("lg-ai-save")?.addEventListener("click", saveAiSettings);
    $("lg-sync")?.addEventListener("click", syncQuotes);
    $("lg-a-add")?.addEventListener("click", addAccount);
    $("lg-i-add")?.addEventListener("click", addInstrument);

    document.querySelectorAll("[data-lg-close]").forEach((btn) => {
      btn.addEventListener("click", () => btn.closest("dialog")?.close());
    });

    // 双击标题进入账户/标的维护：低频操作不占主界面按钮
    $("lg-title")?.addEventListener("dblclick", async () => {
      await loadMaster();
      renderMaster();
      $("lg-master-dialog").showModal();
    });
  }

  // 语言切换后重渲染（R-I5：不改变业务状态）
  document.addEventListener("fund:language-changed", () => {
    if (state.loaded) {
      renderOverview();
      if (state.view === "holdings") renderHoldings();
    }
  });

  Fund.ledgerV2 = { enter, setView, refresh: loadView };
  bind();

  // 账本是默认首屏：DOM 就绪后自动加载一次。
  // api() 内部会等待握手令牌，故不额外轮询；失败时界面显示错误并可由用户重试。
  function boot() {
    document.querySelector(".workspace-body")?.classList.add("ledger-active");
    const view = document.getElementById("view-ledger");
    if (view && !view.hidden) enter();
  }
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", boot);
  } else {
    boot();
  }
})();
