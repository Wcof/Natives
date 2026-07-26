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

/** 单个 cron 字段：* 、*\/N 、或逗号分隔的 N / A-B 列表 */
const CRON_FIELD_RE = /^(\*(\/\d{1,3})?|\d{1,3}(-\d{1,3})?(,\d{1,3}(-\d{1,3})?)*)$/;

export function validateCronExpression(expr: string): boolean {
  const fields = expr.trim().split(/\s+/);
  if (fields.length !== 5) return false;
  return fields.every((f) => CRON_FIELD_RE.test(f));
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
