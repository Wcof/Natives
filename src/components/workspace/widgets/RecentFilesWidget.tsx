'use client';

/**
 * Recent Files Widget（B-022）—— 迁移到 V2 Definition。
 * load 复用 recent-files/files domain（recent-files adapter）。
 * 打开文件复用文件域事件（navigate-files），由 FileBrowser 消费。
 */

import { z } from 'zod';
import { FileText } from 'lucide-react';
import { t, useLocale } from '@/i18n';
import { FONT_SIZE, SPACING } from '@/lib/design-tokens';
import { dispatchFileEvent } from '@/lib/file-events';
import type { WidgetDefinition, WidgetProps } from '@/lib/workspace/widgets';
import {
  loadRecentFiles,
  recentFilesAdapterKey,
} from '@/lib/workspace/widgets/adapters/recent-files';
import type { RecentFilesData } from '@/lib/workspace/widgets/adapters/recent-files';

type RecentFilesSettings = Record<string, unknown>;

function RecentFilesView({ data, loading }: WidgetProps<RecentFilesData, RecentFilesSettings>) {
  const locale = useLocale();
  const paths = data?.paths ?? [];

  const open = (path: string) => {
    dispatchFileEvent('navigate-files', path);
  };

  if (loading && paths.length === 0) {
    return (
      <div className="ws-shell-state" style={{ color: 'var(--text-tertiary)', fontSize: FONT_SIZE.xs }}>
        {t(locale, 'common.loading')}
      </div>
    );
  }

  if (paths.length === 0) {
    return (
      <div className="ws-shell-state" style={{ color: 'var(--text-tertiary)', fontSize: FONT_SIZE.xs }}>
        {t(locale, 'home.noRecentFiles')}
      </div>
    );
  }

  return (
    <ul
      style={{
        display: 'flex',
        flexDirection: 'column',
        gap: 2,
        height: '100%',
        overflowY: 'auto',
        padding: '4px 8px',
        margin: 0,
        listStyle: 'none',
      }}
    >
      {paths.map((path) => {
        const name = path.split('/').pop() || path;
        return (
          <li key={path}>
            <button
              type="button"
              onClick={() => open(path)}
              title={path}
              style={{
                display: 'flex',
                width: '100%',
                alignItems: 'center',
                gap: SPACING.sm,
                padding: '4px 8px',
                borderRadius: 8,
                border: 'none',
                background: 'transparent',
                textAlign: 'left',
                cursor: 'pointer',
                fontSize: FONT_SIZE.xs,
                color: 'var(--text-secondary)',
              }}
              onMouseEnter={(e) => {
                e.currentTarget.style.background = 'var(--surface-hover)';
                e.currentTarget.style.color = 'var(--text)';
              }}
              onMouseLeave={(e) => {
                e.currentTarget.style.background = 'transparent';
                e.currentTarget.style.color = 'var(--text-secondary)';
              }}
            >
              <FileText size={13} style={{ flexShrink: 0, color: 'var(--text-tertiary)' }} />
              <span
                style={{
                  minWidth: 0,
                  flex: 1,
                  overflow: 'hidden',
                  textOverflow: 'ellipsis',
                  whiteSpace: 'nowrap',
                }}
              >
                {name}
              </span>
            </button>
          </li>
        );
      })}
    </ul>
  );
}

export const recentFilesWidgetDefinition: WidgetDefinition<RecentFilesData, RecentFilesSettings> = {
  type: 'recent_files',
  titleKey: 'home.widgetRecentFiles',
  descriptionKey: 'home.widgetRecentFiles',
  configVersion: 1,
  defaultConfig: {},
  configSchema: z.record(z.string(), z.unknown()),
  size: 'medium',
  surfacePolicy: { surfaces: ['plain', 'material'], allowBlur: false, allowGlow: false },
  adapterKeyBuilder: recentFilesAdapterKey,
  load: loadRecentFiles,
  Component: RecentFilesView,
};

export default recentFilesWidgetDefinition;
