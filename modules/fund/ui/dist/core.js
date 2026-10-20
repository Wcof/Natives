// Fund 模块共享基座：全局状态唯一归属 + API 基座 + 主题取色 + 跨域 action 注册表。
// 领域模块（market-table/chart/dashboard/ledger/app）从这里取共享状态，
// 并把自己的公开动作注册到 actions，跨域调用一律经 actions，不直接 import 对方。

// 握手令牌（app.js 的握手流程写入，api()/authFetch() 读取）
let _tokenVal = null;
let tokenResolver = null;
let tokenPromise = new Promise(resolve => { tokenResolver = resolve; });
// token 过期（15 分钟 TTL）后无续期机制，长开页面必然 401。
// 401 时向扩展申请重新握手：发 renew-session → 扩展 issue 新 token → welcome 写入。
let _renewing = null;
function rehandshake() {
  if (!_renewing) {
    _tokenVal = null;
    tokenPromise = new Promise(resolve => { tokenResolver = resolve; });
    window.parent.postMessage({ type: "renew-session" }, "*");
    _renewing = Promise.race([
      tokenPromise,
      new Promise(resolve => setTimeout(() => resolve(null), 4000)),
    ]).finally(() => { _renewing = null; });
  }
  return _renewing;
}

const state = {
  get token() { return _tokenVal; },
  set token(val) {
    _tokenVal = val;
    if (val && tokenResolver) {
      tokenResolver(val);
      tokenResolver = null;
    }
  },
  currentSymbol: "",
  currentName: "",
  currentPeriod: "minute",
  currentTool: "cursor",
  magnetEnabled: true,
  colorMode: "cn", // "cn" 红涨绿跌 / "intl" 绿涨红跌
  activeGroup: "",
  currentMarketTab: "indices",
  watchlistItems: [],
  lastQuotesMap: {},
  alertRules: {}, // symbol -> threshold
  triggeredAlertsCount: 0,
  currentMinuteData: null,
  currentKlineData: null,
  indexMinuteData: null,
  drawings: [],
  selectedDrawing: null,
  isDrawing: false,
  tempDrawing: null,
  isDraggingAnchor: -1,
  pendingImportPreview: null,
  language: "zh-CN", // 父页握手后同步（07 §5.2）；切换走 storage 变更 + reload
  surfaceGeneration: null,
  surfaceChallenge: null,
};

// ---------- 最小 i18n 层（审计 C3 骨架）----------
// 双语文案表：先骨架后补全——JS 动态生成的用户可见文案逐批迁入，
// 静态 HTML 文案随后续迭代迁移。缺 key 时回退中文原文。
const I18N_MESSAGES = {
  en: {
    alertTitle: "Investment Alert",
    alertBody: "Symbol {name} ({symbol}) moved {badge}, please review.",
    alertSetPrefix: "Set ±",
    alertSetSuffix: "%",
  },
  "zh-CN": {
    alertTitle: "投资异动预警",
    alertBody: "标的 {name}（{symbol}）涨跌幅已达 {badge}，请关注。",
    alertSetPrefix: "已设 ±",
    alertSetSuffix: "%",
  },
};

function t(key, params) {
  const table = I18N_MESSAGES[state.language] || I18N_MESSAGES["zh-CN"];
  let text = table[key] ?? I18N_MESSAGES["zh-CN"][key] ?? key;
  if (params) {
    for (const [k, v] of Object.entries(params)) {
      text = text.split("{" + k + "}").join(String(v));
    }
  }
  return text;
}

function applyI18nDom(root) {
  // 全文档静态文案（方案 frontend i18n：data-i18n 为权威键）。
  // 只写 textContent 的纯文本节点安全；带子元素的节点由 partial 用 <span data-i18n> 包裹。
  (root || document).querySelectorAll("[data-i18n]").forEach(node => {
    node.textContent = t(node.dataset.i18n);
  });
  for (const attr of ["title", "aria-label", "placeholder"]) {
    (root || document).querySelectorAll(`[data-i18n-${attr}]`).forEach(node => {
      node.setAttribute(attr, t(node.getAttribute(`data-i18n-${attr}`)));
    });
  }
}

function applyLanguage(language) {
  state.language = language === "en" ? "en" : "zh-CN";
  document.documentElement.lang = state.language;
  applyI18nDom();
  document.dispatchEvent(new CustomEvent("fund:language-changed", { detail: { language: state.language } }));
}

function registerMessages(language, messages) {
  Object.assign(I18N_MESSAGES[language], messages);
}

// 跨域动作注册表：领域模块在文件底部注册，调用方经 actions.xxx() 访问。
const actions = {};

// 等待握手 Token 到达，杜绝时序竞态引起的 APP_SESSION_INVALID
async function ensureToken() {
  if (_tokenVal) return _tokenVal;
  const timeout = new Promise(resolve => setTimeout(() => resolve(null), 2500));
  return Promise.race([tokenPromise, timeout]);
}

// 后端稳定错误码 → 本地化文案键（R-I14/R-E14）：优先按 code 映射，
// zh 环境再附带服务端中文细节；en 环境只展示稳定文案，不回显中文原文。
function localizeApiError(status, code, serverMessage) {
  const upper = String(code || "").toUpperCase();
  let key;
  if (upper === "INFORMATION_REFRESH_IN_PROGRESS") key = "fundHomeInformationBusy";
  else if (status === 409) key = "fundHomeDataConflict";
  else if (upper.endsWith("_INVALID") || upper === "UNIQUE_CONFLICT") key = "fundHomeInputInvalid";
  else if (upper.endsWith("_NOT_FOUND")) key = "fundErrorNotFound";
  else if (upper === "ASSET_DUPLICATE") key = "fundErrorDuplicate";
  else if (upper === "ASSET_AMOUNT_OVERFLOW") key = "fundErrorAmountOverflow";
  else if (status >= 500) key = "fundHomeRequestFailed";
  else if (state.language === "en") key = "fundErrorUnknown";
  else return serverMessage || t("fundErrorUnknown");
  const detail = state.language.startsWith("zh") && status < 500 && serverMessage ? ` · ${serverMessage}` : "";
  return t(key) + detail;
}

// API 与通信基座
async function api(method, url, body, options = {}) {
  const headers = { "Content-Type": "application/json" };
  const send = () => {
    if (_tokenVal) headers["Authorization"] = "Bearer " + _tokenVal;
    return fetch(url, {
      method: method,
      headers: headers,
      signal: options.signal,
      body: body ? JSON.stringify(body) : undefined
    });
  };
  let r = await send();
  // token 过期（APP_SESSION_INVALID）时重新握手换新 token 并重试一次。
  if (r.status === 401 && await rehandshake()) r = await send();
  // A snapshot may race another committed write; retry reads once, never writes.
  if (method === 'GET' && r.status === 409 && options.retryConflict !== false) r = await send();
  if (!r.ok) {
    const j = await r.json().catch(() => ({}));
    const error=new Error(localizeApiError(r.status, j.error, j.message||j.error_text||""));
    error.status=r.status;error.code=j.error;throw error;
  }
  const result = await r.json();
  if (!["GET", "HEAD"].includes(method) && !url.includes("preview") && state.surfaceGeneration && state.surfaceChallenge) {
    window.parent.postMessage({type:"natives:data-changed",generation:state.surfaceGeneration,challenge:state.surfaceChallenge},"*");
  }
  return result;
}

// 带鉴权头的 fetch 请求，避免出现 APP_SESSION_INVALID
async function authFetch(url, options = {}) {
  const send = () => {
    const headers = Object.assign({}, options.headers);
    if (_tokenVal && !headers["Authorization"] && !headers["authorization"]) {
      headers["Authorization"] = "Bearer " + _tokenVal;
    }
    return fetch(url, Object.assign({}, options, { headers }));
  };
  let r = await send();
  // token 过期（APP_SESSION_INVALID）时重新握手换新 token 并重试一次。
  if (r.status === 401 && await rehandshake()) r = await send();
  return r;
}

// 主题取色：Canvas 绘制统一从 CSS 变量读取，随 volt/archive 主题与涨跌色同步。
// 缺角色回 muted（不静默统一退灰到固定色：muted 本身随主题变化）。
function themeColor(name) {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim()
    || getComputedStyle(document.documentElement).getPropertyValue("--natives-text-muted").trim()
    || "#888888";
}

// 支持实际使用的 HEX（#rgb/#rrggbb）与 rgb()/rgba() 变量值；alpha 以
// replace 尾部插入，解析失败才回退 muted 透明色（DES 02 图表取色契约）。
function themeRgba(name, alpha) {
  const c = themeColor(name);
  const hex = c.match(/^#([0-9a-fA-F]{6})$/);
  if (hex) {
    const n = parseInt(hex[1], 16);
    return "rgba(" + ((n >> 16) & 255) + "," + ((n >> 8) & 255) + "," + (n & 255) + "," + alpha + ")";
  }
  const rgb = c.match(/^rgba?\(([^)]+)\)$/);
  if (rgb) {
    const parts = rgb[1].split(/[\s,\/]+/).filter(Boolean).slice(0, 3);
    if (parts.length === 3) return "rgba(" + parts.join(",") + "," + alpha + ")";
  }
  return "rgba(136,136,136," + alpha + ")";
}

// Shared NAV sync owner: Home and Positions use one bounded, cancellable client.
const navAttempts = new Map();
let navAttemptDay = "", navSyncPromise = null;
async function syncHoldingNav(navs, signal, manual = false) {
  if(signal.aborted)return {changed:false,failed:false};
  if (navSyncPromise) { await navSyncPromise; if (signal.aborted) return {changed:false,failed:false}; }
  const today = new Date(Date.now()+8*3600000).toISOString().slice(0,10), now=Date.now();
  if (manual || navAttemptDay !== today) { navAttempts.clear(); navAttemptDay=today; }
  const codes=[...new Set((navs||[]).filter(n=>n.navDate!==today).map(n=>n.fundCode))];
  if(Fund.state.pendingFundNav!==Boolean(codes.length)){Fund.state.pendingFundNav=Boolean(codes.length);document.dispatchEvent(new Event('fund:nav-state-changed'));}
  for (const code of navAttempts.keys()) if (!codes.includes(code)) navAttempts.delete(code);
  const due=codes.filter(c=>now-(navAttempts.get(c)?.at||0)>=Math.max(30,Number(Fund.state.quoteRefreshSeconds)||30)*1000).slice(0,10);
  if(signal.aborted)return {changed:false,failed:false};
  due.forEach(c=>navAttempts.set(c,{at:now,failed:false}));
  if(!due.length)return {changed:false,failed:codes.some(c=>navAttempts.get(c)?.failed)};
  const owned=Promise.allSettled(due.map(c=>api("POST","/api/nav/sync",{fundCode:c},{signal})));
  navSyncPromise=owned;
  try {
    const replies=await owned;
    if(signal.aborted){due.forEach(c=>navAttempts.delete(c));return {changed:false,failed:false};}
    replies.forEach((r,i)=>navAttempts.set(due[i],{at:now,failed:r.status==="rejected"}));
    return {changed:replies.some(r=>r.status==="fulfilled"&&(r.value.added>0||r.value.updated>0)),failed:replies.some(r=>r.status==="rejected")};
  } finally {if(navSyncPromise===owned)navSyncPromise=null;}
}

// 挂载命名空间：bundle 模式下后续文件的唯一依赖入口
function decimalToMinor(value) {
  if(value==null)return null;
  const match=String(value).match(/^(-?)(\d+)(?:\.(\d{1,2}))?$/);
  if(!match)throw Error(t("fundHomeAmountInvalid"));
  return (match[1]? -1n:1n)*(BigInt(match[2])*100n+BigInt((match[3]||"").padEnd(2,"0")));
}
function formatMinorExact(value,grouping=true) {
  if(value==null)return "–";
  const n=BigInt(value),abs=n<0n?-n:n;
  return `${n<0n?"-":""}${(abs/100n).toLocaleString(state.language,{useGrouping:grouping})}.${String(abs%100n).padStart(2,"0")}`;
}
globalThis.Fund = { state, actions, api, authFetch, localizeApiError, themeColor, themeRgba, t, applyLanguage, applyI18nDom, registerMessages, ensureEcharts, decimalToMinor, formatMinorExact, syncHoldingNav };
applyI18nDom();

// ECharts 按需加载（05 §8 初始资源预算）：首页无图表，echarts.min.js 仅在
// 进入趋势分析/个股详情等图表视图前注入；失败可重试（清缓存后再次注入）。
function ensureEcharts() {
  if (typeof window.echarts !== "undefined") return Promise.resolve();
  if (!ensureEcharts._p) {
    ensureEcharts._p = new Promise((resolve, reject) => {
      const script = document.createElement("script");
      script.src = "/echarts.min.js";
      script.onload = () => resolve();
      script.onerror = () => {
        ensureEcharts._p = null;
        reject(new Error("图表库加载失败"));
      };
      document.head.appendChild(script);
    });
  }
  return ensureEcharts._p;
}
