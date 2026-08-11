'use client';

/**
 * FileStatusBar — 文件浏览器状态栏（F3-04 布局子组件，ARCH-002）。
 *
 * 纯展示组件：条目统计（数量/文件夹/文件/总大小）、选中数、剪贴板提示、
 * 当前光标条目名、磁盘占用入口。不含任何业务逻辑。
 */

import { SPACING, FONT_SIZE } from '@/lib/design-tokens';
import { t, type Locale } from '@/i18n';
import { type FileEntry } from '@/types/file';
import { fmtSize } from '@/lib/format';

export interface FileStatusBarProps {
  locale: Locale;
  /** 渲染时传入过滤后的列表（父层保证非空才渲染本组件） */
  entries: FileEntry[];
  selectedIndex: number;
  selectedPaths: Set<string>;
  clipBoard: { mode: 'copy' | 'cut'; paths: string[] } | null;
  onOpenDiskUsage: () => void;
}

export default function FileStatusBar({
  locale,
  entries,
  selectedIndex,
  selectedPaths,
  clipBoard,
  onOpenDiskUsage,
}: FileStatusBarProps) {
  const dirs = entries.filter((e) => e.isDir).length;
  const files = entries.length - dirs;
  const totalSize = entries.reduce((sum, e) => sum + (e.isDir ? 0 : e.size), 0);

  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: SPACING.md,
        padding: `${SPACING.sm}px ${SPACING.md}px`,
        fontSize: FONT_SIZE.sm,
        fontFamily: 'var(--font-mono)',
        color: 'var(--text-secondary)',
        borderTop: '1px solid var(--border)',
        background: 'var(--surface)',
      }}
    >
      <span>{t(locale, 'fileBrowser.statusItems').replace('{count}', String(entries.length))}</span>
      {dirs > 0 && <span>{t(locale, 'fileBrowser.statusFolders').replace('{count}', String(dirs))}</span>}
      {files > 0 && <span>{t(locale, 'fileBrowser.statusFiles').replace('{count}', String(files))}</span>}
      {totalSize > 0 && <span>{fmtSize(totalSize)}</span>}
      {selectedPaths.size > 0 && (
        <span style={{ color: 'var(--primary)' }}>
          {t(locale, 'fileBrowser.selectedCount').replace('{count}', String(selectedPaths.size))}
        </span>
      )}
      {clipBoard && (
        <span style={{ opacity: 0.75 }}>
          {clipBoard.mode === 'cut' ? t(locale, 'fileBrowser.cut') : t(locale, 'fileBrowser.copy')}
          {' · '}
          {clipBoard.paths.length}
        </span>
      )}
      <div style={{ flex: 1 }} />
      {selectedIndex >= 0 && selectedIndex < entries.length && selectedPaths.size <= 1 && (
        <span style={{ opacity: 0.7 }} title={t(locale, 'fileBrowser.multiHint')}>
          {entries[selectedIndex]!.name}
        </span>
      )}
      <span
        onClick={onOpenDiskUsage}
        style={{
          cursor: 'pointer',
          color: 'var(--primary)',
          textDecoration: 'none',
          fontSize: FONT_SIZE.sm,
        }}
        onMouseEnter={(e) => {
          (e.currentTarget as HTMLElement).style.textDecoration = 'underline';
        }}
        onMouseLeave={(e) => {
          (e.currentTarget as HTMLElement).style.textDecoration = 'none';
        }}
      >
        {t(locale, 'fileBrowser.diskUsage')} →
      </span>
    </div>
  );
}
