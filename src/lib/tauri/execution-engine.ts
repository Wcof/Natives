/**
 * tauri/execution-engine — 执行引擎设置域 facade（ARCH-002）
 *
 * 唯一持久化权威（A6 backend）；业务组件只允许经本 facade 访问。
 * 唯一 raw invoke 在 ./core.ts。
 */

import { cmd } from './core';
import type { NativesAPI, ExecutionEngineSettings, ExecutionEngineSnapshot } from './types';

  // Execution Engine settings V2（唯一持久化权威 — A6 backend）
export const executionEngine: NativesAPI['executionEngine'] = {
    // legacyRuntimeId = 一次性迁移种子（MIG-001）：首个快照仅传旧 localStorage
    // 值让后端做 one-way 迁移；迁移完成后前端立即清除旧 key，此后恒为 null。
    getSnapshot: (legacyRuntimeId?: string | null) =>
      cmd('execution_engine_get_snapshot', { legacy_runtime_id: legacyRuntimeId ?? null }),
    saveSettings: (settings: ExecutionEngineSettings) =>
      cmd('execution_engine_save_settings', { settings }),
    detectRuntimes: () => cmd('execution_engine_detect_runtimes'),
    getDiagnostics: () => cmd('execution_engine_get_diagnostics'),
};

