'use client';

import { useState, useEffect, useCallback } from 'react';
import { type FileChangeEvent } from '@/types/agent';
import { Eye, EyeOff, PlusCircle, XCircle, Edit3, Folder } from 'lucide-react';
import { t, useLocale } from '@/i18n';
import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';
import { FILE_EVENTS, dispatchFileEvent } from '@/lib/file-events';
import { fsWatchApiOrNull } from '@/lib/files-api';

/**
 * 变更收件箱 — 订阅真实 fs-watch-change 管道（fsWatch.onChange）。
 * 旧实现监听后端从不发射的 `file:changed` 频道，恒为空。
 * 噪声过滤在渲染层进行（旧实现在采集层丢弃，切换开关无法回显历史）。
 */

// ── Noise filter: exclude system/generated files ──
const NOISE_PATTERNS = [
  /\.git\//, /\.svn\//, /\.hg\//,
  /node_modules\//, /\.next\//, /\.nuxt\//, /dist\//, /build\//, /out\//,
  /__pycache__\//, /\.pyc$/, /\.pyo$/,
  /\.DS_Store$/, /Thumbs\.db$/, /desktop\.ini$/,
  /\.cache\//, /\.tmp\//, /\.temp\//,
  /\.db-wal$/, /\.db-shm$/, /\.db-journal$/,
  /\.lock$/, /package-lock\.json$/, /yarn\.lock$/, /pnpm-lock\.yaml$/,
];

function isNoisyChange(path: string): boolean {
  return NOISE_PATTERNS.some(p => p.test(path));
}

function mapKind(kind: string): FileChangeEvent['type'] {
  if (kind === 'create') return 'create';
  if (kind === 'remove') return 'delete';
  return 'modify';
}

interface ChangeItem extends FileChangeEvent {
  project: string;
  count: number;
  noisy: boolean;
}

export default function ChangeInbox() {
  const [items, setItems] = useState<ChangeItem[]>([]);
  const [showFiltered, setShowFiltered] = useState(false);
  const locale = useLocale();

  // 常驻订阅；噪声事件也入库（带 noisy 标记），渲染层决定是否展示
  useEffect(() => {
    const api = fsWatchApiOrNull();
    if (!api) return;
    const unsub = api.onChange((event) => {
      const eventType = mapKind(event.kind);
      const parts = event.path.split('/');
      const project = parts[parts.length - 2] || '';

      setItems((prev) => {
        const existing = prev.find(p => p.path === event.path);
        if (existing) {
          return prev.map(p =>
            p.path === event.path ? { ...p, count: p.count + 1, timestamp: Date.now(), type: eventType } : p
          );
        }
        return [
          {
            path: event.path,
            type: eventType,
            timestamp: Date.now(),
            project,
            count: 1,
            noisy: isNoisyChange(event.path),
          },
          ...prev,
        ].slice(0, 200);
      });
    });
    return unsub;
  }, []);

  const handleClear = useCallback(() => setItems([]), []);

  const handleNavigate = useCallback((path: string) => {
    const dir = path.substring(0, path.lastIndexOf('/')) || '/';
    dispatchFileEvent(FILE_EVENTS.navigateFiles, dir);
  }, []);

  const visible = showFiltered ? items : items.filter((i) => !i.noisy);
  const sorted = [...visible].sort((a, b) => b.timestamp - a.timestamp);

  const unknownLabel = t(locale, 'aiWorkbench.inbox.unknownProject');
  const groupedByProject = sorted.reduce<Record<string, ChangeItem[]>>((acc, item) => {
    const project = item.project || unknownLabel;
    if (!acc[project]) acc[project] = [];
    acc[project]!.push(item);
    return acc;
  }, {});

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      {/* Header */}
      <div style={{
        padding: '8px 10px', borderBottom: '1px solid var(--border)',
        display: 'flex', alignItems: 'center', justifyContent: 'space-between',
      }}>
        <div style={{ fontSize: FONT_SIZE.sm, fontWeight: 600, color: 'var(--text-secondary)', textTransform: 'uppercase', letterSpacing: 0.5 }}>
          {t(locale, 'aiWorkbench.changeInbox')} ({visible.length})
        </div>
        <div style={{ display: 'flex', gap: SPACING.xs, alignItems: 'center' }}>
          <button
            type="button"
            className="btn-ghost"
            onClick={() => setShowFiltered(!showFiltered)}
            style={{ fontSize: FONT_SIZE.xs, padding: '2px 6px', color: showFiltered ? 'var(--primary)' : 'var(--text-disabled)', display: 'inline-flex' }}
            title={showFiltered ? t(locale, 'aiWorkbench.inbox.hideSystem') : t(locale, 'aiWorkbench.inbox.showSystem')}
            aria-label={showFiltered ? t(locale, 'aiWorkbench.inbox.hideSystem') : t(locale, 'aiWorkbench.inbox.showSystem')}
            aria-pressed={showFiltered}
          >
            {showFiltered ? <Eye size={13} /> : <EyeOff size={13} />}
          </button>
          {items.length > 0 && (
            <button type="button" className="btn-ghost" onClick={handleClear} style={{ fontSize: FONT_SIZE.xs, padding: '2px 6px', color: 'var(--text-disabled)' }}>
              {t(locale, 'common.clear')}
            </button>
          )}
        </div>
      </div>

      {/* Changes list */}
      <div style={{ flex: 1, overflow: 'auto', padding: 6 }}>
        {visible.length === 0 ? (
          <div style={{ padding: SPACING.xl, textAlign: 'center', color: 'var(--text-disabled)', fontSize: 'var(--fs-sm)' }}>
            {t(locale, 'aiWorkbench.noChanges')}
          </div>
        ) : (
          Object.entries(groupedByProject).map(([project, changes]) => (
            <div key={project} style={{ marginBottom: 10 }}>
              <div style={{ fontSize: FONT_SIZE.xs, fontWeight: 600, color: 'var(--text-secondary)', marginBottom: SPACING.xs, padding: '0 4px' }}>
                <Folder size={12} style={{ marginRight: 4, color: 'var(--text-secondary)' }} /> {project}
              </div>
              {changes.map((ch) => (
                <div
                  key={ch.path}
                  onClick={() => handleNavigate(ch.path)}
                  style={{
                    padding: '4px 8px', fontSize: FONT_SIZE.sm, color: 'var(--text)',
                    borderRadius: BORDER_RADIUS.sm, cursor: 'pointer', marginBottom: 2,
                    background: 'var(--surface)',
                    borderLeft: `3px solid ${ch.type === 'create' ? 'var(--diff-add)' : ch.type === 'delete' ? 'var(--danger)' : 'var(--warning)'}`,
                    display: 'flex', alignItems: 'center', gap: SPACING.xs,
                    opacity: ch.noisy ? 0.6 : 1,
                  }}
                >
                  <span style={{ flexShrink: 0, display: 'inline-flex' }}>
                    {ch.type === 'create' ? <PlusCircle size={12} style={{ color: 'var(--diff-add)' }} /> : ch.type === 'delete' ? <XCircle size={12} style={{ color: 'var(--danger)' }} /> : <Edit3 size={12} style={{ color: 'var(--warning)' }} />}
                  </span>
                  <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', flex: 1 }}>
                    {ch.path.split('/').pop()}
                  </span>
                  {ch.count > 1 && (
                    <span style={{
                      fontSize: FONT_SIZE.xs, fontWeight: 600, color: 'var(--primary)',
                      background: 'var(--primary-soft)', padding: '0 4px', borderRadius: BORDER_RADIUS.sm,
                      flexShrink: 0,
                    }}>
                      ×{ch.count}
                    </span>
                  )}
                </div>
              ))}
            </div>
          ))
        )}
      </div>
    </div>
  );
}
