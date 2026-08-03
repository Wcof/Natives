/**
 * Local creative add wizard — pure helpers (node:test friendly).
 */

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
  startAfterSave: boolean;
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
    startAfterSave: input.startAfterSave,
  };
}

export function planSummaryLines(plan: LaunchPlan | null | undefined, locale: 'zh' | 'en' = 'zh'): string[] {
  if (!plan) return locale === 'zh' ? ['尚未生成启动方案'] : ['No launch plan yet'];
  const lines: string[] = [];
  lines.push(`${locale === 'zh' ? '运行时' : 'Runtime'}: ${plan.runtime}`);
  lines.push(`${locale === 'zh' ? '程序' : 'Program'}: ${plan.program}`);
  if (plan.script) lines.push(`script: ${plan.script}`);
  if (plan.entryFile) lines.push(`entry: ${plan.entryFile}`);
  lines.push(`cwd: ${plan.cwdRelative || '.'}`);
  lines.push(
    `port: ${plan.port.mode}${plan.port.value != null ? `=${plan.port.value}` : ''}`,
  );
  lines.push(`open: ${plan.openPath}`);
  if (plan.reason) lines.push(`${locale === 'zh' ? '理由' : 'Reason'}: ${plan.reason}`);
  return lines;
}

export function deleteLocalConfirmNote(locale: 'zh' | 'en' = 'zh'): string {
  return locale === 'zh'
    ? '仅删除 Natives 中的记录与应用日志，不会删除项目目录中的任何文件。'
    : 'Only removes the Natives record and app logs. Project files are never deleted.';
}

export function localIssueLabel(
  code: string | undefined,
  locale: 'zh' | 'en' = 'zh',
): string | null {
  if (!code) return null;
  const zh: Record<string, string> = {
    path_missing: '项目路径不存在',
    environment_missing: '运行环境缺失',
    dependencies_missing: '依赖未安装',
    port_conflict: '端口冲突',
    config_invalid: '启动配置无效',
    ai_error: 'AI 分析失败',
    start_unhealthy: '进程已启动但健康检查失败',
    orphaned_process: '发现残留进程',
    stop_failed: '停止失败：资源未确认释放',
  };
  const en: Record<string, string> = {
    path_missing: 'Project path missing',
    environment_missing: 'Runtime environment missing',
    dependencies_missing: 'Dependencies missing',
    port_conflict: 'Port conflict',
    config_invalid: 'Invalid launch config',
    ai_error: 'AI analysis failed',
    start_unhealthy: 'Process started but health check failed',
    orphaned_process: 'Orphaned process detected',
    stop_failed: 'Stop failed: resources not confirmed released',
  };
  return (locale === 'zh' ? zh : en)[code] ?? code;
}
