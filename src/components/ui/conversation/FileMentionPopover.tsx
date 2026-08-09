'use client';

import { t } from '@/i18n';
import { useEffect, useState } from 'react';
import { fsApi, hasNativeFiles, searchApi } from '@/lib/files-api';

export interface ProjectFileHit {
  path: string;
  name: string;
}

interface FileMentionPopoverProps {
  open: boolean;
  query: string;
  projectPath: string | null;
  locale: string;
  onSelect: (file: ProjectFileHit) => void;
  onClose: () => void;
}

/**
 * `@` project file search for composer — best-effort via files-api（文件域唯一入口）when available.
 */
export default function FileMentionPopover({
  open,
  query,
  projectPath,
  locale,
  onSelect,
  onClose,
}: FileMentionPopoverProps) {
  const [hits, setHits] = useState<ProjectFileHit[]>([]);
  const [index, setIndex] = useState(0);

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    (async () => {
      const q = query.trim().toLowerCase();
      // Prefer project search API if present（files-api 契约：不可用时探测为 null，静默走降级分支）
      const search = (() => {
        try {
          return searchApi();
        } catch {
          return null;
        }
      })();
      try {
        let results: ProjectFileHit[] = [];
        if (search) {
          // 保持原行为：projectPath 缺省时以 undefined 作为 root 透传（宽松签名兼容旧调用）
          const paths = await (search.files as unknown as (q: string, root?: string) => Promise<string[]>)(q || '', projectPath ?? undefined);
          results = (paths ?? []).slice(0, 30).map((path) => ({
            path,
            name: path.split('/').pop() || path,
          }));
        } else if (projectPath && hasNativeFiles()) {
          // Fallback: shallow list
          const entries = ((await fsApi().listDir(projectPath)) ?? []) as Array<{ name: string; path: string; isDir?: boolean }>;
          results = entries
            .filter((e) => !e.isDir)
            .map((e) => ({ path: e.path, name: e.name }))
            .filter((e) => !q || e.name.toLowerCase().includes(q) || e.path.toLowerCase().includes(q))
            .slice(0, 30);
        } else {
          // 无文件能力（浏览器 dev）→ 诚实空态；禁止展示虚构文件列表（假数据红线）
          results = [];
        }
        if (!cancelled) {
          setHits(results);
          setIndex(0);
        }
      } catch {
        if (!cancelled) setHits([]);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [open, query, projectPath]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        onClose();
      } else if (e.key === 'ArrowDown') {
        e.preventDefault();
        setIndex((i) => Math.min(i + 1, Math.max(0, hits.length - 1)));
      } else if (e.key === 'ArrowUp') {
        e.preventDefault();
        setIndex((i) => Math.max(0, i - 1));
      } else if (e.key === 'Enter' && hits[index]) {
        e.preventDefault();
        e.stopPropagation();
        onSelect(hits[index]!);
      }
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [open, hits, index, onClose, onSelect]);

  if (!open) return null;

  return (
    <div
      className="absolute bottom-full left-0 z-50 mb-2 w-80 overflow-hidden rounded-xl border border-[var(--border)] bg-[var(--surface)] shadow-popup"
      role="listbox"
      aria-label={t(locale, 'fileMention.ariaLabel')}
    >
      <div className="border-b border-[var(--border)] px-3 py-1.5 text-[11px] text-[var(--text-disabled)]">
        {t(locale, 'fileMention.hint')}
      </div>
      <ul className="max-h-56 overflow-y-auto py-1">
        {hits.length === 0 && (
          <li className="px-3 py-4 text-center text-xs text-[var(--text-disabled)]">
            {t(locale, 'fileMention.noMatch')}
          </li>
        )}
        {hits.map((hit, i) => (
          <li key={hit.path}>
            <button
              type="button"
              role="option"
              aria-selected={i === index}
              className={`flex w-full flex-col px-3 py-1.5 text-left text-xs ${
                i === index ? 'bg-[var(--surface-hover)]' : 'hover:bg-[var(--surface-hover)]'
              }`}
              onClick={() => onSelect(hit)}
            >
              <span className="font-medium">{hit.name}</span>
              <span className="truncate text-[10px] text-[var(--text-disabled)]">{hit.path}</span>
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}
