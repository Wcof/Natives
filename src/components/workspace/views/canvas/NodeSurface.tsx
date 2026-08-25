'use client';

/**
 * NodeSurface (WS-03) — single node rendering with drag/cancel boundaries.
 *
 * Pointer ownership stays on the Stage: a node never captures the pointer. It
 * only announces a drag-intent (or a cancel intent) through data attributes
 * that the Stage reads on pointer-down:
 *   - body (`[data-node-cancel]`)  → cancel (no drag starts from an interactive
 *     body; widgets handle clicks/drags themselves).
 *   - header band (`[data-node-drag]`) → drag (明确拖拽区启动移动).
 *
 * `locked` nodes are immutable: no resize overlay is rendered for them and the
 * gesture controller refuses any drag on them.
 */

import { Lock, StickyNote } from 'lucide-react';
import { t, useLocale } from '@/i18n';
import type { CanvasNode } from '@/lib/workspace/canvas/types';

export interface NodeSurfaceProps {
  node: CanvasNode;
  selected: boolean;
  /** Content area: either the widget renderer or undefined for a plain body. */
  content?: React.ReactNode;
}

export function NodeSurface({ node, selected, content }: NodeSurfaceProps) {
  const locale = useLocale();
  return (
    <div
      data-node-id={node.id}
      data-testid={`canvas-node-${node.kind}`}
      className={`absolute touch-none select-none overflow-hidden rounded-xl border ${
        node.kind === 'frame'
          ? 'border-dashed border-[var(--border)] bg-[var(--surface-hover)]/40'
          : node.kind === 'group'
            ? 'border-[var(--primary)]/40 bg-[var(--primary-soft)]/20'
            : 'border-[var(--border-subtle)] bg-[var(--surface)] shadow-sm'
      } ${selected ? 'outline-2 outline-[var(--primary)]' : ''} ${node.locked ? 'opacity-70' : ''}`}
      style={{
        left: node.x,
        top: node.y,
        width: node.w,
        height: node.h,
        zIndex: node.z,
        cursor: node.locked ? 'default' : 'grab',
      }}
    >
      {content ? (
        <div className="flex h-full w-full flex-col" data-node-cancel>
          {content}
        </div>
      ) : (
        <div className="flex h-full w-full flex-col">
          <div
            className="group flex h-6 shrink-0 items-center gap-1 border-b border-[var(--border-subtle)] px-2"
            data-node-drag
            data-testid="canvas-node-header"
          >
            {node.kind === 'note' && <StickyNote size={12} className="shrink-0 text-[var(--text-secondary)]" />}
            <span className="min-w-0 flex-1 truncate text-[0.625rem] text-[var(--text-secondary)]">
              {node.label}
            </span>
            {node.locked && (
              <Lock size={11} className="shrink-0 text-[var(--text-disabled)]" aria-label={t(locale, 'workspace.canvasLockedHint')} />
            )}
          </div>
          <div
            className="flex min-h-0 flex-1 flex-col px-2 py-1.5"
            data-node-cancel
            data-testid="canvas-node-body"
          >
            <span className="text-[0.6875rem] leading-relaxed text-[var(--text-secondary)]">
              {node.kind === 'frame' || node.kind === 'group'
                ? t(locale, 'workspace.canvasNodeItems', { count: node.members?.length ?? 0 })
                : t(locale, 'workspace.canvasDoubleClickHint')}
            </span>
          </div>
        </div>
      )}
    </div>
  );
}