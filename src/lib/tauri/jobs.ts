/**
 * tauri/jobs — 任务模块域 facade（ARCH-002）
 *
 * 业务组件只允许经本 facade 访问 job_* 命令；唯一 raw invoke 在 ./core.ts。
 */

import { cmd } from './core';
import type { NativesAPI } from './types';

  // Job module（任务）— 契约 v1：入参 JSON snake_case，与后端命令面一致
export const jobs: NativesAPI['jobs'] = {
    list: () => cmd('job_list'),
    get: (id: string) => cmd('job_get', { id }),
    create: (payload: Record<string, unknown>) => cmd('job_create', payload),
    update: (payload: Record<string, unknown>) => cmd('job_update', payload),
    delete: (id: string) => cmd('job_delete', { id }),
    setEnabled: (id: string, enabled: boolean) => cmd('job_set_enabled', { id, enabled }),
    runNow: (id: string) => cmd('job_run_now', { id }),
    listRuns: (params: { job_id?: string; limit?: number; offset?: number }) =>
      cmd('job_runs_list', params as Record<string, unknown>),
};

