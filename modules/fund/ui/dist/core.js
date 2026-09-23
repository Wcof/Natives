// Fund 模块共享基座：全局状态唯一归属 + API 基座 + 主题取色 + 跨域 action 注册表。
// 领域模块（market-table/chart/dashboard/ledger/app）从这里取共享状态，
// 并把自己的公开动作注册到 actions，跨域调用一律经 actions，不直接 import 对方。

// 握手令牌（app.js 的握手流程写入，api()/authFetch() 读取）
let _tokenVal = null;
let tokenResolver = null;
const tokenPromise = new Promise(resolve => { tokenResolver = resolve; });

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
  await ensureToken();
  const headers = { "Content-Type": "application/json" };
  if (_tokenVal) headers["Authorization"] = "Bearer " + _tokenVal;
  const r = await fetch(url, {
    method: method,
    headers: headers,
    body: body ? JSON.stringify(body) : undefined
  });
  if (!r.ok) {
    const j = await r.json().catch(() => ({}));
    throw new Error(j.message || j.error || r.statusText);
  }
  return r.json();
}

// 带鉴权头的 fetch 请求，避免出现 APP_SESSION_INVALID
async function authFetch(url, options = {}) {
  await ensureToken();
  const headers = Object.assign({}, options.headers);
  if (_tokenVal && !headers["Authorization"] && !headers["authorization"]) {
    headers["Authorization"] = "Bearer " + _tokenVal;
  }
  return fetch(url, Object.assign({}, options, { headers }));
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
