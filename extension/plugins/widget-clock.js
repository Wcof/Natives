// One visible clock per document; active workspace ownership bounds subscribers.
const subscribers = new Map();
let timer = null, period = 0, suspended = false;

function stop() {
  if (timer !== null) clearInterval(timer);
  timer = null;
  period = 0;
}

function publish(now, force = false) {
  for (const [update, entry] of subscribers) {
    if (!force && now < entry.next) continue;
    entry.next = now + entry.period;
    try { update(); }
    catch (error) {
      if (globalThis.reportError) globalThis.reportError(error);
      else console.error(error);
    }
  }
}

function schedule() {
  const next = !suspended && !document.hidden && subscribers.size
    ? Math.min(...[...subscribers.values()].map(entry => entry.period)) : 0;
  if (next === period) return;
  stop();
  if (next) {
    period = next;
    timer = setInterval(() => publish(Date.now()), period);
  }
}

function visible() {
  if (!document.hidden && !suspended) publish(Date.now(), true);
  schedule();
}
function hide(event) {
  suspended = true;
  stop();
  if (!event.persisted) {
    subscribers.clear();
    detach();
  }
}
function show() { suspended = false; visible(); }
function detach() {
  document.removeEventListener('visibilitychange', visible);
  window.removeEventListener('pagehide', hide);
  window.removeEventListener('pageshow', show);
}

export function subscribeWidgetClock(update, milliseconds = 1000) {
  const cadence = Number.isFinite(milliseconds) ? Math.max(1000, milliseconds) : 1000;
  if (!subscribers.size) {
    suspended = false;
    document.addEventListener('visibilitychange', visible);
    window.addEventListener('pagehide', hide);
    window.addEventListener('pageshow', show);
  }
  subscribers.set(update, {period: cadence, next: Date.now() + cadence});
  schedule();
  return () => {
    subscribers.delete(update);
    if (!subscribers.size) { stop(); detach(); }
    else schedule();
  };
}

export function relativeTimeArgs(milliseconds) {
  const seconds = Math.abs(milliseconds) / 1000;
  if (seconds >= 86400) return [Math.round(milliseconds / 86400000), 'day'];
  if (seconds >= 3600) return [Math.round(milliseconds / 3600000), 'hour'];
  if (seconds >= 60) return [Math.round(milliseconds / 60000), 'minute'];
  return [Math.round(milliseconds / 1000), 'second'];
}
