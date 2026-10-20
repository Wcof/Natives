// 空间投资卡页面级数据源（ADR-0036 / 方案 02 §2）。
//
// 硬约束：
// - 当前页全部投资卡共享一次短命只读投影查询（一个 Port、一次 app:card_snapshot）；
//   卡片数量不增加查询、Port 或进程（R-P4）。
// - 查询只在页面可见、存在投资卡、本页没有正在释放的 Fund 嵌入会话时发起；
//   无后台轮询、无自动重试；一次手动刷新对应一次查询尝试。
// - readiness 非 ready（或缺失）时绝不 app:start，卡片进入引导态。
// - APP_RUNNING_ELSEWHERE 是诚实不可用：不广播停机、不抢占其他标签页会话。
// - 迟到响应按 query generation 丢弃；pagehide/隐藏/卡片移除/嵌入详情打开
//   都取消本次查询并断开 Port。
// - 一次查询只有一个终态（成功/失败/取消）：到达终态后再迟到的响应不得
//   把「已不可用」翻回「已更新」（15 秒预算超时后迟到的成功响应同样丢弃）。
// - 取消竞态（方案 03 P0 回归）：每次查询拥有自己的 client 与预算定时器；
//   被取代/取消的旧查询迟到回调只回收自有资源，绝不清算新查询的 Port 或
//   定时器，也不改写新查询的状态。
// - 移除最后一张投资卡时终止进行中的查询并退出 loading，不留假加载态。
// - 金额遮蔽是页面级内存态，不写 Chrome storage；刷新后默认隐藏。

import { createAppNativeClient } from './native-app-client.js';

export const INVESTMENT_WIDGET_KEYS = [
  'widget/investmentHoldings',
  'widget/investmentAssets',
  'widget/investmentGoal',
  'widget/investmentAllocation',
  'widget/investmentAllocationCheck',
];

const FUND_APP_ID = 'fund';
const QUERY_BUDGET_MS = 15_000;

const listeners = new Set();
let initialized = false;
let coreCall = null; // (method, params) => Promise<result>；复用页面共享 Files Host client

let status = 'idle'; // idle | loading | ready | unavailable | unsupported
let errorCode = null; // 稳定错误码（R-S10：不携带堆栈/路径/SQL/金额）
let readiness = null; // 最近一次 data_status 的 readiness
let snapshot = null; // fund.cards.v1 payload（本页内存）
let snapshotGeneratedAt = null;
let fetchedAt = 0; // 最近一次成功读取的本地时间戳
let moneyVisible = false;

let queryGeneration = 0; // 迟到响应丢弃
let terminalGeneration = 0; // 已到达终态的查询：其后续响应一律丢弃
let inFlightGen = 0; // 拥有「进行中」语义的 generation；0 = 无查询在飞行
let hasCards = false; // 当前 Workspace 是否存在投资卡
let suspended = true; // 页面隐藏/嵌入详情打开/初始化前为 true
let navigationHandler = null; // (view) => void；space.js 注入同页嵌入入口

// 当前 inFlight 查询的句柄（gen + abort），供 suspend/移除卡片同步取消。
let activeQuery = null;
// 仍存活（未 done）的查询上下文：仅供 reset 兜底回收自有资源。
const liveQueries = new Set();

function notify() {
  const state = getInvestmentCardState();
  for (const listener of listeners) {
    try {
      listener(state);
    } catch {
      /* 单个卡片渲染失败不拖垮其他订阅者 */
    }
  }
}

function setStatus(nextStatus, nextError = null) {
  const changed = status !== nextStatus || errorCode !== nextError;
  status = nextStatus;
  errorCode = nextError;
  if (changed) notify();
}

function classifyFailure(error) {
  const code = String(error?.code || '');
  if (/APP_RUNNING_ELSEWHERE|APP_ALREADY_RUNNING/.test(code)) return 'APP_RUNNING_ELSEWHERE';
  if (code === 'request_timeout') return 'APP_CARD_TIMEOUT';
  if (code === 'host_disconnected') return 'APP_HOST_DISCONNECTED';
  if (/APP_CARD_SNAPSHOT_UNSUPPORTED/.test(String(error?.message || ''))) {
    return 'CARD_SNAPSHOT_UNSUPPORTED';
  }
  return 'APP_CARD_QUERY_FAILED';
}

// 一次投影查询：apps:get 校验 → handshake → data_status(readiness) → start
// → card_snapshot → 立即断开。整个流程共享一个 15 秒总 deadline。
//
// 资源所有权：client 与预算定时器都是本次查询的闭包私有资源；任何终态
// （成功/失败/取消/被取代）只 release 自有资源。被取代的旧查询即使迟到
// 继续执行，也绝不清算新查询的 Port、定时器或状态。
async function runQuery(gen) {
  inFlightGen = gen;
  setStatus('loading', null);
  let timer = null;
  let client = null;
  let done = false;
  const release = () => {
    if (timer) {
      clearTimeout(timer);
      timer = null;
    }
    if (client) {
      const owned = client;
      client = null;
      try {
        owned.disconnect();
      } catch {
        /* 已断开的 Port 忽略 */
      }
    }
  };
  const query = {
    gen,
    abort() {
      // 被取消/被取代：只回收自有资源；inFlight 归还；若仍是最新代则
      // 标记终态使迟到响应被丢弃。不改写页面状态（取消方负责）。
      if (done) return;
      done = true;
      liveQueries.delete(query);
      if (activeQuery === query) activeQuery = null;
      release();
      if (inFlightGen === gen) inFlightGen = 0;
      if (gen === queryGeneration) terminalGeneration = gen;
    },
  };
  liveQueries.add(query);
  const finish = (nextStatus, code) => {
    if (done) return;
    done = true;
    liveQueries.delete(query);
    if (activeQuery === query) activeQuery = null;
    release();
    if (inFlightGen === gen) inFlightGen = 0;
    // 已被更新的查询取代：只回收资源，不改状态、不标记终态。
    if (gen !== queryGeneration) return;
    terminalGeneration = gen;
    setStatus(nextStatus, code);
  };
  const stale = () => done || terminalGeneration === gen || gen !== queryGeneration || suspended;
  activeQuery = query;

  timer = setTimeout(() => {
    finish('unavailable', 'APP_CARD_TIMEOUT');
  }, QUERY_BUDGET_MS);

  try {
    const detail = await coreCall('apps:get', { appId: FUND_APP_ID });
    if (stale()) return void query.abort();
    const app = detail?.app;
    if (!app || !app.enabled) return void finish('unavailable', 'APP_DISABLED');
    if (typeof detail.runtime_host !== 'string' || !detail.runtime_host) {
      return void finish('unavailable', 'APP_START_FAILED');
    }
    client = createAppNativeClient({
      runtimeHost: detail.runtime_host,
      onDisconnect: (_error, intentional) => {
        // 主动断开（release/abort）或查询已终态/被取代：不影响页面状态。
        if (!intentional && !done && gen === queryGeneration) {
          finish('unavailable', 'APP_HOST_DISCONNECTED');
        }
      },
    });
    const handshake = await client.call('app:handshake', {
      protocolVersion: 2,
      appId: FUND_APP_ID,
      productVersion: app.version,
      activationGeneration: app.product_generation ?? app.productGeneration,
    });
    if (stale()) return void query.abort();
    if ((handshake?.appId || handshake?.app_id) !== FUND_APP_ID) return void finish('unavailable', 'APP_PROTOCOL_MISMATCH');

    const dataStatus = await client.call('app:data_status', { appId: FUND_APP_ID });
    if (stale()) return void query.abort();
    readiness = typeof dataStatus?.readiness === 'string' ? dataStatus.readiness : null;
    if (!readiness) {
      // 模块不提供 readiness（旧版本/非内置实现）：这里是「不支持」而非「暂时失败」。
      return void finish('unsupported', 'CARD_SNAPSHOT_UNSUPPORTED');
    }
    if (readiness !== 'ready') {
      // 初始化／迁移／恢复由用户进入投资详情触发，卡片不 app:start。
      return void finish('unavailable', 'APP_DATA_NOT_READY');
    }

    const started = await client.call('app:start', { appId: FUND_APP_ID });
    if (stale()) return void query.abort();
    if (started?.state !== 'ready' || typeof started?.instanceId !== 'string') {
      return void finish('unavailable', 'APP_START_FAILED');
    }
    const snap = await client.call('app:card_snapshot', {
      instanceId: started.instanceId,
      generation: started.generation,
    });
    if (stale()) return void query.abort();
    if (!snap || snap.schema !== 'fund.cards.v1' || typeof snap.payload !== 'object') {
      return void finish('unavailable', 'APP_CARD_QUERY_FAILED');
    }
    snapshot = snap.payload || null;
    snapshotGeneratedAt = snap.generatedAt || null;
    fetchedAt = Date.now();
    finish('ready', null);
  } catch (error) {
    if (stale()) return void query.abort();
    finish('unavailable', classifyFailure(error));
  }
}

function shouldQuery() {
  return initialized && !suspended && inFlightGen === 0 && hasCards
    && typeof document !== 'undefined'
    && document.visibilityState === 'visible';
}

function maybeQuery() {
  if (!shouldQuery()) return;
  const gen = ++queryGeneration;
  void runQuery(gen);
}

function getInvestmentCardState() {
  return {
    status,
    errorCode,
    readiness,
    snapshot,
    generatedAt: snapshotGeneratedAt,
    fetchedAt,
    moneyVisible,
    loading: status === 'loading',
  };
}

export function initInvestmentFeed({ coreCall: core } = {}) {
  if (typeof core !== 'function') throw new Error('initInvestmentFeed: coreCall is required');
  coreCall = core;
  initialized = true;
  suspended =
    typeof document === 'undefined' || document.visibilityState !== 'visible';
}

// 仅测试使用：完整重置模块级状态（feed 是页面级单例）。
export function resetInvestmentFeedForTests() {
  for (const query of [...liveQueries]) query.abort();
  liveQueries.clear();
  activeQuery = null;
  initialized = false;
  coreCall = null;
  status = 'idle';
  errorCode = null;
  readiness = null;
  snapshot = null;
  snapshotGeneratedAt = null;
  fetchedAt = 0;
  moneyVisible = false;
  queryGeneration = 0;
  terminalGeneration = 0;
  inFlightGen = 0;
  hasCards = false;
  suspended = true;
}

export function setInvestmentNavigationHandler(handler) {
  navigationHandler = typeof handler === 'function' ? handler : null;
}

export function openInvestmentDetail(view) {
  const target = typeof view === 'string' && /^[a-z][a-z0-9-]{0,31}$/.test(view) ? view : '';
  if (!target) return;
  try {
    navigationHandler?.(target);
  } catch {
    /* 导航失败静默：卡片入口不阻塞页面 */
  }
}

export function setInvestmentCardsPresent(present) {
  const next = Boolean(present);
  if (hasCards === next) return;
  hasCards = next;
  if (next) {
    maybeQuery();
    return;
  }
  // 移除最后一张投资卡：终止进行中的查询并退出 loading——页面已没有
  // 等待数据的卡片，不得停留「正在读取」；迟到响应按终态丢弃。
  if (inFlightGen !== 0) {
    const gen = inFlightGen;
    activeQuery?.abort();
    inFlightGen = 0;
    if (gen === queryGeneration) terminalGeneration = gen;
    setStatus('idle', null);
  }
}

export function suspendInvestmentFeed() {
  suspended = true;
  if (inFlightGen !== 0) {
    const gen = inFlightGen;
    ++queryGeneration; // 迟到响应全部丢弃
    activeQuery?.abort(); // 只回收该查询自有资源
    inFlightGen = 0;
    // 被打断的查询回到等待态：保留上次本页快照，但不得继续显示「正在读取」。
    // SUSPENDED 是主动让出（隐藏/打开嵌入详情），不是查询失败。
    setStatus('unavailable', 'SUSPENDED');
  }
}

export function resumeInvestmentFeed() {
  if (!initialized) return;
  suspended =
    typeof document === 'undefined' || document.visibilityState !== 'visible';
  if (!suspended) maybeQuery();
}

export function refreshInvestmentCards() {
  // 用户主动刷新：进行中的请求保持（合并为一次进行中的查询），否则重查一次。
  if (inFlightGen !== 0) return;
  maybeQuery();
}

export function invalidateInvestmentCards() {
  if (inFlightGen !== 0) {
    ++queryGeneration;
    activeQuery?.abort();
    inFlightGen = 0;
  }
  maybeQuery();
}

export function isMoneyVisible() {
  return moneyVisible;
}

export function setMoneyVisible(visible) {
  const next = Boolean(visible);
  if (moneyVisible === next) return;
  moneyVisible = next; // 仅本页内存；刷新后默认隐藏（方案 04 §7）
  notify();
}

export function subscribeInvestmentCards(listener) {
  listeners.add(listener);
  listener(getInvestmentCardState());
  return () => listeners.delete(listener);
}

// 诊断用快照（无业务金额）：错误码与状态可定位，不含栈/路径/SQL。
export function investmentFeedDiagnostics() {
  return { status, errorCode, readiness, inFlight: inFlightGen !== 0, queryGeneration, terminalGeneration };
}
