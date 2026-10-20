// 场外基金操作（N09）：新增/编辑持仓、加仓、减仓、转换、定投计划。
// 复用 home.panel 表单弹层；金额权威在后端定点，前端只展示与登记。
// 渠道确认结果全部手工登记；不向渠道发起交易、不自动扣款。
(() => {
const { state, api, t, formatMinorExact } = globalThis.Fund;
const esc = (s) => String(s == null ? "" : s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
const home = () => globalThis.Fund.home;
const today = () => new Date(Date.now() + 8 * 3600000).toISOString().slice(0, 10);
const WEEKDAY_LABELS = () => Array.from({ length: 7 }, (_, i) =>
  new Intl.DateTimeFormat(state.language === "en" ? "en-US" : "zh-CN", { weekday: "short" }).format(new Date(2024, 0, 1 + i)));

function cutoffHtml(key) {
  return `<fieldset class="otc-cutoff"><legend>${esc(t("fundLedgerOtcCutoff"))}</legend>
    <label><input type="radio" name="${key}" value="before_1500" checked>${esc(t("fundLedgerOtcCutoffBefore"))}</label>
    <label><input type="radio" name="${key}" value="after_1500">${esc(t("fundLedgerOtcCutoffAfter"))}</label></fieldset>`;
}
const readCutoff = (root, key) => root.querySelector(`[name=${key}]:checked`)?.value || "before_1500";
function previewHtml(p, extra = "") {
  if (!p) return "";
  const rows = [
    [t("fundLedgerOtcCost"), p.cost],
    [t("fundLedgerOtcShares"), p.shares ? esc(p.shares) + (p.sharesDerived ? ` · ${t("fundLedgerOtcSharesDerived")}` : "") : "–"],
    [t("fundLedgerOtcNav"), p.navRaw ? `${esc(p.navRaw)} · ${esc(p.navDate || "")}` : t("fundLedgerOtcNavMissing")],
  ].map(([k, v]) => `<p class="otc-preview-row"><span>${esc(k)}</span><b>${v}</b></p>`).join("");
  return `<div class="otc-preview" role="status">${rows}${extra}</div>`;
}

// ---------- 订单列表（确认 / 取消 / 到账） ----------
async function ordersSection(root, holding, opFilter, draft) {
  const box = root.querySelector("[data-orders]");
  const data = await api("GET", `/api/otc/orders?account=${encodeURIComponent(holding.account)}&fund=${encodeURIComponent(holding.fundCode)}`);
  draft.orders = data.orders;
  draft.available = data.availableShares;
  const avail = root.querySelector("[data-available]");
  if (avail) avail.textContent = data.availableShares;
  const rows = data.orders.filter(o => !opFilter || opFilter.includes(o.op)).map(o => {
    const statusKey = { pending: "fundLedgerOtcStatusPending", confirmed: "fundLedgerOtcStatusConfirmed", cancelled: "fundLedgerOtcStatusCancelled", failed: "fundLedgerOtcStatusFailed" }[o.status];
    const ops = [];
    if (o.status === "pending") {
      ops.push(`<button type="button" data-confirm="${o.id}" data-ver="${o.version}">${esc(t("fundLedgerOtcConfirm"))}</button>`);
      ops.push(`<button type="button" data-cancel="${o.id}" data-ver="${o.version}">${esc(t("fundLedgerOtcCancel"))}</button>`);
    }
    if (o.status === "confirmed" && !o.cashArrival && o.op === "sell") {
      ops.push(`<button type="button" data-arrive="${o.id}">${esc(t("fundLedgerOtcArrival"))}</button>`);
    }
    const meta = [o.appliedDate,
      o.confirmedNavDate ? `${t("fundLedgerOtcConfirmedNavDate")} ${o.confirmedNavDate}` : (o.expectedNavDate ? `${t("fundLedgerOtcExpectedNavDate")} ${o.expectedNavDate}` : ""),
      o.feeKnown ? (o.fee != null ? `${t("fundLedgerOtcFee")} ${o.fee}` : null) : (o.fee != null ? `${t("fundLedgerOtcFee")} ${o.fee} · ${t("fundLedgerOtcFeeDerived")}` : t("fundLedgerOtcFeeUnknown")),
      o.cashArrival ? `${t("fundLedgerOtcArrival")} ${o.cashArrival}` : null].filter(Boolean).join(" · ");
    const operationKey = { buy: "fundLedgerOtcBuyTitle", sell: "fundLedgerOtcSellTitle", convert_out: "fundLedgerOtcConversionOut", convert_in: "fundLedgerOtcConversionIn" }[o.op];
    return `<li class="otc-order" data-oid="${o.id}"><p><strong>${esc(o.fundName || o.fundCode)} · ${esc(o.fundCode)}</strong> · ${esc(t(operationKey))}</p><div><b>${esc(t("fundLedgerOtcAmount"))} ${o.amount != null ? esc(o.amount) : "?"}</b> · ${esc(t("fundLedgerOtcShares"))} ${o.confirmedShares || o.shares || "?"} · <span class="otc-badge otc-badge-${o.status}">${esc(t(statusKey))}</span></div>
      <p class="otc-order-meta">${esc(meta)}</p>
      ${ops.length ? `<div class="otc-order-ops">${ops.join("")}</div>` : ""}</li>`;
  }).join("");
  box.innerHTML = `<h4>${esc(t("fundLedgerOtcOrders"))}</h4><ul class="otc-orders">${rows || `<li class="otc-empty">${esc(t("fundLedgerOtcOrdersEmpty"))}</li>`}</ul>`;
  box.querySelectorAll("[data-confirm]").forEach(btn => btn.onclick = () => confirmOrderDialog(holding, btn, draft));
  box.querySelectorAll("[data-cancel]").forEach(btn => btn.onclick = async () => {
    btn.disabled = true;
    try {
      await api("POST", "/api/otc/orders/cancel", { id: Number(btn.dataset.cancel), expectedVersion: Number(btn.dataset.ver), outcome: "cancelled", reason: "用户取消" });
      await ordersSection(root, holding, opFilter, draft);
      globalThis.Fund.actions.loadPositions?.();
    } catch (e) { home().failure(e); btn.disabled = false; }
  });
  box.querySelectorAll("[data-arrive]").forEach(btn => btn.onclick = async () => {
    btn.disabled = true;
    try {
      await api("POST", "/api/otc/orders/arrival", { id: Number(btn.dataset.arrive), date: today(), mode: "post" });
      await ordersSection(root, holding, opFilter, draft);
      globalThis.Fund.actions.loadPositions?.();
    } catch (e) { home().failure(e); btn.disabled = false; }
  });
}

function confirmOrderDialog(holding, btn, draft) {
  const order = draft.orders.find(o => o.id === Number(btn.dataset.confirm));
  const H = home();
  H.panel(
    () => t("fundLedgerOtcConfirm") + " · " + (order.fundName || order.fundCode),
    () => `<form>
      <label>${esc(t("fundLedgerOtcConfirmedNavDate"))}<input name="navDate" type="date" required value="${today()}"></label>
      <label>${esc(t("fundLedgerOtcNav"))}<input name="nav" type="number" step="0.0001" min="0" required></label>
      <label>${esc(t("fundLedgerOtcConfirmedShares"))}<input name="shares" type="number" step="0.0001" min="0" placeholder="${order.shares || ""}"></label>
      <label>${esc(t("fundLedgerOtcConfirmedAmount"))}<input name="amount" type="number" step="0.01" min="0" placeholder="${order.amount || ""}"></label>
      <label>${esc(t("fundLedgerOtcFee"))}<input name="fee" type="number" step="0.01" min="0" placeholder="${order.fee ?? ""}"></label>
      <label class="otc-check"><input type="checkbox" name="feeKnown" ${order.feeKnown ? "checked" : ""}>${esc(t("fundLedgerOtcFeeKnownHint"))}</label>
      <button type="submit">${esc(t("fundHomeConfirm"))}</button></form>`,
    r => {
      H.submit(r, async v => {
        try {
          await api("POST", "/api/otc/orders/confirm", {
            id: order.id, expectedVersion: order.version,
            confirmedShares: v.shares || undefined, confirmedAmount: v.amount || undefined,
            fee: v.fee || undefined, feeKnown: !!v.feeKnown,
            confirmedNavDate: v.navDate, confirmedNav: v.nav,
          });
          orderDialog(holding, order.op === "buy" ? "buy" : order.op === "sell" ? "sell" : "convert");
          globalThis.Fund.actions.loadPositions?.();
        } catch (e) { H.failure(e); }
      });
    },
  );
}

// ---------- 新增 / 编辑持仓 ----------
function holdingPreviewHtml(draft) {
  const p = draft.preview;
  return previewHtml(p) +
    (p?.navIsExact === false && p.navRaw ? `<label class="otc-check"><input type="checkbox" name="acceptEarlierNav" data-accept>${esc(t("fundLedgerOtcAcceptEarlierNav"))}</label>` : "") +
    (draft.mode === "calibration" && p ? `<p class="otc-preview-row"><span>${esc(t("fundLedgerOtcBefore"))}</span><b>${esc(p.before?.quantity ?? "–")} / ${esc(p.before?.cost ?? "–")}</b></p>
      <p class="otc-preview-row"><span>${esc(t("fundLedgerOtcAfter"))}</span><b>${esc(p.after?.quantity ?? "–")} / ${esc(p.after?.cost ?? "–")}</b></p>` : "");
}
function holdingFormHtml(draft, editing = false) {
  return `<form>
    <label>${esc(t("fundLedgerOtcValue"))}<input name="value" type="number" step="0.01" min="0.01" required value="${draft.value}"></label>
    <label>${esc(t("fundLedgerOtcGain"))}<input name="gain" type="number" step="0.01" required value="${draft.gain}"></label>
    <label>${esc(t("fundLedgerOtcAsOf"))}<input name="asOf" type="date" required value="${draft.asOf}"></label>
    <label>${esc(t("fundLedgerOtcActualShares"))}<input name="shares" type="number" step="0.0001" min="0.0001" placeholder="${t("fundLedgerOtcOptional")}"></label>
    <button type="submit">${esc(t("fundLedgerOtcPreview"))}</button></form>
    <p class="otc-hint">${esc(t("fundLedgerOtcGainHint"))}</p>
    <div data-preview role="status">${holdingPreviewHtml(draft)}</div>
    ${editing ? "" : `<button type="button" data-save ${draft.preview && !draft.saving ? "" : "disabled"}>${esc(t("fundHomeSave"))}</button>`}`;
}
function bindHoldingDraft(root, draft) {
  root.querySelector('form').addEventListener('input', () => {
    draft.previewGeneration = (draft.previewGeneration || 0) + 1;
    draft.preview = null;
    root.querySelector('[data-preview]').replaceChildren();
    root.querySelectorAll('[data-save],[data-save2]').forEach(button => { button.disabled = true; });
    const ext = root.querySelector('[data-edit-ext]');
    if (ext) ext.hidden = true;
  });
}

function otcHoldingNewDialog(seed) {
  const H = home(); if (H.sensitive()) return;
  const key = H.requestId();
  const draft = { value: "", gain: "0.00", asOf: today(), shares: "", preview: null, fundCode: seed?.fundCode || "", fundName: seed?.fundName || "" };
  H.panel(
    () => t("fundLedgerOtcNewTitle") + " · " + (draft.fundName || draft.fundCode || ""),
    () => holdingFormHtml(draft),
    root => {
      bindHoldingDraft(root, draft);
      H.submit(root, async v => {
        const own = draft.previewGeneration = (draft.previewGeneration || 0) + 1;
        try {
          draft.value = v.value; draft.gain = v.gain; draft.asOf = v.asOf; draft.shares = v.shares || "";
          const preview = await api("POST", "/api/otc/holding/preview", {
            account: seed.account, fundCode: draft.fundCode, value: v.value, gain: v.gain, asOf: v.asOf, shares: v.shares || undefined,
          });
          if (!root.isConnected || own !== draft.previewGeneration) return;
          draft.preview = preview;
          root.querySelector("[data-preview]").innerHTML = holdingPreviewHtml(draft) + `<p class="otc-hint">${esc(t("fundLedgerOtcNavSourceHint"))}</p>`;
          root.querySelector("[data-save]").disabled = false;
        } catch (e) {
          if (!root.isConnected || own !== draft.previewGeneration) return;
          draft.preview = null;
          root.querySelector("[data-preview]").innerHTML = previewHtml(null);
          root.querySelector("[data-save]").disabled = true;
          H.failure(e);
        }
      });
      root.querySelector("[data-save]").onclick = async e => {
        if (draft.saving || !draft.preview) return;
        draft.saving = true;
        e.target.disabled = true;
        try {
          await api("POST", "/api/otc/holding", {
            account: seed.account, fundCode: draft.fundCode, fundName: draft.fundName,
            value: draft.value, gain: draft.gain, asOf: draft.asOf,
            shares: draft.shares || undefined,
            acceptEarlierNav: !!root.querySelector("[data-accept]")?.checked,
            requestId: key,
          });
          if (root.isConnected) H.closePanel();
          await globalThis.Fund.actions.loadPositions?.();
        } catch (err) { if (root.isConnected) { H.failure(err); root.querySelector('[data-save]').disabled = false; } }
        finally { draft.saving = false; }
      };
    },
  );
}

function otcHoldingEditDialog(holding) {
  const H = home(); if (H.sensitive()) return;
  const key = H.requestId();
  const draft = { value: "", gain: "0.00", asOf: today(), shares: "", preview: null, mode: null, openingVersion: null };
  H.panel(
    () => t("fundLedgerOtcEditTitle") + " · " + (holding.fundName || holding.fundCode),
    () => holdingFormHtml(draft, true) +
      `<div data-edit-ext ${draft.preview ? "" : "hidden"}><p class="otc-hint" data-mode-hint>${draft.mode ? esc(t(draft.mode === "calibration" ? "fundLedgerOtcCalibrationHint" : "fundLedgerOtcOpeningHint")) : ""}</p>
      <label>${esc(t("fundLedgerOtcReason"))}<input name="reason" type="text" required maxlength="200"></label>
      <button type="button" data-save2 ${draft.preview && !draft.saving ? "" : "disabled"}>${esc(t("fundHomeSave"))}</button></div>`,
    root => {
      const ext = root.querySelector("[data-edit-ext]");
      bindHoldingDraft(root, draft);
      H.submit(root, async v => {
        const own = draft.previewGeneration = (draft.previewGeneration || 0) + 1;
        try {
          draft.value = v.value; draft.gain = v.gain; draft.asOf = v.asOf; draft.shares = v.shares || "";
          const preview = await api("POST", "/api/otc/holding/edit/preview", {
            account: holding.account, fundCode: holding.fundCode, value: v.value, gain: v.gain, asOf: v.asOf, shares: v.shares || undefined,
          });
          if (!root.isConnected || own !== draft.previewGeneration) return;
          draft.preview = preview;
          draft.mode = draft.preview.mode;
          draft.openingVersion = draft.preview.opening?.version ?? null;
          root.querySelector("[data-preview]").innerHTML = holdingPreviewHtml(draft);
          ext.hidden = false;
          root.querySelector("[data-mode-hint]").textContent = t(draft.mode === "calibration" ? "fundLedgerOtcCalibrationHint" : "fundLedgerOtcOpeningHint");
          root.querySelector("[data-save2]").disabled = false;
        } catch (e) {
          if (!root.isConnected || own !== draft.previewGeneration) return;
          draft.preview = null; ext.hidden = true; H.failure(e);
        }
      });
      root.querySelector("[data-save2]").onclick = async e => {
        if (draft.saving || !draft.preview) return;
        const reason = root.querySelector('[name=reason]');
        if (!reason.reportValidity()) return;
        draft.saving = true;
        e.target.disabled = true;
        try {
          await api("POST", "/api/otc/holding/edit", {
            mode: draft.mode, account: holding.account, fundCode: holding.fundCode,
            value: draft.value, gain: draft.gain, asOf: draft.asOf, shares: draft.shares || undefined,
            acceptEarlierNav: !!root.querySelector("[data-accept]")?.checked,
            reason: root.querySelector("[name=reason]").value,
            openingVersion: draft.mode === "opening" ? draft.openingVersion : undefined,
            requestId: key,
          });
          if (root.isConnected) H.closePanel();
          await globalThis.Fund.actions.loadPositions?.();
        } catch (err) {
          if (root.isConnected) { H.failure(err); root.querySelector('[data-save2]').disabled = false; }
        } finally { draft.saving = false; }
      };
    },
  );
}

// ---------- 加仓 / 减仓 / 转换 ----------
function orderDialog(holding, op) {
  const H = home(); if (H.sensitive()) return;
  const draft = { orders: [], available: null };
  let attemptPayload, attemptId;
  const quick = op !== "buy"
    ? `<div class="otc-quick" role="group" aria-label="${esc(t("fundLedgerOtcQuick"))}">
        ${["1/4", "1/3", "1/2"].map(r => `<button type="button" data-quick="${r}">${r}</button>`).join("")}
        <button type="button" data-quick="all">${esc(t("fundLedgerOtcAll"))}</button></div>` : "";
  const body = () => {
    if (op === "buy") {
      return `<form>
      <label>${esc(t("fundLedgerOtcAmount"))}<input name="amount" type="number" step="0.01" min="0.01" required></label>
      <label>${esc(t("fundLedgerOtcAppliedDate"))}<input name="date" type="date" required value="${today()}"></label>
      ${cutoffHtml("cutoff")}
      <label>${esc(t("fundLedgerOtcFee"))}<input name="fee" type="number" step="0.01" min="0" placeholder="${t("fundLedgerOtcFeeUnknown")}"></label>
      <label class="otc-check"><input type="checkbox" name="feeKnown">${esc(t("fundLedgerOtcFeeKnownHint"))}</label>
      <button type="submit">${esc(t("fundLedgerOtcApply"))}</button></form>
      <p class="otc-hint">${esc(t("fundLedgerOtcHoldHint"))}</p>`;
    }
    if (op === "sell") {
      return `<p>${esc(t("fundLedgerOtcAvailable"))}：<b data-available>…</b></p>${quick}
      <form>
      <label>${esc(t("fundLedgerOtcSharesLabel"))}<input name="shares" type="number" step="0.0001" min="0.0001" required></label>
      <label>${esc(t("fundLedgerOtcAppliedDate"))}<input name="date" type="date" required value="${today()}"></label>
      ${cutoffHtml("cutoff")}
      <button type="submit">${esc(t("fundLedgerOtcApply"))}</button></form>
      <p class="otc-hint">${esc(t("fundLedgerOtcQuickHint"))}</p>`;
    }
    return `<p>${esc(t("fundLedgerOtcAvailable"))}：<b data-available>…</b></p>${quick}
    <form>
    <label>${esc(t("fundLedgerOtcSharesLabel"))}<input name="shares" type="number" step="0.0001" min="0.0001" required></label>
    <label>${esc(t("fundLedgerOtcTarget"))}<input name="target" type="search" required placeholder="${t("fundLedgerOtcTargetHint")}"></label>
    <label>${esc(t("fundLedgerOtcAppliedDate"))}<input name="date" type="date" required value="${today()}"></label>
    ${cutoffHtml("cutoff")}
    <button type="submit">${esc(t("fundLedgerOtcApply"))}</button></form>
    <p class="otc-hint">${esc(t("fundLedgerOtcConvertHint"))}</p>`;
  };
  const opFilter = op === "buy" ? ["buy"] : op === "sell" ? ["sell"] : ["convert_out", "convert_in"];
  H.panel(
    () => t(op === "buy" ? "fundLedgerOtcBuyTitle" : op === "sell" ? "fundLedgerOtcSellTitle" : "fundLedgerOtcConvertTitle") + " · " + (holding.fundName || holding.fundCode) + " · " + holding.account,
    () => body() + `<div data-orders role="status">${esc(t("fundLedgerOtcLoading"))}</div>`,
    root => {
      ordersSection(root, holding, opFilter, draft).catch(e => H.failure(e));
      if (op !== "buy") {
        root.querySelectorAll("[data-quick]").forEach(btn => btn.onclick = () => {
          const input = root.querySelector("[name=shares]");
          if (!input || !draft.available) return;
          const [whole, fraction = ""] = String(draft.available).split(".");
          const units = BigInt(whole) * 10000n + BigInt(fraction.padEnd(4, "0"));
          const divisor = { "1/4": 4n, "1/3": 3n, "1/2": 2n, all: 1n }[btn.dataset.quick];
          const shares = units / divisor;
          input.value = `${shares / 10000n}.${String(shares % 10000n).padStart(4, "0")}`;
        });
      }
      H.submit(root, async v => {
        try {
          const payload = {
            account: holding.account, fundCode: holding.fundCode, fundName: holding.fundName,
            appliedDate: v.date, cutoff: readCutoff(root, "cutoff"),
          };
          if (op === "buy") {
            payload.op = "buy"; payload.amount = v.amount;
            payload.fee = v.fee || undefined; payload.feeKnown = !!v.feeKnown;
          } else if (op === "sell") {
            payload.op = "sell"; payload.shares = v.shares;
          } else {
            payload.op = "convert"; payload.shares = v.shares; payload.targetCode = v.target;
          }
          const fingerprint = JSON.stringify(payload);
          if (fingerprint !== attemptPayload) { attemptPayload = fingerprint; attemptId = H.requestId(); }
          // A lost response may follow a committed order; retry the same intent.
          await api("POST", "/api/otc/orders", {...payload,requestId:attemptId});
          root.querySelector('form').reset();
          attemptPayload = undefined;
          await ordersSection(root, holding, opFilter, draft);
          globalThis.Fund.actions.loadPositions?.();
        } catch (e) { H.failure(e); }
      });
    },
  );
}

// ---------- 定投计划 ----------
function otcPlansDialog(holding) {
  const H = home(); if (H.sensitive()) return;
  const key = H.requestId();
  const draft = { plans: [], showForm: false, form: {} };
  const formHtmlFor = (p = {}) => `<form data-plan-form>
    ${H.input("amount", t("fundLedgerOtcAmount"), p.amount || "", "number", 'step="0.01" min="0.01" required')}
    ${H.select("freq", t("fundLedgerOtcFreq"), [["weekly",t("fundLedgerOtcFreqWeekly")],["monthly",t("fundLedgerOtcFreqMonthly")],["daily",t("fundLedgerOtcFreqDaily")]], p.freq || "weekly")}
    <div data-weekday>${H.select("weekday",t("fundLedgerOtcWeekday"),WEEKDAY_LABELS().map((label,i)=>[String(i+1),label]),String(p.weekday || 1))}</div>
    <div data-monthday>${H.input("monthday",t("fundLedgerOtcMonthday"),p.monthday || 1,"number",'min="1" max="31"')}</div>
    ${H.input("startDate",t("fundLedgerOtcStart"),p.startDate || today(),"date","required")}
    ${H.input("endDate",t("fundLedgerOtcEnd"),p.endDate || "","date")}
    ${H.select("nonTrading",t("fundLedgerOtcNonTrading"),[["next",t("fundLedgerOtcNonTradingNext")],["prev",t("fundLedgerOtcNonTradingPrev")],["skip",t("fundLedgerOtcNonTradingSkip")]],p.nonTrading || "next")}
    <button type="submit">${esc(t("fundHomeSave"))}</button></form>`;
  const formHtml = () => draft.showForm ? formHtmlFor(draft.form) : "";
  const planInput = v => ({
    account: holding.account, fundCode: holding.fundCode, fundName: holding.fundName,
    amount: v.amount, freq: v.freq,
    weekday: v.freq === "weekly" ? Number(v.weekday) : undefined,
    monthday: v.freq === "monthly" ? Number(v.monthday) : undefined,
    startDate: v.startDate, endDate: v.endDate || undefined, nonTrading: v.nonTrading,
  });
  function bindFrequency(root) {
    const form = root.querySelector('[data-plan-form]');
    const update = () => {
      form.querySelector('[data-weekday]').hidden = form.elements.freq.value !== 'weekly';
      form.querySelector('[data-monthday]').hidden = form.elements.freq.value !== 'monthly';
      draft.form = Object.fromEntries(new FormData(form));
    };
    form.addEventListener('input',update);form.addEventListener('change',update);update();
  }
  const plansHtml = () => draft.plans.map(p => {
    const statusKey = { active: "fundLedgerOtcPlanActive", paused: "fundLedgerOtcPlanPaused", ended: "fundLedgerOtcPlanEnded" }[p.status];
    const actions = [];
    if (p.status !== "ended") actions.push(`<button type="button" data-edit-plan="${p.id}">${esc(t("fundHomeEdit"))}</button>`);
    if (p.status === "active") actions.push(`<button type="button" data-pause="${p.id}" data-ver="${p.version}">${esc(t("fundLedgerOtcPlanPause"))}</button>`);
    if (p.status === "paused") actions.push(`<button type="button" data-resume="${p.id}" data-ver="${p.version}">${esc(t("fundLedgerOtcPlanResume"))}</button>`);
    if (p.status !== "ended") actions.push(`<button type="button" data-end="${p.id}" data-ver="${p.version}">${esc(t("fundLedgerOtcPlanEnd"))}</button>`);
    actions.push(`<button type="button" data-runs="${p.id}">${esc(t("fundLedgerOtcRuns"))}</button>`);
    if (p.status === "active") actions.push(`<button type="button" data-register="${p.id}" data-next="${p.nextDate || ""}">${esc(t("fundLedgerOtcRegisterRun"))}</button>`);
    return `<li class="otc-plan"><div><b>${esc(p.amount)}</b> · ${esc(t({ weekly: "fundLedgerOtcFreqWeekly", monthly: "fundLedgerOtcFreqMonthly", daily: "fundLedgerOtcFreqDaily" }[p.freq]))}${p.freq === "weekly" ? " · " + esc(WEEKDAY_LABELS()[(p.weekday || 1) - 1]) : p.freq === "monthly" ? ` · ${esc(t("fundLedgerOtcMonthday"))} ${p.monthday}` : ""} · <span class="otc-badge otc-badge-${p.status}">${esc(t(statusKey))}</span></div>
      <p class="otc-order-meta">${esc(t("fundLedgerOtcNextDate"))} ${p.nextDate ? esc(p.nextDate) + (p.tradingDayVerified ? "" : " · " + esc(t("fundLedgerOtcHolidayHint"))) : "–"} · ${esc(t("fundLedgerOtcRuns"))} ${p.registeredRuns}</p>
      <div class="otc-order-ops">${actions.join("")}</div><div data-runs-for="${p.id}"></div></li>`;
  }).join("");
  const reload = async root => {
    const data = await api("GET", `/api/otc/plans?account=${encodeURIComponent(holding.account)}&fund=${encodeURIComponent(holding.fundCode)}`);
    draft.plans = data.plans;
    root.querySelector("[data-plans]").innerHTML =
      `<ul class="otc-orders">${plansHtml() || `<li class="otc-empty">${esc(t("fundLedgerOtcPlansEmpty"))}</li>`}</ul>
       <button type="button" data-new-plan>${esc(t("fundLedgerOtcPlanNew"))}</button>${formHtml()}`;
    bindPlanActions(root);
  };
  async function bindPlanActions(root) {
    root.querySelectorAll('[data-edit-plan]').forEach(button => button.onclick = () => {
      const plan = draft.plans.find(p => p.id === Number(button.dataset.editPlan));
      if (!plan) return;
      H.panel(() => t('fundHomeEdit') + ' · ' + holding.fundName, () => formHtmlFor(plan), editor => {
        bindFrequency(editor);
        H.submit(editor, async values => {
          await api('POST', '/api/otc/plans/update', {
            ...planInput(values), id: plan.id, expectedVersion: plan.version, action: 'edit',
          });
          H.closePanel(); otcPlansDialog(holding);
        });
      });
    });
    root.querySelector("[data-new-plan]").onclick = () => { draft.showForm = !draft.showForm; reload(root).catch(e => H.failure(e)); };
    const form = root.querySelector("[data-plan-form]");
    if (form) {
      bindFrequency(root);
      // home.submit 绑定 root 下全部 form（元素自身不参与 querySelectorAll）。
      H.submit(root, async v => {
        try {
          await api("POST", "/api/otc/plans", planInput(v));
          draft.form = {};
          draft.showForm = false;
          await reload(root);
        } catch (e) { H.failure(e); }
      });
    }
    root.querySelectorAll("[data-pause],[data-resume],[data-end]").forEach(btn => btn.onclick = async () => {
      btn.disabled = true;
      const action = btn.dataset.pause ? "pause" : btn.dataset.resume ? "resume" : "end";
      const id = Number(btn.dataset.pause || btn.dataset.resume || btn.dataset.end);
      try {
        await api("POST", "/api/otc/plans/update", { id, expectedVersion: Number(btn.dataset.ver), action });
        await reload(root);
      } catch (e) { H.failure(e); btn.disabled = false; }
    });
    root.querySelectorAll("[data-runs]").forEach(btn => btn.onclick = async () => {
      const box = root.querySelector(`[data-runs-for="${btn.dataset.runs}"]`);
      const runs = await api("GET", "/api/otc/plans/runs?planId=" + btn.dataset.runs);
      box.innerHTML = `<ul class="otc-runs">${runs.runs.map(r => `<li>${esc(r.periodKey)} · ${esc(t(r.status === "registered" ? "fundLedgerOtcRunRegistered" : "fundLedgerOtcRunSkipped"))}${r.orderStatus ? " · " + esc(t({ pending: "fundLedgerOtcStatusPending", confirmed: "fundLedgerOtcStatusConfirmed", cancelled: "fundLedgerOtcStatusCancelled", failed: "fundLedgerOtcStatusFailed" }[r.orderStatus])) : ""}</li>`).join("") || `<li>${esc(t("fundLedgerOtcRunsEmpty"))}</li>`}</ul>`;
    });
    root.querySelectorAll("[data-register]").forEach(btn => btn.onclick = async () => {
      btn.disabled = true;
      try {
        await api("POST", "/api/otc/plans/runs/register", { planId: Number(btn.dataset.register), periodKey: today(), mode: "buy", requestId: key + "-run-" + Date.now() });
        await reload(root);
      } catch (e) { H.failure(e); btn.disabled = false; }
    });
  }
  H.panel(
    () => t("fundLedgerOtcPlansTitle") + " · " + (holding.fundName || holding.fundCode) + " · " + holding.account,
    () => `<div data-plans role="status">${esc(t("fundLedgerOtcLoading"))}</div>
      <p class="otc-hint">${esc(t("fundLedgerOtcPlanHint"))}</p>`,
    root => { reload(root).catch(e => H.failure(e)); },
  );
}

globalThis.Fund.otcOps = {
  holdingNew: otcHoldingNewDialog,
  holdingEdit: otcHoldingEditDialog,
  buy: (holding) => orderDialog(holding, "buy"),
  sell: (holding) => orderDialog(holding, "sell"),
  convert: (holding) => orderDialog(holding, "convert"),
  plans: otcPlansDialog,
};
})();
