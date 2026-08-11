'use client';

// WorkbenchHeader — workbench 顶部工具栏。
//
// UX-12/13/14（W8）：会话头部追加两项只读能力——
//   - 当前 conversation 绑定的 linked-worktree chip（UX-13/14）
//   - 「导出诊断」动作入口（UX-12，仅回调由 shell 注入，header 保持纯展示）

import { Download, PanelRightClose, PanelRightOpen } from 'lucide-react';
import type { Locale } from '@/i18n';
import { t } from '@/i18n';
import { WorktreeChip } from '../worktree/WorktreeChip';

export interface WorkbenchHeaderProps {
  locale: Locale;
  onOpenPalette: () => void;
  showRight: boolean;
  onToggleRightPanel: () => void;
  /** 当前会话绑定的项目路径（null = 未绑定）；用于 linked-worktree chip。 */
  projectPath?: string | null;
  /** UX-12 导出诊断（shell 注入；undefined = 该 surface 不提供导出）。 */
  onExportDiagnostics?: () => void;
  exportingDiagnostics?: boolean;
}

/**
 * Top toolbar of the workbench: worktree chip + diagnostics export + command
 * palette hint + activity panel toggle.
 *
 * Pure presentation — all behavior arrives as callbacks from the shell so the
 * header never reads the store itself.
 */
export function WorkbenchHeader({
  locale,
  onOpenPalette,
  showRight,
  onToggleRightPanel,
  projectPath = null,
  onExportDiagnostics,
  exportingDiagnostics = false,
}: WorkbenchHeaderProps) {
  return (
    <div className="flex items-center gap-1 border-b border-[var(--border)] px-3 py-1">
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, minWidth: 0, flex: 1 }}>
        <WorktreeChip locale={locale} projectPath={projectPath} />
      </div>
      {onExportDiagnostics ? (
        <button
          type="button"
          onClick={onExportDiagnostics}
          disabled={exportingDiagnostics}
          aria-label={t(locale, 'assistant.exportDiagnostics')}
          aria-busy={exportingDiagnostics}
          title={t(locale, 'assistant.exportDiagnosticsTitle')}
          className="rounded p-1.5 text-[var(--text-disabled)] hover:bg-[var(--surface-hover)] hover:text-[var(--text-secondary)] disabled:opacity-50"
        >
          <Download size={16} />
        </button>
      ) : null}
      <button
        type="button"
        onClick={onOpenPalette}
        className="rounded px-2 py-1 text-[11px] text-[var(--text-disabled)] hover:bg-[var(--surface-hover)] hover:text-[var(--text-secondary)]"
        title={t(locale, 'assistant.commandPaletteHint')}
      >
        {t(locale, 'assistant.commands')}
      </button>
      <button
        type="button"
        onClick={onToggleRightPanel}
        className="rounded p-1.5 text-[var(--text-disabled)] hover:bg-[var(--surface-hover)] hover:text-[var(--text-secondary)]"
        title={t(locale, 'assistant.activityPanel')}
        aria-label={t(locale, 'assistant.toggleActivityPanel')}
      >
        {showRight ? <PanelRightClose size={16} /> : <PanelRightOpen size={16} />}
      </button>
    </div>
  );
}

export default WorkbenchHeader;
