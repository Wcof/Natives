'use client';

/**
 * 最近文件 Widget（默认 5 Widget 之一）。
 * 复用 `useRecentFiles`（现有查询层），不直接扫描文件系统。
 */

import { FileText } from 'lucide-react';
import { t, useLocale } from '@/i18n';
import { useRecentFiles } from '@/lib/recent-files-client';
import { dispatchFileEvent } from '@/lib/file-events';

const LIMIT = 8;

export function RecentFilesWidget() {
  const locale = useLocale();
  const { paths, loading } = useRecentFiles(LIMIT);

  const open = (path: string) => {
    // 打开文件：复用文件域事件（navigate-files），由 FileBrowser 消费。
    dispatchFileEvent('navigate-files', path);
  };

  if (loading) {
    return (
      <div className="flex h-full items-center justify-center text-xs text-[var(--text-disabled)]">
        {t(locale, 'common.loading')}
      </div>
    );
  }

  if (paths.length === 0) {
    return (
      <div className="flex h-full items-center justify-center text-xs text-[var(--text-disabled)]">
        {t(locale, 'home.noRecentFiles')}
      </div>
    );
  }

  return (
    <ul className="flex h-full flex-col gap-1 overflow-y-auto px-1">
      {paths.map((path) => {
        const name = path.split('/').pop() || path;
        return (
          <li key={path}>
            <button
              type="button"
              onClick={() => open(path)}
              title={path}
              className="flex w-full items-center gap-2 rounded-lg px-2 py-1 text-left text-xs text-[var(--text-secondary)] hover:bg-[var(--surface-hover)] hover:text-[var(--text)]"
            >
              <FileText size={13} className="shrink-0 text-[var(--text-disabled)]" />
              <span className="min-w-0 flex-1 truncate">{name}</span>
            </button>
          </li>
        );
      })}
    </ul>
  );
}
