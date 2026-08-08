'use client';

/**
 * T17 · ArchiveRenderer — 只消费 { kind: 'archive' } PreviewModel。
 * 纯展示：entries/truncated 来自 provider，本组件不做 IO/解压。
 * 有界渲染：超预算时显式 truncated 提示（R-P4）。
 */

import type { PreviewModel } from '@/lib/preview/contracts';

export type ArchiveModel = Extract<PreviewModel, { kind: 'archive' }>;

export default function ArchiveRenderer({ model }: { model: ArchiveModel }) {
  return (
    <div data-preview-kind="archive" style={{ padding: 12, fontFamily: 'var(--font-mono)', fontSize: 13 }}>
      {model.truncated && (
        <div style={{ padding: '4px 0 8px', color: 'var(--text-secondary)' }}>
          压缩包条目较多，仅显示前 {model.entries.length} 项
        </div>
      )}
      <ul style={{ margin: 0, padding: 0, listStyle: 'none' }}>
        {model.entries.map((entry, i) => (
          <li key={i} style={{ padding: '3px 4px', borderBottom: '1px solid var(--border)', display: 'flex', gap: 8 }}>
            <span style={{ flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
              {entry.isDir ? '📁 ' : '📄 '}
              {entry.name}
            </span>
            <span style={{ color: 'var(--text-secondary)', flexShrink: 0 }}>{entry.size}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}
