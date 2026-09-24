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
  activeGroup: "default",
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
  ws: null,
  wsConnected: false,
};

// 跨域动作注册表：领域模块在文件底部注册，调用方经 actions.xxx() 访问。
const actions = {};

// 等待握手 Token 到达，杜绝时序竞态引起的 APP_SESSION_INVALID
async function ensureToken() {
  if (_tokenVal) return _tokenVal;
  const timeout = new Promise(resolve => setTimeout(() => resolve(null), 2500));
  return Promise.race([tokenPromise, timeout]);
}

// API 与通信基座
async function api(method, url, body) {
  const headers = { "Content-Type": "application/json" };
  const send = () => {
    if (_tokenVal) headers["Authorization"] = "Bearer " + _tokenVal;
    return fetch(url, {
      method: method,
      headers: headers,
      body: body ? JSON.stringify(body) : undefined
    });
  };
  let r = await send();
  // token 过期（APP_SESSION_INVALID）时重新握手换新 token 并重试一次。
  if (r.status === 401 && await rehandshake()) r = await send();
  if (!r.ok) {
    const j = await r.json().catch(() => ({}));
    throw new Error(j.message || j.error || r.statusText);
  }
  return r.json();
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
function themeColor(name) {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim() || "#888888";
}

function themeRgba(name, alpha) {
  const c = themeColor(name);
  if (c.charAt(0) !== "#" || c.length < 7) return "rgba(136,136,136," + alpha + ")";
  const n = parseInt(c.slice(1), 16);
  return "rgba(" + ((n >> 16) & 255) + "," + ((n >> 8) & 255) + "," + (n & 255) + "," + alpha + ")";
}

// 挂载命名空间：bundle 模式下后续文件的唯一依赖入口
globalThis.Fund = { state, actions, api, authFetch, themeColor, themeRgba };
