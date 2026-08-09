'use client';

import { useState } from 'react';
import { GripVertical, Pencil, Trash2, Zap } from 'lucide-react';
import { t } from '@/i18n';
import type { PromptQueueItem } from '@/lib/assistant-protocol';

interface PromptQueuePanelProps {
  items: PromptQueueItem[];
  locale: string;
  onEdit: (id: string, content: string) => void;
  onRemove: (id: string) => void;
  onSendNow: (id: string) => void;
  onReorder: (ids: string[]) => void;
  onClear?: () => void;
}

export default function PromptQueuePanel({
  items,
  locale,
  onEdit,
  onRemove,
  onSendNow,
  onReorder,
  onClear,
}: PromptQueuePanelProps) {
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draft, setDraft] = useState('');
  const [dragId, setDragId] = useState<string | null>(null);

  if (items.length === 0) return null;

  return (
    <div className="border-t border-[var(--border)] bg-[var(--surface-hover)]/40 px-3 py-2">
      <div className="mb-1.5 flex items-center justify-between text-[11px] font-medium text-[var(--text-secondary)]">
        <span>{t(locale, 'promptQueue.title', { count: items.length })}</span>
        {onClear && (
          <button type="button" onClick={onClear} className="hover:text-[var(--danger)]">
            {t(locale, 'promptQueue.clear')}
          </button>
        )}
      </div>
      <ul className="space-y-1">
        {items.map((item, index) => (
          <li
            key={item.id}
            draggable
            onDragStart={() => setDragId(item.id)}
            onDragOver={(e) => e.preventDefault()}
            onDrop={() => {
              if (!dragId || dragId === item.id) return;
              const ids = items.map((i) => i.id);
              const from = ids.indexOf(dragId);
              const to = ids.indexOf(item.id);
              if (from < 0 || to < 0) return;
              ids.splice(from, 1);
              ids.splice(to, 0, dragId);
              onReorder(ids);
              setDragId(null);
            }}
            className="flex items-start gap-1 rounded-md border border-[var(--border-subtle)] bg-[var(--surface)] px-2 py-1.5 text-xs"
          >
            <GripVertical size={12} className="mt-0.5 shrink-0 cursor-grab text-[var(--text-disabled)]" />
            <span className="w-4 shrink-0 text-[var(--text-disabled)]">{index + 1}</span>
            {editingId === item.id ? (
              <div className="flex flex-1 flex-col gap-1">
                <textarea
                  value={draft}
                  onChange={(e) => setDraft(e.target.value)}
                  className="min-h-[48px] w-full rounded border border-[var(--border)] bg-transparent px-1 py-0.5"
                  rows={2}
                />
                <div className="flex gap-1">
                  <button
                    type="button"
                    className="rounded bg-[var(--primary)] px-2 py-0.5 text-white"
                    onClick={() => {
                      onEdit(item.id, draft);
                      setEditingId(null);
                    }}
                  >
                    {t(locale, 'common.save')}
                  </button>
                  <button type="button" onClick={() => setEditingId(null)}>
                    {t(locale, 'common.cancel')}
                  </button>
                </div>
              </div>
            ) : (
              <div className="min-w-0 flex-1">
                <div className="line-clamp-2 whitespace-pre-wrap">{item.content}</div>
                <div className="mt-0.5 text-[10px] text-[var(--text-disabled)]">
                  {item.source}
                  {item.clientTempId ? ' · pending' : ''}
                </div>
              </div>
            )}
            <div className="flex shrink-0 gap-0.5">
              <button
                type="button"
                title={t(locale, 'promptQueue.sendNow')}
                className="rounded p-1 hover:bg-[var(--surface-hover)]"
                onClick={() => onSendNow(item.id)}
              >
                <Zap size={12} />
              </button>
              <button
                type="button"
                title={t(locale, 'common.edit')}
                className="rounded p-1 hover:bg-[var(--surface-hover)]"
                onClick={() => {
                  setEditingId(item.id);
                  setDraft(item.content);
                }}
              >
                <Pencil size={12} />
              </button>
              <button
                type="button"
                title={t(locale, 'promptQueue.remove')}
                className="rounded p-1 hover:bg-[var(--surface-hover)]"
                onClick={() => onRemove(item.id)}
              >
                <Trash2 size={12} />
              </button>
            </div>
          </li>
        ))}
      </ul>
    </div>
  );
}
