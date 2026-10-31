/**
 * 空间卡片的纯数据函数：日期键、七日窗口、进度、每日稳定选择与时区时间。
 * 不依赖 DOM / Chrome API，可在 Node 测试里直接跑。
 */

/** 整数夹取；非法值回退。 */
export function clampInt(value, min, max, fallback = min) {
  const number = Math.floor(Number(value));
  if (!Number.isFinite(number)) return fallback;
  return Math.max(min, Math.min(max, number));
}

/** 本地日期键 YYYY-MM-DD。 */
export function dayKey(date = new Date()) {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, '0');
  const day = String(date.getDate()).padStart(2, '0');
  return `${year}-${month}-${day}`;
}

/** 把 YYYY-MM-DD 解析为本地中午，避免夏令时把日期算偏。 */
export function parseDayKey(key) {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(String(key || ''));
  if (!match) return null;
  const date = new Date(Number(match[1]), Number(match[2]) - 1, Number(match[3]), 12);
  return Number.isNaN(date.getTime()) ? null : date;
}

/** to - from 的整天数；非法输入返回 null。 */
export function daysBetween(fromKey, toKey) {
  const from = parseDayKey(fromKey);
  const to = parseDayKey(toKey);
  if (!from || !to) return null;
  return Math.round((to.getTime() - from.getTime()) / 86400000);
}

/** 含今天的最近 n 天，升序。 */
export function lastNDays(count, date = new Date()) {
  const total = clampInt(count, 1, 366, 7);
  const days = [];
  for (let offset = total - 1; offset >= 0; offset -= 1) {
    const cursor = new Date(date.getFullYear(), date.getMonth(), date.getDate() - offset, 12);
    days.push(dayKey(cursor));
  }
  return days;
}

/** 今天、本周（周一起）、本月、今年的流逝比例，范围 0–1。 */
export function timeProgress(date = new Date()) {
  const now = date.getTime();
  const year = date.getFullYear();
  const month = date.getMonth();
  const day = date.getDate();
  const fraction = (start, end) => {
    const span = end - start;
    if (span <= 0) return 0;
    return Math.max(0, Math.min(1, (now - start) / span));
  };
  const dayStart = new Date(year, month, day).getTime();
  const dayEnd = new Date(year, month, day + 1).getTime();
  const mondayOffset = (date.getDay() + 6) % 7;
  const weekStart = new Date(year, month, day - mondayOffset).getTime();
  const weekEnd = new Date(year, month, day - mondayOffset + 7).getTime();
  return {
    day: fraction(dayStart, dayEnd),
    week: fraction(weekStart, weekEnd),
    month: fraction(new Date(year, month, 1).getTime(), new Date(year, month + 1, 1).getTime()),
    year: fraction(new Date(year, 0, 1).getTime(), new Date(year + 1, 0, 1).getTime()),
  };
}

/**
 * 稳定的「每日一选」：同一天所有窗口得到同一个下标。
 * FNV-1a，纯函数，不依赖随机数。
 */
export function dailyIndex(day, length, offset = 0) {
  const size = Math.floor(Number(length));
  if (!Number.isFinite(size) || size <= 0) return -1;
  let hash = 2166136261;
  for (const char of String(day)) {
    hash ^= char.charCodeAt(0);
    hash = Math.imul(hash, 16777619);
  }
  return (((hash >>> 0) + Math.floor(Number(offset) || 0)) % size + size) % size;
}

/** 指定时区下的 HH:mm 与「相对本地日期」的偏移天数。 */
export function zonedClock(date, timeZone) {
  const timeFormatter = new Intl.DateTimeFormat('en-GB', {
    timeZone,
    hour: '2-digit',
    minute: '2-digit',
    hour12: false,
  });
  let time = timeFormatter.format(date);
  if (time === '24:00') time = '00:00';
  const dateFormatter = new Intl.DateTimeFormat('en-CA', {
    timeZone,
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
  });
  const parts = Object.fromEntries(
    dateFormatter.formatToParts(date).filter((part) => part.type !== 'literal').map((part) => [part.type, part.value]),
  );
  const zonedDay = `${parts.year}-${parts.month}-${parts.day}`;
  return { time, offset: daysBetween(dayKey(date), zonedDay) ?? 0 };
}

/** 快速记录的时间戳：M/D HH:mm。 */
export function formatStamp(timestamp, date = new Date(timestamp)) {
  const month = date.getMonth() + 1;
  const day = date.getDate();
  const hour = String(date.getHours()).padStart(2, '0');
  const minute = String(date.getMinutes()).padStart(2, '0');
  return `${month}/${day} ${hour}:${minute}`;
}

/**
 * 保留最近 keepDays 天的日键映射；用于习惯记录等按日累积的数据，防止 JSON 无限增长。
 */
export function pruneDayMap(map, keepDays, today = dayKey()) {
  const source = map && typeof map === 'object' && !Array.isArray(map) ? map : {};
  const days = lastNDays(keepDays);
  const cutoff = days[0] || today;
  const result = {};
  for (const [key, value] of Object.entries(source)) {
    if (/^\d{4}-\d{2}-\d{2}$/.test(key) && key >= cutoff) result[key] = value;
  }
  return result;
}

/** 截断文本到 max 个字符，避免单卡 JSON 超过 Host 的 64KB 上限。 */
export function capText(value, max) {
  const text = String(value ?? '');
  return text.length > max ? text.slice(0, max) : text;
}

/** 生成稳定的本地 id；crypto 不可用时退回时间戳 + 随机串。 */
export function createId(prefix = 'id') {
  try {
    if (globalThis.crypto?.randomUUID) return globalThis.crypto.randomUUID();
  } catch {
    /* 继续走回退 */
  }
  return `${prefix}_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 8)}`;
}
