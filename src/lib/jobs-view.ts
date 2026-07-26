/**
 * jobs-view — 任务模块纯展示/校验逻辑（无 IPC、无副作用，可单测）
 *
 * 契约依据（任务模块接口契约冻结稿 v1 第 1 节 schedule 语义）：
 * - once: ISO8601 时刻
 * - interval: 秒数（最小 60）
 * - cron: 5 字段表达式（分 时 日 月 周，支持 * N *\/N A-B A,B,C）
 *
 * 前端校验只做「格式级」把关，存在性等以后端错误码为准
 * （JOB_INVALID_SCHEDULE / JOB_INVALID_PROJECT_PATH）。
 */

import type { JobRunStatus, JobScheduleType } from './jobs-api';

// ── 表单校验 ──

export type JobFormField = 'name' | 'prompt' | 'project_path' | 'schedule_value';

/** 值为 jobs.form.* 下的错误文案子键（errRequired / errInvalidCron …） */
export type JobFormErrors = Partial<Record<JobFormField, string>>;

export const JOB_INTERVAL_MIN_SECONDS = 60;

/** 五字段值域：分 0-59、时 0-23、日 1-31、月 1-12、周 0-7（7=周日，与 vixie cron 一致） */
const CRON_FIELD_BOUNDS: ReadonlyArray<readonly [number, number]> = [
  [0, 59],
  [0, 23],
  [1, 31],
  [1, 12],
  [0, 7],
];

/** 单个 cron 字段：* 、*\/N（N≥1）、或逗号分隔的 N / A-B（A≤B）列表，均须在值域内 */
function validateCronField(field: string, min: number, max: number): boolean {
  if (field === '*') return true;
  const step = /^\*\/(\d{1,3})$/.exec(field);
  if (step) {
    const n = Number(step[1]);
    return n >= 1 && n <= max;
  }
  return field.split(',').every((part) => {
    const m = /^(\d{1,3})(?:-(\d{1,3}))?$/.exec(part);
    if (!m) return false;
    const a = Number(m[1]);
    if (a < min || a > max) return false;
    if (m[2] === undefined) return true;
    const b = Number(m[2]);
    return b >= min && b <= max && a <= b;
  });
}

export function validateCronExpression(expr: string): boolean {
  const fields = expr.trim().split(/\s+/);
  if (fields.length !== 5) return false;
  return fields.every((f, i) => {
    const bounds = CRON_FIELD_BOUNDS[i]!;
    return validateCronField(f, bounds[0], bounds[1]);
  });
}

// ── cron 触发时刻预览（本地时区；供表单「下 N 次运行」提示） ──

/** 把单字段展开为允许值集合；weekday 的 7 归一为 0 */
function cronFieldSet(field: string, min: number, max: number, normalizeSeven: boolean): Set<number> {
  const out = new Set<number>();
  const add = (n: number) => out.add(normalizeSeven && n === 7 ? 0 : n);
  if (field === '*') {
    for (let n = min; n <= max; n++) add(n);
    return out;
  }
  const step = /^\*\/(\d{1,3})$/.exec(field);
  if (step) {
    const s = Number(step[1]);
    for (let n = min; n <= max; n += s) add(n);
    return out;
  }
  for (const part of field.split(',')) {
    const m = /^(\d{1,3})(?:-(\d{1,3}))?$/.exec(part);
    if (!m) continue;
    const a = Number(m[1]);
    const b = m[2] === undefined ? a : Number(m[2]);
    for (let n = a; n <= b; n++) add(n);
  }
  return out;
}

/**
 * 计算 cron 表达式在本地时区的下 count 次触发时刻。
 * 语义与 vixie cron 一致：日/周字段都受限时取「或」，否则取「且」。
 * 表达式无效或一年内无匹配时返回空数组（纯函数，便于单测）。
 */
export function nextCronRuns(expr: string, from: Date, count = 3): Date[] {
  if (!validateCronExpression(expr)) return [];
  const [minF, hourF, domF, monF, dowF] = expr.trim().split(/\s+/) as [string, string, string, string, string];
  const minutes = cronFieldSet(minF, 0, 59, false);
  const hours = cronFieldSet(hourF, 0, 23, false);
  const doms = cronFieldSet(domF, 1, 31, false);
  const months = cronFieldSet(monF, 1, 12, false);
  const dows = cronFieldSet(dowF, 0, 7, true);
  const domRestricted = domF !== '*';
  const dowRestricted = dowF !== '*';

  const results: Date[] = [];
  const cursor = new Date(from.getTime());
  cursor.setSeconds(0, 0);
  cursor.setMinutes(cursor.getMinutes() + 1);
  const limit = from.getTime() + 366 * 24 * 60 * 60 * 1000;
  while (results.length < count && cursor.getTime() <= limit) {
    if (!months.has(cursor.getMonth() + 1)) {
      // 跳到下月 1 日 00:00，避免整月逐分钟扫描
      cursor.setMonth(cursor.getMonth() + 1, 1);
      cursor.setHours(0, 0, 0, 0);
      continue;
    }
    const domMatch = doms.has(cursor.getDate());
    const dowMatch = dows.has(cursor.getDay());
    const dayMatch =
      domRestricted && dowRestricted ? domMatch || dowMatch : domMatch && dowMatch;
    if (!dayMatch) {
      cursor.setDate(cursor.getDate() + 1);
      cursor.setHours(0, 0, 0, 0);
      continue;
    }
    if (!hours.has(cursor.getHours())) {
      cursor.setHours(cursor.getHours() + 1, 0, 0, 0);
      continue;
    }
    if (!minutes.has(cursor.getMinutes())) {
      cursor.setMinutes(cursor.getMinutes() + 1, 0, 0);
      continue;
    }
    results.push(new Date(cursor.getTime()));
    cursor.setMinutes(cursor.getMinutes() + 1, 0, 0);
  }
  return results;
}

export function validateScheduleValue(
  type: JobScheduleType,
  value: string,
): string | null {
  const v = value.trim();
  if (!v) return 'errRequired';
  switch (type) {
    case 'once': {
      const ms = Date.parse(v);
      return Number.isFinite(ms) ? null : 'errInvalidOnce';
    }
    case 'interval': {
      if (!/^\d+$/.test(v)) return 'errInvalidInterval';
      return Number(v) >= JOB_INTERVAL_MIN_SECONDS ? null : 'errIntervalMin';
    }
    case 'cron':
      return validateCronExpression(v) ? null : 'errInvalidCron';
  }
}

export function validateJobForm(fields: {
  name: string;
  prompt: string;
  project_path: string;
  schedule_type: JobScheduleType;
  schedule_value: string;
}): JobFormErrors {
  const errors: JobFormErrors = {};
  if (!fields.name.trim()) errors.name = 'errRequired';
  if (!fields.prompt.trim()) errors.prompt = 'errRequired';
  const path = fields.project_path.trim();
  if (!path) errors.project_path = 'errRequired';
  else if (!isAbsolutePath(path)) errors.project_path = 'errNotAbsolute';
  const scheduleError = validateScheduleValue(fields.schedule_type, fields.schedule_value);
  if (scheduleError) errors.schedule_value = scheduleError;
  return errors;
}

/** POSIX 绝对路径或 Windows 盘符路径（存在性由后端校验） */
export function isAbsolutePath(path: string): boolean {
  return path.startsWith('/') || /^[A-Za-z]:[\\/]/.test(path);
}

// ── last_status 解析（scheduled_tasks.last_status，第 1 节） ──

export interface ParsedJobStatus {
  /** 徽标 i18n 子键（jobs.jobStatus.*） */
  key:
    | 'never'
    | 'succeeded'
    | 'failed'
    | 'expired'
    | 'skipped'
    | 'dispatchErrorNotWired'
    | 'dispatchError'
    | 'raw';
  tone: 'success' | 'danger' | 'warning' | 'neutral';
  /** 冒号后的错误码（如 failed:<code> 的 code）；raw 时为原始字符串 */
  detail?: string;
}

export function parseJobLastStatus(lastStatus: string | null | undefined): ParsedJobStatus {
  if (!lastStatus) return { key: 'never', tone: 'neutral' };
  const sep = lastStatus.indexOf(':');
  const head = sep === -1 ? lastStatus : lastStatus.slice(0, sep);
  const rest = sep === -1 ? undefined : lastStatus.slice(sep + 1);
  switch (head) {
    case 'succeeded':
      return { key: 'succeeded', tone: 'success' };
    case 'failed':
      return { key: 'failed', tone: 'danger', detail: rest };
    case 'expired':
      return { key: 'expired', tone: 'neutral' };
    case 'skipped':
      return { key: 'skipped', tone: 'warning' };
    case 'dispatch_error':
      return rest === 'not_wired'
        ? { key: 'dispatchErrorNotWired', tone: 'warning' }
        : { key: 'dispatchError', tone: 'danger', detail: rest };
    default:
      // 未知状态如实展示原始字符串（三态诚实，不猜测归类）
      return { key: 'raw', tone: 'neutral', detail: lastStatus };
  }
}

// ── run 状态徽标色调（状态机全集，第 2 节） ──

export function runStatusTone(
  status: JobRunStatus | string,
): 'success' | 'danger' | 'warning' | 'neutral' | 'info' {
  switch (status) {
    case 'succeeded':
      return 'success';
    case 'failed':
    case 'dispatch_error':
      return 'danger';
    case 'skipped':
    case 'cancelled':
      return 'warning';
    case 'running':
    case 'dispatched':
      return 'info';
    case 'pending':
    default:
      return 'neutral';
  }
}

// ── schedule 摘要参数（组件负责用 t() 渲染） ──

export type ScheduleSummarySpec =
  | { kind: 'once'; timeMs: number | null; raw: string }
  | { kind: 'interval'; seconds: number | null; raw: string }
  | { kind: 'cron'; expr: string };

export function scheduleSummarySpec(
  type: JobScheduleType | string,
  value: string,
): ScheduleSummarySpec {
  if (type === 'once') {
    const ms = Date.parse(value);
    return { kind: 'once', timeMs: Number.isFinite(ms) ? ms : null, raw: value };
  }
  if (type === 'interval') {
    const n = /^\d+$/.test(value.trim()) ? Number(value.trim()) : null;
    return { kind: 'interval', seconds: n, raw: value };
  }
  return { kind: 'cron', expr: value };
}

/** 本地时区的 GMT 偏移标注（如 JST → "GMT+9"），用于时间展示的时区提示 */
export function gmtOffsetLabel(): string {
  const minutes = -new Date().getTimezoneOffset();
  const sign = minutes >= 0 ? '+' : '-';
  const abs = Math.abs(minutes);
  const h = Math.floor(abs / 60);
  const m = abs % 60;
  return `GMT${sign}${h}${m ? `:${String(m).padStart(2, '0')}` : ''}`;
}

// ── datetime-local 输入与 ISO8601 互转（once 型 schedule_value） ──

/** ISO8601（UTC）→ <input type="datetime-local"> 本地值；无效返回空串 */
export function isoToLocalInput(iso: string): string {
  const ms = Date.parse(iso);
  if (!Number.isFinite(ms)) return '';
  const d = new Date(ms);
  const p = (x: number) => String(x).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}T${p(d.getHours())}:${p(d.getMinutes())}`;
}

/** <input type="datetime-local"> 本地值 → ISO8601（UTC）；无效返回 null */
export function localInputToIso(local: string): string | null {
  if (!local) return null;
  const ms = Date.parse(local);
  if (!Number.isFinite(ms)) return null;
  return new Date(ms).toISOString();
}
