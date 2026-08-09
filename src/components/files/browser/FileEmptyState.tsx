'use client';

/**
 * FileEmptyState — 文件浏览器空态（F3-04 布局子组件，ARCH-002）。
 *
 * 两种空态：最近打开模式提示；普通目录空态 → 新建文件/文件夹/粘贴入口。
 * 纯展示，动作回调由父层（FileBrowser 组合）注入。
 */

import { SPACING, FONT_SIZE } from '@/lib/design-tokens';
import { t, type Locale } from '@/i18n';

export interface FileEmptyStateProps {
  locale: Locale;
  recentOpenedMode: boolean;
  onNewFile: () => void;
  onNewFolder: () => void;
  onPaste: () => void;
  canPaste: boolean;
}

export default function FileEmptyState({
  locale,
  recentOpenedMode,
  onNewFile,
  onNewFolder,
  onPaste,
  canPaste,
}: FileEmptyStateProps) {
  if (recentOpenedMode) {
    return (
      <div
        style={{
          display: 'flex',
          justifyContent: 'center',
          padding: SPACING.md,
          borderTop: '1px solid var(--border)',
          fontSize: FONT_SIZE.sm,
          color: 'var(--text-secondary)',
        }}
      >
        {t(locale, 'fileBrowser.recentOpenedEmpty')}
      </div>
    );
  }

  return (
    <div
      style={{
        display: 'flex',
        gap: SPACING.sm,
        justifyContent: 'center',
        padding: SPACING.md,
        borderTop: '1px solid var(--border)',
      }}
    >
      <button className="btn btn-ghost" onClick={onNewFile}>
        {t(locale, 'fileBrowser.newFile')}
      </button>
      <button className="btn btn-primary" onClick={onNewFolder}>
        {t(locale, 'fileBrowser.newFolder')}
      </button>
      {canPaste && (
        <button className="btn btn-ghost" onClick={onPaste}>
          {t(locale, 'fileBrowser.paste')}
        </button>
      )}
    </div>
  );
}
