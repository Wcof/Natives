(() => {
// Fund 交易时段与刷新调度：职责一句话——判定 A 股开市状态，并按状态选择
// 轮询周期（开市秒级 / 休市 30 分钟级），对外提供手动立即刷新入口。
// import 方向：core（无状态依赖）；被 app.js 的轮询循环消费。

// ---------- 交易时段判定（本地时区） ----------
// A 股：周一~周五 09:15–11:30 / 13:00–15:05（含集合竞价与收盘定格余量）。
// 法定节假日暂不识别（无交易日历数据源）：节假日轮询照常触发，但行情
// 数据不变，watchlist 表的行情指纹守卫会跳过无变化的 DOM 重建。
function isMarketOpen(now) {
  const d = now || new Date();
  const day = d.getDay();
  if (day === 0 || day === 6) return false;
  const hm = d.getHours() * 100 + d.getMinutes();
  return (hm >= 915 && hm <= 1130) || (hm >= 1300 && hm <= 1505);
}

// ---------- 分级刷新周期 ----------
// 开市：3s（秒级，与 tickers/详情轮询共用一个节拍）；
// 休市：30min（分钟级兜底，覆盖盘后数据修正/节假日差异，页面保持静止）。
const OPEN_INTERVAL_MS = 3000;
const CLOSED_INTERVAL_MS = 30 * 60 * 1000;

function currentInterval() {
  return isMarketOpen() ? OPEN_INTERVAL_MS : CLOSED_INTERVAL_MS;
}

// ---------- 调度器 ----------
// 自调整 setTimeout 循环：每轮按 currentInterval() 重估周期，开市↔休市
// 切换时下一轮自动换挡。tick 回调收到 { open, manual, reason }，内部自行
// 决定是否跳过工作（休市且无手动/初始需求时可直接 return 保持页面静止）。
let _timer = null;
let _tickFn = null;
let _manualRequested = false;
let _stopped = true;

function tickOnce(reason) {
  if (!_tickFn) return;
  const open = isMarketOpen();
  const manual = _manualRequested;
  _manualRequested = false;
  try { _tickFn({ open, manual, reason: reason || null }); } catch (e) {}
}

function scheduleNext() {
  if (_stopped) return;
  _timer = setTimeout(function() {
    tickOnce(null);
    scheduleNext();
  }, currentInterval());
}

// 启动调度：立即执行一次初始 tick（reason="init"，用于页面首刷），之后按
// 当前周期循环。开市↔休市切换瞬间无需特殊处理：下一轮 tick 本来就在新
// 周期上执行，回调可对比上次 open 状态做换挡后首刷。
function startScheduler(tickFn) {
  stopScheduler();
  _tickFn = tickFn;
  _stopped = false;
  tickOnce("init");
  scheduleNext();
}

function stopScheduler() {
  _stopped = true;
  if (_timer) { clearTimeout(_timer); _timer = null; }
}

// ---------- 手动刷新 ----------
// 任意模块（刷新按钮/快捷键）调用后，1s 内强制执行一次完整 tick，不受
// 休市限制；之后回到当前状态的常规周期。调度未启动时仅置位标记。
function requestManualRefresh() {
  _manualRequested = true;
  if (_timer) {
    clearTimeout(_timer);
    _timer = setTimeout(function() {
      tickOnce("manual");
      scheduleNext();
    }, 1000);
  }
}

const marketSession = {
  isMarketOpen,
  currentInterval,
  OPEN_INTERVAL_MS,
  CLOSED_INTERVAL_MS,
  startScheduler,
  stopScheduler,
  requestManualRefresh,
};
globalThis.Fund.marketSession = marketSession;

})();
