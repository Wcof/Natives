'use client';

import { useMemo, useState } from 'react';
import dynamic from 'next/dynamic';
import { computeLineDiff } from '@/lib/diff-utils';

const MonacoDiffView = dynamic(() => import('@/components/ui/diff/MonacoDiffView'), {
  ssr: false,
  loading: () => (
    <div className="grid h-48 place-items-center text-xs text-[var(--text-disabled)]">
      Loading Monaco…
    </div>
  ),
});

interface DiffViewerProps {
  oldContent: string;
  newContent: string;
  fileName: string;
  onRollback?: () => void;
  onOpenFile?: () => void;
  /** Compact timeline mode vs full inspector. */
  mode?: 'inline' | 'full';
  locale?: string;
  /** Open hunk snippets immediately without opting into Monaco. */
  defaultExpanded?: boolean;
}

/**
 * Real hunk/diffstat viewer (insert-aware LCS), with optional Monaco full view.
 * Replaces the old line-index-aligned comparison that broke on inserts.
 */
export default function DiffViewer({
  oldContent,
  newContent,
  fileName,
  onRollback,
  onOpenFile,
  mode = 'inline',
  locale = 'en',
  defaultExpanded = false,
}: DiffViewerProps) {
  const zh = locale.startsWith('zh');
  const [expanded, setExpanded] = useState(mode === 'full' || defaultExpanded);
  const [useMonaco, setUseMonaco] = useState(mode === 'full');
  const result = useMemo(
    () => computeLineDiff(oldContent, newContent),
    [oldContent, newContent],
  );

  return (
    <div className="overflow-hidden rounded-lg border border-[var(--border-subtle)]" data-diff-viewer="hunk">
      <div className="flex items-center justify-between gap-2 border-b border-[var(--border-subtle)] bg-[var(--surface)] px-3 py-2">
        <div className="min-w-0">
          <div className="truncate text-xs font-medium text-[var(--text-secondary)]">{fileName}</div>
          <div className="text-[10px] text-[var(--text-disabled)]" data-diffstat>
            {result.diffstat}
            {result.hunks.length > 0 ? ` · ${result.hunks.length} hunks` : ''}
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-2 text-[0.625rem]">
          <span className="text-green-500">+{result.additions}</span>
          <span className="text-red-500">−{result.deletions}</span>
          <button
            type="button"
            className="underline text-[var(--text-secondary)] hover:text-[var(--text)]"
            onClick={() => {
              setUseMonaco(true);
              setExpanded(true);
            }}
          >
            {zh ? '完整查看' : 'Full view'}
          </button>
          {onOpenFile && (
            <button type="button" className="underline" onClick={onOpenFile}>
              {zh ? '打开' : 'Open'}
            </button>
          )}
          {onRollback && (
            <button
              type="button"
              onClick={onRollback}
              className="text-amber-500 underline hover:text-amber-400"
              data-testid="diff-rollback"
            >
              {zh ? '回滚' : 'Rollback'}
            </button>
          )}
          {mode === 'inline' && (
            <button type="button" className="underline" onClick={() => setExpanded((v) => !v)}>
              {expanded ? (zh ? '折叠' : 'Collapse') : (zh ? '展开' : 'Expand')}
            </button>
          )}
        </div>
      </div>

      {expanded && useMonaco && (
        <div className="h-[360px]">
          <MonacoDiffView
            original={result.original}
            modified={result.modified}
            fileName={fileName}
          />
        </div>
      )}

      {expanded && !useMonaco && (
        <div className="max-h-[300px] overflow-auto font-mono text-[0.6875rem]">
          {result.hunks.length === 0 ? (
            <div className="p-3 text-[var(--text-disabled)]">{zh ? '无差异' : 'No changes'}</div>
          ) : (
            result.hunks.map((hunk, hi) => (
              <div key={hi} className="border-b border-[var(--border-subtle)] last:border-0">
                <div className="bg-[var(--surface-hover)] px-2 py-1 text-[var(--text-disabled)]">
                  {hunk.header}
                </div>
                {hunk.lines.map((line, li) => {
                  const bg =
                    line.kind === 'add'
                      ? 'bg-green-500/10 text-green-700 dark:text-green-300'
                      : line.kind === 'del'
                        ? 'bg-red-500/10 text-red-700 dark:text-red-300'
                        : 'text-[var(--text-secondary)]';
                  const mark = line.kind === 'add' ? '+' : line.kind === 'del' ? '−' : ' ';
                  return (
                    <div key={li} className={`flex ${bg}`}>
                      <span className="w-10 shrink-0 select-none border-r border-[var(--border-subtle)] px-1 text-right text-[var(--text-disabled)]">
                        {line.oldNo ?? ''}
                      </span>
                      <span className="w-10 shrink-0 select-none border-r border-[var(--border-subtle)] px-1 text-right text-[var(--text-disabled)]">
                        {line.newNo ?? ''}
                      </span>
                      <span className="w-4 shrink-0 select-none text-center">{mark}</span>
                      <span className="whitespace-pre px-1">{line.text}</span>
                    </div>
                  );
                })}
              </div>
            ))
          )}
          <div className="border-t border-[var(--border-subtle)] p-2 text-right">
            <button
              type="button"
              className="text-[10px] underline text-[var(--primary)]"
              onClick={() => setUseMonaco(true)}
            >
              {zh ? '在 Monaco 中打开' : 'Open in Monaco'}
            </button>
          </div>
        </div>
      )}

      {!expanded && (
        <button
          type="button"
          className="w-full px-3 py-2 text-left text-[11px] text-[var(--text-disabled)] hover:bg-[var(--surface-hover)]"
          onClick={() => setExpanded(true)}
        >
          {result.diffstat} · {zh ? '点击查看 hunk' : 'Click to view hunks'}
        </button>
      )}
    </div>
  );
}
