'use client';

import { useState, useEffect, useCallback } from 'react';
import { X } from 'lucide-react';
import { type FileChangeEvent } from '@/types/agent';
import { t as tr, useLocale } from '@/i18n';
import { SPACING, FONT_SIZE, BORDER_RADIUS } from '@/lib/design-tokens';
import { FILE_EVENTS, dispatchFileEvent } from '@/lib/file-events';
import { fsWatchApiOrNull } from '@/lib/files-api';

/**
 * Agent 变更实时流 — 订阅真实 fs-watch-change 管道（fsWatch.onChange）。
 * 旧实现监听 db-state-changed 的 `file:changed` 频道，该频道后端从不发射，
 * 面板永远空转。事件来自当前活跃的监听集（文件浏览器目录 / follow 模式等）。
 */

/** fs_watch 的 wire kind → 展示三分类 */
function mapKind(kind: string): FileChangeEvent['type'] {
  if (kind === 'create') return 'create';
  if (kind === 'remove') return 'delete';
  return 'modify';
}

export default function AgentDashboard() {
  const [changes, setChanges] = useState<FileChangeEvent[]>([]);
  const [paused, setPaused] = useState(false);
  const [frozen, setFrozen] = useState<FileChangeEvent[] | null>(null);
  const [filter, setFilter] = useState<'all' | 'create' | 'modify' | 'delete'>('all');
  const locale = useLocale();
  const t = (key: string) => tr(locale, key);

  // 订阅常驻：暂停只冻结展示，不退订（旧实现暂停即退订，事件永久丢失）
  useEffect(() => {
    const api = fsWatchApiOrNull();
    if (!api) return;
    const unsub = api.onChange((event) => {
      setChanges((prev) => [
        { path: event.path, type: mapKind(event.kind), timestamp: Date.now() },
        ...prev,
      ].slice(0, 100));
    });
    return unsub;
  }, []);

  const handleTogglePause = useCallback(() => {
    setPaused((prev) => {
      if (!prev) setFrozen(changes);
      else setFrozen(null);
      return !prev;
    });
  }, [changes]);

  const handleClear = useCallback(() => { setChanges([]); setFrozen(null); }, []);

  const handleNavigate = useCallback((path: string) => {
    const dir = path.substring(0, path.lastIndexOf('/')) || '/';
    dispatchFileEvent(FILE_EVENTS.navigateFiles, dir);
  }, []);

  const source = paused && frozen ? frozen : changes;
  const filtered = filter === 'all' ? source : source.filter((c) => c.type === filter);
  const getIntensity = (idx: number) => Math.max(0.2, 1 - idx * 0.03);

  const typeIcons: Record<string, string> = { create: '+', delete: '−', modify: '✎' };
  const typeColors: Record<string, string> = { create: 'var(--diff-add)', delete: 'var(--danger)', modify: 'var(--primary)' };

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      {/* Header */}
      <div style={{
        padding: '8px 10px',
        borderBottom: '1px solid var(--border)',
        display: 'flex', alignItems: 'center', justifyContent: 'space-between',
      }}>
        <div style={{ fontSize: FONT_SIZE.sm, fontWeight: 600, color: 'var(--text-secondary)', textTransform: 'uppercase', letterSpacing: 0.5 }}>
          {t('aiWorkbench.agentChanges')}
        </div>
        <div style={{ display: 'flex', gap: SPACING.xs }}>
          {/* Filter */}
          {(['all', 'create', 'modify', 'delete'] as const).map((f) => (
            <button
              key={f}
              type="button"
              className="btn-ghost"
              onClick={() => setFilter(f)}
              style={{
                fontSize: FONT_SIZE.xs, padding: '2px 6px', borderRadius: BORDER_RADIUS.sm,
                color: filter === f ? 'var(--primary)' : 'var(--text-disabled)',
                background: filter === f ? 'var(--primary-soft)' : 'transparent',
              }}
            >
              {f === 'all' ? t('aiWorkbench.dashboard.all') : `${typeIcons[f]} ${t('aiWorkbench.dashboard.' + f)}`}
            </button>
          ))}
          {/* Pause/Resume */}
          <button
            type="button"
            className="btn-ghost"
            onClick={handleTogglePause}
            style={{
              fontSize: FONT_SIZE.xs, padding: '2px 6px', borderRadius: BORDER_RADIUS.sm,
              color: paused ? 'var(--warning)' : 'var(--text-disabled)',
            }}
            title={paused ? t('aiWorkbench.dashboard.resume') : t('aiWorkbench.dashboard.pause')}
          >
            {paused ? '▶' : '⏸'}
          </button>
          {/* Clear */}
          <button
            type="button"
            className="btn-ghost"
            onClick={handleClear}
            style={{ fontSize: FONT_SIZE.xs, padding: '2px 6px', borderRadius: BORDER_RADIUS.sm, color: 'var(--text-disabled)' }}
            title={t('aiWorkbench.dashboard.clear')}
          >
            <X size={12} />
          </button>
        </div>
      </div>

      {/* Changes list */}
      <div style={{ flex: 1, overflow: 'auto', padding: 6 }}>
        {filtered.length === 0 ? (
          <div style={{ padding: SPACING.xl, textAlign: 'center', color: 'var(--text-disabled)', fontSize: 'var(--fs-sm)' }}>
            {paused ? t('aiWorkbench.dashboard.paused') : t('aiWorkbench.waitingForChanges')}
          </div>
        ) : (
          filtered.map((ch, i) => (
            <div
              key={`${ch.path}-${ch.timestamp}-${i}`}
              onClick={() => handleNavigate(ch.path)}
              style={{
                padding: '5px 8px', marginBottom: 3, borderRadius: BORDER_RADIUS.sm, cursor: 'pointer',
                fontSize: FONT_SIZE.sm, transition: 'opacity 0.3s',
                opacity: getIntensity(i),
                background: 'var(--surface)',
                borderLeft: `3px solid ${typeColors[ch.type] || typeColors.modify}`,
                animation: i === 0 && !paused ? 'livePulse 1.1s ease-in-out infinite' : undefined,
                color: 'var(--text)',
                overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
              }}
              title={ch.path}
            >
              <span style={{ fontSize: FONT_SIZE.xs, marginRight: 4, color: typeColors[ch.type] }}>
                {typeIcons[ch.type] || '✎'}
              </span>
              <span style={{ opacity: 0.5, fontSize: FONT_SIZE.xs, marginRight: 4 }}>
                {ch.path.split('/').slice(-2, -1)[0]}/
              </span>
              {ch.path.split('/').pop()}
            </div>
          ))
        )}
      </div>

      {/* Footer */}
      <div style={{
        padding: '4px 10px', borderTop: '1px solid var(--border)',
        fontSize: FONT_SIZE.xs, color: 'var(--text-disabled)', display: 'flex', justifyContent: 'space-between',
      }}>
        <span>{tr(locale, 'aiWorkbench.dashboard.changesCount', { n: filtered.length })}</span>
        {paused && <span style={{ color: 'var(--warning)' }}>⏸ {t('aiWorkbench.dashboard.paused')}</span>}
      </div>
    </div>
  );
}
