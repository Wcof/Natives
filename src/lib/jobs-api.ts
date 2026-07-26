'use client';

/**
 * jobs-api — 任务模块（Job Module）访问 nativesAPI 的唯一入口
 *
 * 依据「任务模块接口契约冻结稿 v1」第 1、2、5 节：
 * - 8 个 Tauri 命令：job_list / job_get / job_create / job_update /
 *   job_delete / job_set_enabled / job_run_now / job_runs_list
 * - JSON 字段 snake_case，与后端一致
 * - 错误码字符串：JOB_NOT_FOUND / JOB_INVALID_SCHEDULE /
 *   JOB_INVALID_PROJECT_PATH / JOB_DISPATCHER_NOT_WIRED
 *
 * 规则（与 files-api 相同的收敛姿态）：任务域组件必须 import 本模块，
 * 禁止直接触碰 `window.nativesAPI`。非 Tauri 环境（浏览器 dev）取用会抛
 * `JobsApiUnavailableError`，调用方要么先探测、要么捕获降级。
 */

import type { NativesAPI } from '@/lib/tauri-adapter';

// ── 契约类型（第 1 节 数据模型） ──

export type JobScheduleType = 'once' | 'interval' | 'cron';
export type JobPermissionProfile = 'readonly' | 'ask' | 'full_access';
export type JobTrigger = 'schedule' | 'manual';

/** task_runs 状态机（第 2 节冻结）：主线 + 旁路终态 */
export const JOB_RUN_STATUSES = [
  'pending',
  'dispatched',
  'running',
  'succeeded',
  'failed',
  'cancelled',
  'skipped',
  'dispatch_error',
] as const;
export type JobRunStatus = (typeof JOB_RUN_STATUSES)[number];

export interface JobSummary {
  id: string;
  name: string;
  description?: string | null;
  schedule_type: JobScheduleType;
  schedule_value: string;
  project_path?: string | null;
  next_run?: string | null;
  enabled: boolean;
  last_status?: string | null;
  last_run_at?: string | null;
  consecutive_errors?: number;
  expires_at?: string | null;
  created_at?: string;
}

export interface JobRun {
  id: string;
  task_id?: string | null;
  job_id?: string | null;
  status: JobRunStatus | string;
  trigger?: JobTrigger | string | null;
  run_id?: string | null;
  conversation_id?: string | null;
  started_at?: string | null;
  finished_at?: string | null;
  error_code?: string | null;
  detail?: string | null;
  result_summary?: string | null;
  error?: string | null;
}

export interface JobDetail extends JobSummary {
  prompt: string;
  provider_id?: string | null;
  model_id?: string | null;
  key_id?: string | null;
  agent_profile_id?: string | null;
  /** JSON 数组（后端存 TEXT，返回可能已解析或仍为字符串） */
  capability_refs?: string[] | string | null;
  permission_profile?: JobPermissionProfile | string | null;
  max_steps?: number | null;
  effort?: string | null;
  runtime_id?: string | null;
  /** 最近 run 摘要（契约第 5 节 job_get） */
  recent_runs?: JobRun[];
}

/** job_create 必填：name / schedule_type / schedule_value / project_path / prompt */
export interface JobPayload {
  name: string;
  schedule_type: JobScheduleType;
  schedule_value: string;
  project_path: string;
  prompt: string;
  description?: string;
  provider_id?: string;
  model_id?: string;
  key_id?: string;
  agent_profile_id?: string;
  capability_refs?: string[];
  permission_profile?: JobPermissionProfile;
  max_steps?: number;
  effort?: string;
  runtime_id?: string;
  expires_at?: string;
}

export interface JobListResult {
  jobs: JobSummary[];
  dispatcher_wired: boolean;
}

export interface JobRunsResult {
  runs: JobRun[];
  total: number;
}

/** job_run_now 成功返回（契约第 3 节 DispatchReceipt） */
export interface JobDispatchReceipt {
  conversation_id: string;
  run_id: string;
  idempotency_key: string;
}

// ── 错误码（第 5 节冻结） ──

export const JOB_ERROR_CODES = [
  'JOB_NOT_FOUND',
  'JOB_INVALID_SCHEDULE',
  'JOB_INVALID_PROJECT_PATH',
  'JOB_DISPATCHER_NOT_WIRED',
] as const;
export type JobErrorCode = (typeof JOB_ERROR_CODES)[number];

/**
 * 后端 Error 序列化为字符串（error.rs Serialize 实现），错误码以子串形式
 * 出现在消息里；tauri-adapter 的 cmd() 又包了一层前缀。此处按子串提取。
 */
export function extractJobErrorCode(err: unknown): JobErrorCode | null {
  let msg = '';
  if (err instanceof Error) msg = err.message;
  else if (typeof err === 'string') msg = err;
  else if (err != null) {
    try {
      msg = JSON.stringify(err);
    } catch {
      msg = String(err);
    }
  }
  for (const code of JOB_ERROR_CODES) {
    if (msg.includes(code)) return code;
  }
  return null;
}

export class JobsApiUnavailableError extends Error {
  constructor() {
    super('[jobs-api] nativesAPI.jobs not available (Tauri IPC required)');
    this.name = 'JobsApiUnavailableError';
  }
}

function jobsSection(): NativesAPI['jobs'] {
  const api = typeof window === 'undefined' ? null : window.nativesAPI?.jobs;
  if (!api) throw new JobsApiUnavailableError();
  return api;
}

/** 任务命令面是否可用（浏览器 dev 模式为 false） */
export function hasJobsApi(): boolean {
  return typeof window !== 'undefined' && !!window.nativesAPI?.jobs;
}

// ── 8 个命令的类型化封装（契约第 5 节） ──

export async function jobList(): Promise<JobListResult> {
  return (await jobsSection().list()) as JobListResult;
}

export async function jobGet(id: string): Promise<JobDetail> {
  return (await jobsSection().get(id)) as JobDetail;
}

export async function jobCreate(payload: JobPayload): Promise<JobDetail> {
  return (await jobsSection().create(payload as unknown as Record<string, unknown>)) as JobDetail;
}

export async function jobUpdate(
  id: string,
  patch: Partial<JobPayload>,
): Promise<JobDetail> {
  return (await jobsSection().update({ id, ...patch } as Record<string, unknown>)) as JobDetail;
}

export async function jobDelete(id: string): Promise<{ ok: true }> {
  return (await jobsSection().delete(id)) as { ok: true };
}

export async function jobSetEnabled(id: string, enabled: boolean): Promise<JobDetail> {
  return (await jobsSection().setEnabled(id, enabled)) as JobDetail;
}

export async function jobRunNow(id: string): Promise<JobDispatchReceipt> {
  return (await jobsSection().runNow(id)) as JobDispatchReceipt;
}

export async function jobRunsList(params?: {
  job_id?: string;
  limit?: number;
  offset?: number;
}): Promise<JobRunsResult> {
  return (await jobsSection().listRuns(params ?? {})) as JobRunsResult;
}
