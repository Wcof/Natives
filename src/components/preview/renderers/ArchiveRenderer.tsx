'use client';

/**
 * T17 · ArchiveRenderer — 只消费 { kind: 'archive' } PreviewModel。
 * 纯展示：entries/truncated 来自 provider（Host list 已有 1000 条 entry budget），
 * 本组件不做 IO/解压。有界渲染（R-P4）：展示端再做一级 DOM 上限，
 * 超出显式 truncated 提示。图标用 SVG（lucide），禁用 Emoji（R-U6）。
 */

import { File, Folder } from 'lucide-react';
import { t, useLocale } from '@/i18n';
import type { PreviewModel } from '@/lib/preview/contracts';

export type ArchiveModel = Extract<PreviewModel, { kind: 'archive' }>;

/** 展示预算：最多渲染 200 条 entry（R-P4） */
export const ARCHIVE_RENDER_MAX_ENTRIES = 200;

export default function ArchiveRenderer({ model }: { model: ArchiveModel }) {
  const locale = useLocale();
  const capped = model.entries.length > ARCHIVE_RENDER_MAX_ENTRIES;
  const shown = capped ? model.entries.slice(0, ARCHIVE_RENDER_MAX_ENTRIES) : model.entries;

  return (
    <div data-preview-kind="archive" style={{ padding: 12, fontFamily: 'var(--font-mono)', fontSize: 13 }}>
      {model.truncated && (
        <div style={{ padding: '4px 0 8px', color: 'var(--text-secondary)' }}>
          {t(locale, 'preview.archiveTruncated', { count: shown.length })}
        </div>
      )}
      <ul style={{ margin: 0, padding: 0, listStyle: 'none' }}>
        {shown.map((entry, i) => (
          <li key={i} style={{ padding: '3px 4px', borderBottom: '1px solid var(--border)', display: 'flex', alignItems: 'center', gap: 8 }}>
            {entry.isDir ? (
              <Folder size={14} style={{ flexShrink: 0, color: 'var(--text-secondary)' }} aria-hidden />
            ) : (
              <File size={14} style={{ flexShrink: 0, color: 'var(--text-secondary)' }} aria-hidden />
            )}
            <span style={{ flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
              {entry.name}
            </span>
            <span style={{ color: 'var(--text-secondary)', flexShrink: 0 }}>{entry.size}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}
