/**
 * tauri/terminal — 终端域 facade（ARCH-002）
 *
 * 业务组件只允许经本 facade 访问 terminal / shell 能力；唯一 raw invoke
 * 在 ./core.ts（`cmd` / `subscribe`）。
 */

import { cmd, subscribe } from './core';
import type { NativesAPI, RenderStatePayload } from './types';

/** Terminal / PTY 域命令与事件订阅。 */
export const terminal: NativesAPI['terminal'] = {
  create: async (profileId?: string, cols?: number, rows?: number) => {
    const sessionId = await cmd<string>('terminal_create', { profileId, cols, rows });
    return { sessionId };
  },
  write: (sessionId: string, data: string) => cmd('terminal_write', { sessionId, data }),
  resize: (sessionId: string, cols: number, rows: number) =>
    cmd('terminal_resize', { sessionId, cols, rows }),
  kill: (sessionId: string) => cmd('terminal_kill', { sessionId }),
  cwd: async (sessionId: string) => {
    const result = await cmd<{ cwd: string; source: string }>('terminal_cwd', { sessionId });
    return result;
  },
  proc: (sessionId: string) => cmd<{ processName: string; pid: number }>('terminal_proc', { sessionId }),
  sessionState: (sessionId: string) => cmd<{
    sessionId: string; cols: number; rows: number;
    title: string; cwd: string; foregroundProcess: string;
    pid: number; status: string;
  }>('terminal_session_state', { sessionId }),
  listSessions: () => cmd<Array<{
    sessionId: string; cols: number; rows: number;
    title: string; cwd: string; foregroundProcess: string;
    pid: number; status: string;
  }>>('terminal_list_sessions'),
  onData: (callback: (data: { sessionId: string; data: string }) => void) =>
    subscribe<{ sessionId: string; data: string }>('terminal:data', (payload) => callback(payload)),
  onExit: (callback: (data: { sessionId: string; exitCode: number }) => void) =>
    subscribe<{ sessionId: string; exitCode: number }>('terminal:exit', (payload) => callback(payload)),
  onRenderState: (callback: (data: RenderStatePayload) => void) =>
    subscribe<RenderStatePayload>('terminal:render-state', (payload) => callback(payload)),
  onTitleChanged: (callback: (data: { sessionId: string; title: string }) => void) =>
    subscribe<{ sessionId: string; title: string }>('terminal:title-changed', (payload) => callback(payload)),
  onPwdChanged: (callback: (data: { sessionId: string; pwd: string }) => void) =>
    subscribe<{ sessionId: string; pwd: string }>('terminal:pwd-changed', (payload) => callback(payload)),
  onBell: (callback: (data: { sessionId: string }) => void) =>
    subscribe<{ sessionId: string }>('terminal:bell', (payload) => callback(payload)),
  renderState: (sessionId: string) => cmd<RenderStatePayload>('terminal_render_state', { sessionId }),
  recordStart: (sessionId: string, cols: number, rows: number) =>
    cmd('terminal_record_start', { sessionId, cols, rows }),
  recordStop: (sessionId: string) => cmd('terminal_record_stop', { sessionId }),
  recordList: () => cmd('terminal_record_list'),
  recordPlay: (id: string) => cmd<string>('terminal_record_play', { id }),
  recordExport: (id: string, format: string) =>
    cmd<{ ok: boolean; path: string; format: string; fellBack?: string }>('terminal_record_export', { id, format }),
  recordPrune: () => cmd('terminal_record_prune'),
};
