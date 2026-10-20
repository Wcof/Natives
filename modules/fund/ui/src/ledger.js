(() => {
const { state, actions, api, t, decimalToMinor, formatMinorExact } = globalThis.Fund;
// Fund 领域模块：记账与数据维护。职责一句话——持仓记账、交易流水、
// 批量导入与公募净值四个视图的表单与取数渲染。
// import 方向：core（状态/api/t）；表单提交经 api 直达模块 Host。
// 安全（审计 P0-4）：所有单元格经 esc() 模板（与 assets.js 同款），金额
// 单元格由三态显示策略统一遮蔽；错误显式展示，不静默吞掉。

const esc = (s) => String(s == null ? "" : s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
const fmt = n => n==null?t("fundHomeNeedsVerificationccf920"):typeof n==="bigint"?formatMinorExact(n):String(n).includes(".")?String(n).padEnd(String(n).indexOf(".")+3,"0"):String(n)+".00";

// 三态显示（方案 02 §2）：percent/hidden 不展示金额、成本等可反推金额的内容；
// 数量与金额一并遮蔽。displayMode 由 home 控制器维护，读取失败按 hidden（安全默认）。
function displayMode() {
  const home = globalThis.Fund.home;
  const mode = home && typeof home.displayMode === "function" ? home.displayMode() : null;
  return ["amount", "percent", "hidden"].includes(mode) ? mode : "hidden";
}
const maskAmounts = () => displayMode() !== "amount";
const money = (v) => (maskAmounts() ? "****" : fmt(v));

// ---------- 持仓页（T04）：渲染与快照迁移至 positions-view.js ----------
// 本文件保留写入域表单（期初/更正/估值/流水），把详情与估值入口注册到
// actions 供持仓页行操作复用；持仓列表的取数与渲染见 positions-view。
function showLoadError(tableSelector, e) {
  const tbody = document.querySelector(tableSelector + " tbody");
  if (tbody) tbody.innerHTML = "<tr><td colspan='8' class='err'>" + esc(t("fundLedgerLoadFailed") + (e && e.message ? e.message : e)) + "</td></tr>";
}

// ---------- 交易流水模块 ----------
let txGeneration=0,txController=null,txLoading=false,txError=null,txLoadedScope=null,txWriting=false,txRetry=null;
const txScope=()=>document.getElementById("tx-show-deleted").checked;
const txActive=()=>!document.getElementById("view-tx").hidden&&!document.hidden;
actions.leaveTx=()=>{txGeneration++;txController?.abort();txController=null;txLoading=false;};
window.addEventListener("pagehide",actions.leaveTx);
document.addEventListener("visibilitychange",()=>{if(document.hidden)actions.leaveTx();});
function renderTxTable(txs) {
  const current=txLoadedScope===txScope(),filter=document.getElementById("tx-state-filter").value,q=document.getElementById('tx-search').value.trim().toLowerCase();
  txs=current?txs.filter(tx=>(!filter||tx.state===filter)&&(!q||(displayMode()!=='hidden'&&[tx.account,tx.fundCode,txTypeLabel(tx.type)].join(' ').toLowerCase().includes(q)))):[];
  const tbody = document.querySelector("#tx-table tbody");
  tbody.innerHTML = "";
  txs.forEach(function(tx) {
    const tr = document.createElement("tr");
    tr.innerHTML = "<td>" + esc(tx.tradeDate) + "</td>" +
                   "<td>" + (displayMode()==="hidden"?"****":esc(tx.account)) + "</td>" +
                   "<td><b>" + (displayMode()==="hidden"?"****":esc(tx.fundCode)) + "</b><small>"+(displayMode()==="hidden"?"****":esc(tx.account))+" · "+esc(txTypeLabel(tx.type))+"</small></td>" +
                   "<td><span class='badge " + (tx.type === "BUY" ? "up" : "down") + "'>" + esc(txTypeLabel(tx.type)) + "</span></td>" +
                   "<td class='num'>" + (maskAmounts()?"****":esc(tx.quantity)) + "</td>" +
                   "<td class='num'>" + money(tx.price) + "</td>" +
                   "<td class='num'>" + money(tx.amount) + "</td>" +
                   "<td class='num'>" + money(tx.fee || "0.00") + "</td><td>"+esc(tx.deleted?t("fundLedgerArchivedStatus"):txStateLabel(tx.state))+"</td><td>"+(tx.deleted?"<button data-transaction='restore' data-id='"+tx.id+"'>"+t("fundHomeRestore")+"</button>":"<button data-transaction='edit' data-id='"+tx.id+"'>"+t("fundHomeEdit")+"</button><button data-transaction='delete' data-id='"+tx.id+"'>"+t("fundLedgerArchiveRecord")+"</button>"+(tx.state==="pending"?"<button data-transaction='confirm' data-id='"+tx.id+"'>"+t("fundHomeConfirm")+"</button>":""))+"</td>";
    tbody.appendChild(tr);
  });
  if(!txs.length)tbody.innerHTML=`<tr><td colspan="10" class="investment-row-empty"><p>${esc(t(txLoading?"fundHomeLoading":txError?"fundLedgerLoadFailed":"fundLedgerTransactionsEmpty"))}</p></td></tr>`;
  const summary=(key,value)=>`<div><span>${t(key)}</span><b>${value}</b></div>`;
  document.getElementById("tx-summary").innerHTML=current?summary("fundLedgerVisibleRecords",txs.length)+summary("fundLedgerStateConfirmed",txs.filter(tx=>!tx.deleted&&tx.state==="confirmed").length)+summary("fundLedgerStatePending",txs.filter(tx=>!tx.deleted&&tx.state==="pending").length):"";
  document.getElementById("tx-load-msg").textContent=txLoading?t("fundHomeLoading"):txError?t("fundLedgerLoadFailed")+" · "+txError.message:"";
  document.getElementById("tx-retry").hidden=!txError;
  document.getElementById("tx-table").setAttribute("aria-busy",String(txLoading));
}
// 状态枚举不允许原样进入 UI（R-I2/A08）：未知值回退原文，由外层 esc 保证安全。
function txStateLabel(stateValue) {
  return stateValue === "pending" ? t("fundLedgerStatePending") : stateValue === "confirmed" ? t("fundLedgerStateConfirmed") : stateValue === "cancelled" ? t("fundLedgerStateCancelled") : stateValue;
}
function txTypeLabel(type) {
  return type === "BUY" ? t("fundLedgerTypeBuy") : type === "SELL" ? t("fundLedgerTypeSell") : type;
}
async function loadTx() {
  if(!txActive())return;
  const gen=++txGeneration,scope=txScope();txController?.abort();const owned=txController=new AbortController();
  txLoading=true;txError=null;renderTxTable(state.lastTxList||[]);
  try{
    const res=await api("GET",`/api/transactions${scope?"?includeDeleted=1":""}`,null,{signal:owned.signal});
    if(gen!==txGeneration||!txActive())return;
    state.lastTxList=res.transactions||[];txLoadedScope=scope;updateTxSidebar();
  }catch(e){if(gen===txGeneration&&!owned.signal.aborted)txError=e;}
  finally{if(gen===txGeneration){txLoading=false;txController=null;renderTxTable(state.lastTxList||[]);}}
}
actions.loadTx = loadTx;

function updateTxSidebar(txList) {
  const list = (txList || state.lastTxList || []).filter(tx=>!tx.deleted&&tx.state==="confirmed");
  let buyCount = 0, sellCount = 0, buyAmount = 0n, sellAmount = 0n, totFee = 0n;
  list.forEach(function(tx) {
    const amt = decimalToMinor(tx.amount || "0");
    const fee = decimalToMinor(tx.fee || "0");
    totFee += fee;
    if (tx.type === "BUY") {
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

  if (totalEl) totalEl.textContent = t("fundLedgerTxCount").replace("{n}", list.length);
  if (buyEl) buyEl.textContent = money(buyAmount);
  if (sellEl) sellEl.textContent = money(sellAmount);
  if (feeEl) feeEl.textContent = money(totFee);
  if (ratioEl) ratioEl.textContent = t("fundLedgerBuySellRatio").replace("{b}", buyCount).replace("{s}", sellCount);
}
actions.updateTxSidebar = updateTxSidebar;

function renderNavCounts(list) {
  const fundsMap = {};
  list.forEach(function(n) { if (n.fundCode) fundsMap[n.fundCode] = true; });
  const totalFunds = Object.keys(fundsMap).length;
  const fundsEl = document.getElementById("side-nav-total-funds");
  const recordsEl = document.getElementById("side-nav-total-records");
  if (fundsEl) fundsEl.textContent = t("fundLedgerFundsCount").replace("{n}", totalFunds);
  if (recordsEl) recordsEl.textContent = t("fundLedgerTxCount").replace("{n}", list.length);
}
function updateNavSidebar() {
  api("GET", "/api/nav").then(function(res) {
    renderNavCounts(res.navs || res.items || []);
  }).catch(function() {});
}
actions.updateNavSidebar = updateNavSidebar;

document.getElementById("tx-form").addEventListener("submit", function(e) {
  e.preventDefault();
  if(txWriting||globalThis.Fund.home.sensitive()||!e.target.reportValidity())return;
  const fd = new FormData(e.target);
  const body = {
    account: fd.get("account"),
    fundCode: fd.get("fundCode"),instrumentKind:fd.get("instrumentKind"),market:fd.get("instrumentKind")==="fund"?"CN":fd.get("market"),
    state:fd.get("instrumentKind")==="fund"?fd.get("state"):"confirmed",
    type: fd.get("type"),
    quantity: fd.get("quantity"),
    price: fd.get("price"),
    fee: fd.get("fee"),
    tradeDate: fd.get("tradeDate"),
    source: "manual"
  };
  const signature=JSON.stringify(body);
  if(txRetry?.signature!==signature)txRetry={signature,requestId:globalThis.Fund.home.requestId()};
  body.requestId=txRetry.requestId;
  const msg = document.getElementById("tx-msg");
  msg.dataset.i18n = "fundLedgerRecording";
  msg.textContent = t("fundLedgerRecording");
  msg.className = "";
  txWriting=true;
  const controls=[...e.target.elements].filter(el=>!el.disabled);controls.forEach(el=>el.disabled=true);
  api("POST", "/api/transactions", body).then(async function() {
    txRetry=null;e.target.reset();syncTxForm(true);
    msg.textContent = t("fundLedgerRecordOk");
    msg.dataset.i18n = "fundLedgerRecordOk";
    msg.className = "success";
    document.getElementById("tx-entry").open=false;
    if(txActive())document.querySelector("#tx-entry > summary").focus();
    await loadTx();await globalThis.Fund.home.refresh();
  }).catch(function(err) {
    delete msg.dataset.i18n;
    msg.textContent = t("fundLedgerRecordFailed") + err.message;
    msg.className = "err";
  }).finally(()=>{txWriting=false;controls.forEach(el=>el.disabled=false);syncTxForm();});
});
function syncTxForm(resetDate=false){
 const form=document.getElementById("tx-form"),kind=form.elements.instrumentKind.value;
 const date=new Date(Date.now()+8*3600000).toISOString().slice(0,10);
 if(resetDate||!form.elements.tradeDate.value)form.elements.tradeDate.value=date;
 form.elements.tradeDate.max=date;
 form.elements.quantity.min=kind==="stock"?"1":"0.0001";form.elements.quantity.step=kind==="stock"?"1":"0.0001";
 document.getElementById("tx-market-row").hidden=kind==="fund";
 document.getElementById("tx-state-row").hidden=kind!=="fund";
 document.getElementById("tx-otc-hint").hidden=kind!=="fund";
 if(kind!=="fund")form.elements.state.value="confirmed";
}
document.querySelector("#tx-form [name=instrumentKind]").onchange=()=>{document.querySelector("#tx-form [name=state]").value="pending";syncTxForm();};
document.getElementById("tx-refresh").onclick=loadTx;
document.getElementById("tx-retry").onclick=loadTx;
document.getElementById("tx-state-filter").onchange=()=>renderTxTable(state.lastTxList||[]);
document.getElementById('tx-search').oninput=()=>renderTxTable(state.lastTxList||[]);
document.getElementById('tx-detail-columns').onchange=e=>document.getElementById('tx-table').classList.toggle('show-detail-columns',e.target.checked);
document.getElementById('nav-return-positions').onclick=()=>document.getElementById('tab-positions').click();

// ---------- 批量导入模块 ----------
function renderImportPreview(res) {
  document.getElementById('import-impact').innerHTML=[['fundLedgerVisibleRecords',res.total_rows||0],['fundLedgerValid',res.valid_rows||0],['fundLedgerImportInvalidRows',(res.total_rows||0)-(res.valid_rows||0)]].map(([key,value])=>`<div><span>${esc(t(key))}</span><b>${value}</b></div>`).join('');
  const tbody = document.querySelector("#import-table tbody");
  tbody.innerHTML = "";
  (res.lines || []).forEach(function(l) {
    const tr = document.createElement("tr");
    tr.innerHTML = "<td>" + esc(l.line_no) + "</td><td>" + esc(l.account) + "</td><td>" + esc(l.fund_code) + "</td><td>" + esc(l.kind) + "</td><td class='num'>" + esc(l.quantity_raw) + "</td><td class='num'>" + esc(l.amount_raw) + "</td><td>" + esc(l.error || t("fundLedgerValid")) + "</td>";
    tbody.appendChild(tr);
  });
}

// ---------- 批量导入（DES-P10/D05）：五步交互与稳定幂等身份 ----------
// 后端契约字段为 csv（api/import_api.rs）；sourceId 对同一提交内容固定
// （后端按 sourceId+template+fileSha256+paramsHash 幂等去重），不用
// Date.now 产生新批次身份。输入改动使预览立即失效（重新预览前禁提交）。
let importPreviewText = null; // 预览对应的 CSV 原文（预览身份）
let importGeneration=0,importController=null;
actions.leaveImport=()=>{importGeneration++;importController?.abort();importController=null;document.getElementById('import-preview-btn').disabled=false;};
window.addEventListener('pagehide',actions.leaveImport);
function importStep(step){document.querySelectorAll('#import-steps [data-step]').forEach(item=>{item.toggleAttribute('aria-current',Number(item.dataset.step)===step);if(Number(item.dataset.step)===step)item.setAttribute('aria-current','step');});}
function importSourceId(text) {
  // 稳定 sourceId：与内容绑定而非时间；重复点击/断连重试同身份不重入账。
  return "manual-import";
}
function importStale() {
  const current = document.getElementById("import-text").value.trim();
  return current !== importPreviewText;
}
document.getElementById("import-text").addEventListener("input", function() {
  actions.leaveImport();
  importStep(1);
  // 输入改动使旧预览失效：保留旧结果仅作为「预览已过期」提示，禁提交。
  if (state.pendingImportPreview && importStale()) {
    state.pendingImportPreview = null;
    document.getElementById("import-commit-btn").disabled = true;
    const msg = document.getElementById("import-msg");
    msg.textContent = t("fundLedgerPreviewStale");
    msg.className = "warn";
  }
});

document.getElementById("import-preview-btn").addEventListener("click", function() {
  const text = document.getElementById("import-text").value.trim();
  if (!text) return;
  actions.leaveImport();const generation=importGeneration;importController=new AbortController();this.disabled=true;
  const msg = document.getElementById("import-msg");
  msg.textContent = t("fundLedgerParsing");
  msg.className = "";
  importStep(2);
  api("POST", "/api/import/preview", {
    csv: text,
    templateVersion: 1
  },{signal:importController.signal}).then(function(res) {
    if(generation!==importGeneration||document.getElementById('import-text').value.trim()!==text)return;
    importPreviewText = text;
    state.pendingImportPreview = res;
    const invalid = (res.total_rows || 0) - (res.valid_rows || 0);
    document.getElementById("import-commit-btn").disabled = !(res.valid_rows > 0);
    msg.textContent = t("fundLedgerParseDone").replace("{total}", res.total_rows).replace("{valid}", res.valid_rows)
      + (invalid > 0 ? " · " + t("fundLedgerParseInvalid").replace("{n}", invalid) : "");
    renderImportPreview(res);
    importStep(3);
  }).catch(function(err) {if(generation!==importGeneration||err.name==='AbortError')return;msg.textContent = t("fundLedgerImportError") + err.message; msg.className = "err"; }).finally(()=>{if(generation===importGeneration)document.getElementById('import-preview-btn').disabled=false;});
});

document.getElementById("import-commit-btn").addEventListener("click", function() {
  // 提交前提：存在预览且输入未改动（原子重校验由后端 commit 负责）。
  if (!state.pendingImportPreview || importStale()) return;
  const btn = document.getElementById("import-commit-btn");
  const text = document.getElementById("import-text").value.trim();
  btn.disabled = true; // 提交中禁重复；失败后重新预览再启用。
  importStep(4);
  api("POST", "/api/import/commit", {
    csv: text,
    templateVersion: 1,
    sourceId: importSourceId(text)
  }).then(function(res) {
    document.getElementById("import-msg").textContent = t("fundLedgerCommitOk").replace("{funds}", res.funds_created).replace("{rows}", res.rows_imported);
    document.getElementById("import-msg").className = "";
    state.pendingImportPreview = null;
    importPreviewText = null;
    importStep(5);
  }).catch(function(err) {
    // 失败留文：输入与预览保留，重新预览后可再提交。
    const msg = document.getElementById("import-msg");
    msg.textContent = t("fundLedgerImportError") + err.message;
    msg.className = "err";
    btn.disabled = !state.pendingImportPreview || importStale();
    importStep(3);
  });
});

// ---------- 公募基金净值模块 ----------
document.getElementById("nav-sync-btn").addEventListener("click", function() {
  const code = document.getElementById("nav-code-input").value.trim();
  if (!code) return;
  const msg = document.getElementById("nav-msg");
  msg.textContent = t("fundLedgerNavSyncing");
  msg.className = "";
  api("POST", "/api/nav/sync", { fundCode: code }).then(function(res) {
    msg.textContent = t("fundLedgerNavSyncOk").replace("{n}", res.added);
    loadNavData(code);
  }).catch(function(err) { msg.textContent = t("fundLedgerNavSyncFailed") + err.message; msg.className = "err"; });
});

function renderNavRow(n) {
  return "<tr><td><b>" + esc(n.fundCode) + "</b></td><td>" + esc(n.navDate) + "</td><td class='num'>" + money(n.unitNav) + "</td><td>" + esc(navSourceLabel(n.source)) + "</td><td>" + esc(new Date(Number(n.fetchedAt) * 1000).toLocaleString(state.language)) + "</td></tr>";
}
function renderNavTable(navs) {
  const tbody = document.querySelector("#nav-table tbody");
  tbody.innerHTML = navs.map(renderNavRow).join("");
}
function navSourceLabel(source) {
  const map = { "eastmoney-mobile": t("fundLedgerNavSourceEastmoney"), user_confirmed: t("fundLedgerNavSourceManual") };
  return map[source] || source;
}
// DES-P11/D05：默认本地最近 30 条；「查看更多」走 beforeDate+limit 有界
// 分页（nextCursor），没有更早数据时显示历史起点并隐藏按钮。
let navCursor = null;
function navScopeLabel(list) {
  if (!list.length) return "";
  const from = list[list.length - 1].navDate, to = list[0].navDate;
  return t("fundLedgerNavRangeLabel").replace("{n}", list.length).replace("{from}", from).replace("{to}", to);
}
function loadNavData(code, opts = {}) {
  const append = opts.append === true;
  const query = "/api/nav?code=" + encodeURIComponent(code)
    + (append && navCursor ? "&beforeDate=" + encodeURIComponent(navCursor) + "&limit=30" : "");
  return api("GET", query).then(function(res) {
    const navs = res.nav || [];
    if (append) {
      const tbody = document.querySelector("#nav-table tbody");
      const beforeRows = navs.map(renderNavRow).join("");
      tbody.insertAdjacentHTML("beforeend", beforeRows);
      state.lastNavList = (state.lastNavList || []).concat(navs);
    } else {
      state.lastNavList = navs;
      renderNavTable(navs);
    }
    navCursor = res.nextCursor || null;
    const scope = document.getElementById("nav-scope");
    if (scope && !append) scope.textContent = navScopeLabel(navs);
    const moreBtn = document.getElementById("nav-more-btn");
    const moreEnd = document.getElementById("nav-more-end");
    if (moreBtn) moreBtn.hidden = !navCursor;
    if (moreEnd) moreEnd.hidden = !!navCursor || append === false && navs.length === 0;
    renderNavCounts(state.lastNavList || []);
  });
}
document.getElementById("nav-more-btn")?.addEventListener("click", function() {
  const code = document.getElementById("nav-code-input").value.trim();
  if (!code || !navCursor) return;
  const btn = this;
  btn.disabled = true;
  loadNavData(code, { append: true }).catch(function(err) {
    const msg = document.getElementById("nav-msg");
    msg.textContent = t("fundLedgerNavSyncFailed") + err.message;
    msg.className = "err";
  }).finally(function() { btn.disabled = false; });
});


let legacyHoldings=[],legacyPage=1;
function renderLegacyHoldings(){
  const area=document.getElementById("holding-legacy");
  area.innerHTML=legacyHoldings.length?`<h3>${t("fundHomeLegacyAssets")}</h3><p>${t("fundHomeLegacyHoldingHint")}</p>${legacyHoldings.map(a=>`<article>${displayMode()==="hidden"?"****":esc(a.name)} · ${money(a.marketValue)}<button data-legacy-holding="${a.id}" ${a.deleted?"disabled":""}>${t("fundHomeConvertHolding")}</button><button data-legacy-archive="${a.id}" data-op="${a.deleted?"restore":"delete"}">${t(a.deleted?"fundHomeRestore":"fundHomeRemove")}</button></article>`).join("")}`:"";
  if(legacyHoldings.length||legacyPage>1){area.insertAdjacentHTML("beforeend",`<button data-legacy-prev ${legacyPage===1?"disabled":""}>${t("fundHomePrevious")}</button> ${legacyPage} <button data-legacy-next ${legacyHoldings.length<50?"disabled":""}>${t("fundHomeNext")}</button>`);area.querySelector("[data-legacy-prev]").onclick=()=>{legacyPage--;void loadLegacyHoldings();};area.querySelector("[data-legacy-next]").onclick=()=>{legacyPage++;void loadLegacyHoldings();};}
}
async function loadLegacyHoldings(){try{const data=await api("GET",`/api/legacy-assets?kind=holdings&page=${legacyPage}`);legacyHoldings=(data.assets||[]).filter(a=>a.productType!=="gold"&&!a.convertedRef);renderLegacyHoldings();}catch(e){globalThis.Fund.home&&globalThis.Fund.home.status(e.message);}}
document.addEventListener("click",async e=>{
 const b=e.target.closest("[data-legacy-holding],[data-legacy-archive]"),h=globalThis.Fund.home;if(!b||h.sensitive())return;const a=legacyHoldings.find(a=>a.id===Number(b.dataset.legacyHolding||b.dataset.legacyArchive));if(!a)return;
 if(b.dataset.legacyArchive){try{if(!await h.confirm(t("fundHomeOfflineDeleteHint")))return;await api("POST","/api/legacy-assets/change",{id:a.id,expectedVersion:a.version,op:b.dataset.op});await reload();}catch(e){h.status(e.message);}return;}
 h.panel(()=>t("fundHomeConvertHolding"),()=>`<p>${t("fundHomeLegacyHoldingHint")}</p><p>${esc(a.account)} · ${esc(a.instrumentKey)} · ${money(a.marketValue)}</p><form>${h.input("quantity",t("fundLedgerLabelQuantity"),"","number",'required min="0.0001" step="0.0001"')}${h.input("price",t("fundLedgerLabelPrice"),"","number",'required min="0" step="0.0001"')}${h.input("totalCost",t("fundHomeCostOptional"),"","number",'min="0" step="0.01"')}${h.input("asOf",t("fundHomeActualEventDate"),a.valuedAt,"date",`required max="${today()}"`)}<button type="submit">${t("fundHomePreview")}</button></form><p data-preview role="status"></p><button data-save disabled>${t("fundHomeConfirm")}</button>`,root=>{
 let payload;root.querySelector("form").oninput=()=>{payload=null;root.querySelector("[data-save]").disabled=true;};h.submit(root,async v=>{const fixed=(value,scale)=>{const [a,b=""]=value.split(".");if(!/^\d+(?:\.\d+)?$/.test(value)||b.length>scale)throw Error(t("fundHomeAmountInvalid"));return BigInt(a)*10n**BigInt(scale)+BigInt(b.padEnd(scale,"0"));};const newMinor=fixed(v.quantity,4)*fixed(v.price,4)/1000000n;root.querySelector("[data-preview]").textContent=`${t("fundHomeCurrentValuation")}: ${newMinor/100n}.${String(newMinor%100n).padStart(2,"0")} · ${t("fundHomeLegacyConversionConfirm")}`;payload={...v,id:a.id,expectedVersion:a.version,op:"convert",confirmed:true};root.querySelector("[data-save]").disabled=false;});root.querySelector("[data-save]").onclick=async e=>{if(!payload)return;e.target.disabled=true;try{await api("POST","/api/legacy-assets/change",payload);h.closePanel();await reload();}catch(error){h.failure(error);e.target.disabled=false;}};
 });
});

// 表单与写入同一 owner；详情中只修改期初事实，不直接写 positions 投影。
const today=()=>new Date(Date.now()+8*3600000).toISOString().slice(0,10);
const reload=async()=>{if(actions.loadPositions)await actions.loadPositions();await loadTx();await globalThis.Fund.home.refresh();};
function openingForm(p={},opening){
 const h=globalThis.Fund.home;
 h.panel(()=>t("fundHomeOpeningHolding"),()=>`<form>${opening?"":h.input("account",t("fundHomeAccount"),p.account||"","text",'required maxlength="64"')+h.select("instrumentKind",t("fundHomeType"),[["fund",t("fundHomeFunds")],["stock",t("fundHomeStocks")],["etf","ETF"]],p.instrumentKind||"fund")+h.select("market",t("fundHomeMarket"),[["SSE","SSE"],["SZSE","SZSE"],["BSE","BSE"]],p.market||"SSE")+h.input("fundCode",t("fundLedgerLabelCode"),p.fundCode||"","text",'required pattern="[0-9]{6}"')+h.input("fundName",t("fundHomeName"),p.fundName||"","text",'maxlength="64"')}${h.input("quantity",t("fundLedgerLabelQuantity"),opening?.quantity||"","number",p.instrumentKind==="stock"?'min="1" step="1" required':'min="0.0001" step="0.0001" required')}${h.input("totalCost",t("fundHomeCostOptional"),opening?.totalCost??"","number",'min="0" step="0.01"')}${h.input("asOf",t("fundHomeActualEventDate"),opening?.asOf||today(),"date",`required max="${today()}"`)}<p>${t("fundHomeOpeningHint")}</p><button type="submit">${t("fundHomeSave")}</button></form>`,root=>h.submit(root,async v=>{if(opening)await api("POST","/api/opening/change",{...v,accountId:p.accountId,fundId:p.fundId,expectedVersion:opening.version,op:"update"});else await api("POST","/api/openings",v);h.closePanel();await reload();if(p.returnFocusId)document.getElementById(p.returnFocusId)?.focus();}));
}
async function openingDetail(p){
 const h=globalThis.Fund.home;if(h.sensitive())return;
 try{const d=await api("GET",`/api/opening/history?accountId=${p.accountId}&fundId=${p.fundId}`),o=d.opening;
 state.lastOpeningDetail=d;
 h.panel(()=>t("fundHomeOpeningHistory"),()=>`<p>${o?esc(o.asOf)+" · "+esc(o.quantity)+" · "+esc(o.totalCost??t("fundHomeUnknown")):t("fundHomeNoOpening")}</p>${o?o.deleted?`<button data-op="restore">${t("fundHomeRestore")}</button>`:`<button data-edit>${t("fundHomeEdit")}</button><button data-op="delete">${t("fundHomeRemove")}</button>`:""}<details open><summary>${t("fundHomeHistory")}</summary><table><thead><tr><th>${t("fundLedgerHistoryVersion")}</th><th>${t("fundLedgerLabelQuantity")}</th><th>${t("fundHomeCostOptional")}</th><th>${t("fundHomeActualEventDate")}</th><th>${t("fundLedgerHistoryRecordedAt")}</th><th>${t("fundLedgerStatus")}</th></tr></thead><tbody>${openingHistoryRows(d.history)}</tbody></table><details><summary>${t("fundLedgerHistoryDiagnostics")}</summary><pre>${esc(JSON.stringify(d.history,null,2))}</pre></details></details>`,root=>{root.querySelector("[data-edit]")?.addEventListener("click",()=>openingForm(p,o));root.querySelectorAll("[data-op]").forEach(b=>b.onclick=async()=>{try{if(!await h.confirm(t("fundHomeOpeningChangeHint")))return;await api("POST","/api/opening/change",{accountId:p.accountId,fundId:p.fundId,expectedVersion:o.version,op:b.dataset.op});h.closePanel();await reload();}catch(e){h.failure(e);}});});
 }catch(e){h.status(e.message);}
}
// 历史字段化展示（T02/A08）：版本、数量、成本、事件日期、记录时间与状态；
// 原始 JSON 仅作为诊断导出折叠区。payload_json 存定点整数原始值，按精度换算。
function rawFixed(value, scale){
  if(value==null)return "-";
  const n=BigInt(value);const base=10n**BigInt(scale);const abs=n<0n?-n:n;
  return `${n<0n?"-":""}${abs/base}.${String(abs%base).padStart(scale,"0")}`;
}
function openingHistoryRows(history){
 return (history||[]).map(row=>{const r=row.record||{};return `<tr><td>${esc(row.version)}</td><td>${esc(rawFixed(r.quantity,4))}</td><td>${r.cost==null?esc(t("fundHomeUnknown")):esc(rawFixed(r.cost,2))}</td><td>${esc(r.asOf??"-")}</td><td>${esc(formatRecordedAt(row.recordedAt))}</td><td>${r.deleted?t("fundHomeArchived"):t("fundLedgerHistoryActive")}</td></tr>`;}).join("");
}
function formatRecordedAt(value){
 if(value==null)return "-";
 const n=Number(value);
 return Number.isFinite(n)&&n>0?new Date(n*1000).toLocaleString(state.language):String(value);
}
function correctionForm(tx){
 const h=globalThis.Fund.home,key=h.requestId();
 const raw=(s,scale)=>{if(!/^\d+(?:\.\d+)?$/.test(s))throw Error(t("fundHomeAmountInvalid"));const [a,b=""]=s.split(".");if(b.length>scale)throw Error(t("fundHomeAmountInvalid"));return (BigInt(a)*10n**BigInt(scale)+BigInt(b.padEnd(scale,"0"))).toString();};
 h.panel(()=>t("fundHomeCorrectTransaction"),()=>`<form>${h.input("quantity",t("fundLedgerLabelQuantity"),tx.quantity,"number",tx.fundCode.startsWith("stock:")?'min="1" step="1" required':'min="0.0001" step="0.0001" required')}${h.input("price",t("fundLedgerLabelPrice"),tx.price,"number",'min="0.0001" step="0.0001" required')}${h.input("fee",t("fundHomeConfirmFee"),tx.fee,"number",'min="0" step="0.01" required')}${h.input("tradeDate",t("fundHomeActualEventDate"),tx.tradeDate,"date",`required max="${today()}"`)}${h.input("reason",t("fundHomeCorrectionReason"),"","text",'required maxlength="512"')}${h.check("cashConfirmed",t("fundHomeCorrectionCashConfirm"))}${h.input("settledAt",t("fundHomeActualEventDate"),today(),"date",`max="${today()}"`)}${h.input("cashNote",t("fundHomeReconciliationEvidence"),"","text",'maxlength="4096"')}<button type="submit">${t("fundHomePreview")}</button></form><pre data-preview></pre><button data-save disabled>${t("fundHomeConfirm")}</button>`,root=>{
 let payload=null;root.querySelector("form").oninput=()=>{payload=null;root.querySelector("[data-save]").disabled=true;};
 h.submit(root,async v=>{const p={id:tx.id,expectedVersion:tx.version,quantityRaw:raw(v.quantity,4),priceRaw:raw(v.price,4),feeRaw:raw(v.fee,2),tradeDate:v.tradeDate,reason:v.reason,requestId:key};if(root.querySelector("[name=cashConfirmed]").checked)p.cashCorrection={confirmed:true,settledAt:v.settledAt,note:v.cashNote};const r=await api("POST","/api/transactions/correction/preview",p);const fixed=(value,scale)=>{const n=BigInt(value),base=10n**BigInt(scale),abs=n<0n?-n:n;return `${n<0n?"-":""}${abs/base}.${String(abs%base).padStart(scale,"0")}`;};root.querySelector("[data-preview]").textContent=`${t("fundHomeCorrectionPreview")}: ${fixed(r.originalQuantityRaw,4)} → ${fixed(r.resultingQuantityRaw,4)}\n${t("fundLedgerLabelAmount")}: ${fixed(r.originalAmountRaw,2)} → ${fixed(r.newAmountRaw,2)}\n${t("fundHomeCashDifference")}: ${r.cashDifferenceMinor==null?t("fundHomeNeedsVerificationccf920"):fixed(r.cashDifferenceMinor,2)}\n${(r.blockers||[]).join("；")}`;if(r.requiresCashConfirmation&&!p.cashCorrection)throw Error(t("fundHomeCorrectionCashConfirm"));payload=r.blockers?.length?null:p;root.querySelector("[data-save]").disabled=!payload;});
 root.querySelector("[data-save]").onclick=async e=>{if(!payload)return;e.target.disabled=true;try{await api("POST","/api/transactions/correction",payload);h.closePanel();await reload();if(tx.returnFocusId)document.getElementById(tx.returnFocusId)?.focus();}catch(error){h.failure(error);e.target.disabled=false;}};
 });
}
document.getElementById("tx-show-deleted").onchange=loadTx;
async function openingList(page=1){const h=globalThis.Fund.home;if(h.sensitive())return;try{const d=await api("GET",`/api/opening/list?page=${page}`);h.panel(()=>t("fundHomeOpeningHistory"),()=>d.openings.map(p=>`<button data-open="${p.accountId}:${p.fundId}">${esc(p.account)} · ${esc(p.fundCode)}${p.deleted?" · "+t("fundHomeArchived"):""}</button>`).join("")+`<button data-opening-prev ${page===1?"disabled":""}>${t("fundHomePrevious")}</button> ${page} <button data-opening-next ${d.openings.length<50?"disabled":""}>${t("fundHomeNext")}</button>`,root=>{root.querySelector("[data-opening-prev]").onclick=()=>openingList(page-1);root.querySelector("[data-opening-next]").onclick=()=>openingList(page+1);root.querySelectorAll("[data-open]").forEach(b=>b.onclick=()=>openingDetail(d.openings.find(p=>`${p.accountId}:${p.fundId}`===b.dataset.open)));});}catch(e){h.status(e.message);}}
// 估值表单：持仓页（positions-view.js）行操作与旧入口共用；dataRevision
// 优先取持仓快照，回退首页快照，保证 POST /api/holding/valuation 乐观锁一致。
function valuationDialog(p){
 const h=globalThis.Fund.home;
 // 币种默认 CNY（境内公募口径）：显式保存即用户确认；旧基金/外币仍可改 unknown/unsupported。
 // 提交遇账本并发（页面自身缺净值同步等写入）409 时，重读快照取新 dataRevision 自愈重试一次。
 h.panel(()=>t("fundHomeValuation")+(p.fundCode?" · "+p.fundCode:""),()=>`<form>${h.input("price",t("fundLedgerLabelPrice"),"","number",'min="0" step="0.0001" required')}${h.input("asOf",t("fundHomeValuationDate"),today(),"date",`required max="${today()}"`)}${h.select("navCurrency",t("fundHomeCurrency"),[["CNY","CNY"],["unknown",t("fundHomeUnknown")],["unsupported",t("fundHomeUnsupported")]],"CNY")}${h.select("riskClass",t("fundHomeRiskClass"),[["unknown",t("fundHomeUnknown")],["equity",t("fundHomeEquity")],["fixed_income",t("fundHomeFixedIncome")],["gold",t("fundHomeGold")],["other",t("fundHomeOther")]])}<p>${t("fundHomeManualQuoteHint")}</p><button type="submit">${t("fundHomeSave")}</button></form>`,root=>h.submit(root,async v=>{
  const post=revision=>api("POST","/api/holding/valuation",{...v,fundId:p.fundId,dataRevision:revision});
  try{
   await post(p.dataRevision||h.snapshot()?.dataRevision);
  }catch(error){
   // 仅账本并发冲突（409）自愈重试；真正的输入校验错误原样抛出。
   if(error.status!==409)throw error;
   const fresh=await api("GET","/api/positions/detail");
   await post(fresh.dataRevision);
  }
  h.closePanel();await reload();
 }));
}
actions.openValuationDialog=valuationDialog;
actions.openOpeningDetail=openingDetail;
actions.editOpeningRecord=openingForm;
actions.editTransactionRecord=correctionForm;
actions.openOpeningList=()=>openingList();
actions.loadLegacyHoldings=loadLegacyHoldings;
document.addEventListener("click",async e=>{
 const h=globalThis.Fund.home,b=e.target.closest("[data-transaction]");if(!b||h.sensitive())return;
 if(b.disabled)return;b.disabled=true;
 const feedback=message=>{const node=document.getElementById("tx-msg");delete node.dataset.i18n;node.textContent=message;node.className="err";};
 try{
  const tx=state.lastTxList.find(tx=>tx.id===Number(b.dataset.id));if(!tx)return;
  if(b.dataset.transaction==="edit"){correctionForm(tx);return;}
  if(b.dataset.transaction==="delete"){const r=await api("POST","/api/transactions/delete/preview",{id:tx.id});if(r.blockers?.length){feedback(t("fundLedgerArchiveBlocked")+" · "+r.blockers.join("；"));return;}if(!await h.confirm(t("fundHomeDeleteTransactionHint")+" · "+tx.account+" · "+tx.fundCode))return;}
  await api("POST",`/api/transactions/${b.dataset.transaction}`,{id:tx.id,expectedVersion:tx.version,reason:"user"});await reload();
 }catch(error){feedback(error.message);}finally{if(b.isConnected)b.disabled=false;}
});
document.addEventListener("fund:privacy-changed",()=>{renderTxTable(state.lastTxList||[]);updateTxSidebar();const form=document.getElementById("tx-form"),entry=document.getElementById("tx-entry");if(maskAmounts()){form.reset();syncTxForm(true);form.hidden=true;entry.open=false;entry.hidden=true;}else {form.hidden=false;entry.hidden=false;}});
// 语言切换立即重绘可见投影（R-I5/A09）：不重新请求、不重置筛选与草稿。
// 持仓页与侧栏由 positions-view.js 的同名监听重绘（T04 迁移后唯一 owner）。
document.addEventListener("fund:language-changed",()=>{
  if(state.lastTxList)renderTxTable(state.lastTxList);
  if(state.lastNavList)renderNavTable(state.lastNavList);
  if(state.pendingImportPreview)renderImportPreview(state.pendingImportPreview);
  renderLegacyHoldings();
  updateTxSidebar();
  if(state.lastNavList)renderNavCounts(state.lastNavList);
});
document.getElementById("tx-form").hidden=true;
document.getElementById("tx-entry").hidden=true;syncTxForm(true);
})();
