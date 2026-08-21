/**
 * tauri/integrations — AI Tool Integration domain facade（ADR-0020 / CLD/CDX/GEM/OPC-001..007）。
 */

import { cmd } from './core';

export interface DetectResult {
  installed: boolean;
  version?: string;
  executable?: string;
}

export interface InspectResult {
  configPath?: string;
  exists: boolean;
  managed: boolean;
  sensitiveKeys: string[];
  summary: string;
}

export interface BackupResult {
  backupPath?: string;
  created: boolean;
}

export interface PlannedPatch {
  target: string;
  patchJson: string;
  summary: string;
}

export interface VerifyResult {
  ok: boolean;
  checks: string[];
  error?: string;
}

export interface RollbackResult {
  restored: boolean;
  backupPath?: string;
}

export const integrationsApi = {
  detect: (tool: string) => cmd<DetectResult>('tool_detect', { tool }),
  inspect: (tool: string) => cmd<InspectResult>('tool_inspect', { tool }),
  backup: (tool: string) => cmd<BackupResult>('tool_backup', { tool }),
  plan: (input: { tool: string; envRefs?: Array<{ key: string; source: string }> }) =>
    cmd<PlannedPatch>('tool_plan', { input }),
  apply: (input: { tool: string; patch: PlannedPatch; userApproved: boolean }) =>
    cmd<boolean>('tool_apply', { input }),
  verify: (tool: string) => cmd<VerifyResult>('tool_verify', { tool }),
  rollback: (input: { tool: string; backupPath?: string }) => cmd<RollbackResult>('tool_rollback', { input }),
};
