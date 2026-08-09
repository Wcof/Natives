/**
 * Local creative add wizard — pure helpers (node:test friendly).
 */

import { t } from '@/i18n';
import type {
  CreateLocalCreativeRequest,
  LaunchPlan,
  LocalProjectScanResult,
  PackageManager,
} from '@/lib/tauri-adapter';

export type LocalWizardStep = 'basic' | 'scan' | 'launch' | 'confirm';

export function defaultLocalTitleFromPath(projectRoot: string): string {
  const parts = projectRoot.replace(/\\/g, '/').split('/').filter(Boolean);
  return parts[parts.length - 1] || 'Local project';
}

export function pickPackageManager(scan: LocalProjectScanResult): PackageManager | undefined {
  if (scan.packageManager) return scan.packageManager;
  if (scan.packageManagerChoices.length === 1) return scan.packageManagerChoices[0];
  return undefined;
}

export function canProceedFromScan(scan: LocalProjectScanResult | null): boolean {
  if (!scan) return false;
  if (scan.blockers.length > 0) return false;
  return true;
}

export function buildCreateRequest(input: {
  projectRoot: string;
  title: string;
  description?: string;
  launchMode: 'smart' | 'custom';
  launchPlan?: LaunchPlan | null;
  autoOpen: boolean;
  packageManager?: PackageManager;
}): CreateLocalCreativeRequest {
  const title = input.title.trim() || defaultLocalTitleFromPath(input.projectRoot);
  let plan = input.launchPlan ?? undefined;
  if (plan && input.packageManager) {
    // Align program with chosen package manager for node_dev plans.
    if (plan.runtime === 'node_dev_server' && plan.program !== 'node' && plan.program !== 'internal') {
      plan = {
        ...plan,
        program: input.packageManager,
        source: plan.source === 'ai' ? 'ai' : 'user',
      };
    }
  }
  return {
    projectRoot: input.projectRoot,
    title,
    description: input.description?.trim() || undefined,
    launchMode: input.launchMode,
    launchPlan: input.launchMode === 'custom' ? plan : plan,
    autoOpen: input.autoOpen,
  };
}

export function planSummaryLines(plan: LaunchPlan | null | undefined, locale = 'zh'): string[] {
  if (!plan) return [t(locale, 'localWizard.noLaunchPlan')];
  const lines: string[] = [];
  lines.push(`${t(locale, 'localWizard.runtime')}: ${plan.runtime}`);
  lines.push(`${t(locale, 'localWizard.program')}: ${plan.program}`);
  if (plan.script) lines.push(`script: ${plan.script}`);
  if (plan.entryFile) lines.push(`entry: ${plan.entryFile}`);
  lines.push(`cwd: ${plan.cwdRelative || '.'}`);
  lines.push(
    `port: ${plan.port.mode}${plan.port.value != null ? `=${plan.port.value}` : ''}`,
  );
  lines.push(`open: ${plan.openPath}`);
  if (plan.reason) lines.push(`${t(locale, 'localWizard.reason')}: ${plan.reason}`);
  return lines;
}

export function deleteLocalConfirmNote(locale = 'zh'): string {
  return t(locale, 'localWizard.deleteNote');
}

const LOCAL_ISSUE_KEYS: Record<string, string> = {
  path_missing: 'localWizard.issuePathMissing',
  environment_missing: 'localWizard.issueEnvironmentMissing',
  dependencies_missing: 'localWizard.issueDependenciesMissing',
  port_conflict: 'localWizard.issuePortConflict',
  config_invalid: 'localWizard.issueConfigInvalid',
  ai_error: 'localWizard.issueAiError',
  start_unhealthy: 'localWizard.issueStartUnhealthy',
  orphaned_process: 'localWizard.issueOrphanedProcess',
  stop_failed: 'localWizard.issueStopFailed',
};

export function localIssueLabel(
  code: string | undefined,
  locale = 'zh',
): string | null {
  if (!code) return null;
  const key = LOCAL_ISSUE_KEYS[code];
  if (!key) return code;
  return t(locale, key);
}
